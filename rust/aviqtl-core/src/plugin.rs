use crate::abi::{
    STATUS_BUFFER_TOO_SMALL, STATUS_INVALID_ARGUMENT, STATUS_INVALID_JSON, STATUS_OK,
    STATUS_OVERLAPPING_BUFFERS, ranges_overlap, slice_is_valid,
};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;

fn text(value: Option<&Value>) -> String {
    value.and_then(Value::as_str).unwrap_or_default().to_owned()
}

fn integer(value: Option<&Value>) -> i64 {
    value.and_then(Value::as_i64).unwrap_or_default()
}

fn objects(value: Option<&Value>) -> Vec<Map<String, Value>> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .cloned()
        .collect()
}

fn normalize_category(category: &str) -> String {
    let category = category.trim();
    let lower = category.to_lowercase();
    match lower.as_str() {
        "synth" | "instrument" => "Synth".to_owned(),
        "delay" | "reverb" => "Delay".to_owned(),
        "eq" => "EQ".to_owned(),
        "filter" => "Filter".to_owned(),
        "distortion" => "Distortion".to_owned(),
        "dynamics" => "Dynamics".to_owned(),
        "modulator" | "modulation" => "Modulator".to_owned(),
        "utility" | "tools" | "tool" => "Utility".to_owned(),
        "" | "other" | "unknown" | "misc" | "none" | "null" => "Other".to_owned(),
        _ => {
            let mut characters = lower.chars();
            match characters.next() {
                Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
                None => "Other".to_owned(),
            }
        }
    }
}

fn carla_category(category: i32) -> &'static str {
    match category {
        1 => "Synth",
        2 => "Delay",
        3 => "EQ",
        4 => "Filter",
        5 => "Distortion",
        6 => "Dynamics",
        7 => "Modulator",
        8 => "Utility",
        _ => "Other",
    }
}

fn category_rank(category: &str) -> i32 {
    match normalize_category(category).as_str() {
        "Filter" => 0,
        "EQ" => 1,
        "Dynamics" => 2,
        "Delay" => 3,
        "Distortion" => 4,
        "Modulator" => 5,
        "Utility" => 6,
        "Synth" => 7,
        _ => 100,
    }
}

fn normalize_plugin(mut plugin: Map<String, Value>, fallback_name: &str) -> Map<String, Value> {
    let label = text(plugin.get("label")).trim().to_owned();
    let mut name = text(plugin.get("name")).trim().to_owned();
    if name.is_empty() {
        name = if label.is_empty() {
            fallback_name.trim().to_owned()
        } else {
            label.clone()
        };
    }
    let label = if label.is_empty() {
        name.clone()
    } else {
        label
    };
    let category = normalize_category(&text(plugin.get("category")));
    plugin.insert("name".to_owned(), Value::String(name));
    plugin.insert("label".to_owned(), Value::String(label));
    plugin.insert("category".to_owned(), Value::String(category));
    plugin
}

fn finish_discovery_plugin(
    mut plugin: Map<String, Value>,
    fallback_name: &str,
) -> Option<Map<String, Value>> {
    plugin = normalize_plugin(plugin, fallback_name);
    let name = text(plugin.get("name"));
    if name.is_empty() {
        return None;
    }
    let format = text(plugin.get("format"));
    let label = text(plugin.get("label"));
    let unique_id = integer(plugin.get("uniqueId"));
    plugin.insert(
        "id".to_owned(),
        Value::String(format!("{format}:{label}:{unique_id}")),
    );
    Some(plugin)
}

fn parse_discovery_output(
    output: &str,
    format: &str,
    file_path: &str,
    fallback_name: &str,
) -> Vec<Map<String, Value>> {
    let mut plugins = Vec::new();
    let mut current = Map::new();
    let mut in_block = false;
    for raw_line in output.lines() {
        let line = raw_line.trim();
        let Some(payload) = line.strip_prefix("carla-discovery::") else {
            continue;
        };
        let mut parts = payload.split("::");
        let key = parts.next().unwrap_or_default();
        let value = parts.collect::<Vec<_>>().join("::");
        match key {
            "begin" | "init" => {
                current = Map::new();
                current.insert("format".to_owned(), Value::String(format.to_owned()));
                current.insert("path".to_owned(), Value::String(file_path.to_owned()));
                current.insert("uniqueId".to_owned(), Value::from(0));
                current.insert("index".to_owned(), Value::from(0));
                current.insert("audioIns".to_owned(), Value::from(0));
                current.insert("audioOuts".to_owned(), Value::from(0));
                in_block = true;
            }
            _ if !in_block => {}
            "name" | "label" | "maker" => {
                current.insert(key.to_owned(), Value::String(value));
            }
            "uniqueId" => {
                current.insert(
                    "uniqueId".to_owned(),
                    Value::from(value.trim().parse::<i64>().unwrap_or_default()),
                );
            }
            "category" => {
                let trimmed = value.trim();
                let category = trimmed
                    .parse::<i32>()
                    .ok()
                    .map(carla_category)
                    .unwrap_or(trimmed);
                current.insert(
                    "category".to_owned(),
                    Value::String(normalize_category(category)),
                );
            }
            "audio.ins" => {
                current.insert(
                    "audioIns".to_owned(),
                    Value::from(value.trim().parse::<i32>().unwrap_or_default()),
                );
            }
            "audio.outs" => {
                current.insert(
                    "audioOuts".to_owned(),
                    Value::from(value.trim().parse::<i32>().unwrap_or_default()),
                );
            }
            "end" => {
                if let Some(plugin) = finish_discovery_plugin(current, fallback_name) {
                    plugins.push(plugin);
                }
                current = Map::new();
                in_block = false;
            }
            _ => {}
        }
    }
    plugins
}

fn deduplicate(plugins: Vec<Map<String, Value>>) -> Vec<Map<String, Value>> {
    let mut ids = BTreeSet::new();
    plugins
        .into_iter()
        .filter(|plugin| {
            let id = text(plugin.get("id"));
            !id.is_empty() && ids.insert(id)
        })
        .collect()
}

fn public_plugin(plugin: &Map<String, Value>, include_io: bool) -> Map<String, Value> {
    let mut result = Map::new();
    for key in ["id", "name", "format", "category", "maker"] {
        result.insert(key.to_owned(), plugin.get(key).cloned().unwrap_or_default());
    }
    if include_io {
        result.insert(
            "audioIns".to_owned(),
            plugin.get("audioIns").cloned().unwrap_or(Value::from(0)),
        );
        result.insert(
            "audioOuts".to_owned(),
            plugin.get("audioOuts").cloned().unwrap_or(Value::from(0)),
        );
    }
    result
}

fn categories(plugins: &[Map<String, Value>]) -> Vec<Value> {
    let mut categories = BTreeSet::new();
    for plugin in plugins {
        categories.insert(normalize_category(&text(plugin.get("category"))));
    }
    let mut categories: Vec<_> = categories.into_iter().collect();
    categories.sort_by(|left, right| {
        category_rank(left)
            .cmp(&category_rank(right))
            .then_with(|| left.to_lowercase().cmp(&right.to_lowercase()))
    });
    categories.into_iter().map(Value::String).collect()
}

fn filtered_plugins(plugins: Vec<Map<String, Value>>, category: &str) -> Vec<Value> {
    let wanted = normalize_category(category);
    let mut plugins: Vec<_> = plugins
        .into_iter()
        .filter(|plugin| normalize_category(&text(plugin.get("category"))) == wanted)
        .collect();
    plugins.sort_by_key(|plugin| text(plugin.get("name")).to_lowercase());
    plugins
        .iter()
        .map(|plugin| Value::Object(public_plugin(plugin, false)))
        .collect()
}

fn apply(input: &Map<String, Value>) -> Option<Map<String, Value>> {
    match text(input.get("operation")).as_str() {
        "parseDiscovery" => Some(
            json!({
                "plugins": parse_discovery_output(
                    &text(input.get("output")),
                    &text(input.get("format")),
                    &text(input.get("filePath")),
                    &text(input.get("fallbackName")),
                )
            })
            .as_object()
            .cloned()?,
        ),
        "deduplicate" => Some(
            json!({"plugins": deduplicate(objects(input.get("plugins")))})
                .as_object()
                .cloned()?,
        ),
        "list" => Some(
            json!({
                "plugins": objects(input.get("plugins"))
                    .iter()
                    .map(|plugin| Value::Object(public_plugin(plugin, true)))
                    .collect::<Vec<_>>()
            })
            .as_object()
            .cloned()?,
        ),
        "categories" => Some(
            json!({"categories": categories(&objects(input.get("plugins")))})
                .as_object()
                .cloned()?,
        ),
        "filter" => Some(
            json!({
                "plugins": filtered_plugins(
                    objects(input.get("plugins")),
                    &text(input.get("category"))
                )
            })
            .as_object()
            .cloned()?,
        ),
        _ => None,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_plugin_document_apply_json(
    input: *const u8,
    input_length: usize,
    output: *mut u8,
    output_capacity: usize,
    output_length: *mut usize,
) -> u32 {
    if !slice_is_valid(input, input_length)
        || !slice_is_valid(output, output_capacity)
        || !slice_is_valid(output_length, 1)
    {
        return STATUS_INVALID_ARGUMENT;
    }
    let overlaps = [
        ranges_overlap(input, input_length, output, output_capacity),
        ranges_overlap(input, input_length, output_length, 1),
        ranges_overlap(output, output_capacity, output_length, 1),
    ];
    if overlaps.iter().any(Option::is_none) {
        return STATUS_INVALID_ARGUMENT;
    }
    if overlaps.into_iter().flatten().any(|overlap| overlap) {
        return STATUS_OVERLAPPING_BUFFERS;
    }
    let input = if input_length == 0 {
        &[]
    } else {
        // SAFETY: The input range was validated and checked against every output.
        unsafe { std::slice::from_raw_parts(input, input_length) }
    };
    let Some(input) = serde_json::from_slice::<Value>(input)
        .ok()
        .and_then(|value| value.as_object().cloned())
    else {
        return STATUS_INVALID_JSON;
    };
    let Some(result) = apply(&input) else {
        return STATUS_INVALID_ARGUMENT;
    };
    let Ok(json) = serde_json::to_vec(&result) else {
        return STATUS_INVALID_JSON;
    };
    // SAFETY: The output length was validated and checked against the other ranges.
    unsafe { output_length.write(json.len()) };
    if output_capacity < json.len() {
        return STATUS_BUFFER_TOO_SMALL;
    }
    if !json.is_empty() {
        // SAFETY: The output range has sufficient capacity and is disjoint from the inputs.
        let output = unsafe { std::slice::from_raw_parts_mut(output, output_capacity) };
        output[..json.len()].copy_from_slice(&json);
    }
    STATUS_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_parser_normalizes_complete_plugin_blocks() {
        let output = "ignored\n\
carla-discovery::init\n\
carla-discovery::name::  \n\
carla-discovery::label::Example::Stereo\n\
carla-discovery::maker::AviQtl\n\
carla-discovery::uniqueId::42\n\
carla-discovery::category::4\n\
carla-discovery::audio.ins::2\n\
carla-discovery::audio.outs::2\n\
carla-discovery::end";
        let plugins = parse_discovery_output(output, "VST3", "/tmp/example.vst3", "example");
        assert_eq!(plugins.len(), 1);
        assert_eq!(text(plugins[0].get("name")), "Example::Stereo");
        assert_eq!(text(plugins[0].get("category")), "Filter");
        assert_eq!(text(plugins[0].get("id")), "VST3:Example::Stereo:42");
        assert_eq!(integer(plugins[0].get("audioIns")), 2);

        let uncategorized = parse_discovery_output(
            "carla-discovery::init\ncarla-discovery::name::No Category\ncarla-discovery::category::0\ncarla-discovery::end",
            "VST2",
            "/tmp/no-category.so",
            "no-category",
        );
        assert_eq!(text(uncategorized[0].get("category")), "Other");
    }

    #[test]
    fn catalog_deduplication_categories_and_filtering_are_stable() {
        let plugins = objects(Some(&json!([
            {"id":"first","name":"Zulu","category":"reverb","format":"LV2"},
            {"id":"second","name":"Alpha","category":"delay","format":"VST3"},
            {"id":"first","name":"Duplicate","category":"filter","format":"VST2"},
            {"id":"third","name":"EQ","category":"eq","format":"LV2"},
            {"id":"fourth","name":"Custom One","category":"Custom","format":"LV2"},
            {"id":"fifth","name":"Custom Two","category":"custom","format":"LV2"}
        ])));
        let plugins = deduplicate(plugins);
        assert_eq!(plugins.len(), 5);
        assert_eq!(
            categories(&plugins),
            vec![json!("EQ"), json!("Delay"), json!("Custom")]
        );
        let filtered = filtered_plugins(plugins.clone(), "Delay");
        assert_eq!(
            filtered
                .iter()
                .filter_map(|plugin| plugin.get("name").and_then(Value::as_str))
                .collect::<Vec<_>>(),
            ["Alpha", "Zulu"]
        );
        assert_eq!(filtered_plugins(plugins, "CUSTOM").len(), 2);
    }
}
