use crate::permission::{PluginPermission, PluginPermissionState};
use luna_core::runtime::Value as LuaValue;
use luna_core::version::LuaVersion;
use luna_core::vm::{LuaError, Vm};
use serde_json::Value as JsonValue;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter};

const INSTRUCTION_BUDGET: i64 = 1_000_000;
const MEMORY_CAP: usize = 16 * 1024 * 1024;
const LOADER_INPUT_BUDGET: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptClipSnapshot {
    pub id: i32,
    pub clip_type: String,
    pub layer: i32,
    pub start_frame: i32,
    pub duration: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScriptHostSnapshot {
    pub current_frame: i32,
    pub is_playing: bool,
    pub project_width: i32,
    pub project_height: i32,
    pub project_fps: f64,
    pub clips: Vec<ScriptClipSnapshot>,
    pub plugin_settings: BTreeMap<String, String>,
}

impl Default for ScriptHostSnapshot {
    fn default() -> Self {
        Self {
            current_frame: 0,
            is_playing: false,
            project_width: 1920,
            project_height: 1080,
            project_fps: 30.0,
            clips: Vec::new(),
            plugin_settings: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScriptHostCommand {
    Log(String),
    TransportPlay,
    TransportPause,
    TransportToggle,
    TransportSeek(i32),
    ClipCreate {
        clip_type: String,
        start_frame: i32,
        layer: i32,
    },
    ClipDelete(i32),
    ClipUpdate {
        clip_id: i32,
        layer: i32,
        start_frame: i32,
        duration: i32,
    },
    ClipSelect(i32),
    ClipSplit {
        clip_id: i32,
        frame: i32,
    },
    ClipCopy(i32),
    ClipCut(i32),
    ClipPaste {
        frame: i32,
        layer: i32,
    },
    EffectAdd {
        clip_id: i32,
        effect_type: String,
    },
    EffectRemove {
        clip_id: i32,
        effect_index: i32,
    },
    EffectSetParameter {
        clip_id: i32,
        effect_index: i32,
        name: String,
        value: JsonValue,
    },
    ProjectSave(String),
    ProjectLoad(String),
    Undo,
    Redo,
    SettingsSet {
        key: String,
        value: String,
    },
    SceneCreate(String),
    SceneRemove(i32),
    SceneSwitch(i32),
    CommandBeginGroup(String),
    CommandEndGroup,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScriptExecution {
    pub plugin_id: String,
    pub commands: Vec<ScriptHostCommand>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptRuntimeError {
    message: String,
}

impl ScriptRuntimeError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Display for ScriptRuntimeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ScriptRuntimeError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptHook {
    Load,
    Unload,
    Update,
    ProjectOpen(String),
    ProjectSave(String),
    ClipChange,
}

impl ScriptHook {
    fn function_name(&self) -> &'static str {
        match self {
            Self::Load => "AviQtlOnLoad",
            Self::Unload => "AviQtlOnUnload",
            Self::Update => "AviQtlUpdateHook",
            Self::ProjectOpen(_) => "AviQtlOnProjectOpen",
            Self::ProjectSave(_) => "AviQtlOnProjectSave",
            Self::ClipChange => "AviQtlOnClipChange",
        }
    }

    fn argument(&self) -> Option<&str> {
        match self {
            Self::ProjectOpen(path) | Self::ProjectSave(path) => Some(path),
            _ => None,
        }
    }
}

struct ScriptCallContext {
    plugin_id: String,
    granted: BTreeSet<PluginPermission>,
    snapshot: ScriptHostSnapshot,
    execution: ScriptExecution,
}

thread_local! {
    static ACTIVE_CONTEXT: RefCell<Option<ScriptCallContext>> = const { RefCell::new(None) };
}

pub struct ScriptRuntime {
    plugin_id: String,
    vm: Vm,
}

impl ScriptRuntime {
    pub fn load(
        plugin_id: impl Into<String>,
        source: &str,
        chunk_name: &str,
        parameters: &BTreeMap<String, JsonValue>,
        permissions: &PluginPermissionState,
        snapshot: ScriptHostSnapshot,
    ) -> Result<(Self, ScriptExecution), ScriptRuntimeError> {
        if source.len() > LOADER_INPUT_BUDGET {
            return Err(ScriptRuntimeError::new(
                "script exceeds the loader input budget",
            ));
        }
        let plugin_id = plugin_id.into();
        let mut vm = Vm::sandbox(LuaVersion::Lua51)
            .open_base()
            .open_math()
            .open_string()
            .open_table()
            .with_instr_budget(INSTRUCTION_BUDGET)
            .with_memory_cap(MEMORY_CAP)
            .build();
        vm.set_loader_input_budget(LOADER_INPUT_BUDGET);
        remove_unsafe_globals(&mut vm)?;
        install_math_shortcuts(&mut vm)?;
        install_aviqtl_api(&mut vm)?;
        inject_parameters(&mut vm, parameters)?;

        let mut runtime = Self { plugin_id, vm };
        let (result, execution) = runtime.with_context(permissions, snapshot, |vm| {
            vm.eval_chunk(source, chunk_name)
        })?;
        if let Err(error) = result {
            return Err(ScriptRuntimeError::new(format!(
                "{chunk_name}: {}",
                runtime.vm.error_text(&error)
            )));
        }
        Ok((runtime, execution))
    }

    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    pub fn set_parameters(
        &mut self,
        parameters: &BTreeMap<String, JsonValue>,
    ) -> Result<(), ScriptRuntimeError> {
        inject_parameters(&mut self.vm, parameters)
    }

    pub fn has_hook(&mut self, hook: &ScriptHook) -> bool {
        lua_function(global(&mut self.vm, hook.function_name()))
    }

    pub fn dispatch(
        &mut self,
        hook: ScriptHook,
        permissions: &PluginPermissionState,
        snapshot: ScriptHostSnapshot,
    ) -> ScriptExecution {
        let function = global(&mut self.vm, hook.function_name());
        if !lua_function(function) {
            return ScriptExecution {
                plugin_id: self.plugin_id.clone(),
                ..ScriptExecution::default()
            };
        }
        let arguments = hook
            .argument()
            .map(|argument| vec![LuaValue::Str(self.vm.intern_str(argument))])
            .unwrap_or_default();
        match self.with_context(permissions, snapshot, |vm| {
            vm.call_value(function, &arguments)
        }) {
            Ok((Ok(_), execution)) => execution,
            Ok((Err(error), mut execution)) => {
                execution.diagnostics.push(format!(
                    "{}: {}",
                    hook.function_name(),
                    self.vm.error_text(&error)
                ));
                execution
            }
            Err(error) => ScriptExecution {
                plugin_id: self.plugin_id.clone(),
                diagnostics: vec![error.to_string()],
                ..ScriptExecution::default()
            },
        }
    }

    fn with_context<T>(
        &mut self,
        permissions: &PluginPermissionState,
        snapshot: ScriptHostSnapshot,
        operation: impl FnOnce(&mut Vm) -> T,
    ) -> Result<(T, ScriptExecution), ScriptRuntimeError> {
        let context = ScriptCallContext {
            plugin_id: self.plugin_id.clone(),
            granted: permissions.granted(&self.plugin_id).into_iter().collect(),
            snapshot,
            execution: ScriptExecution {
                plugin_id: self.plugin_id.clone(),
                ..ScriptExecution::default()
            },
        };
        let entered = ACTIVE_CONTEXT.with(|active| {
            let mut active = active.borrow_mut();
            if active.is_some() {
                false
            } else {
                *active = Some(context);
                true
            }
        });
        if !entered {
            return Err(ScriptRuntimeError::new(
                "nested script runtime dispatch is not supported",
            ));
        }
        self.vm.set_instr_budget(Some(INSTRUCTION_BUDGET));
        let result = operation(&mut self.vm);
        let context = ACTIVE_CONTEXT
            .with(|active| active.borrow_mut().take())
            .ok_or_else(|| ScriptRuntimeError::new("script host context was lost"))?;
        Ok((result, context.execution))
    }
}

fn remove_unsafe_globals(vm: &mut Vm) -> Result<(), ScriptRuntimeError> {
    for name in [
        "load",
        "loadstring",
        "loadfile",
        "dofile",
        "print",
        "require",
        "module",
        "io",
        "os",
        "debug",
        "package",
        "ffi",
    ] {
        vm.set_global(name, ())
            .map_err(|error| runtime_error(vm, error))?;
    }
    Ok(())
}

fn install_math_shortcuts(vm: &mut Vm) -> Result<(), ScriptRuntimeError> {
    let math = global(vm, "math");
    vm.set_global("m", math)
        .map_err(|error| runtime_error(vm, error))?;
    let LuaValue::Table(table) = math else {
        return Err(ScriptRuntimeError::new("math library was not initialized"));
    };
    for name in [
        "sin", "cos", "tan", "abs", "max", "min", "floor", "ceil", "random",
    ] {
        let key = LuaValue::Str(vm.intern_str(name));
        vm.set_global(name, table.get(key))
            .map_err(|error| runtime_error(vm, error))?;
    }
    let key = LuaValue::Str(vm.intern_str("pi"));
    vm.set_global("pi", table.get(key))
        .map_err(|error| runtime_error(vm, error))?;
    Ok(())
}

fn install_aviqtl_api(vm: &mut Vm) -> Result<(), ScriptRuntimeError> {
    let transport_play = vm.native(api_transport_play);
    let transport_pause = vm.native(api_transport_pause);
    let transport_toggle = vm.native(api_transport_toggle);
    let transport_seek = vm.native(api_transport_seek);
    let transport_get_frame = vm.native(api_transport_get_frame);
    let transport_is_playing = vm.native(api_transport_is_playing);
    let transport = vm
        .new_table()
        .with("play", transport_play)
        .with("pause", transport_pause)
        .with("toggle", transport_toggle)
        .with("seek", transport_seek)
        .with("get_frame", transport_get_frame)
        .with("is_playing", transport_is_playing)
        .build();

    let clip_create = vm.native(api_clip_create);
    let clip_delete = vm.native(api_clip_delete);
    let clip_update = vm.native(api_clip_update);
    let clip_select = vm.native(api_clip_select);
    let clip_split = vm.native(api_clip_split);
    let clip_copy = vm.native(api_clip_copy);
    let clip_cut = vm.native(api_clip_cut);
    let clip_paste = vm.native(api_clip_paste);
    let clip_list = vm.native(api_clip_list);
    let clip = vm
        .new_table()
        .with("create", clip_create)
        .with("delete", clip_delete)
        .with("update", clip_update)
        .with("select", clip_select)
        .with("split", clip_split)
        .with("copy", clip_copy)
        .with("cut", clip_cut)
        .with("paste", clip_paste)
        .with("list", clip_list)
        .build();

    let effect_add = vm.native(api_effect_add);
    let effect_remove = vm.native(api_effect_remove);
    let effect_set_parameter = vm.native(api_effect_set_parameter);
    let effect = vm
        .new_table()
        .with("add", effect_add)
        .with("remove", effect_remove)
        .with("set_param", effect_set_parameter)
        .build();

    let project_width = vm.native(api_project_width);
    let project_height = vm.native(api_project_height);
    let project_fps = vm.native(api_project_fps);
    let project_save = vm.native(api_project_save);
    let project_load = vm.native(api_project_load);
    let project = vm
        .new_table()
        .with("width", project_width)
        .with("height", project_height)
        .with("fps", project_fps)
        .with("save", project_save)
        .with("load", project_load)
        .build();

    let scene_create = vm.native(api_scene_create);
    let scene_remove = vm.native(api_scene_remove);
    let scene_switch = vm.native(api_scene_switch);
    let scene = vm
        .new_table()
        .with("create", scene_create)
        .with("remove", scene_remove)
        .with("switch", scene_switch)
        .build();

    let settings_set = vm.native(api_settings_set);
    let settings_get = vm.native(api_settings_get);
    let settings = vm
        .new_table()
        .with("set", settings_set)
        .with("get", settings_get)
        .build();

    let command_begin_group = vm.native(api_command_begin_group);
    let command_end_group = vm.native(api_command_end_group);
    let command = vm
        .new_table()
        .with("begin_group", command_begin_group)
        .with("end_group", command_end_group)
        .build();

    let log = vm.native(api_log);
    let undo = vm.native(api_undo);
    let redo = vm.native(api_redo);
    let aviqtl = vm
        .new_table()
        .with("transport", LuaValue::Table(transport))
        .with("clip", LuaValue::Table(clip))
        .with("effect", LuaValue::Table(effect))
        .with("project", LuaValue::Table(project))
        .with("scene", LuaValue::Table(scene))
        .with("settings", LuaValue::Table(settings))
        .with("command", LuaValue::Table(command))
        .with("log", log)
        .with("undo", undo)
        .with("redo", redo)
        .build();
    vm.set_global("aviqtl", LuaValue::Table(aviqtl))
        .map_err(|error| runtime_error(vm, error))
}

fn inject_parameters(
    vm: &mut Vm,
    parameters: &BTreeMap<String, JsonValue>,
) -> Result<(), ScriptRuntimeError> {
    for (name, value) in parameters {
        let value = json_to_lua(vm, value);
        vm.set_global(name, value)
            .map_err(|error| runtime_error(vm, error))?;
    }
    Ok(())
}

fn json_to_lua(vm: &mut Vm, value: &JsonValue) -> LuaValue {
    match value {
        JsonValue::Null => LuaValue::Nil,
        JsonValue::Bool(value) => LuaValue::Bool(*value),
        JsonValue::Number(value) => value
            .as_i64()
            .map(LuaValue::Int)
            .or_else(|| value.as_f64().map(LuaValue::Float))
            .unwrap_or(LuaValue::Nil),
        JsonValue::String(value) => LuaValue::Str(vm.intern_str(value)),
        JsonValue::Array(_) | JsonValue::Object(_) => {
            LuaValue::Str(vm.intern_str(&value.to_string()))
        }
    }
}

fn global(vm: &mut Vm, name: &str) -> LuaValue {
    let key = LuaValue::Str(vm.intern_str(name));
    vm.globals().get(key)
}

fn lua_function(value: LuaValue) -> bool {
    matches!(value, LuaValue::Closure(_) | LuaValue::Native(_))
}

fn runtime_error(vm: &Vm, error: LuaError) -> ScriptRuntimeError {
    ScriptRuntimeError::new(vm.error_text(&error))
}

fn host_error(vm: &mut Vm, message: &str) -> LuaError {
    LuaError(LuaValue::Str(vm.intern_str(message)))
}

fn require_permission(vm: &mut Vm, api_name: &str) -> Result<(), LuaError> {
    let Some(permission) = PluginPermission::for_api(api_name) else {
        return Err(host_error(vm, "unknown AviQtl API permission"));
    };
    let allowed = ACTIVE_CONTEXT.with(|active| {
        active
            .borrow()
            .as_ref()
            .is_some_and(|context| context.granted.contains(&permission))
    });
    if allowed {
        Ok(())
    } else {
        ACTIVE_CONTEXT.with(|active| {
            if let Some(context) = active.borrow_mut().as_mut() {
                context.execution.diagnostics.push(format!(
                    "{} denied {}",
                    context.plugin_id,
                    permission.name()
                ));
            }
        });
        Err(host_error(
            vm,
            &format!("permission denied: {}", permission.name()),
        ))
    }
}

fn push_command(command: ScriptHostCommand) {
    ACTIVE_CONTEXT.with(|active| {
        if let Some(context) = active.borrow_mut().as_mut() {
            context.execution.commands.push(command);
        }
    });
}

fn snapshot<T>(read: impl FnOnce(&ScriptHostSnapshot) -> T) -> Option<T> {
    ACTIVE_CONTEXT.with(|active| {
        active
            .borrow()
            .as_ref()
            .map(|context| read(&context.snapshot))
    })
}

fn string_arg(vm: &mut Vm, function: u32, arguments: u32, index: u32) -> Result<String, LuaError> {
    match vm.nat_arg(function, arguments, index) {
        LuaValue::Str(value) => Ok(String::from_utf8_lossy(value.as_bytes()).into_owned()),
        _ => Err(host_error(vm, "string argument expected")),
    }
}

fn integer_arg(vm: &mut Vm, function: u32, arguments: u32, index: u32) -> Result<i32, LuaError> {
    let value = match vm.nat_arg(function, arguments, index) {
        LuaValue::Int(value) => value,
        LuaValue::Float(value) if value.is_finite() && value.fract() == 0.0 => value as i64,
        _ => return Err(host_error(vm, "integer argument expected")),
    };
    i32::try_from(value).map_err(|_| host_error(vm, "integer argument is out of range"))
}

fn json_arg(vm: &mut Vm, function: u32, arguments: u32, index: u32) -> JsonValue {
    match vm.nat_arg(function, arguments, index) {
        LuaValue::Nil => JsonValue::Null,
        LuaValue::Bool(value) => JsonValue::Bool(value),
        LuaValue::Int(value) => JsonValue::from(value),
        LuaValue::Float(value) => JsonValue::from(value),
        LuaValue::Str(value) => {
            JsonValue::String(String::from_utf8_lossy(value.as_bytes()).into_owned())
        }
        _ => JsonValue::Null,
    }
}

fn api_log(vm: &mut Vm, function: u32, arguments: u32) -> Result<u32, LuaError> {
    require_permission(vm, "log")?;
    push_command(ScriptHostCommand::Log(string_arg(
        vm, function, arguments, 0,
    )?));
    Ok(0)
}

fn api_transport_play(vm: &mut Vm, _: u32, _: u32) -> Result<u32, LuaError> {
    require_permission(vm, "transport_play")?;
    push_command(ScriptHostCommand::TransportPlay);
    Ok(0)
}

fn api_transport_pause(vm: &mut Vm, _: u32, _: u32) -> Result<u32, LuaError> {
    require_permission(vm, "transport_pause")?;
    push_command(ScriptHostCommand::TransportPause);
    Ok(0)
}

fn api_transport_toggle(vm: &mut Vm, _: u32, _: u32) -> Result<u32, LuaError> {
    require_permission(vm, "transport_toggle")?;
    push_command(ScriptHostCommand::TransportToggle);
    Ok(0)
}

fn api_transport_seek(vm: &mut Vm, function: u32, arguments: u32) -> Result<u32, LuaError> {
    require_permission(vm, "transport_seek")?;
    push_command(ScriptHostCommand::TransportSeek(integer_arg(
        vm, function, arguments, 0,
    )?));
    Ok(0)
}

fn api_transport_get_frame(vm: &mut Vm, function: u32, _: u32) -> Result<u32, LuaError> {
    require_permission(vm, "transport_get_frame")?;
    let frame = snapshot(|snapshot| snapshot.current_frame)
        .ok_or_else(|| host_error(vm, "script host context is unavailable"))?;
    Ok(vm.nat_return(function, &[LuaValue::Int(i64::from(frame))]))
}

fn api_transport_is_playing(vm: &mut Vm, function: u32, _: u32) -> Result<u32, LuaError> {
    require_permission(vm, "transport_is_playing")?;
    let playing = snapshot(|snapshot| snapshot.is_playing)
        .ok_or_else(|| host_error(vm, "script host context is unavailable"))?;
    Ok(vm.nat_return(function, &[LuaValue::Bool(playing)]))
}

fn api_clip_create(vm: &mut Vm, function: u32, arguments: u32) -> Result<u32, LuaError> {
    require_permission(vm, "clip_create")?;
    push_command(ScriptHostCommand::ClipCreate {
        clip_type: string_arg(vm, function, arguments, 0)?,
        start_frame: integer_arg(vm, function, arguments, 1)?,
        layer: integer_arg(vm, function, arguments, 2)?,
    });
    Ok(0)
}

fn api_clip_delete(vm: &mut Vm, function: u32, arguments: u32) -> Result<u32, LuaError> {
    require_permission(vm, "clip_delete")?;
    push_command(ScriptHostCommand::ClipDelete(integer_arg(
        vm, function, arguments, 0,
    )?));
    Ok(0)
}

fn api_clip_update(vm: &mut Vm, function: u32, arguments: u32) -> Result<u32, LuaError> {
    require_permission(vm, "clip_update")?;
    push_command(ScriptHostCommand::ClipUpdate {
        clip_id: integer_arg(vm, function, arguments, 0)?,
        layer: integer_arg(vm, function, arguments, 1)?,
        start_frame: integer_arg(vm, function, arguments, 2)?,
        duration: integer_arg(vm, function, arguments, 3)?,
    });
    Ok(0)
}

fn api_clip_select(vm: &mut Vm, function: u32, arguments: u32) -> Result<u32, LuaError> {
    require_permission(vm, "clip_select")?;
    push_command(ScriptHostCommand::ClipSelect(integer_arg(
        vm, function, arguments, 0,
    )?));
    Ok(0)
}

fn api_clip_split(vm: &mut Vm, function: u32, arguments: u32) -> Result<u32, LuaError> {
    require_permission(vm, "clip_split")?;
    push_command(ScriptHostCommand::ClipSplit {
        clip_id: integer_arg(vm, function, arguments, 0)?,
        frame: integer_arg(vm, function, arguments, 1)?,
    });
    Ok(0)
}

fn api_clip_copy(vm: &mut Vm, function: u32, arguments: u32) -> Result<u32, LuaError> {
    require_permission(vm, "clip_copy")?;
    push_command(ScriptHostCommand::ClipCopy(integer_arg(
        vm, function, arguments, 0,
    )?));
    Ok(0)
}

fn api_clip_cut(vm: &mut Vm, function: u32, arguments: u32) -> Result<u32, LuaError> {
    require_permission(vm, "clip_cut")?;
    push_command(ScriptHostCommand::ClipCut(integer_arg(
        vm, function, arguments, 0,
    )?));
    Ok(0)
}

fn api_clip_paste(vm: &mut Vm, function: u32, arguments: u32) -> Result<u32, LuaError> {
    require_permission(vm, "clip_paste")?;
    push_command(ScriptHostCommand::ClipPaste {
        frame: integer_arg(vm, function, arguments, 0)?,
        layer: integer_arg(vm, function, arguments, 1)?,
    });
    Ok(0)
}

fn api_clip_list(vm: &mut Vm, function: u32, _: u32) -> Result<u32, LuaError> {
    require_permission(vm, "clip_list")?;
    let clips = snapshot(|snapshot| snapshot.clips.clone())
        .ok_or_else(|| host_error(vm, "script host context is unavailable"))?;
    let items = clips
        .into_iter()
        .map(|clip| {
            vm.new_table()
                .with("id", i64::from(clip.id))
                .with("type", clip.clip_type)
                .with("layer", i64::from(clip.layer))
                .with("startFrame", i64::from(clip.start_frame))
                .with("duration", i64::from(clip.duration))
                .build()
        })
        .collect::<Vec<_>>();
    let mut list = vm.new_table();
    for (index, item) in items.into_iter().enumerate() {
        list = list.with((index + 1) as i64, LuaValue::Table(item));
    }
    let list = list.build();
    Ok(vm.nat_return(function, &[LuaValue::Table(list)]))
}

fn api_effect_add(vm: &mut Vm, function: u32, arguments: u32) -> Result<u32, LuaError> {
    require_permission(vm, "effect_add")?;
    push_command(ScriptHostCommand::EffectAdd {
        clip_id: integer_arg(vm, function, arguments, 0)?,
        effect_type: string_arg(vm, function, arguments, 1)?,
    });
    Ok(0)
}

fn api_effect_remove(vm: &mut Vm, function: u32, arguments: u32) -> Result<u32, LuaError> {
    require_permission(vm, "effect_remove")?;
    push_command(ScriptHostCommand::EffectRemove {
        clip_id: integer_arg(vm, function, arguments, 0)?,
        effect_index: integer_arg(vm, function, arguments, 1)?,
    });
    Ok(0)
}

fn api_effect_set_parameter(vm: &mut Vm, function: u32, arguments: u32) -> Result<u32, LuaError> {
    require_permission(vm, "effect_set_param")?;
    push_command(ScriptHostCommand::EffectSetParameter {
        clip_id: integer_arg(vm, function, arguments, 0)?,
        effect_index: integer_arg(vm, function, arguments, 1)?,
        name: string_arg(vm, function, arguments, 2)?,
        value: json_arg(vm, function, arguments, 3),
    });
    Ok(0)
}

fn api_project_width(vm: &mut Vm, function: u32, _: u32) -> Result<u32, LuaError> {
    require_permission(vm, "project_width")?;
    let value = snapshot(|snapshot| snapshot.project_width)
        .ok_or_else(|| host_error(vm, "script host context is unavailable"))?;
    Ok(vm.nat_return(function, &[LuaValue::Int(i64::from(value))]))
}

fn api_project_height(vm: &mut Vm, function: u32, _: u32) -> Result<u32, LuaError> {
    require_permission(vm, "project_height")?;
    let value = snapshot(|snapshot| snapshot.project_height)
        .ok_or_else(|| host_error(vm, "script host context is unavailable"))?;
    Ok(vm.nat_return(function, &[LuaValue::Int(i64::from(value))]))
}

fn api_project_fps(vm: &mut Vm, function: u32, _: u32) -> Result<u32, LuaError> {
    require_permission(vm, "project_fps")?;
    let value = snapshot(|snapshot| snapshot.project_fps)
        .ok_or_else(|| host_error(vm, "script host context is unavailable"))?;
    Ok(vm.nat_return(function, &[LuaValue::Float(value)]))
}

fn api_project_save(vm: &mut Vm, function: u32, arguments: u32) -> Result<u32, LuaError> {
    require_permission(vm, "project_save")?;
    push_command(ScriptHostCommand::ProjectSave(string_arg(
        vm, function, arguments, 0,
    )?));
    Ok(vm.nat_return(function, &[LuaValue::Bool(true)]))
}

fn api_project_load(vm: &mut Vm, function: u32, arguments: u32) -> Result<u32, LuaError> {
    require_permission(vm, "project_load")?;
    push_command(ScriptHostCommand::ProjectLoad(string_arg(
        vm, function, arguments, 0,
    )?));
    Ok(vm.nat_return(function, &[LuaValue::Bool(true)]))
}

fn api_undo(vm: &mut Vm, _: u32, _: u32) -> Result<u32, LuaError> {
    require_permission(vm, "undo")?;
    push_command(ScriptHostCommand::Undo);
    Ok(0)
}

fn api_redo(vm: &mut Vm, _: u32, _: u32) -> Result<u32, LuaError> {
    require_permission(vm, "redo")?;
    push_command(ScriptHostCommand::Redo);
    Ok(0)
}

fn api_settings_set(vm: &mut Vm, function: u32, arguments: u32) -> Result<u32, LuaError> {
    require_permission(vm, "settings_set")?;
    let key = string_arg(vm, function, arguments, 0)?;
    let value = string_arg(vm, function, arguments, 1)?;
    ACTIVE_CONTEXT.with(|active| {
        if let Some(context) = active.borrow_mut().as_mut() {
            context
                .snapshot
                .plugin_settings
                .insert(key.clone(), value.clone());
            context
                .execution
                .commands
                .push(ScriptHostCommand::SettingsSet { key, value });
        }
    });
    Ok(0)
}

fn api_settings_get(vm: &mut Vm, function: u32, arguments: u32) -> Result<u32, LuaError> {
    require_permission(vm, "settings_get")?;
    let key = string_arg(vm, function, arguments, 0)?;
    let value = snapshot(|snapshot| {
        snapshot
            .plugin_settings
            .get(&key)
            .cloned()
            .unwrap_or_default()
    })
    .ok_or_else(|| host_error(vm, "script host context is unavailable"))?;
    let value = LuaValue::Str(vm.intern_str(&value));
    Ok(vm.nat_return(function, &[value]))
}

fn api_scene_create(vm: &mut Vm, function: u32, arguments: u32) -> Result<u32, LuaError> {
    require_permission(vm, "scene_create")?;
    push_command(ScriptHostCommand::SceneCreate(string_arg(
        vm, function, arguments, 0,
    )?));
    Ok(0)
}

fn api_scene_remove(vm: &mut Vm, function: u32, arguments: u32) -> Result<u32, LuaError> {
    require_permission(vm, "scene_remove")?;
    push_command(ScriptHostCommand::SceneRemove(integer_arg(
        vm, function, arguments, 0,
    )?));
    Ok(0)
}

fn api_scene_switch(vm: &mut Vm, function: u32, arguments: u32) -> Result<u32, LuaError> {
    require_permission(vm, "scene_switch")?;
    push_command(ScriptHostCommand::SceneSwitch(integer_arg(
        vm, function, arguments, 0,
    )?));
    Ok(0)
}

fn api_command_begin_group(vm: &mut Vm, function: u32, arguments: u32) -> Result<u32, LuaError> {
    require_permission(vm, "command_begin_group")?;
    push_command(ScriptHostCommand::CommandBeginGroup(string_arg(
        vm, function, arguments, 0,
    )?));
    Ok(0)
}

fn api_command_end_group(vm: &mut Vm, _: u32, _: u32) -> Result<u32, LuaError> {
    require_permission(vm, "command_end_group")?;
    push_command(ScriptHostCommand::CommandEndGroup);
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::inspect_script_metadata;
    use serde_json::json;
    use std::fs;
    use std::path::Path;

    fn permissions(names: &[PluginPermission]) -> PluginPermissionState {
        let mut permissions = PluginPermissionState::default();
        for permission in names {
            permissions.set("plugin", *permission, true);
        }
        permissions
    }

    #[test]
    fn loads_sandboxed_plugins_and_dispatches_permission_checked_hooks() {
        let source = r#"
            local updates = 0
            aviqtl.log("top-level")
            function AviQtlOnLoad()
                aviqtl.log("loaded " .. tostring(rate))
            end
            function AviQtlUpdateHook()
                updates = updates + 1
                aviqtl.transport.seek(aviqtl.transport.get_frame() + updates)
            end
        "#;
        let mut parameters = BTreeMap::new();
        parameters.insert("rate".to_owned(), json!(3));
        let permissions = permissions(&[
            PluginPermission::LogOutput,
            PluginPermission::TransportControl,
        ]);
        let snapshot = ScriptHostSnapshot {
            current_frame: 10,
            ..ScriptHostSnapshot::default()
        };
        let (mut runtime, initial) = ScriptRuntime::load(
            "plugin",
            source,
            "plugin.lua",
            &parameters,
            &permissions,
            snapshot.clone(),
        )
        .expect("plugin loads");
        assert_eq!(
            initial.commands,
            vec![ScriptHostCommand::Log("top-level".to_owned())]
        );
        assert!(runtime.has_hook(&ScriptHook::Load));
        assert_eq!(
            runtime
                .dispatch(ScriptHook::Load, &permissions, snapshot.clone())
                .commands,
            vec![ScriptHostCommand::Log("loaded 3".to_owned())]
        );
        assert_eq!(
            runtime
                .dispatch(ScriptHook::Update, &permissions, snapshot)
                .commands,
            vec![ScriptHostCommand::TransportSeek(11)]
        );
    }

    #[test]
    fn denied_calls_are_catchable_and_do_not_emit_commands() {
        let source = r#"
            function AviQtlOnLoad()
                local ok = pcall(aviqtl.transport.play)
                if ok then error("permission unexpectedly granted") end
            end
        "#;
        let permissions = PluginPermissionState::default();
        let (mut runtime, _) = ScriptRuntime::load(
            "plugin",
            source,
            "plugin.lua",
            &BTreeMap::new(),
            &permissions,
            ScriptHostSnapshot::default(),
        )
        .expect("plugin loads");
        let execution = runtime.dispatch(
            ScriptHook::Load,
            &permissions,
            ScriptHostSnapshot::default(),
        );
        assert!(execution.commands.is_empty());
        assert_eq!(execution.diagnostics, ["plugin denied transport.control"]);
    }

    #[test]
    fn exposes_clip_project_and_scoped_settings_snapshots() {
        let source = r#"
            function AviQtlOnLoad()
                local clips = aviqtl.clip.list()
                aviqtl.log(string.format("%dx%d %.1f %s", aviqtl.project.width(), aviqtl.project.height(), aviqtl.project.fps(), clips[1].type))
                aviqtl.settings.set("greeting", "hello")
                aviqtl.log(aviqtl.settings.get("greeting"))
            end
        "#;
        let permissions = permissions(&[
            PluginPermission::ClipRead,
            PluginPermission::ProjectRead,
            PluginPermission::SettingsRead,
            PluginPermission::SettingsWrite,
            PluginPermission::LogOutput,
        ]);
        let snapshot = ScriptHostSnapshot {
            project_width: 1280,
            project_height: 720,
            project_fps: 60.0,
            clips: vec![ScriptClipSnapshot {
                id: 7,
                clip_type: "text".to_owned(),
                layer: 2,
                start_frame: 10,
                duration: 30,
            }],
            ..ScriptHostSnapshot::default()
        };
        let (mut runtime, _) = ScriptRuntime::load(
            "plugin",
            source,
            "plugin.lua",
            &BTreeMap::new(),
            &permissions,
            snapshot.clone(),
        )
        .expect("plugin loads");
        let execution = runtime.dispatch(ScriptHook::Load, &permissions, snapshot);
        assert_eq!(
            execution.commands,
            vec![
                ScriptHostCommand::Log("1280x720 60.0 text".to_owned()),
                ScriptHostCommand::SettingsSet {
                    key: "greeting".to_owned(),
                    value: "hello".to_owned(),
                },
                ScriptHostCommand::Log("hello".to_owned()),
            ]
        );
    }

    #[test]
    fn rejects_unsafe_globals_and_bounds_untrusted_execution() {
        let permissions = PluginPermissionState::default();
        let forbidden = r#"
            assert(os == nil and io == nil and debug == nil and package == nil and ffi == nil)
            assert(load == nil and loadstring == nil and loadfile == nil and dofile == nil)
            while true do end
        "#;
        let error = ScriptRuntime::load(
            "plugin",
            forbidden,
            "untrusted.lua",
            &BTreeMap::new(),
            &permissions,
            ScriptHostSnapshot::default(),
        )
        .err()
        .expect("instruction budget rejects the script");
        assert!(error.to_string().contains("instruction budget"));
    }

    #[test]
    fn repository_plugins_load_and_run_their_load_hooks() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins");
        let mut permissions = PluginPermissionState::default();
        permissions.grant_all("plugin");
        for directory in [
            "example_animation",
            "example_transport",
            "example_project_info",
            "example_clip_ops",
        ] {
            let path = root.join(directory).join("main.lua");
            let source = fs::read_to_string(&path).expect("repository plugin source");
            let parameters = inspect_script_metadata(&source)
                .parameters
                .into_iter()
                .map(|parameter| (parameter.var_name, parameter.default_value))
                .collect();
            let (mut runtime, _) = ScriptRuntime::load(
                "plugin",
                &source,
                &path.display().to_string(),
                &parameters,
                &permissions,
                ScriptHostSnapshot::default(),
            )
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let execution = runtime.dispatch(
                ScriptHook::Load,
                &permissions,
                ScriptHostSnapshot::default(),
            );
            assert!(
                execution.diagnostics.is_empty(),
                "{}: {:?}",
                path.display(),
                execution.diagnostics
            );
        }
    }
}
