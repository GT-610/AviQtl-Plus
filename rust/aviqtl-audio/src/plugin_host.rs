use aviqtl_carla::{CarlaLibraryPaths, CarlaPluginInfo, CarlaPluginProcessor};
use aviqtl_rust_core::api::EvaluatedAudioPlugin;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use truce_rack::core::buffer::{AudioBuffer, BusRange};
use truce_rack::core::events::EventList;
use truce_rack::core::info::{ParameterFlags, PluginCategory, PluginInfo};
use truce_rack::core::plugin::{Plugin, ProcessContext, ProcessStatus};
use truce_rack::core::scanner::PluginScanner;
use truce_rack::core::transport::TransportInfo;

pub(crate) const DEFAULT_PLUGIN_BLOCK_SIZE: usize = 1_024;

/// Modern plugin formats hosted directly from Rust without a linked native SDK.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ModernAudioPluginFormat {
    Clap,
    Vst3,
}

impl ModernAudioPluginFormat {
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "clap" => Some(Self::Clap),
            "vst3" => Some(Self::Vst3),
            _ => None,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Clap => "CLAP",
            Self::Vst3 => "VST3",
        }
    }

    fn rack_name(self) -> &'static str {
        match self {
            Self::Clap => "clap",
            Self::Vst3 => "vst3",
        }
    }
}

/// Stable metadata returned by the Rust-native CLAP/VST3 scanners.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModernAudioPluginInfo {
    pub id: String,
    pub name: String,
    pub format: ModernAudioPluginFormat,
    pub category: String,
    pub path: PathBuf,
    pub native_id: String,
    pub vendor: String,
    pub version: u32,
    pub has_editor: bool,
}

/// Parameter metadata used by the egui inspector and project defaults.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioPluginParameterInfo {
    pub index: usize,
    pub name: String,
    pub unit: String,
    pub minimum: f64,
    pub maximum: f64,
    pub default_value: f64,
    pub step_count: u32,
    pub read_only: bool,
    pub hidden: bool,
}

/// Metadata that can be queried without opening a plugin's custom editor.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioPluginDescription {
    pub info: ModernAudioPluginInfo,
    pub parameters: Vec<AudioPluginParameterInfo>,
}

/// Scans configured directories using the format-specific pure Rust host wrapper.
pub fn scan_modern_audio_plugins(
    format: ModernAudioPluginFormat,
    paths: &[PathBuf],
) -> (Vec<ModernAudioPluginInfo>, Vec<String>) {
    match format {
        ModernAudioPluginFormat::Clap => scan_with(&truce_rack::clap::ClapScanner::new(), paths),
        ModernAudioPluginFormat::Vst3 => scan_with(&truce_rack::vst3::Vst3Scanner::new(), paths),
    }
}

fn scan_with<S: PluginScanner>(
    scanner: &S,
    paths: &[PathBuf],
) -> (Vec<ModernAudioPluginInfo>, Vec<String>) {
    let mut plugins = Vec::new();
    let mut diagnostics = Vec::new();
    for path in paths {
        match scanner.scan_path(path) {
            Ok(found) => plugins.extend(found.into_iter().filter_map(modern_info)),
            Err(error) => diagnostics.push(format!("{}: {error}", path.display())),
        }
    }
    let mut identities = BTreeSet::new();
    plugins.retain(|plugin| {
        identities.insert((plugin.format, plugin.native_id.clone(), plugin.path.clone()))
    });
    plugins.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.native_id.cmp(&right.native_id))
            .then_with(|| left.path.cmp(&right.path))
    });
    (plugins, diagnostics)
}

fn modern_info(info: PluginInfo) -> Option<ModernAudioPluginInfo> {
    let format = ModernAudioPluginFormat::from_name(info.format)?;
    let native_id = info.unique_id.trim().to_owned();
    if native_id.is_empty() {
        return None;
    }
    Some(ModernAudioPluginInfo {
        id: format!("{}:{native_id}:0", format.display_name()),
        name: info.name,
        format,
        category: category_name(info.category).to_owned(),
        path: info.path,
        native_id,
        vendor: info.vendor,
        version: info.version,
        has_editor: info.has_editor,
    })
}

fn category_name(category: PluginCategory) -> &'static str {
    match category {
        PluginCategory::Effect => "Effect",
        PluginCategory::Instrument => "Instrument",
        PluginCategory::NoteEffect => "MIDI",
        PluginCategory::Analyzer => "Analyzer",
        PluginCategory::Tool => "Utility",
    }
}

fn category_from_name(category: &str) -> PluginCategory {
    match category.trim().to_ascii_lowercase().as_str() {
        "instrument" | "synth" => PluginCategory::Instrument,
        "midi" | "note effect" => PluginCategory::NoteEffect,
        "analyzer" => PluginCategory::Analyzer,
        "utility" | "tool" => PluginCategory::Tool,
        _ => PluginCategory::Effect,
    }
}

/// Loads one plugin long enough to expose its host-editable parameter schema.
pub fn inspect_audio_plugin(
    info: &ModernAudioPluginInfo,
) -> Result<AudioPluginDescription, String> {
    let plugin = load_plugin(&PluginIdentity::from_info(info))?;
    let mut discovered = info.clone();
    discovered.has_editor = plugin.info().has_editor;
    let parameters = (0..plugin.parameter_count())
        .filter_map(|index| match plugin.parameter_info(index) {
            Ok(parameter) => Some(AudioPluginParameterInfo {
                index,
                name: parameter.name,
                unit: parameter.unit,
                minimum: parameter.min,
                maximum: parameter.max,
                default_value: parameter.default,
                step_count: parameter.step_count,
                read_only: parameter.flags.contains(ParameterFlags::READ_ONLY),
                hidden: parameter.flags.contains(ParameterFlags::HIDDEN),
            }),
            Err(_) => None,
        })
        .collect();
    Ok(AudioPluginDescription {
        info: discovered,
        parameters,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PluginIdentity {
    project_id: String,
    backend: PluginBackend,
    category: String,
    path: PathBuf,
    native_id: String,
    name: String,
    vendor: String,
    version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PluginBackend {
    Modern(ModernAudioPluginFormat),
    Carla(Box<CarlaPluginInfo>),
}

impl PluginIdentity {
    fn from_info(info: &ModernAudioPluginInfo) -> Self {
        Self {
            project_id: info.id.clone(),
            backend: PluginBackend::Modern(info.format),
            category: info.category.clone(),
            path: info.path.clone(),
            native_id: info.native_id.clone(),
            name: info.name.clone(),
            vendor: info.vendor.clone(),
            version: info.version,
        }
    }

    fn from_plan(plugin: &EvaluatedAudioPlugin) -> Result<Self, String> {
        let format_name = plugin
            .extra
            .get("format")
            .and_then(serde_json::Value::as_str)
            .or_else(|| plugin.id.split(':').next())
            .ok_or_else(|| format!("{} is missing its plugin format", plugin.id))?;
        let path = plugin
            .extra
            .get("path")
            .and_then(serde_json::Value::as_str)
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| format!("{} is missing its plugin path", plugin.id))?;
        let text = |key: &str| {
            plugin
                .extra
                .get(key)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let version = plugin
            .extra
            .get("version")
            .and_then(serde_json::Value::as_u64)
            .and_then(|version| u32::try_from(version).ok())
            .unwrap_or_default();
        let host_backend = plugin
            .extra
            .get("hostBackend")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let use_carla = host_backend.eq_ignore_ascii_case("carla")
            || ModernAudioPluginFormat::from_name(format_name).is_none();
        let (backend, native_id) = if use_carla {
            let mut id_parts = plugin.id.split(':');
            let _format = id_parts.next();
            let label = plugin
                .extra
                .get("label")
                .and_then(serde_json::Value::as_str)
                .filter(|label| !label.is_empty())
                .map(str::to_owned)
                .or_else(|| id_parts.next().map(str::to_owned))
                .unwrap_or_default();
            let unique_id = plugin
                .extra
                .get("uniqueId")
                .and_then(serde_json::Value::as_i64)
                .or_else(|| id_parts.next().and_then(|value| value.parse().ok()))
                .unwrap_or_default();
            let libraries = carla_library_paths(plugin)?;
            (
                PluginBackend::Carla(Box::new(CarlaPluginInfo {
                    id: plugin.id.clone(),
                    name: text("name"),
                    format: format_name.to_owned(),
                    category: text("category"),
                    path: path.clone(),
                    label,
                    maker: text("vendor"),
                    unique_id,
                    index: plugin
                        .extra
                        .get("index")
                        .and_then(serde_json::Value::as_i64)
                        .and_then(|value| i32::try_from(value).ok())
                        .unwrap_or_default(),
                    libraries,
                })),
                String::new(),
            )
        } else {
            let native_id = plugin
                .extra
                .get("nativeId")
                .and_then(serde_json::Value::as_str)
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
                .or_else(|| {
                    let mut parts = plugin.id.split(':');
                    let _format = parts.next()?;
                    let native_id = parts.next()?;
                    (!native_id.is_empty()).then(|| native_id.to_owned())
                })
                .ok_or_else(|| format!("{} is missing its native plugin id", plugin.id))?;
            (
                PluginBackend::Modern(
                    ModernAudioPluginFormat::from_name(format_name)
                        .expect("the modern format was checked above"),
                ),
                native_id,
            )
        };
        Ok(Self {
            project_id: plugin.id.clone(),
            backend,
            category: text("category"),
            path,
            native_id,
            name: text("name"),
            vendor: text("vendor"),
            version,
        })
    }

    fn rack_info(&self) -> PluginInfo {
        let PluginBackend::Modern(format) = &self.backend else {
            unreachable!("only modern plugins have truce-rack metadata")
        };
        PluginInfo {
            name: self.name.clone(),
            vendor: self.vendor.clone(),
            version: self.version,
            category: category_from_name(&self.category),
            path: self.path.clone(),
            unique_id: self.native_id.clone(),
            format: format.rack_name(),
            has_editor: false,
            accepts_midi: false,
        }
    }
}

fn carla_library_paths(plugin: &EvaluatedAudioPlugin) -> Result<CarlaLibraryPaths, String> {
    let path = |key: &str| {
        plugin
            .extra
            .get(key)
            .and_then(serde_json::Value::as_str)
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
    };
    match (
        path("carlaNativeLibrary"),
        path("carlaHostLibrary"),
        path("carlaResourceDir"),
    ) {
        (Some(native_plugin), Some(host_plugin), Some(resource_dir))
            if native_plugin.is_file() && host_plugin.is_file() && resource_dir.is_dir() =>
        {
            Ok(CarlaLibraryPaths {
                native_plugin,
                host_plugin,
                resource_dir,
            })
        }
        _ => CarlaLibraryPaths::discover(None)
            .ok_or_else(|| format!("{} requires the Carla host libraries", plugin.id)),
    }
}

#[derive(Debug, Clone)]
struct ProcessorParameterInfo {
    min: f64,
    max: f64,
}

trait AudioPluginProcessor: Send {
    fn parameter_count(&self) -> usize;
    fn parameter_info(&self, index: usize) -> Result<ProcessorParameterInfo, String>;
    fn set_parameter(&mut self, index: usize, value: f64) -> Result<(), String>;
    fn process(
        &mut self,
        input_left: &[f32],
        input_right: &[f32],
        output_left: &mut [f32],
        output_right: &mut [f32],
        transport: TransportInfo,
    ) -> Result<ProcessStatus, String>;
}

struct TrucePluginProcessor {
    plugin: Box<dyn Plugin<f32>>,
    sample_rate: f64,
    max_block_size: usize,
    events: EventList,
    output_events: EventList,
}

impl AudioPluginProcessor for TrucePluginProcessor {
    fn parameter_count(&self) -> usize {
        self.plugin.parameter_count()
    }

    fn parameter_info(&self, index: usize) -> Result<ProcessorParameterInfo, String> {
        self.plugin
            .parameter_info(index)
            .map(|info| ProcessorParameterInfo {
                min: info.min,
                max: info.max,
            })
            .map_err(|error| error.to_string())
    }

    fn set_parameter(&mut self, index: usize, value: f64) -> Result<(), String> {
        self.plugin
            .set_parameter(index, value)
            .map_err(|error| error.to_string())
    }

    fn process(
        &mut self,
        input_left: &[f32],
        input_right: &[f32],
        output_left: &mut [f32],
        output_right: &mut [f32],
        transport: TransportInfo,
    ) -> Result<ProcessStatus, String> {
        let frames = input_left.len();
        let inputs: [&[f32]; 2] = [input_left, input_right];
        let mut outputs: [&mut [f32]; 2] = [output_left, output_right];
        let input_buses = [BusRange::new(0, 2)];
        let output_buses = [BusRange::new(0, 2)];
        let mut buffer =
            AudioBuffer::new(&inputs, &mut outputs, frames, &input_buses, &output_buses);
        self.output_events.clear();
        let mut context = ProcessContext {
            sample_rate: self.sample_rate,
            max_block_size: self.max_block_size,
            transport: Some(transport),
            output_events: &mut self.output_events,
        };
        self.plugin
            .process(&mut buffer, &self.events, &mut context)
            .map_err(|error| error.to_string())
    }
}

impl AudioPluginProcessor for CarlaPluginProcessor {
    fn parameter_count(&self) -> usize {
        self.parameter_count()
    }

    fn parameter_info(&self, index: usize) -> Result<ProcessorParameterInfo, String> {
        self.parameter_info(index)
            .map(|info| ProcessorParameterInfo {
                min: info.minimum,
                max: info.maximum,
            })
    }

    fn set_parameter(&mut self, index: usize, value: f64) -> Result<(), String> {
        self.set_parameter(index, value)
    }

    fn process(
        &mut self,
        input_left: &[f32],
        input_right: &[f32],
        output_left: &mut [f32],
        output_right: &mut [f32],
        transport: TransportInfo,
    ) -> Result<ProcessStatus, String> {
        self.process(
            input_left,
            input_right,
            output_left,
            output_right,
            transport.song_position_samples.unwrap_or_default(),
        )?;
        Ok(ProcessStatus::Continue)
    }
}

fn load_plugin(identity: &PluginIdentity) -> Result<Box<dyn Plugin<f32>>, String> {
    let info = identity.rack_info();
    let PluginBackend::Modern(format) = &identity.backend else {
        return Err("Carla plugins do not use truce-rack".to_owned());
    };
    match format {
        ModernAudioPluginFormat::Clap => truce_rack::clap::ClapScanner::new()
            .load(&info)
            .map(|plugin| Box::new(plugin) as Box<dyn Plugin<f32>>)
            .map_err(|error| error.to_string()),
        ModernAudioPluginFormat::Vst3 => truce_rack::vst3::Vst3Scanner::new()
            .load(&info)
            .map(|plugin| Box::new(plugin) as Box<dyn Plugin<f32>>)
            .map_err(|error| error.to_string()),
    }
}

fn activate_plugin(
    identity: &PluginIdentity,
    sample_rate: u32,
    max_block_size: usize,
) -> Result<Box<dyn AudioPluginProcessor>, String> {
    if let PluginBackend::Carla(info) = &identity.backend {
        return CarlaPluginProcessor::load(info.as_ref().clone(), sample_rate, max_block_size)
            .map(|processor| Box::new(processor) as Box<dyn AudioPluginProcessor>);
    }
    let mut plugin = load_plugin(identity)?;
    let layout = plugin
        .supported_layouts()
        .iter()
        .find(|layout| layout.total_input_channels() == 2 && layout.total_output_channels() == 2)
        .cloned()
        .ok_or_else(|| "plugin has no stereo input/output layout".to_owned())?;
    plugin
        .activate(layout, f64::from(sample_rate), max_block_size)
        .map_err(|error| error.to_string())?;
    Ok(Box::new(TrucePluginProcessor {
        plugin,
        sample_rate: f64::from(sample_rate),
        max_block_size,
        events: EventList::new(),
        output_events: EventList::new(),
    }))
}

struct PluginSlot {
    identity: PluginIdentity,
    processor: Result<Box<dyn AudioPluginProcessor>, String>,
    parameters: BTreeMap<usize, f64>,
}

impl PluginSlot {
    fn load(identity: PluginIdentity, sample_rate: u32, max_block_size: usize) -> Self {
        let processor = activate_plugin(&identity, sample_rate, max_block_size);
        Self {
            identity,
            processor,
            parameters: BTreeMap::new(),
        }
    }
}

/// Per-clip stateful plugin chain. It is reset on seeks, topology changes, or sample-rate changes.
pub(crate) struct AudioPluginChain {
    identities: Vec<PluginIdentity>,
    slots: Vec<PluginSlot>,
    sample_rate: u32,
    max_block_size: usize,
    last_timeline_frame: Option<i32>,
}

#[derive(Clone, Copy)]
pub(crate) struct AudioPluginProcessContext {
    pub(crate) timeline_frame: i32,
    pub(crate) start_sample: i64,
    pub(crate) sample_rate: u32,
    pub(crate) max_block_size: usize,
}

impl Default for AudioPluginChain {
    fn default() -> Self {
        Self {
            identities: Vec::new(),
            slots: Vec::new(),
            sample_rate: 0,
            max_block_size: DEFAULT_PLUGIN_BLOCK_SIZE,
            last_timeline_frame: None,
        }
    }
}

impl AudioPluginChain {
    pub(crate) fn process(
        &mut self,
        plugins: &[EvaluatedAudioPlugin],
        context: AudioPluginProcessContext,
        samples: &mut [f32],
        errors: &mut Vec<String>,
    ) {
        let max_block_size = context.max_block_size.max(1);
        let identities = plugins
            .iter()
            .map(PluginIdentity::from_plan)
            .collect::<Result<Vec<_>, _>>();
        let identities = match identities {
            Ok(identities) => identities,
            Err(error) => {
                errors.push(error);
                self.clear();
                return;
            }
        };
        let discontinuous = self
            .last_timeline_frame
            .is_some_and(|previous| context.timeline_frame != previous.saturating_add(1));
        if self.identities != identities
            || self.sample_rate != context.sample_rate
            || self.max_block_size != max_block_size
            || discontinuous
        {
            self.rebuild(identities, context.sample_rate, max_block_size);
        }
        self.last_timeline_frame = Some(context.timeline_frame);

        for (plugin, slot) in plugins.iter().zip(&mut self.slots) {
            if !plugin.enabled {
                continue;
            }
            let processor = match &mut slot.processor {
                Ok(processor) => processor,
                Err(error) => {
                    errors.push(format!(
                        "audio plugin {} ({}): {error}",
                        slot.identity.project_id,
                        slot.identity.path.display()
                    ));
                    continue;
                }
            };
            apply_parameters(plugin, processor.as_mut(), &mut slot.parameters, errors);
            process_blocks(
                processor.as_mut(),
                samples,
                AudioPluginProcessContext {
                    max_block_size,
                    ..context
                },
                &slot.identity,
                errors,
            );
        }
    }

    fn rebuild(
        &mut self,
        identities: Vec<PluginIdentity>,
        sample_rate: u32,
        max_block_size: usize,
    ) {
        self.slots = identities
            .iter()
            .cloned()
            .map(|identity| PluginSlot::load(identity, sample_rate, max_block_size))
            .collect();
        self.identities = identities;
        self.sample_rate = sample_rate;
        self.max_block_size = max_block_size;
        self.last_timeline_frame = None;
    }

    fn clear(&mut self) {
        self.identities.clear();
        self.slots.clear();
        self.sample_rate = 0;
        self.last_timeline_frame = None;
    }
}

fn apply_parameters(
    plugin: &EvaluatedAudioPlugin,
    processor: &mut dyn AudioPluginProcessor,
    previous: &mut BTreeMap<usize, f64>,
    errors: &mut Vec<String>,
) {
    for (key, value) in &plugin.params {
        let Ok(index) = key.parse::<usize>() else {
            continue;
        };
        let Some(mut value) = value
            .as_f64()
            .or_else(|| value.as_bool().map(|value| f64::from(u8::from(value))))
            .filter(|value| value.is_finite())
        else {
            continue;
        };
        if index >= processor.parameter_count() {
            continue;
        }
        if let Ok(info) = processor.parameter_info(index) {
            value = value.clamp(info.min, info.max);
        }
        if previous.get(&index).is_some_and(|old| *old == value) {
            continue;
        }
        match processor.set_parameter(index, value) {
            Ok(()) => {
                previous.insert(index, value);
            }
            Err(error) => errors.push(format!(
                "audio plugin {} parameter {index}: {error}",
                plugin.id
            )),
        }
    }
}

fn process_blocks(
    processor: &mut dyn AudioPluginProcessor,
    samples: &mut [f32],
    context: AudioPluginProcessContext,
    identity: &PluginIdentity,
    errors: &mut Vec<String>,
) {
    let frame_count = samples.len() / 2;
    let mut left = vec![0.0; context.max_block_size];
    let mut right = vec![0.0; context.max_block_size];
    let mut output_left = vec![0.0; context.max_block_size];
    let mut output_right = vec![0.0; context.max_block_size];
    for first_frame in (0..frame_count).step_by(context.max_block_size) {
        let frames = (frame_count - first_frame).min(context.max_block_size);
        for frame in 0..frames {
            left[frame] = samples[(first_frame + frame) * 2];
            right[frame] = samples[(first_frame + frame) * 2 + 1];
            output_left[frame] = 0.0;
            output_right[frame] = 0.0;
        }
        let block_start = context
            .start_sample
            .saturating_add(i64::try_from(first_frame).unwrap_or(0));
        let transport = TransportInfo {
            song_position_samples: Some(block_start),
            playing: true,
            ..TransportInfo::default()
        };
        match processor.process(
            &left[..frames],
            &right[..frames],
            &mut output_left[..frames],
            &mut output_right[..frames],
            transport,
        ) {
            Ok(ProcessStatus::Error) => errors.push(format!(
                "audio plugin {} returned a processing error at sample {block_start}",
                identity.project_id
            )),
            Ok(_) => {
                for frame in 0..frames {
                    samples[(first_frame + frame) * 2] = output_left[frame];
                    samples[(first_frame + frame) * 2 + 1] = output_right[frame];
                }
            }
            Err(error) => errors.push(format!(
                "audio plugin {} failed at sample {block_start} ({sample_rate} Hz): {error}",
                identity.project_id,
                sample_rate = context.sample_rate
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aviqtl_rust_core::api::ExtraFields;
    use serde_json::{Value, json};

    struct GainProcessor {
        gain: f64,
        calls: Vec<(usize, i64)>,
    }

    impl AudioPluginProcessor for GainProcessor {
        fn parameter_count(&self) -> usize {
            1
        }

        fn parameter_info(&self, index: usize) -> Result<ProcessorParameterInfo, String> {
            if index != 0 {
                return Err("invalid parameter".to_owned());
            }
            Ok(ProcessorParameterInfo { min: 0.0, max: 2.0 })
        }

        fn set_parameter(&mut self, index: usize, value: f64) -> Result<(), String> {
            if index != 0 {
                return Err("invalid parameter".to_owned());
            }
            self.gain = value;
            Ok(())
        }

        fn process(
            &mut self,
            input_left: &[f32],
            input_right: &[f32],
            output_left: &mut [f32],
            output_right: &mut [f32],
            transport: TransportInfo,
        ) -> Result<ProcessStatus, String> {
            self.calls.push((
                input_left.len(),
                transport.song_position_samples.unwrap_or_default(),
            ));
            for ((output_left, output_right), (input_left, input_right)) in output_left
                .iter_mut()
                .zip(output_right)
                .zip(input_left.iter().zip(input_right))
            {
                *output_left = *input_left * self.gain as f32;
                *output_right = *input_right * self.gain as f32;
            }
            Ok(ProcessStatus::Continue)
        }
    }

    fn plugin(value: Value) -> EvaluatedAudioPlugin {
        EvaluatedAudioPlugin {
            id: "CLAP:org.example.gain:0".to_owned(),
            enabled: true,
            params: BTreeMap::from([("0".to_owned(), value)]),
            extra: ExtraFields::from([
                ("format".to_owned(), json!("CLAP")),
                ("path".to_owned(), json!("/plugins/gain.clap")),
                ("nativeId".to_owned(), json!("org.example.gain")),
            ]),
        }
    }

    #[test]
    fn block_processing_clamps_parameters_and_preserves_transport_offsets() {
        let mut processor = GainProcessor {
            gain: 1.0,
            calls: Vec::new(),
        };
        let mut previous = BTreeMap::new();
        let mut errors = Vec::new();
        apply_parameters(
            &plugin(json!(3.0)),
            &mut processor,
            &mut previous,
            &mut errors,
        );
        assert_eq!(processor.gain, 2.0);
        assert!(errors.is_empty());

        let mut samples = vec![0.25, -0.5, 0.25, -0.5, 0.25, -0.5, 0.25, -0.5, 0.25, -0.5];
        let identity = PluginIdentity::from_plan(&plugin(json!(2.0))).expect("valid identity");
        process_blocks(
            &mut processor,
            &mut samples,
            AudioPluginProcessContext {
                timeline_frame: 0,
                start_sample: 100,
                sample_rate: 48_000,
                max_block_size: 3,
            },
            &identity,
            &mut errors,
        );
        assert_eq!(processor.calls, [(3, 100), (2, 103)]);
        assert_eq!(
            samples,
            vec![0.5, -1.0, 0.5, -1.0, 0.5, -1.0, 0.5, -1.0, 0.5, -1.0]
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn failed_blocks_leave_the_dry_audio_untouched() {
        struct FailedProcessor;
        impl AudioPluginProcessor for FailedProcessor {
            fn parameter_count(&self) -> usize {
                0
            }
            fn parameter_info(&self, _index: usize) -> Result<ProcessorParameterInfo, String> {
                Err("invalid parameter".to_owned())
            }
            fn set_parameter(&mut self, _index: usize, _value: f64) -> Result<(), String> {
                Err("invalid parameter".to_owned())
            }
            fn process(
                &mut self,
                _input_left: &[f32],
                _input_right: &[f32],
                output_left: &mut [f32],
                output_right: &mut [f32],
                _transport: TransportInfo,
            ) -> Result<ProcessStatus, String> {
                output_left.fill(99.0);
                output_right.fill(99.0);
                Ok(ProcessStatus::Error)
            }
        }

        let identity = PluginIdentity::from_plan(&plugin(json!(1.0))).expect("valid identity");
        let mut samples = vec![0.25, -0.5, 0.75, -1.0];
        let dry = samples.clone();
        let mut errors = Vec::new();
        process_blocks(
            &mut FailedProcessor,
            &mut samples,
            AudioPluginProcessContext {
                timeline_frame: 0,
                start_sample: 0,
                sample_rate: 48_000,
                max_block_size: 64,
            },
            &identity,
            &mut errors,
        );
        assert_eq!(samples, dry);
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn plan_identity_requires_path_and_native_id() {
        let invalid = EvaluatedAudioPlugin {
            id: "CLAP".to_owned(),
            enabled: true,
            params: BTreeMap::new(),
            extra: ExtraFields::new(),
        };
        assert!(PluginIdentity::from_plan(&invalid).is_err());
    }

    #[test]
    fn plan_identity_routes_qt_legacy_plugins_to_the_carla_backend() {
        let plugin = EvaluatedAudioPlugin {
            id: "LV2:bundle.lv2/http://example.org/gain:42".to_owned(),
            enabled: true,
            params: BTreeMap::new(),
            extra: ExtraFields::from([
                ("hostBackend".to_owned(), json!("carla")),
                ("format".to_owned(), json!("LV2")),
                ("path".to_owned(), json!("/plugins/gain.lv2")),
                (
                    "label".to_owned(),
                    json!("bundle.lv2/http://example.org/gain"),
                ),
                ("uniqueId".to_owned(), json!(42)),
                ("name".to_owned(), json!("Gain")),
                ("vendor".to_owned(), json!("Example")),
                ("category".to_owned(), json!("Utility")),
                ("carlaNativeLibrary".to_owned(), json!("/carla/native")),
                ("carlaHostLibrary".to_owned(), json!("/carla/host")),
                ("carlaResourceDir".to_owned(), json!("/carla/resources")),
            ]),
        };
        let identity = PluginIdentity::from_plan(&plugin).expect("Carla identity");
        let PluginBackend::Carla(info) = identity.backend else {
            panic!("legacy plugin did not select Carla");
        };
        assert_eq!(info.format, "LV2");
        assert_eq!(info.label, "bundle.lv2/http://example.org/gain");
        assert_eq!(info.unique_id, 42);
    }
}
