use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::agent_support::UiLanguage;

const DEFAULT_MODEL: &str = "deepseek-v4-flash";
pub const THINKING_MODE_ERROR: &str = "THINKING_MODE_NOT_SUPPORTED";

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiProvider {
    #[default]
    DeepSeek,
    OpenAi,
    OpenRouter,
    SiliconFlow,
    Custom,
}

impl ApiProvider {
    fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "deepseek" => Ok(Self::DeepSeek),
            "openai" | "open_ai" => Ok(Self::OpenAi),
            "openrouter" | "open_router" => Ok(Self::OpenRouter),
            "siliconflow" | "silicon_flow" => Ok(Self::SiliconFlow),
            "custom" => Ok(Self::Custom),
            _ => Err("unsupported API provider".to_string()),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::DeepSeek => "deepseek",
            Self::OpenAi => "openai",
            Self::OpenRouter => "openrouter",
            Self::SiliconFlow => "siliconflow",
            Self::Custom => "custom",
        }
    }

    fn preset_base_url(self) -> Option<&'static str> {
        match self {
            Self::DeepSeek => Some("https://api.deepseek.com"),
            Self::OpenAi => Some("https://api.openai.com/v1"),
            Self::OpenRouter => Some("https://openrouter.ai/api/v1"),
            Self::SiliconFlow => Some("https://api.siliconflow.cn/v1"),
            Self::Custom => None,
        }
    }

    fn environment_key(self) -> Option<String> {
        let name = match self {
            Self::DeepSeek => "DEEPSEEK_API_KEY",
            Self::OpenAi => "OPENAI_API_KEY",
            Self::OpenRouter => "OPENROUTER_API_KEY",
            Self::SiliconFlow => "SILICONFLOW_API_KEY",
            Self::Custom => return None,
        };
        std::env::var(name)
            .ok()
            .filter(|key| !key.trim().is_empty())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
struct PersistedSettings {
    language: Option<String>,
    provider: ApiProvider,
    base_url: String,
    model: String,
    api_key: Option<String>,
}

impl Default for PersistedSettings {
    fn default() -> Self {
        Self {
            language: None,
            provider: ApiProvider::DeepSeek,
            base_url: ApiProvider::DeepSeek
                .preset_base_url()
                .unwrap_or_default()
                .to_string(),
            model: DEFAULT_MODEL.to_string(),
            api_key: None,
        }
    }
}

#[derive(Clone)]
pub struct SettingsManager {
    inner: Arc<Mutex<PersistedSettings>>,
    path: Arc<PathBuf>,
    revision: Arc<AtomicU64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    language: Option<String>,
    provider: String,
    base_url: String,
    model: String,
    has_api_key: bool,
    api_key_hint: Option<String>,
    api_key_source: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsInput {
    language: Option<String>,
    provider: String,
    base_url: String,
    model: String,
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    clear_api_key: bool,
}

#[derive(Clone, Debug)]
pub struct ApiConfig {
    pub provider: ApiProvider,
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
}

impl ApiConfig {
    pub fn chat_completions_url(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        if base.ends_with("/chat/completions") {
            base.to_string()
        } else {
            format!("{base}/chat/completions")
        }
    }
}

static GLOBAL_SETTINGS: OnceLock<SettingsManager> = OnceLock::new();

pub fn initialize() -> SettingsManager {
    let manager = SettingsManager::load();
    let _ = GLOBAL_SETTINGS.set(manager.clone());
    manager
}

pub fn effective_api_config() -> Option<ApiConfig> {
    GLOBAL_SETTINGS
        .get()
        .cloned()
        .unwrap_or_else(SettingsManager::load)
        .api_config()
}

impl SettingsManager {
    fn load() -> Self {
        let path = settings_path();
        let settings = std::fs::read_to_string(&path)
            .ok()
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_default();
        Self {
            inner: Arc::new(Mutex::new(settings)),
            path: Arc::new(path),
            revision: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn language(&self) -> Option<UiLanguage> {
        self.inner.lock().ok().and_then(|settings| {
            settings
                .language
                .as_deref()
                .map(|value| UiLanguage::parse(Some(value)))
        })
    }

    pub fn set_language(&self, language: UiLanguage) -> Result<(), String> {
        let mut settings = self.inner.lock().map_err(|e| e.to_string())?;
        let mut next = settings.clone();
        next.language = Some(language.as_str().to_string());
        self.persist(&next)?;
        *settings = next;
        Ok(())
    }

    pub fn view(&self) -> Result<SettingsView, String> {
        let settings = self.inner.lock().map_err(|e| e.to_string())?;
        Ok(settings_view(&settings))
    }

    pub fn update(&self, input: SettingsInput) -> Result<SettingsView, String> {
        let mut settings = self.inner.lock().map_err(|e| e.to_string())?;
        let mut next = settings.clone();
        apply_input(&mut next, input)?;
        self.persist(&next)?;
        *settings = next;
        self.revision.fetch_add(1, Ordering::SeqCst);
        Ok(settings_view(&settings))
    }

    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::SeqCst)
    }

    fn api_config(&self) -> Option<ApiConfig> {
        let settings = self.inner.lock().ok()?;
        let api_key = settings
            .api_key
            .clone()
            .filter(|key| !key.trim().is_empty())
            .or_else(|| settings.provider.environment_key());
        if api_key.is_none() && settings.provider != ApiProvider::Custom {
            return None;
        }
        Some(ApiConfig {
            provider: settings.provider,
            base_url: effective_base_url(&settings),
            model: settings.model.clone(),
            api_key,
        })
    }

    fn persist(&self, settings: &PersistedSettings) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
        std::fs::write(self.path.as_ref(), json).map_err(|e| e.to_string())
    }
}

fn settings_view(settings: &PersistedSettings) -> SettingsView {
    let saved_key = settings
        .api_key
        .as_deref()
        .filter(|key| !key.trim().is_empty());
    let environment_key = settings.provider.environment_key();
    let effective_key = saved_key.map(str::to_string).or(environment_key);
    SettingsView {
        language: settings.language.clone(),
        provider: settings.provider.as_str().to_string(),
        base_url: effective_base_url(settings),
        model: settings.model.clone(),
        has_api_key: effective_key.is_some(),
        api_key_hint: effective_key.as_deref().map(mask_key),
        api_key_source: if saved_key.is_some() {
            Some("settings".to_string())
        } else if effective_key.is_some() {
            Some("environment".to_string())
        } else {
            None
        },
    }
}

fn apply_input(settings: &mut PersistedSettings, input: SettingsInput) -> Result<(), String> {
    let provider = ApiProvider::parse(&input.provider)?;
    let language = input
        .language
        .as_deref()
        .map(|value| match value {
            "zh-CN" | "en" => Ok(value.to_string()),
            _ => Err("unsupported language".to_string()),
        })
        .transpose()?;
    let model = clean_field(&input.model, 160, "model")?;
    if model.is_empty() {
        return Err("model is required".to_string());
    }
    validate_non_thinking_model(&model)?;
    let base_url = if let Some(preset) = provider.preset_base_url() {
        preset.to_string()
    } else {
        validate_custom_base_url(&input.base_url)?
    };

    settings.language = language;
    settings.provider = provider;
    settings.base_url = base_url;
    settings.model = model;
    if input.clear_api_key {
        settings.api_key = None;
    } else if let Some(key) = input.api_key {
        let key = clean_field(&key, 2048, "API key")?;
        if !key.is_empty() {
            settings.api_key = Some(key);
        }
    }
    Ok(())
}

fn effective_base_url(settings: &PersistedSettings) -> String {
    settings
        .provider
        .preset_base_url()
        .unwrap_or(&settings.base_url)
        .trim_end_matches('/')
        .to_string()
}

fn validate_custom_base_url(value: &str) -> Result<String, String> {
    let value = clean_field(value, 2048, "base URL")?;
    let url = url::Url::parse(&value).map_err(|_| "invalid base URL".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("base URL must use http or https and include a host".to_string());
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("base URL cannot contain credentials, query, or fragment".to_string());
    }
    Ok(value.trim_end_matches('/').to_string())
}

fn clean_field(value: &str, max_chars: usize, name: &str) -> Result<String, String> {
    let value = value.trim();
    if value.chars().count() > max_chars || value.chars().any(char::is_control) {
        return Err(format!("invalid {name}"));
    }
    Ok(value.to_string())
}

/// Reject identifiers that unambiguously select a reasoning-only family.
/// Hybrid models remain allowed because the request explicitly disables
/// thinking, and `llm.rs` checks the response again.
pub fn validate_non_thinking_model(model: &str) -> Result<(), String> {
    let normalized = model.trim().to_ascii_lowercase().replace('_', "-");
    let id = normalized
        .rsplit('/')
        .next()
        .unwrap_or(&normalized)
        .split(':')
        .next()
        .unwrap_or(&normalized);
    let named_reasoning = normalized.contains("reasoner")
        || normalized.contains("reasoning")
        || normalized.contains("thinking")
        || id == "qwq"
        || id.starts_with("qwq-")
        || id == "r1"
        || id.starts_with("r1-")
        || id.contains("-r1-")
        || id.ends_with("-r1");
    let openai_reasoning_family = ["o1", "o3", "o4"]
        .iter()
        .any(|family| id == *family || id.starts_with(&format!("{family}-")))
        || id == "gpt-5"
        || id.starts_with("gpt-5-")
        || id.starts_with("gpt-5.")
        || id.starts_with("gpt-oss")
        || id.starts_with("codex-mini");

    if named_reasoning || openai_reasoning_family {
        Err(THINKING_MODE_ERROR.to_string())
    } else {
        Ok(())
    }
}

fn mask_key(key: &str) -> String {
    let suffix: String = key
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("•••• {suffix}")
}

fn settings_path() -> PathBuf {
    if let Some(dir) = std::env::var_os("AGENT_DASHBOARD_CONFIG_DIR") {
        return PathBuf::from(dir).join("settings.json");
    }
    if let Some(dir) = std::env::var_os("APPDATA") {
        return PathBuf::from(dir)
            .join("Agent Dashboard")
            .join("settings.json");
    }
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(dir)
            .join("agent-dashboard")
            .join("settings.json");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".config")
            .join("agent-dashboard")
            .join("settings.json");
    }
    PathBuf::from("runtime").join("settings.json")
}

#[tauri::command]
pub fn get_app_settings(state: tauri::State<'_, SettingsManager>) -> Result<SettingsView, String> {
    state.view()
}

#[tauri::command]
pub fn save_app_settings(
    state: tauri::State<'_, SettingsManager>,
    sessions: tauri::State<'_, crate::session::SessionManager>,
    input: SettingsInput,
) -> Result<SettingsView, String> {
    let view = state.update(input)?;
    sessions.invalidate_summaries();
    Ok(view)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(provider: &str, base_url: &str) -> SettingsInput {
        SettingsInput {
            language: Some("en".to_string()),
            provider: provider.to_string(),
            base_url: base_url.to_string(),
            model: "test-model".to_string(),
            api_key: Some("secret-12345678".to_string()),
            clear_api_key: false,
        }
    }

    #[test]
    fn provider_forces_official_base_url() {
        let mut settings = PersistedSettings::default();
        apply_input(
            &mut settings,
            input("openrouter", "https://evil.example/v1"),
        )
        .unwrap();
        assert_eq!(
            effective_base_url(&settings),
            "https://openrouter.ai/api/v1"
        );
    }

    #[test]
    fn custom_provider_validates_and_builds_endpoint() {
        let mut settings = PersistedSettings::default();
        apply_input(&mut settings, input("custom", "http://127.0.0.1:11434/v1/")).unwrap();
        let config = ApiConfig {
            provider: settings.provider,
            base_url: effective_base_url(&settings),
            model: settings.model.clone(),
            api_key: settings.api_key.clone(),
        };
        assert_eq!(
            config.chat_completions_url(),
            "http://127.0.0.1:11434/v1/chat/completions"
        );
        assert!(apply_input(&mut settings, input("custom", "file:///tmp/model")).is_err());
    }

    #[test]
    fn settings_view_never_serializes_full_key() {
        let mut settings = PersistedSettings::default();
        settings.api_key = Some("secret-12345678".to_string());
        let json = serde_json::to_string(&settings_view(&settings)).unwrap();
        assert!(!json.contains("secret-12345678"));
        assert!(json.contains("5678"));
    }

    #[test]
    fn rejects_reasoning_only_models() {
        for model in [
            "deepseek-reasoner",
            "deepseek-ai/DeepSeek-R1",
            "openai/o3-mini",
            "openai/gpt-5.4-mini",
            "Qwen/QwQ-32B",
        ] {
            assert_eq!(
                validate_non_thinking_model(model),
                Err(THINKING_MODE_ERROR.to_string()),
                "{model} should be rejected"
            );
        }
    }

    #[test]
    fn allows_models_with_an_explicit_non_thinking_mode() {
        for model in [
            "deepseek-v4-flash",
            "gpt-4.1-mini",
            "openai/gpt-4.1-mini",
            "Pro/zai-org/GLM-4.7",
            "Qwen/Qwen3.5-9B",
        ] {
            assert!(
                validate_non_thinking_model(model).is_ok(),
                "{model} should be allowed"
            );
        }
    }
}
