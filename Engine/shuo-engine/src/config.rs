use serde::Deserialize;

// Shared JSON schema with the Swift settings UI and other tooling. The Rust helper
// only consumes the shortcut subset today, but it still needs to deserialize the
// full config document without dropping fields from the canonical on-disk shape.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
pub(crate) struct ContextConfig {
    #[serde(default)]
    pub(crate) version: u32,
    #[serde(default)]
    pub(crate) recognition: RecognitionConfig,
    #[serde(default)]
    pub(crate) hotwords: Vec<String>,
    #[serde(default)]
    pub(crate) user_terms: Vec<String>,
    #[serde(default)]
    pub(crate) text_context: TextContextConfig,
    #[serde(default)]
    pub(crate) ime_context: ImeContextConfig,
    #[serde(default)]
    pub(crate) advanced: AdvancedConfig,
    #[serde(default)]
    pub(crate) shortcut: ShortcutConfig,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct ShortcutConfig {
    #[serde(default = "default_shortcut_key")]
    pub(crate) key: String,
    #[serde(default = "default_shortcut_mode")]
    pub(crate) mode: String,
}

impl Default for ShortcutConfig {
    fn default() -> Self {
        Self {
            key: "right_command".into(),
            mode: "hold".into(),
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct RecognitionConfig {
    #[serde(default = "default_true")]
    pub(crate) enable_punctuation: bool,
    #[serde(default)]
    pub(crate) enable_speech_rejection: bool,
}

impl Default for RecognitionConfig {
    fn default() -> Self {
        Self {
            enable_punctuation: true,
            enable_speech_rejection: false,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct TextContextConfig {
    #[serde(default = "default_auto")]
    pub(crate) mode: String,
    #[serde(default = "default_max_chars")]
    pub(crate) max_chars: u32,
    #[serde(default)]
    pub(crate) text: String,
    #[serde(default)]
    pub(crate) cursor_position: u32,
}

impl Default for TextContextConfig {
    fn default() -> Self {
        Self {
            mode: "auto".into(),
            max_chars: 256,
            text: String::new(),
            cursor_position: 0,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
pub(crate) struct ImeContextConfig {
    #[serde(default = "default_input_type")]
    pub(crate) input_type: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct AdvancedConfig {
    #[serde(default = "default_true")]
    pub(crate) use_user_dictionary: bool,
    #[serde(default = "default_true")]
    pub(crate) enable_text_filter: bool,
    #[serde(default = "default_true")]
    pub(crate) enable_asr_twopass: bool,
    #[serde(default = "default_true")]
    pub(crate) enable_asr_threepass: bool,
    #[serde(default = "default_false")]
    pub(crate) remove_space_between_han_eng: bool,
    #[serde(default = "default_false")]
    pub(crate) remove_space_between_han_num: bool,
    #[serde(default)]
    pub(crate) strong_ddc: bool,
}

impl Default for AdvancedConfig {
    fn default() -> Self {
        Self {
            use_user_dictionary: true,
            enable_text_filter: true,
            enable_asr_twopass: true,
            enable_asr_threepass: true,
            remove_space_between_han_eng: false,
            remove_space_between_han_num: false,
            strong_ddc: false,
        }
    }
}

fn default_shortcut_key() -> String {
    "right_command".into()
}

fn default_shortcut_mode() -> String {
    "hold".into()
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

fn default_auto() -> String {
    "auto".into()
}

fn default_max_chars() -> u32 {
    256
}

fn default_input_type() -> String {
    "default".into()
}

fn default_context_config_path() -> Option<String> {
    if let Ok(path) = std::env::var("CONTEXT_CONFIG_PATH") {
        if !path.is_empty() {
            return Some(path);
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(|path| path.to_path_buf());
        for _ in 0..6 {
            if let Some(current) = &dir {
                for name in ["configs/shuo.context.json", "configs/hj_dictation.context.json"] {
                    let candidate = current.join(name);
                    if candidate.exists() {
                        return Some(candidate.to_string_lossy().into_owned());
                    }
                }
                dir = current.parent().map(|path| path.to_path_buf());
            }
        }
    }

    None
}

pub(crate) fn load_context_config_from_path(path: Option<&str>) -> Result<ContextConfig, String> {
    let path = path.map(ToOwned::to_owned).or_else(default_context_config_path);
    let Some(path) = path else {
        return Ok(ContextConfig {
            version: 1,
            ..Default::default()
        });
    };
    let data = std::fs::read_to_string(&path)
        .map_err(|error| format!("read config failed for {path}: {error}"))?;
    serde_json::from_str::<ContextConfig>(&data)
        .map_err(|error| format!("parse config failed for {path}: {error}"))
}

pub(crate) fn load_context_config() -> ContextConfig {
    load_context_config_from_path(None).unwrap_or(ContextConfig {
        version: 1,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::ContextConfig;

    #[test]
    fn shared_context_config_fixture_parses() {
        let raw = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../Shared/Fixtures/context-config.v1.json"
        ));
        let config: ContextConfig = serde_json::from_str(raw).expect("shared context fixture");
        assert_eq!(config.version, 1);
        assert_eq!(config.shortcut.key, "right_command");
        assert_eq!(config.shortcut.mode, "hold");
        assert_eq!(config.text_context.mode, "auto");
        assert_eq!(config.text_context.max_chars, 256);
        assert!(config.recognition.enable_punctuation);
        assert!(config.advanced.use_user_dictionary);
        assert_eq!(config.hotwords, vec!["Shuo".to_string(), "Doubao".to_string()]);
        assert_eq!(config.user_terms, vec!["Swift".to_string(), "Rust".to_string()]);
    }
}
