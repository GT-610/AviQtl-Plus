use crate::abi::{
    STATUS_BUFFER_TOO_SMALL, STATUS_INVALID_ARGUMENT, STATUS_INVALID_JSON, STATUS_OK,
    STATUS_OVERLAPPING_BUFFERS, ranges_overlap, slice_is_valid,
};
use serde::Serialize;
use serde_json::{Number, Value};

const DEFAULT_COLOR: u32 = 0x00ff_ffff;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
enum ParameterType {
    Track,
    Check,
    Color,
    Select,
    Text,
    String,
    File,
    Folder,
    Value,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ParameterOption {
    label: String,
    value: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Parameter {
    #[serde(rename = "type")]
    parameter_type: ParameterType,
    var_name: String,
    label: String,
    default_value: Value,
    min_value: Value,
    max_value: Value,
    step: Value,
    options: Vec<ParameterOption>,
    group_name: String,
    is_section_check: bool,
}

impl Parameter {
    fn new(parameter_type: ParameterType) -> Self {
        Self {
            parameter_type,
            var_name: String::new(),
            label: String::new(),
            default_value: Value::Null,
            min_value: Value::Null,
            max_value: Value::Null,
            step: Value::Null,
            options: Vec::new(),
            group_name: String::new(),
            is_section_check: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Group {
    name: String,
    default_expanded: bool,
    params: Vec<Parameter>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct Metadata {
    information: String,
    script_type: String,
    require_version: i32,
    is_filter: bool,
    label: String,
    params: Vec<Parameter>,
    groups: Vec<Group>,
}

type ParameterParser = fn(&str) -> Parameter;

fn finite_number(value: &str) -> f64 {
    value
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .unwrap_or(0.0)
}

fn number(value: f64) -> Value {
    Number::from_f64(value).map_or(Value::Null, Value::Number)
}

fn split_var_label(value: &str) -> (String, String) {
    if let Some(index) = value.find(':').filter(|index| *index > 0) {
        (
            value[..index].trim().to_owned(),
            value[index + 1..].trim().to_owned(),
        )
    } else {
        let value = value.trim().to_owned();
        (value.clone(), value)
    }
}

fn parse_track(definition: &str) -> Parameter {
    let mut parameter = Parameter::new(ParameterType::Track);
    let parts: Vec<_> = definition.split(',').collect();
    if parts.len() < 4 {
        return parameter;
    }
    (parameter.var_name, parameter.label) = split_var_label(parts[0]);
    let minimum = finite_number(parts[1]);
    let maximum = finite_number(parts[2]);
    parameter.min_value = number(minimum);
    parameter.max_value = number(maximum);
    parameter.default_value = number(finite_number(parts[3]));
    parameter.step = if let Some(step) = parts.get(4) {
        number(finite_number(step))
    } else {
        number(match maximum - minimum {
            range if range <= 10.0 => 0.01,
            range if range <= 100.0 => 0.1,
            _ => 1.0,
        })
    };
    parameter
}

fn parse_check(definition: &str) -> Parameter {
    let mut parameter = Parameter::new(ParameterType::Check);
    let Some(colon) = definition.find(':') else {
        return parameter;
    };
    parameter.var_name = definition[..colon].trim().to_owned();
    let rest = definition[colon + 1..].trim();
    if let Some(comma) = rest.rfind(',').filter(|comma| *comma > 0) {
        parameter.label = rest[..comma].trim().to_owned();
        let value = rest[comma + 1..].trim().to_ascii_lowercase();
        parameter.default_value = Value::Bool(matches!(value.as_str(), "true" | "1"));
    } else {
        parameter.label = rest.to_owned();
        parameter.default_value = Value::Bool(false);
    }
    parameter
}

fn parse_color(definition: &str) -> Parameter {
    let mut parameter = Parameter::new(ParameterType::Color);
    let Some(colon) = definition.find(':') else {
        return parameter;
    };
    parameter.var_name = definition[..colon].trim().to_owned();
    let rest = definition[colon + 1..].trim();
    if let Some(comma) = rest.rfind(',').filter(|comma| *comma > 0) {
        parameter.label = rest[..comma].trim().to_owned();
        let color = u32::from_str_radix(rest[comma + 1..].trim(), 16).unwrap_or(DEFAULT_COLOR);
        parameter.default_value = Value::Number(color.into());
    } else {
        parameter.label = rest.to_owned();
        parameter.default_value = Value::Number(DEFAULT_COLOR.into());
    }
    parameter
}

fn parse_option_value(value: &str) -> Value {
    let value = value.trim();
    if let Ok(integer) = value.parse::<i32>() {
        return Value::Number(integer.into());
    }
    if let Ok(floating) = value.parse::<f64>() {
        if floating.is_finite() {
            if let Some(number) = Number::from_f64(floating) {
                return Value::Number(number);
            }
        }
    }
    match value.to_ascii_lowercase().as_str() {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        _ => Value::String(value.to_owned()),
    }
}

fn parse_select(definition: &str) -> Parameter {
    let mut parameter = Parameter::new(ParameterType::Select);
    let Some(colon) = definition.find(':') else {
        return parameter;
    };
    parameter.var_name = definition[..colon].trim().to_owned();
    let mut rest = definition[colon + 1..].trim();
    let mut default_token = String::new();
    let equals = rest.find('=');
    let first_comma = rest.find(',');
    if let Some(equals) =
        equals.filter(|equals| *equals > 0 && first_comma.is_none_or(|comma| *equals < comma))
    {
        parameter.label = rest[..equals].trim().to_owned();
        let end = first_comma.unwrap_or(rest.len());
        default_token = rest[equals + 1..end].trim().to_owned();
        rest = first_comma.map_or("", |comma| rest[comma + 1..].trim());
    }

    let mut first = true;
    for option in rest.split(',') {
        let Some(equals) = option.find('=').filter(|equals| *equals > 0) else {
            continue;
        };
        let label = option[..equals].trim().to_owned();
        let value_text = option[equals + 1..].trim();
        let value = parse_option_value(value_text);
        if (!default_token.is_empty() && (default_token == label || default_token == value_text))
            || (first && default_token.is_empty())
        {
            parameter.default_value = value.clone();
        }
        parameter.options.push(ParameterOption { label, value });
        first = false;
    }
    parameter
}

fn parse_text(definition: &str, parameter_type: ParameterType) -> Parameter {
    let mut parameter = Parameter::new(parameter_type);
    let Some(colon) = definition.find(':') else {
        return parameter;
    };
    parameter.var_name = definition[..colon].trim().to_owned();
    let rest = definition[colon + 1..].trim();
    if let Some(comma) = rest.rfind(',').filter(|comma| *comma > 0) {
        parameter.label = rest[..comma].trim().to_owned();
        parameter.default_value = Value::String(rest[comma + 1..].trim().to_owned());
    } else {
        parameter.label = rest.to_owned();
        parameter.default_value = Value::String(String::new());
    }
    parameter
}

fn parse_path(definition: &str, parameter_type: ParameterType) -> Parameter {
    let mut parameter = Parameter::new(parameter_type);
    (parameter.var_name, parameter.label) = split_var_label(definition);
    parameter
}

fn parse_value(definition: &str) -> Parameter {
    let mut parameter = Parameter::new(ParameterType::Value);
    let Some(colon) = definition.find(':') else {
        return parameter;
    };
    parameter.var_name = definition[..colon].trim().to_owned();
    let rest = definition[colon + 1..].trim();
    if let Some(comma) = rest.rfind(',').filter(|comma| *comma > 0) {
        parameter.label = rest[..comma].trim().to_owned();
        parameter.default_value = Value::String(rest[comma + 1..].trim().to_owned());
    } else {
        parameter.label = rest.to_owned();
    }
    parameter
}

fn parse_parameter(line: &str) -> Option<Parameter> {
    let mappings: [(&str, ParameterParser); 5] = [
        ("--track@", parse_track),
        ("--check@", parse_check),
        ("--color@", parse_color),
        ("--select@", parse_select),
        ("--value@", parse_value),
    ];
    for (prefix, parser) in mappings {
        if let Some(definition) = line.strip_prefix(prefix) {
            return Some(parser(definition));
        }
    }
    if let Some(definition) = line.strip_prefix("--text@") {
        return Some(parse_text(definition, ParameterType::Text));
    }
    if let Some(definition) = line.strip_prefix("--string@") {
        return Some(parse_text(definition, ParameterType::String));
    }
    if let Some(definition) = line.strip_prefix("--file@") {
        return Some(parse_path(definition, ParameterType::File));
    }
    if let Some(definition) = line.strip_prefix("--folder@") {
        return Some(parse_path(definition, ParameterType::Folder));
    }
    None
}

fn parse_metadata(input: &str) -> Metadata {
    let mut metadata = Metadata::default();
    let mut current_group = String::new();
    for line in input.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if !line.starts_with("--") {
            break;
        }
        if let Some(value) = line.strip_prefix("--information:") {
            metadata.information = value.trim().to_owned();
            continue;
        }
        if let Some(value) = line.strip_prefix("--script:") {
            metadata.script_type = value.trim().to_ascii_lowercase();
            continue;
        }
        if let Some(value) = line.strip_prefix("--require:") {
            metadata.require_version = value.trim().parse::<i32>().unwrap_or(0);
            continue;
        }
        if line == "--filter" {
            metadata.is_filter = true;
            continue;
        }
        if let Some(value) = line.strip_prefix("--label:") {
            metadata.label = value.trim().to_owned();
            continue;
        }
        if let Some(value) = line.strip_prefix("--group:") {
            let definition = value.trim();
            if definition.is_empty() {
                current_group.clear();
            } else {
                let mut parts = definition.split(',');
                current_group = parts.next().unwrap_or_default().trim().to_owned();
                let default_expanded = parts
                    .next()
                    .is_none_or(|value| value.trim().eq_ignore_ascii_case("true"));
                if let Some(group) = metadata
                    .groups
                    .iter_mut()
                    .find(|group| group.name == current_group)
                {
                    group.default_expanded = default_expanded;
                } else {
                    metadata.groups.push(Group {
                        name: current_group.clone(),
                        default_expanded,
                        params: Vec::new(),
                    });
                }
            }
            continue;
        }
        if line.starts_with("--separator:") {
            continue;
        }
        let Some(mut parameter) = parse_parameter(line) else {
            continue;
        };
        if parameter.var_name.is_empty() {
            continue;
        }
        parameter.group_name = current_group.clone();
        metadata.params.push(parameter.clone());
        if let Some(group) = metadata
            .groups
            .iter_mut()
            .find(|group| group.name == current_group && !current_group.is_empty())
        {
            group.params.push(parameter);
        }
    }
    metadata
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aviqtl_script_metadata_parse_json(
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
        // SAFETY: The input range was validated and de-overlapped above.
        unsafe { std::slice::from_raw_parts(input, input_length) }
    };
    let Ok(input) = std::str::from_utf8(input) else {
        return STATUS_INVALID_ARGUMENT;
    };
    let Ok(json) = serde_json::to_vec(&parse_metadata(input)) else {
        return STATUS_INVALID_JSON;
    };
    // SAFETY: The output-length pointer was validated and de-overlapped above.
    unsafe { output_length.write(json.len()) };
    if output_capacity < json.len() {
        return STATUS_BUFFER_TOO_SMALL;
    }
    if !json.is_empty() {
        // SAFETY: The output range was validated and has sufficient capacity.
        let output = unsafe { std::slice::from_raw_parts_mut(output, output_capacity) };
        output[..json.len()].copy_from_slice(&json);
    }
    STATUS_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_groups_and_typed_parameters() {
        let metadata = parse_metadata(
            r#"
--information:Example
--script:LuaJIT
--require:5
--filter
--group:Playback,false
--track@interval:Interval,1,120,60
--check@enabled:Enabled,true
--select@level:Level=warning,debug=0,warning=2,verbose=true
--color@color:Color,ff00aa
--group:
--string@name:Name,Example
local loaded = true
"#,
        );
        assert_eq!(metadata.information, "Example");
        assert_eq!(metadata.script_type, "luajit");
        assert_eq!(metadata.require_version, 5);
        assert!(metadata.is_filter);
        assert_eq!(metadata.params.len(), 5);
        assert_eq!(metadata.groups.len(), 1);
        assert!(!metadata.groups[0].default_expanded);
        assert_eq!(metadata.groups[0].params.len(), 4);
        assert_eq!(metadata.params[0].default_value, number(60.0));
        assert_eq!(metadata.params[2].default_value, Value::Number(2.into()));
        assert_eq!(metadata.params[2].options[2].value, Value::Bool(true));
        assert_eq!(
            metadata.params[3].default_value,
            Value::Number(0xff00aa_u32.into())
        );
    }

    #[test]
    fn stops_at_first_non_comment_line() {
        let metadata = parse_metadata("--track@a:A,0,1,0\nlocal x = 1\n--check@b:B,true");
        assert_eq!(metadata.params.len(), 1);
        assert_eq!(metadata.params[0].var_name, "a");
    }

    #[test]
    fn repeated_groups_are_reused_and_invalid_colors_use_the_default() {
        let metadata = parse_metadata(
            "--group:Shared,true\n--track@first:First,0,1,0\n--group:Shared,false\n--color@color:Color,not-hex",
        );
        assert_eq!(metadata.groups.len(), 1);
        assert!(!metadata.groups[0].default_expanded);
        assert_eq!(metadata.groups[0].params.len(), 2);
        assert_eq!(
            metadata.params[1].default_value,
            Value::Number(DEFAULT_COLOR.into())
        );
    }
}
