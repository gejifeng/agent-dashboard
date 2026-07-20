use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::logit_bias::LlamaLogitBias;
use llama_cpp_2::{send_logs_to_tracing, LogOptions};
use std::num::NonZeroU32;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use crate::agent_support::UiLanguage;
use crate::settings::{validate_non_thinking_model, ApiConfig, ApiProvider, THINKING_MODE_ERROR};

static BACKEND: OnceLock<LlamaBackend> = OnceLock::new();

pub struct LlmEngine {
    model: LlamaModel,
}

impl LlmEngine {
    pub fn new(model_path: &Path) -> Result<Self, String> {
        let backend = BACKEND.get_or_init(|| {
            send_logs_to_tracing(LogOptions::default().with_logs_enabled(false));
            LlamaBackend::init().expect("failed to init llama backend")
        });
        let model_params = LlamaModelParams::default();
        let model_params = std::pin::pin!(model_params);
        let model = LlamaModel::load_from_file(backend, model_path, &model_params)
            .map_err(|e| e.to_string())?;
        Ok(Self { model })
    }

    /// 用模型对一段终端输出生成一句话摘要
    pub fn summarize(&self, text: &str, language: UiLanguage) -> Result<String, String> {
        // 截断到尾部 1500 字符，避免超过 batch/context 限制
        let text: String = {
            let chars: Vec<char> = text.chars().collect();
            if chars.len() > 1500 {
                chars[chars.len() - 1500..].iter().collect()
            } else {
                text.to_string()
            }
        };
        let backend = BACKEND.get().ok_or("backend not initialized")?;
        let tmpl = self.model.chat_template(None).map_err(|e| e.to_string())?;
        let (system, user) = prompts(&text, language);
        let chat = vec![
            LlamaChatMessage::new("system".into(), system.into()).map_err(|e| e.to_string())?,
            LlamaChatMessage::new("user".into(), format!("{user}\n/no_think"))
                .map_err(|e| e.to_string())?,
        ];
        let prompt = self
            .model
            .apply_chat_template(&tmpl, &chat, true)
            .map_err(|e| e.to_string())?;
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(Some(NonZeroU32::new(4096).unwrap()))
            .with_n_threads(4);
        let mut ctx = self
            .model
            .new_context(backend, ctx_params)
            .map_err(|e| e.to_string())?;

        let tokens = self
            .model
            .str_to_token(&prompt, AddBos::Never)
            .map_err(|e| e.to_string())?;
        if tokens.is_empty() {
            return Err("empty tokens".into());
        }

        let mut batch = LlamaBatch::new(4096, 1);
        let last_index = (tokens.len() - 1) as i32;
        for (i, token) in (0_i32..).zip(tokens.into_iter()) {
            let is_last = i == last_index;
            batch
                .add(token, i, &[0], is_last)
                .map_err(|e| e.to_string())?;
        }
        ctx.decode(&mut batch).map_err(|e| e.to_string())?;

        // 禁用 <think> token，强制非 thinking 直接回答
        let mut sampler =
            if let Ok(think_tokens) = self.model.str_to_token("<think>", AddBos::Never) {
                if let Some(&t) = think_tokens.first() {
                    let n_vocab = self.model.n_vocab();
                    LlamaSampler::chain_simple([
                        LlamaSampler::logit_bias(n_vocab, &[LlamaLogitBias::new(t, -100.0)]),
                        LlamaSampler::greedy(),
                    ])
                } else {
                    LlamaSampler::chain_simple([LlamaSampler::greedy()])
                }
            } else {
                LlamaSampler::chain_simple([LlamaSampler::greedy()])
            };
        let mut result = String::new();
        let mut decoder = encoding_rs::UTF_8.new_decoder();
        let max_new = 64;
        let mut n_cur = batch.n_tokens();
        for _ in 0..max_new {
            let token = sampler.sample(&ctx, batch.n_tokens() - 1);
            sampler.accept(token);
            if self.model.is_eog_token(token) {
                break;
            }
            let piece = self
                .model
                .token_to_piece(token, &mut decoder, true, None)
                .map_err(|e| e.to_string())?;
            result.push_str(&piece);
            batch.clear();
            batch
                .add(token, n_cur, &[0], true)
                .map_err(|e| e.to_string())?;
            n_cur += 1;
            ctx.decode(&mut batch).map_err(|e| e.to_string())?;
        }
        if contains_thinking_markup(&result) {
            return Err(THINKING_MODE_ERROR.to_string());
        }
        Ok(result.trim().to_string())
    }
}

static LLM_ENGINE: OnceLock<Result<LlmEngine, String>> = OnceLock::new();
static SUMMARIZE_LOCK: Mutex<()> = Mutex::new(());

fn local_model_path() -> std::path::PathBuf {
    std::env::var_os("LOCAL_LLM_MODEL_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join("models")
                .join("Qwen3.5-2B-Q4_K_M.gguf")
        })
}

/// 全局摘要入口：串行调用外部 API 或本地模型。会话层会合并 pending 状态，
/// 因而这里等待锁不会堆积同一终端的过时屏幕。
pub fn summarize(text: &str, language: UiLanguage) -> Result<String, String> {
    let _guard = SUMMARIZE_LOCK
        .lock()
        .map_err(|_| "summary lock poisoned".to_string())?;
    // Prefer a configured OpenAI-compatible API. If no key is available, use the local model.
    if let Some(config) = crate::settings::effective_api_config() {
        return summarize_external(text, language, &config);
    }
    let model_path = local_model_path();
    let engine = LLM_ENGINE.get_or_init(|| LlmEngine::new(&model_path));
    match engine {
        Ok(e) => e.summarize(text, language),
        Err(e) => Err(e.clone()),
    }
}

/// OpenAI-compatible external summary API.
fn summarize_external(
    text: &str,
    language: UiLanguage,
    config: &ApiConfig,
) -> Result<String, String> {
    validate_non_thinking_model(&config.model)?;
    let (system, user) = prompts(text, language);
    let mut body = serde_json::json!({
        "model": config.model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user}
        ],
        "max_tokens": 1000,
        "temperature": 0,
        "stream": false
    });
    match config.provider {
        ApiProvider::DeepSeek => body["thinking"] = serde_json::json!({"type": "disabled"}),
        ApiProvider::OpenAi => {
            if let Some(object) = body.as_object_mut() {
                object.remove("max_tokens");
                object.remove("temperature");
                object.insert("max_completion_tokens".to_string(), serde_json::json!(1000));
            }
        }
        ApiProvider::SiliconFlow => body["enable_thinking"] = serde_json::json!(false),
        _ => {}
    }
    let endpoint = config.chat_completions_url();
    let mut request = ureq::post(&endpoint).set("Content-Type", "application/json");
    if let Some(api_key) = &config.api_key {
        request = request.set("Authorization", &format!("Bearer {api_key}"));
    }
    let resp = request
        .send_string(&body.to_string())
        .map_err(|e| format!("api: {}", e))?;
    let json: serde_json::Value = resp.into_json().map_err(|e| e.to_string())?;
    let message = &json["choices"][0]["message"];
    let content = message["content"]
        .as_str()
        .ok_or_else(|| "API response has no summary content".to_string())?;
    if response_used_thinking(message, &json) || contains_thinking_markup(content) {
        return Err(THINKING_MODE_ERROR.to_string());
    }
    if content.trim().is_empty() {
        return Err("API response contains an empty summary".to_string());
    }
    Ok(content.to_string())
}

fn contains_thinking_markup(content: &str) -> bool {
    let content = content.to_ascii_lowercase();
    content.contains("<think>") || content.contains("</think>")
}

fn response_used_thinking(message: &serde_json::Value, response: &serde_json::Value) -> bool {
    fn populated(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::Null => false,
            serde_json::Value::String(value) => !value.trim().is_empty(),
            serde_json::Value::Array(value) => !value.is_empty(),
            serde_json::Value::Object(value) => !value.is_empty(),
            serde_json::Value::Number(value) => value.as_u64().unwrap_or(0) > 0,
            serde_json::Value::Bool(value) => *value,
        }
    }

    [
        "reasoning_content",
        "reasoning_details",
        "reasoning",
        "thinking",
    ]
    .iter()
    .any(|field| populated(&message[*field]))
        || response["usage"]["completion_tokens_details"]["reasoning_tokens"]
            .as_u64()
            .unwrap_or(0)
            > 0
}

fn prompts(text: &str, language: UiLanguage) -> (String, String) {
    let output_instruction = match language {
        UiLanguage::ZhCn => "描述必须使用简体中文。",
        UiLanguage::En => "The description must be written in English.",
    };
    let system = format!(
        "You analyze terminal state and produce a stable one-line summary without chain-of-thought. {output_instruction}"
    );
    let user = format!(
        "State rules:\n- ok: the command or agent is actively working (progress/loading/thinking)\n- idle: work has completed and the terminal is waiting for the next command\n- warn: a non-fatal warning or retry is active\n- err: user action is required (error/password/confirmation/input)\n\nFocus on the newest control lines near the bottom. Ignore historical output and dynamic UI values such as spinners, elapsed time, token counts, and progress values.\n\nReturn exactly one line in the form status|description. Use one short sentence describing the current concrete task. Do not quote or reproduce the screen. {output_instruction}\n\nTerminal context:\n{text}"
    );
    (system, user)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_thinking_fields_and_markup() {
        let response = serde_json::json!({
            "choices": [{"message": {"content": "ok|done", "reasoning_content": "hidden"}}]
        });
        assert!(response_used_thinking(
            &response["choices"][0]["message"],
            &response
        ));
        assert!(contains_thinking_markup("<think>work</think>idle|done"));
    }

    #[test]
    fn accepts_plain_non_thinking_response() {
        let response = serde_json::json!({
            "choices": [{"message": {"content": "idle|done", "reasoning_content": ""}}],
            "usage": {"completion_tokens_details": {"reasoning_tokens": 0}}
        });
        assert!(!response_used_thinking(
            &response["choices"][0]["message"],
            &response
        ));
        assert!(!contains_thinking_markup("idle|done"));
    }
}
