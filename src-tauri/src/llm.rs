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
        // Qwen3 thinking 模型：取 </think> 后的实际回答
        let answer = if let Some(idx) = result.find("</think>") {
            &result[idx + "</think>".len()..]
        } else {
            &result[..]
        };
        Ok(answer.trim().to_string())
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
    // 优先外部 API
    if std::env::var("DEEPSEEK_API_KEY").is_ok() {
        return summarize_external(text, language);
    }
    let model_path = local_model_path();
    let engine = LLM_ENGINE.get_or_init(|| LlmEngine::new(&model_path));
    match engine {
        Ok(e) => e.summarize(text, language),
        Err(e) => Err(e.clone()),
    }
}

/// 外部 API（DeepSeek，OpenAI 兼容）摘要
fn summarize_external(text: &str, language: UiLanguage) -> Result<String, String> {
    let key = std::env::var("DEEPSEEK_API_KEY").map_err(|_| "no DEEPSEEK_API_KEY".to_string())?;
    let (system, user) = prompts(text, language);
    let body = serde_json::json!({
        "model": "deepseek-v4-flash",
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user}
        ],
        "max_tokens": 1000,
        "temperature": 0,
        "stream": false,
        "enable_thinking": false
    });
    let resp = ureq::post("https://api.deepseek.com/chat/completions")
        .set("Authorization", &format!("Bearer {}", key))
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| format!("api: {}", e))?;
    let json: serde_json::Value = resp.into_json().map_err(|e| e.to_string())?;
    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| format!("no content: {}", json))?;
    if content.trim().is_empty() {
        return Err(format!("empty content, response: {}", json));
    }
    Ok(content.to_string())
}

fn prompts(text: &str, language: UiLanguage) -> (String, String) {
    let output_instruction = match language {
        UiLanguage::ZhCn => "描述必须使用简体中文。",
        UiLanguage::En => "The description must be written in English.",
    };
    let system = format!(
        "You analyze terminal state and produce a stable one-line summary. {output_instruction}"
    );
    let user = format!(
        "State rules:\n- ok: the command or agent is actively working (progress/loading/thinking)\n- idle: work has completed and the terminal is waiting for the next command\n- warn: a non-fatal warning or retry is active\n- err: user action is required (error/password/confirmation/input)\n\nFocus on the newest control lines near the bottom. Ignore historical output and dynamic UI values such as spinners, elapsed time, token counts, and progress values.\n\nReturn exactly one line in the form status|description. Use one short sentence describing the current concrete task. Do not quote or reproduce the screen. {output_instruction}\n\nTerminal context:\n{text}"
    );
    (system, user)
}
