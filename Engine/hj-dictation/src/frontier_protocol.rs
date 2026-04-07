use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use serde_json::{Map, Number, Value};

use crate::config::ContextConfig;
use crate::engine_state::ContextSnapshot;

pub(crate) const DEFAULT_FRONTIER_WS_URL: &str =
    "wss://frontier-audio-ime-ws.doubao.com/ocean/api/v1/ws?aid=685343&app_name=oime_macos&app_version=0.5";
pub(crate) const DEFAULT_APP_KEY: &str = "OrnqKvSSrs";
pub(crate) const DEFAULT_CONTEXT_APP_NAME: &str = "ime";
pub(crate) const DEFAULT_STANDALONE_APP_NAME: &str = "com.apple.Terminal";
const DEFAULT_DEVICE_ID: &str = "4285264416738169";
const DEFAULT_DEVICE_MODEL: &str = "MacBookAir10,1";
const DEFAULT_APP_VERSION: &str = "0.5.7";
const DEFAULT_VID: &str = "88BF8056-4530-4CD7-9263-3CEA1FF4E411";
const DEFAULT_LOC_IP: &str = "192.168.2.253";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FrontierRuntimeContext {
    pub(crate) app_bundle_id: Option<String>,
    pub(crate) text_context_text: Option<String>,
    pub(crate) text_context_cursor_position: Option<usize>,
    pub(crate) capture_ms: u64,
    pub(crate) source: String,
}

impl FrontierRuntimeContext {
    pub(crate) fn from_context_snapshot(snapshot: &ContextSnapshot) -> Self {
        let app_bundle_id = snapshot
            .frontmost_bundle_id
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let text_context_text = {
            let combined = format!(
                "{}{}",
                snapshot.text_before_cursor, snapshot.text_after_cursor
            );
            if combined.is_empty() {
                None
            } else {
                Some(combined)
            }
        };
        let text_context_cursor_position = text_context_text.as_ref().map(|text| {
            snapshot
                .cursor_position
                .min(text.chars().count())
        });
        Self {
            app_bundle_id,
            text_context_text,
            text_context_cursor_position,
            capture_ms: snapshot.captured_at_ms,
            source: if snapshot.capture_source.trim().is_empty() {
                "host_update".to_string()
            } else {
                snapshot.capture_source.clone()
            },
        }
    }

    pub(crate) fn text_available(&self) -> bool {
        self.text_context_text.is_some() && self.text_context_cursor_position.is_some()
    }
}

pub(crate) fn build_effective_request_profile(
    config: &ContextConfig,
    runtime_context: Option<&FrontierRuntimeContext>,
) -> Value {
    let app_name = runtime_context
        .and_then(|context| context.app_bundle_id.clone())
        .unwrap_or_else(|| DEFAULT_CONTEXT_APP_NAME.to_string());
    let (text_value, cursor_position) = effective_text_context(config, runtime_context);
    let merged_hotwords = dedupe_preserve_order(
        config
            .hotwords
            .iter()
            .chain(config.user_terms.iter())
            .cloned(),
    );

    object([
        ("app_name", string_value(app_name)),
        (
            "payload",
            object([
                (
                    "enable_punctuation",
                    Value::Bool(config.recognition.enable_punctuation),
                ),
                (
                    "enable_speech_rejection",
                    Value::Bool(config.recognition.enable_speech_rejection),
                ),
            ]),
        ),
        (
            "context",
            object([
                (
                    "hotwords",
                    Value::Array(
                        merged_hotwords
                            .into_iter()
                            .map(|word| object([("word", string_value(word))]))
                            .collect(),
                    ),
                ),
                (
                    "chat",
                    Value::Array(vec![object([(
                        "type",
                        string_value("user_input"),
                    ), (
                        "data_json",
                        object([
                            ("text", string_value(text_value)),
                            (
                                "cursor_position",
                                number_value(cursor_position as u64),
                            ),
                        ]),
                    )])]),
                ),
                (
                    "ime_info",
                    object([(
                        "inputType",
                        string_value(config.ime_context.input_type.clone()),
                    )]),
                ),
                ("loc_info", empty_object()),
                ("trackingInfo", empty_object()),
            ]),
        ),
        (
            "extra",
            object([
                (
                    "disable_user_words",
                    Value::Bool(!config.advanced.use_user_dictionary),
                ),
                (
                    "enable_text_filter",
                    Value::Bool(config.advanced.enable_text_filter),
                ),
                (
                    "enable_asr_twopass",
                    Value::Bool(config.advanced.enable_asr_twopass),
                ),
                (
                    "enable_asr_threepass",
                    Value::Bool(config.advanced.enable_asr_threepass),
                ),
                (
                    "remove_space_between_han_eng",
                    Value::Bool(config.advanced.remove_space_between_han_eng),
                ),
                (
                    "remove_space_between_han_num",
                    Value::Bool(config.advanced.remove_space_between_han_num),
                ),
                ("strong_ddc", Value::Bool(config.advanced.strong_ddc)),
            ]),
        ),
    ])
}

pub(crate) fn build_start_task(
    session_id: &str,
    token: Option<&str>,
    app_key: &str,
) -> Vec<u8> {
    let mut msg = Vec::new();
    if let Some(token) = token {
        push_length_delimited(&mut msg, 1, token.as_bytes());
    }
    push_length_delimited(&mut msg, 2, app_key.as_bytes());
    push_length_delimited(&mut msg, 3, b"ASR");
    push_length_delimited(&mut msg, 5, b"StartTask");
    push_length_delimited(&mut msg, 8, session_id.as_bytes());
    msg
}

pub(crate) fn build_start_session_payload(
    audio_format: &str,
    request_profile: Option<&Value>,
    app_name: Option<&str>,
    now_unix_secs: u64,
) -> Result<(Value, Value), String> {
    let resolved_app_name = app_name
        .map(ToOwned::to_owned)
        .or_else(|| request_profile.and_then(resolve_profile_app_name))
        .unwrap_or_else(|| DEFAULT_STANDALONE_APP_NAME.to_string());
    let now_ts = now_unix_secs.to_string();
    let mut context = default_context(&resolved_app_name, &now_ts);
    let mut payload = object([
        (
            "audio_info",
            object([
                ("channel", number_value(1)),
                ("format", string_value(audio_format)),
                ("sample_rate", number_value(16_000)),
            ]),
        ),
        ("enable_punctuation", Value::Bool(true)),
        ("enable_speech_rejection", Value::Bool(false)),
        ("extra", default_extra(&resolved_app_name)),
    ]);

    if let Some(profile) = request_profile {
        apply_request_profile(&mut payload, &mut context, profile, &now_ts)?;
    }

    let context_json =
        serde_json::to_string(&context).map_err(|error| format!("context json failed: {error}"))?;
    let extra = payload
        .get_mut("extra")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "payload.extra missing".to_string())?;
    extra.insert(
        "context".to_string(),
        Value::String(STANDARD.encode(context_json.as_bytes())),
    );
    Ok((payload, context))
}

pub(crate) fn build_start_session(
    session_id: &str,
    token: Option<&str>,
    app_key: &str,
    audio_format: &str,
    request_profile: Option<&Value>,
    app_name: Option<&str>,
    now_unix_secs: u64,
) -> Result<(Vec<u8>, Value, Value), String> {
    let (payload, context) =
        build_start_session_payload(audio_format, request_profile, app_name, now_unix_secs)?;
    let payload_json =
        serde_json::to_string(&payload).map_err(|error| format!("payload json failed: {error}"))?;

    let mut msg = Vec::new();
    if let Some(token) = token {
        push_length_delimited(&mut msg, 1, token.as_bytes());
    }
    push_length_delimited(&mut msg, 2, app_key.as_bytes());
    push_length_delimited(&mut msg, 3, b"ASR");
    push_length_delimited(&mut msg, 5, b"StartSession");
    push_length_delimited(&mut msg, 6, payload_json.as_bytes());
    push_length_delimited(&mut msg, 8, session_id.as_bytes());
    Ok((msg, payload, context))
}

pub(crate) fn build_finish_session(session_id: &str, app_key: &str) -> Vec<u8> {
    let mut msg = Vec::new();
    push_length_delimited(&mut msg, 2, app_key.as_bytes());
    push_length_delimited(&mut msg, 3, b"ASR");
    push_length_delimited(&mut msg, 5, b"FinishSession");
    push_length_delimited(&mut msg, 8, session_id.as_bytes());
    msg
}

pub(crate) fn build_audio_frame(
    session_id: &str,
    audio_data: &[u8],
    timestamp_ms: u64,
    tail_flag: u64,
) -> Vec<u8> {
    let mut msg = Vec::new();
    let timestamp_json = format!(r#"{{"timestamp_ms":{timestamp_ms}}}"#);
    push_length_delimited(&mut msg, 3, b"ASR");
    push_length_delimited(&mut msg, 5, b"TaskRequest");
    push_length_delimited(&mut msg, 6, timestamp_json.as_bytes());
    push_length_delimited(&mut msg, 7, audio_data);
    push_length_delimited(&mut msg, 8, session_id.as_bytes());
    push_varint_field(&mut msg, 9, tail_flag);
    msg
}

fn effective_text_context(
    config: &ContextConfig,
    runtime_context: Option<&FrontierRuntimeContext>,
) -> (String, usize) {
    match config.text_context.mode.as_str() {
        "auto" => runtime_context
            .filter(|context| context.text_available())
            .map(|context| {
                (
                    context.text_context_text.clone().unwrap_or_default(),
                    context.text_context_cursor_position.unwrap_or(0),
                )
            })
            .unwrap_or_else(|| (String::new(), 0)),
        "static" => {
            let text = config.text_context.text.clone();
            let cursor = usize::min(
                config.text_context.cursor_position as usize,
                text.chars().count(),
            );
            (text, cursor)
        }
        _ => (String::new(), 0),
    }
}

fn dedupe_preserve_order(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut merged = Vec::new();
    for value in values {
        if !merged.contains(&value) {
            merged.push(value);
        }
    }
    merged
}

fn resolve_profile_app_name(profile: &Value) -> Option<String> {
    profile
        .get("app_name")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            profile
                .get("extra")
                .and_then(Value::as_object)
                .and_then(|extra| extra.get("app_name"))
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
        .or_else(|| {
            profile
                .get("context")
                .and_then(Value::as_object)
                .and_then(|context| context.get("ime_info"))
                .and_then(Value::as_object)
                .and_then(|ime_info| ime_info.get("app_apk_name"))
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
}

fn default_context(app_name: &str, now_ts: &str) -> Value {
    object([
        (
            "chat",
            Value::Array(vec![object([
                ("time", string_value(now_ts)),
                ("app_apk_name", string_value(app_name)),
                (
                    "data",
                    string_value(python_style_chat_data("", 0)),
                ),
                ("type", string_value("user_input")),
            ])]),
        ),
        (
            "ime_info",
            object([
                ("app_apk_name", string_value(app_name)),
                ("inputType", string_value("default")),
            ]),
        ),
        ("loc_info", object([("ip", string_value(DEFAULT_LOC_IP))])),
        ("hotwords", Value::Array(Vec::new())),
        ("trackingInfo", empty_object()),
    ])
}

fn default_extra(app_name: &str) -> Value {
    object([
        ("aid", string_value("685343")),
        ("app_name", string_value(app_name)),
        ("app_version", string_value(DEFAULT_APP_VERSION)),
        ("build_number", string_value(DEFAULT_APP_VERSION)),
        ("cell_compress_rate", number_value(4)),
        ("cellularProvider", string_value("")),
        ("channel", string_value("release")),
        ("device_brand", string_value("Apple")),
        ("device_id", string_value(DEFAULT_DEVICE_ID)),
        ("device_model", string_value(DEFAULT_DEVICE_MODEL)),
        ("device_platform", string_value("mac")),
        ("device_type", string_value("mac")),
        ("did", string_value(DEFAULT_DEVICE_ID)),
        ("disable_user_words", Value::Bool(false)),
        ("enable_asr_threepass", Value::Bool(true)),
        ("enable_asr_twopass", Value::Bool(true)),
        ("enable_text_filter", Value::Bool(true)),
        ("finish_wait_offline_time", number_value(1000)),
        ("max_wait_switch_offline_time", number_value(1000)),
        (
            "network_change",
            object([
                ("switch_network_ping_timeout", number_value(2000)),
                ("switch_network_quality_threshold", number_value(4)),
                ("switch_network_rtt_threshold", number_value(273)),
            ]),
        ),
        ("offline_wait_online_interval_time", number_value(1000)),
        ("offline_wait_online_time", number_value(2000)),
        ("os", string_value("macOS")),
        ("os_version", string_value("26.4.0")),
        ("region", string_value("CN")),
        ("remove_space_between_han_eng", Value::Bool(true)),
        ("remove_space_between_han_num", Value::Bool(true)),
        (
            "retry_server_code",
            Value::Array(vec![
                number_value(40_100_000),
                number_value(40_100_004),
                number_value(50_000_104),
                number_value(50_700_000),
            ]),
        ),
        ("screen_height", string_value("1280")),
        ("screen_width", string_value("800")),
        ("strong_ddc", Value::Bool(false)),
        ("update_version_code", string_value(DEFAULT_APP_VERSION)),
        ("use_twopass_retry", Value::Bool(true)),
        ("version_code", string_value(DEFAULT_APP_VERSION)),
        ("version_name", string_value(DEFAULT_APP_VERSION)),
        ("vid", string_value(DEFAULT_VID)),
    ])
}

fn apply_request_profile(
    payload: &mut Value,
    context: &mut Value,
    profile: &Value,
    now_ts: &str,
) -> Result<(), String> {
    let payload_object = payload
        .as_object_mut()
        .ok_or_else(|| "payload is not an object".to_string())?;
    let context_object = context
        .as_object_mut()
        .ok_or_else(|| "context is not an object".to_string())?;

    if let Some(overrides) = profile.get("payload").and_then(Value::as_object) {
        for key in ["enable_punctuation", "enable_speech_rejection"] {
            if let Some(value) = overrides.get(key) {
                payload_object.insert(key.to_string(), value.clone());
            }
        }
    }

    if let Some(overrides) = profile.get("extra").and_then(Value::as_object) {
        let extra = payload_object
            .get_mut("extra")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| "payload.extra missing".to_string())?;
        for (key, value) in overrides {
            extra.insert(key.clone(), value.clone());
        }
    }

    if let Some(overrides) = profile.get("context").and_then(Value::as_object) {
        if let Some(hotwords) = overrides.get("hotwords") {
            context_object.insert("hotwords".to_string(), hotwords.clone());
        }
        if let Some(loc_info) = overrides.get("loc_info") {
            context_object.insert("loc_info".to_string(), loc_info.clone());
        }
        if let Some(tracking_info) = overrides.get("trackingInfo") {
            context_object.insert("trackingInfo".to_string(), tracking_info.clone());
        }
        if let Some(ime_info_overrides) = overrides.get("ime_info").and_then(Value::as_object) {
            let ime_info = context_object
                .get_mut("ime_info")
                .and_then(Value::as_object_mut)
                .ok_or_else(|| "context.ime_info missing".to_string())?;
            for (key, value) in ime_info_overrides {
                ime_info.insert(key.clone(), value.clone());
            }
        }
        if let Some(chat_entries) = overrides.get("chat").and_then(Value::as_array) {
            let app_name = context_object
                .get("ime_info")
                .and_then(Value::as_object)
                .and_then(|ime_info| ime_info.get("app_apk_name"))
                .and_then(Value::as_str)
                .unwrap_or(DEFAULT_CONTEXT_APP_NAME);
            context_object.insert(
                "chat".to_string(),
                Value::Array(normalize_chat_entries(chat_entries, app_name, now_ts)),
            );
        }
    }

    Ok(())
}

fn normalize_chat_entries(entries: &[Value], default_app_name: &str, now_ts: &str) -> Vec<Value> {
    entries
        .iter()
        .filter_map(Value::as_object)
        .map(|entry| {
            let mut item = Map::new();
            let mut has_data = false;
            for (key, value) in entry {
                match key.as_str() {
                    "data_json" => {
                        let data = serde_json::to_string(value)
                            .unwrap_or_else(|_| r#"{"text":"","cursor_position":0}"#.to_string());
                        item.insert("data".to_string(), Value::String(data));
                        has_data = true;
                    }
                    "data" => {
                        item.insert(key.clone(), value.clone());
                        has_data = true;
                    }
                    _ => {
                        item.insert(key.clone(), value.clone());
                    }
                }
            }
            if !has_data {
                item.insert(
                    "data".to_string(),
                    Value::String(r#"{"text":"","cursor_position":0}"#.to_string()),
                );
            }
            item.entry("time".to_string())
                .or_insert_with(|| string_value(now_ts));
            item.entry("type".to_string())
                .or_insert_with(|| string_value("user_input"));
            item.entry("app_apk_name".to_string())
                .or_insert_with(|| string_value(default_app_name));
            Value::Object(item)
        })
        .collect()
}

fn python_style_chat_data(text: &str, cursor_position: usize) -> String {
    let escaped_text =
        serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"{{"text": {escaped_text}, "cursor_position": {cursor_position}}}"#
    )
}

fn push_length_delimited(buffer: &mut Vec<u8>, field_number: u32, data: &[u8]) {
    push_varint(buffer, (field_number << 3 | 2) as u64);
    push_varint(buffer, data.len() as u64);
    buffer.extend_from_slice(data);
}

fn push_varint_field(buffer: &mut Vec<u8>, field_number: u32, value: u64) {
    push_varint(buffer, (field_number << 3) as u64);
    push_varint(buffer, value);
}

fn push_varint(buffer: &mut Vec<u8>, mut value: u64) {
    while value > 0x7f {
        buffer.push(((value & 0x7f) as u8) | 0x80);
        value >>= 7;
    }
    buffer.push((value & 0x7f) as u8);
}

fn object<const N: usize>(entries: [(&str, Value); N]) -> Value {
    let mut map = Map::new();
    for (key, value) in entries {
        map.insert(key.to_string(), value);
    }
    Value::Object(map)
}

fn empty_object() -> Value {
    Value::Object(Map::new())
}

fn string_value(value: impl Into<String>) -> Value {
    Value::String(value.into())
}

fn number_value(value: u64) -> Value {
    Value::Number(Number::from(value))
}

#[cfg(test)]
mod tests {
    use super::{
        build_audio_frame, build_effective_request_profile, build_finish_session,
        build_start_session, build_start_task, FrontierRuntimeContext, DEFAULT_APP_KEY,
    };
    use crate::config::{AdvancedConfig, ContextConfig, ImeContextConfig, RecognitionConfig, TextContextConfig};
    use serde_json::Value;

    fn to_hex(bytes: Vec<u8>) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn matches_python_binary_vectors_for_small_messages() {
        let session_id = "SESSION-123";
        let token = Some("TOKEN-XYZ");
        assert_eq!(
            to_hex(build_start_task(session_id, token, DEFAULT_APP_KEY)),
            "0a09544f4b454e2d58595a120a4f726e714b76535372731a034153522a0953746172745461736b420b53455353494f4e2d313233"
        );
        assert_eq!(
            to_hex(build_finish_session(session_id, DEFAULT_APP_KEY)),
            "120a4f726e714b76535372731a034153522a0d46696e69736853657373696f6e420b53455353494f4e2d313233"
        );
        assert_eq!(
            to_hex(build_audio_frame(session_id, &[1, 2, 3, 4], 1_234_567_890, 1)),
            "1a034153522a0b5461736b52657175657374321b7b2274696d657374616d705f6d73223a313233343536373839307d3a0401020304420b53455353494f4e2d3132334801"
        );
    }

    #[test]
    fn builds_effective_profile_from_runtime_context() {
        let config = ContextConfig {
            recognition: RecognitionConfig {
                enable_punctuation: true,
                enable_speech_rejection: false,
            },
            hotwords: vec!["alpha".into()],
            user_terms: vec!["alpha".into(), "beta".into()],
            text_context: TextContextConfig {
                mode: "auto".into(),
                max_chars: 256,
                text: String::new(),
                cursor_position: 0,
            },
            ime_context: ImeContextConfig {
                input_type: "search".into(),
            },
            advanced: AdvancedConfig {
                use_user_dictionary: false,
                enable_text_filter: true,
                enable_asr_twopass: true,
                enable_asr_threepass: false,
                remove_space_between_han_eng: true,
                remove_space_between_han_num: false,
                strong_ddc: true,
            },
            ..Default::default()
        };
        let runtime_context = FrontierRuntimeContext {
            app_bundle_id: Some("com.test.App".into()),
            text_context_text: Some("hello world".into()),
            text_context_cursor_position: Some(5),
            capture_ms: 100,
            source: "host_update".into(),
        };

        let profile = build_effective_request_profile(&config, Some(&runtime_context));
        assert_eq!(profile.get("app_name").and_then(Value::as_str), Some("com.test.App"));
        assert_eq!(
            profile["context"]["hotwords"]
                .as_array()
                .expect("hotwords")
                .iter()
                .filter_map(|item| item.get("word").and_then(Value::as_str))
                .collect::<Vec<_>>(),
            vec!["alpha", "beta"]
        );
        assert_eq!(
            profile["context"]["chat"][0]["data_json"]["text"].as_str(),
            Some("hello world")
        );
        assert_eq!(
            profile["context"]["chat"][0]["data_json"]["cursor_position"].as_u64(),
            Some(5)
        );
        assert_eq!(
            profile["context"]["ime_info"]["inputType"].as_str(),
            Some("search")
        );
        assert_eq!(
            profile["extra"]["disable_user_words"].as_bool(),
            Some(true)
        );
        assert_eq!(
            profile["extra"]["enable_asr_threepass"].as_bool(),
            Some(false)
        );
    }

    #[test]
    fn start_session_payload_uses_compact_chat_data_from_profile() {
        let config = ContextConfig {
            text_context: TextContextConfig {
                mode: "static".into(),
                max_chars: 256,
                text: "abc".into(),
                cursor_position: 2,
            },
            ..Default::default()
        };
        let profile = build_effective_request_profile(&config, None);
        let (_message, payload, decoded_context) = build_start_session(
            "SESSION-123",
            Some("TOKEN-XYZ"),
            DEFAULT_APP_KEY,
            "speech_opus",
            Some(&profile),
            None,
            1_234_567_890,
        )
        .expect("start session");

        assert_eq!(
            payload["extra"]["app_name"].as_str(),
            Some("ime")
        );
        assert_eq!(
            decoded_context["chat"][0]["data"].as_str(),
            Some(r#"{"text":"abc","cursor_position":2}"#)
        );
        assert_eq!(
            decoded_context["chat"][0]["time"].as_str(),
            Some("1234567890")
        );
        assert_eq!(
            decoded_context["ime_info"]["app_apk_name"].as_str(),
            Some("ime")
        );
    }
}
