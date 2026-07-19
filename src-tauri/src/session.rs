use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::Serialize;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, State};

use crate::agent_support::{llm_input, AgentKind, AgentState, NameSource, UiLanguage};
use crate::pty::PtyHandle;

const SUMMARY_INTERVAL_SECS: u64 = 10;
const BUFFER_TAIL_BYTES: usize = 8192;
const SCREEN_SUMMARY_FRESH_SECS: f64 = 12.0;
const DETACHED_QUIET_SECS: f64 = 5.0;
const SUMMARY_CACHE_SIZE: usize = 32;
const STRUCTURED_EVENT_GRACE_SECS: f64 = 20.0;

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

pub struct Session {
    pub id: String,
    pub name: String,
    pub command: String,
    pub name_source: Arc<Mutex<NameSource>>,
    pub external_session_id: Arc<Mutex<Option<String>>>,
    pub external_title: Arc<Mutex<Option<String>>>,
    pub started_at: f64,
    pub alive: Arc<AtomicBool>,
    pub status: Arc<Mutex<String>>,
    pub output: Arc<Mutex<Option<Channel>>>,
    pub summary: Arc<Mutex<String>>,
    pub buffer: Arc<Mutex<Vec<u8>>>,
    pub last_screen_ts: Arc<Mutex<f64>>,
    pub last_output_ts: Arc<Mutex<f64>>,
    pub output_revision: Arc<AtomicU64>,
    pub last_buffer_summary_revision: Arc<AtomicU64>,
    pub agent_kind: Arc<Mutex<AgentKind>>,
    pub agent_state: Arc<Mutex<AgentState>>,
    pub state_source: Arc<Mutex<String>>,
    pub structured_event_ts: Arc<Mutex<f64>>,
    structured_summaries: Arc<Mutex<LocalizedSummaries>>,
    summary_control: Arc<Mutex<SummaryControl>>,
    pub pty: PtyHandle,
}

#[derive(Clone)]
struct SummaryJob {
    key: u64,
    text: String,
    agent: AgentKind,
    state: AgentState,
    state_evidence: String,
    language: UiLanguage,
}

#[derive(Default)]
struct LocalizedSummaries {
    zh_cn: Option<String>,
    en: Option<String>,
    legacy: Option<String>,
}

impl LocalizedSummaries {
    fn get(&self, language: UiLanguage) -> Option<String> {
        let localized = match language {
            UiLanguage::ZhCn => self.zh_cn.as_ref(),
            UiLanguage::En => self.en.as_ref(),
        };
        localized.or(self.legacy.as_ref()).cloned()
    }
}

#[derive(Default)]
struct SummaryControl {
    // 最近请求的语义键；相同键不会再次推理。
    latest_key: Option<u64>,
    in_flight: bool,
    active_key: Option<u64>,
    // 推理期间只保留最新状态，避免排队处理已经过时的中间帧。
    pending: Option<SummaryJob>,
    // 同一会话回到以前出现过的语义屏幕时直接复用确定性结果。
    cache: HashMap<u64, (String, String)>,
}

#[derive(Clone)]
pub struct SessionManager {
    sessions: Arc<Mutex<HashMap<String, Session>>>,
    language: Arc<Mutex<UiLanguage>>,
}

impl SessionManager {
    pub fn new() -> Self {
        let sessions: Arc<Mutex<HashMap<String, Session>>> = Arc::new(Mutex::new(HashMap::new()));
        let sessions_for_summary = Arc::clone(&sessions);
        let language = Arc::new(Mutex::new(UiLanguage::default()));
        let language_for_summary = Arc::clone(&language);
        // 后台兜底摘要：对没有前端屏幕文本更新的会话（detach 状态），用 buffer 摘要
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(SUMMARY_INTERVAL_SECS));
            summarize_all(&sessions_for_summary, &language_for_summary);
        });
        Self { sessions, language }
    }

    /// 接收 OpenCode plugin / Claude Code hook / Codex hook 的结构化生命周期事件。
    /// 结构化信号直接决定状态，不经过 LLM；屏幕摘要只在事件通道暂时沉默时兜底。
    pub fn report_agent_event(
        &self,
        session_id: &str,
        agent: Option<&str>,
        event: &str,
        summary: Option<&str>,
        summary_zh_cn: Option<&str>,
        summary_en: Option<&str>,
        external_session_id: Option<&str>,
        external_title: Option<&str>,
    ) -> Result<(), String> {
        let mut sessions = self.sessions.lock().map_err(|e| e.to_string())?;
        let sess = sessions
            .get_mut(session_id)
            .ok_or_else(|| "session not found".to_string())?;

        let hinted_agent = AgentKind::parse(agent);
        let current_agent = sess.agent_kind.lock().map(|g| *g).unwrap_or_default();
        let agent = if hinted_agent == AgentKind::Generic {
            current_agent
        } else {
            hinted_agent
        };
        let state = AgentState::from_event(event);
        update_external_metadata(sess, agent, external_session_id, external_title);
        let has_summary = [summary, summary_zh_cn, summary_en]
            .into_iter()
            .flatten()
            .any(|value| !value.trim().is_empty());
        if state == AgentState::Unknown && !has_summary {
            log_summary(&format!(
                "{} [structured:{}] agent={} metadata-only",
                session_id,
                event,
                agent.as_str()
            ));
            return Ok(());
        }
        if let Ok(mut value) = sess.agent_kind.lock() {
            *value = agent;
        }
        if let Ok(mut value) = sess.agent_state.lock() {
            *value = state;
        }
        if let Ok(mut value) = sess.state_source.lock() {
            *value = "structured".to_string();
        }
        if let Ok(mut value) = sess.structured_event_ts.lock() {
            *value = now_ts();
        }
        if let Ok(mut control) = sess.summary_control.lock() {
            // 让已在途的屏幕 LLM 结果失效，避免它在 hook 之后反向覆盖准确状态。
            control.latest_key = None;
            control.pending = None;
        }
        if let Some(status) = state.status() {
            if let Ok(mut value) = sess.status.lock() {
                *value = status.to_string();
            }
        }
        let localized = LocalizedSummaries {
            zh_cn: clean_summary(summary_zh_cn),
            en: clean_summary(summary_en),
            legacy: clean_summary(summary),
        };
        let language = self.language.lock().map(|value| *value).unwrap_or_default();
        let description = localized
            .get(language)
            .unwrap_or_else(|| state.default_summary(agent, language));
        if let Ok(mut value) = sess.structured_summaries.lock() {
            *value = localized;
        }
        if let Ok(mut value) = sess.summary.lock() {
            *value = description.clone();
        }
        log_summary(&format!(
            "{} [structured:{}] agent={} state={} 摘要={}",
            session_id,
            event,
            agent.as_str(),
            state.as_str(),
            description
        ));
        Ok(())
    }

    fn set_language_value(&self, language: UiLanguage) -> Result<(), String> {
        let mut current = self.language.lock().map_err(|e| e.to_string())?;
        if *current == language {
            return Ok(());
        }
        *current = language;
        drop(current);

        let mut sessions = self.sessions.lock().map_err(|e| e.to_string())?;
        for sess in sessions.values_mut() {
            if let Ok(mut control) = sess.summary_control.lock() {
                // Language is part of the semantic cache key. Invalidate the visible key so
                // an in-flight result in the previous language cannot update the card.
                control.latest_key = None;
                control.pending = None;
            }
            // A detached session has no front-end screen sampler. Mark its buffer summary as
            // stale so the background worker regenerates the detailed text in the new language.
            sess.last_buffer_summary_revision
                .store(u64::MAX, Ordering::SeqCst);
            let agent = sess.agent_kind.lock().map(|g| *g).unwrap_or_default();
            let state = sess.agent_state.lock().map(|g| *g).unwrap_or_default();
            let source = sess
                .state_source
                .lock()
                .map(|g| g.clone())
                .unwrap_or_default();
            let description = if source == "structured" {
                sess.structured_summaries
                    .lock()
                    .ok()
                    .and_then(|value| value.get(language))
                    .unwrap_or_else(|| state.default_summary(agent, language))
            } else if state != AgentState::Unknown {
                state.default_summary(agent, language)
            } else {
                terminal_session_summary(language, &sess.command)
            };
            if let Ok(mut summary) = sess.summary.lock() {
                *summary = description;
            }
        }
        Ok(())
    }
}

fn clean_summary(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(truncate_summary)
}

fn terminal_session_summary(language: UiLanguage, command: &str) -> String {
    match language {
        UiLanguage::ZhCn => format!("终端会话：{command}"),
        UiLanguage::En => format!("Terminal session: {command}"),
    }
}

fn truncate_summary(value: &str) -> String {
    const MAX_CHARS: usize = 160;
    let mut result: String = value.chars().take(MAX_CHARS).collect();
    if value.chars().count() > MAX_CHARS {
        result.push('…');
    }
    result
}

#[derive(Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub name: String,
    pub command: String,
    pub alive: bool,
    pub started_at: f64,
    pub status: String,
    pub summary: String,
    pub agent_kind: String,
    pub agent_state: String,
    pub state_source: String,
    pub structured_event_ts: f64,
    pub name_source: String,
    pub external_session_id: Option<String>,
    pub external_title: Option<String>,
    pub summary_language: String,
}

#[tauri::command]
pub fn list_sessions(state: State<'_, SessionManager>) -> Result<Vec<SessionInfo>, String> {
    let sessions = state.sessions.lock().map_err(|e| e.to_string())?;
    let language = state.language.lock().map(|g| *g).unwrap_or_default();
    Ok(sessions
        .values()
        .map(|s| SessionInfo {
            id: s.id.clone(),
            name: s.name.clone(),
            command: s.command.clone(),
            alive: s.alive.load(Ordering::SeqCst),
            started_at: s.started_at,
            status: s
                .status
                .lock()
                .map(|g| g.clone())
                .unwrap_or_else(|_| "ok".to_string()),
            summary: s.summary.lock().map(|g| g.clone()).unwrap_or_default(),
            agent_kind: s
                .agent_kind
                .lock()
                .map(|g| g.as_str().to_string())
                .unwrap_or_else(|_| "generic".to_string()),
            agent_state: s
                .agent_state
                .lock()
                .map(|g| g.as_str().to_string())
                .unwrap_or_else(|_| "unknown".to_string()),
            state_source: s
                .state_source
                .lock()
                .map(|g| g.clone())
                .unwrap_or_else(|_| "screen".to_string()),
            structured_event_ts: s.structured_event_ts.lock().map(|g| *g).unwrap_or(0.0),
            name_source: s
                .name_source
                .lock()
                .map(|g| g.as_str().to_string())
                .unwrap_or_else(|_| "fallback".to_string()),
            external_session_id: s
                .external_session_id
                .lock()
                .map(|g| g.clone())
                .unwrap_or_default(),
            external_title: s
                .external_title
                .lock()
                .map(|g| g.clone())
                .unwrap_or_default(),
            summary_language: language.as_str().to_string(),
        })
        .collect())
}

#[tauri::command]
pub fn create_session(
    app: AppHandle,
    state: State<'_, SessionManager>,
    on_event: Channel,
    id: String,
    command: String,
    name: Option<String>,
    args: Option<Vec<String>>,
    cols: Option<u16>,
    rows: Option<u16>,
) -> Result<(), String> {
    {
        let sessions = state.sessions.lock().map_err(|e| e.to_string())?;
        if let Some(session) = sessions.get(&id) {
            if let Ok(mut guard) = session.output.lock() {
                *guard = Some(on_event);
            }
            return Ok(());
        }
    }

    let cols = cols.unwrap_or(80);
    let rows = rows.unwrap_or(24);
    let args = args.unwrap_or_default();
    let manual_name = name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let name_source = Arc::new(Mutex::new(if manual_name.is_some() {
        NameSource::Manual
    } else {
        NameSource::Fallback
    }));
    let name = manual_name.unwrap_or_else(|| command.clone());

    let (pty, mut reader) = PtyHandle::spawn(command.clone(), args, &id, cols, rows)?;

    let alive = Arc::new(AtomicBool::new(true));
    let status: Arc<Mutex<String>> = Arc::new(Mutex::new("ok".to_string()));
    let output: Arc<Mutex<Option<Channel>>> = Arc::new(Mutex::new(Some(on_event)));
    let language = state
        .language
        .lock()
        .map(|value| *value)
        .unwrap_or_default();
    let summary: Arc<Mutex<String>> =
        Arc::new(Mutex::new(terminal_session_summary(language, &command)));
    let buffer: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let last_screen_ts: Arc<Mutex<f64>> = Arc::new(Mutex::new(0.0));
    let last_output_ts: Arc<Mutex<f64>> = Arc::new(Mutex::new(now_ts()));
    let output_revision = Arc::new(AtomicU64::new(0));
    let last_buffer_summary_revision = Arc::new(AtomicU64::new(0));
    let agent_kind = Arc::new(Mutex::new(AgentKind::from_command(&command)));
    let agent_state = Arc::new(Mutex::new(AgentState::Unknown));
    let state_source = Arc::new(Mutex::new("screen".to_string()));
    let structured_event_ts = Arc::new(Mutex::new(0.0));
    let structured_summaries = Arc::new(Mutex::new(LocalizedSummaries::default()));
    let external_session_id = Arc::new(Mutex::new(None));
    let external_title = Arc::new(Mutex::new(None));
    let summary_control = Arc::new(Mutex::new(SummaryControl::default()));
    let output_for_reader = Arc::clone(&output);
    let buffer_for_reader = Arc::clone(&buffer);
    let last_output_ts_for_reader = Arc::clone(&last_output_ts);
    let output_revision_for_reader = Arc::clone(&output_revision);
    let alive_for_reader = Arc::clone(&alive);
    let session_id = id.clone();
    let app_for_reader = app.clone();

    std::thread::spawn(move || {
        let mut buf = [0u8; 16384];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let bytes = buf[..n].to_vec();
                    output_revision_for_reader.fetch_add(1, Ordering::SeqCst);
                    if let Ok(mut ts) = last_output_ts_for_reader.lock() {
                        *ts = now_ts();
                    }
                    if let Ok(mut bg) = buffer_for_reader.lock() {
                        bg.extend_from_slice(&bytes);
                        if bg.len() > BUFFER_TAIL_BYTES {
                            let drain = bg.len() - BUFFER_TAIL_BYTES;
                            bg.drain(0..drain);
                        }
                    }
                    if let Ok(guard) = output_for_reader.lock() {
                        if let Some(ch) = guard.as_ref() {
                            let _ = ch.send(InvokeResponseBody::Raw(bytes));
                        }
                    }
                }
                Err(_) => break,
            }
        }
        alive_for_reader.store(false, Ordering::SeqCst);
        let _ = app_for_reader.emit(&format!("pty_exit:{}", session_id), ());
    });

    let session = Session {
        id: id.clone(),
        name,
        command,
        name_source,
        external_session_id,
        external_title,
        started_at: now_ts(),
        alive,
        status,
        output,
        summary,
        buffer,
        last_screen_ts,
        last_output_ts,
        output_revision,
        last_buffer_summary_revision,
        agent_kind,
        agent_state,
        state_source,
        structured_event_ts,
        structured_summaries,
        summary_control,
        pty,
    };
    state
        .sessions
        .lock()
        .map_err(|e| e.to_string())?
        .insert(id, session);
    Ok(())
}

/// 前端提取并稳定化 xterm.js 语义屏幕后调用。
#[tauri::command]
pub fn summarize_screen(
    state: State<'_, SessionManager>,
    id: String,
    text: String,
    agent_kind: Option<String>,
    state_hint: Option<String>,
    state_evidence: Option<String>,
    language: Option<String>,
) -> Result<(), String> {
    let clean = strip_ansi(&text).trim().to_string();
    if clean.is_empty() {
        return Ok(());
    }

    let language = language
        .as_deref()
        .map(|value| UiLanguage::parse(Some(value)))
        .unwrap_or_else(|| state.language.lock().map(|g| *g).unwrap_or_default());
    state.set_language_value(language)?;
    let sessions = Arc::clone(&state.sessions);

    let job_to_spawn = {
        let mut sessions_guard = sessions.lock().map_err(|e| e.to_string())?;
        let sess = sessions_guard
            .get_mut(&id)
            .ok_or_else(|| "session not found".to_string())?;
        // 收到屏幕状态时立即记为 fresh，不能等 API 成功后再记，否则后端兜底会并发重复摘要。
        if let Ok(mut ts) = sess.last_screen_ts.lock() {
            *ts = now_ts();
        }
        let hinted_agent = AgentKind::parse(agent_kind.as_deref());
        let current_agent = sess.agent_kind.lock().map(|g| *g).unwrap_or_default();
        let agent = if hinted_agent == AgentKind::Generic {
            current_agent
        } else {
            hinted_agent
        };
        let state = AgentState::parse(state_hint.as_deref());
        if let Ok(mut value) = sess.agent_kind.lock() {
            *value = agent;
        }

        let structured_recent = sess
            .structured_event_ts
            .lock()
            .map(|ts| now_ts() - *ts < STRUCTURED_EVENT_GRACE_SECS)
            .unwrap_or(false);
        if structured_recent {
            return Ok(());
        }

        let previous_state = sess.agent_state.lock().map(|g| *g).unwrap_or_default();
        if state != AgentState::Unknown {
            if let Ok(mut value) = sess.agent_state.lock() {
                *value = state;
            }
            if let Ok(mut value) = sess.state_source.lock() {
                *value = "screen_adapter".to_string();
            }
            if let Some(status) = state.status() {
                if let Ok(mut value) = sess.status.lock() {
                    *value = status.to_string();
                }
            }
            if state != previous_state {
                if let Ok(mut value) = sess.summary.lock() {
                    *value = state.default_summary(agent, language);
                }
            }
        }

        let key = semantic_key(agent, state, language, &clean);
        let job = SummaryJob {
            key,
            text: clean.clone(),
            agent,
            state,
            state_evidence: clean_metadata(state_evidence.as_deref(), 240)
                .unwrap_or_else(|| "none".to_string()),
            language,
        };
        let mut control = sess.summary_control.lock().map_err(|e| e.to_string())?;
        if control.latest_key == Some(key) {
            return Ok(());
        }
        control.latest_key = Some(key);
        if let Some((status, desc)) = control.cache.get(&key).cloned() {
            // 已有缓存的状态立即生效，之前排队但尚未开始的中间帧已过时。
            control.pending = None;
            drop(control);
            if let Ok(mut g) = sess.summary.lock() {
                *g = desc;
            }
            if let Ok(mut st) = sess.status.lock() {
                *st = status;
            }
            None
        } else if control.in_flight {
            if control.active_key == Some(key) {
                // 屏幕回到当前正在推理的状态，不要把同一个 key 再排一次。
                control.pending = None;
            } else {
                control.pending = Some(job.clone());
            }
            None
        } else {
            control.in_flight = true;
            control.active_key = Some(key);
            Some(job)
        }
    };

    if let Some(job) = job_to_spawn {
        spawn_summary_worker(sessions, id, job);
    }
    Ok(())
}

fn semantic_key(agent: AgentKind, state: AgentState, language: UiLanguage, text: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    agent.hash(&mut hasher);
    state.hash(&mut hasher);
    language.hash(&mut hasher);
    text.hash(&mut hasher);
    hasher.finish()
}

/// 每个会话最多一个 worker。新状态到达时覆盖 pending，旧结果不会覆盖新卡片。
fn spawn_summary_worker(
    sessions: Arc<Mutex<HashMap<String, Session>>>,
    id: String,
    initial_job: SummaryJob,
) {
    std::thread::spawn(move || {
        let mut job = initial_job;
        loop {
            let ts = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            log_screen(&format!(
                "========== [ts={}] {} (语义屏幕 {}字, key={:016x}, agent={}, state={}, language={}) ==========\n--- 状态证据 ---\n{}\n--- 语义截取 ---\n{}\n--- 模型输出 ---",
                ts,
                id,
                job.text.chars().count(),
                job.key,
                job.agent.as_str(),
                job.state.as_str(),
                job.language.as_str(),
                job.state_evidence,
                job.text
            ));

            let model_input = llm_input(job.agent, job.state, job.language, &job.text);
            let outcome = crate::llm::summarize(&model_input, job.language).map(|result| {
                log_screen(&format!("{}\n", result));
                let (mut status, description) = parse_summary(&result);
                if let Some(authoritative) = job.state.status() {
                    status = authoritative.to_string();
                }
                let description = if description.trim().is_empty() {
                    job.state.default_summary(job.agent, job.language)
                } else {
                    description
                };
                (status, description)
            });
            if let Err(error) = &outcome {
                log_screen(&format!("失败: {}\n", error));
                log_summary(&format!("{} [screen] 失败={}", id, error));
            }

            let next = {
                let mut sessions_guard = match sessions.lock() {
                    Ok(guard) => guard,
                    Err(_) => return,
                };
                let Some(sess) = sessions_guard.get_mut(&id) else {
                    return;
                };
                let mut control = match sess.summary_control.lock() {
                    Ok(guard) => guard,
                    Err(_) => return,
                };

                if let Ok((status, desc)) = outcome {
                    if control.cache.len() >= SUMMARY_CACHE_SIZE {
                        control.cache.clear();
                    }
                    control
                        .cache
                        .insert(job.key, (status.clone(), desc.clone()));
                    log_summary(&format!("{} [screen] 状态={} 摘要={}", id, status, desc));
                    // 只允许当前最新语义状态更新卡片，防止慢请求乱序覆盖。
                    if control.latest_key == Some(job.key) {
                        if let Ok(mut g) = sess.summary.lock() {
                            *g = desc;
                        }
                        if let Ok(mut st) = sess.status.lock() {
                            *st = status;
                        }
                    }
                }

                let pending = control.pending.take();
                if let Some(next) = &pending {
                    control.active_key = Some(next.key);
                } else {
                    control.in_flight = false;
                    control.active_key = None;
                }
                pending
            };

            match next {
                Some(pending) => job = pending,
                None => break,
            }
        }
    });
}

/// 解析模型输出 "级别|描述" -> (status, desc)，含关键词兜底
fn parse_summary(result: &str) -> (String, String) {
    let (level, desc) = if let Some(idx) = result.find('|') {
        (
            result[..idx].trim().to_lowercase(),
            result[idx + 1..].trim().to_string(),
        )
    } else {
        (String::new(), result.trim().to_string())
    };
    let status = match level.as_str() {
        "ok" => "ok",
        "idle" => "idle",
        "warn" => "warn",
        "err" | "error" => "err",
        _ => {
            let d = desc.to_lowercase();
            if d.contains("等待输入")
                || d.contains("waiting for input")
                || d.contains("awaiting input")
                || d.contains("completed")
                || d.contains("complete")
                || d.contains("完成")
                || d.contains("done")
                || d.contains("idle")
            {
                "idle"
            } else if d.contains("密码")
                || d.contains("错误")
                || d.contains("失败")
                || d.contains("error")
                || d.contains("fail")
                || d.contains("确认")
                || d.contains("介入")
                || d.contains("需要输入")
                || d.contains("请输入")
                || d.contains("password")
                || d.contains("approval")
                || d.contains("permission")
                || d.contains("needs attention")
            {
                "err"
            } else if d.contains("警告")
                || d.contains("warn")
                || d.contains("retry")
                || d.contains("重试")
            {
                "warn"
            } else if d.contains("执行")
                || d.contains("运行")
                || d.contains("正在")
                || d.contains("working")
                || d.contains("running")
                || d.contains("executing")
            {
                "ok"
            } else {
                "warn"
            }
        }
    }
    .to_string();
    (status, desc)
}

/// 兜底摘要：只处理真正 detach、输出已经安静且自上次摘要后有新输出的会话。
fn summarize_all(
    sessions: &Arc<Mutex<HashMap<String, Session>>>,
    language: &Arc<Mutex<UiLanguage>>,
) {
    let now = now_ts();
    let language = language.lock().map(|value| *value).unwrap_or_default();
    let snapshot: Vec<(String, Vec<u8>, u64, AgentKind, AgentState)> = {
        let s = match sessions.lock() {
            Ok(s) => s,
            Err(_) => return,
        };
        s.values()
            .filter(|sess| sess.alive.load(Ordering::SeqCst))
            .filter(|sess| sess.output.lock().map(|g| g.is_none()).unwrap_or(false))
            .filter(|sess| {
                let ts = sess.last_screen_ts.lock().map(|g| *g).unwrap_or(0.0);
                now - ts > SCREEN_SUMMARY_FRESH_SECS
            })
            .filter(|sess| {
                let ts = sess.last_output_ts.lock().map(|g| *g).unwrap_or(now);
                now - ts >= DETACHED_QUIET_SECS
            })
            .filter(|sess| {
                let ts = sess.structured_event_ts.lock().map(|g| *g).unwrap_or(0.0);
                now - ts >= STRUCTURED_EVENT_GRACE_SECS
            })
            .filter(|sess| {
                sess.output_revision.load(Ordering::SeqCst)
                    != sess.last_buffer_summary_revision.load(Ordering::SeqCst)
            })
            .map(|sess| {
                let buf = sess.buffer.lock().map(|g| g.clone()).unwrap_or_default();
                let revision = sess.output_revision.load(Ordering::SeqCst);
                let agent = sess.agent_kind.lock().map(|g| *g).unwrap_or_default();
                let state = sess.agent_state.lock().map(|g| *g).unwrap_or_default();
                (sess.id.clone(), buf, revision, agent, state)
            })
            .collect()
    };
    for (id, buf, revision, agent, state) in snapshot {
        if buf.is_empty() {
            continue;
        }
        let text = String::from_utf8_lossy(&buf);
        let clean = strip_ansi(&text);
        let clean = clean.trim();
        if clean.is_empty() {
            continue;
        }
        match crate::llm::summarize(&llm_input(agent, state, language, clean), language) {
            Ok(result) => {
                let (mut status, desc) = parse_summary(&result);
                if let Some(authoritative) = state.status() {
                    status = authoritative.to_string();
                }
                log_summary(&format!("{} [buffer] 状态={} 摘要={}", id, status, desc));
                if let Ok(mut s) = sessions.lock() {
                    if let Some(sess) = s.get_mut(&id) {
                        if let Ok(mut g) = sess.summary.lock() {
                            *g = desc;
                        }
                        if let Ok(mut st) = sess.status.lock() {
                            *st = status;
                        }
                        sess.last_buffer_summary_revision
                            .store(revision, Ordering::SeqCst);
                        if let Ok(mut ts) = sess.last_screen_ts.lock() {
                            *ts = now_ts();
                        }
                    }
                }
            }
            Err(e) => log_summary(&format!("{} [buffer] 失败={}", id, e)),
        }
    }
}

#[tauri::command]
pub fn set_language(state: State<'_, SessionManager>, language: String) -> Result<(), String> {
    state.set_language_value(UiLanguage::parse(Some(&language)))
}

fn log_summary(msg: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(diagnostic_log_path("summary_debug.log"))
    {
        use std::io::Write;
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(f, "[{}] {}", t, msg);
    }
}

fn log_screen(msg: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(diagnostic_log_path("screen_capture.log"))
    {
        use std::io::Write;
        let _ = writeln!(f, "{}", msg);
    }
}

fn diagnostic_log_path(file_name: &str) -> std::path::PathBuf {
    static LOG_DIR: OnceLock<std::path::PathBuf> = OnceLock::new();
    let dir = LOG_DIR.get_or_init(|| {
        let path = std::env::var_os("AGENT_DASHBOARD_LOG_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::env::current_dir()
                    .unwrap_or_else(|_| std::path::PathBuf::from("."))
                    .join("runtime")
                    .join("logs")
            });
        let _ = std::fs::create_dir_all(&path);
        path
    });
    dir.join(file_name)
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    while let Some(&cc) = chars.peek() {
                        chars.next();
                        if cc.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                Some(_) => {
                    chars.next();
                }
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[tauri::command]
pub fn detach_session(state: State<'_, SessionManager>, id: String) -> Result<(), String> {
    let sessions = state.sessions.lock().map_err(|e| e.to_string())?;
    if let Some(session) = sessions.get(&id) {
        if let Ok(mut guard) = session.output.lock() {
            *guard = None;
        }
    }
    Ok(())
}

#[tauri::command]
pub fn rename_session(
    state: State<'_, SessionManager>,
    id: String,
    name: String,
) -> Result<(), String> {
    let mut sessions = state.sessions.lock().map_err(|e| e.to_string())?;
    if let Some(session) = sessions.get_mut(&id) {
        session.name = name;
        if let Ok(mut source) = session.name_source.lock() {
            *source = NameSource::Manual;
        }
        Ok(())
    } else {
        Err("session not found".to_string())
    }
}

#[tauri::command]
pub fn update_session_metadata(
    state: State<'_, SessionManager>,
    id: String,
    agent_kind: Option<String>,
    external_session_id: Option<String>,
    external_title: Option<String>,
) -> Result<(), String> {
    let mut sessions = state.sessions.lock().map_err(|e| e.to_string())?;
    let sess = sessions
        .get_mut(&id)
        .ok_or_else(|| "session not found".to_string())?;
    let hinted_agent = AgentKind::parse(agent_kind.as_deref());
    let current_agent = sess.agent_kind.lock().map(|g| *g).unwrap_or_default();
    let agent = if hinted_agent == AgentKind::Generic {
        current_agent
    } else {
        hinted_agent
    };
    update_external_metadata(
        sess,
        agent,
        external_session_id.as_deref(),
        external_title.as_deref(),
    );
    Ok(())
}

fn update_external_metadata(
    sess: &mut Session,
    agent: AgentKind,
    external_session_id: Option<&str>,
    external_title: Option<&str>,
) {
    if agent != AgentKind::Generic {
        if let Ok(mut value) = sess.agent_kind.lock() {
            *value = agent;
        }
    }
    if let Some(value) = clean_metadata(external_session_id, 160) {
        if let Ok(mut target) = sess.external_session_id.lock() {
            *target = Some(value);
        }
    }
    let Some(title) = clean_metadata(external_title, 120) else {
        return;
    };
    if let Ok(mut target) = sess.external_title.lock() {
        *target = Some(title.clone());
    }
    if let Ok(mut source) = sess.name_source.lock() {
        apply_external_title(&mut sess.name, &mut source, title);
    }
}

fn apply_external_title(name: &mut String, source: &mut NameSource, title: String) {
    if *source != NameSource::Manual {
        *name = title;
        *source = NameSource::Agent;
    }
}

fn clean_metadata(value: Option<&str>, max_chars: usize) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    let cleaned: String = value
        .chars()
        .filter(|c| !c.is_control())
        .take(max_chars)
        .collect();
    (!cleaned.trim().is_empty()).then(|| cleaned.trim().to_string())
}

#[cfg(test)]
mod metadata_tests {
    use super::*;

    #[test]
    fn agent_title_replaces_only_non_manual_name() {
        let mut fallback_name = "codex".to_string();
        let mut fallback_source = NameSource::Fallback;
        apply_external_title(
            &mut fallback_name,
            &mut fallback_source,
            "修复登录流程".to_string(),
        );
        assert_eq!(fallback_name, "修复登录流程");
        assert_eq!(fallback_source, NameSource::Agent);

        let mut manual_name = "我的任务".to_string();
        let mut manual_source = NameSource::Manual;
        apply_external_title(
            &mut manual_name,
            &mut manual_source,
            "CLI 自动标题".to_string(),
        );
        assert_eq!(manual_name, "我的任务");
        assert_eq!(manual_source, NameSource::Manual);
    }

    #[test]
    fn metadata_is_trimmed_and_control_chars_removed() {
        assert_eq!(
            clean_metadata(Some("  task\nname  "), 20).as_deref(),
            Some("taskname")
        );
        assert_eq!(clean_metadata(Some("abcdef"), 3).as_deref(), Some("abc"));
        assert_eq!(clean_metadata(Some("  "), 10), None);
    }

    #[test]
    fn parses_english_and_chinese_fallback_summaries() {
        assert_eq!(parse_summary("Waiting for input").0, "idle");
        assert_eq!(parse_summary("Waiting for user approval").0, "err");
        assert_eq!(parse_summary("正在重试 API 请求").0, "warn");
        assert_eq!(parse_summary("Running the test suite").0, "ok");
    }
}

#[tauri::command]
pub fn pty_input(state: State<'_, SessionManager>, id: String, data: String) -> Result<(), String> {
    let sessions = state.sessions.lock().map_err(|e| e.to_string())?;
    let session = sessions
        .get(&id)
        .ok_or_else(|| "session not found".to_string())?;
    let bytes = B64.decode(data.as_bytes()).map_err(|e| e.to_string())?;
    session.pty.write(&bytes)
}

#[tauri::command]
pub fn pty_resize(
    state: State<'_, SessionManager>,
    id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let sessions = state.sessions.lock().map_err(|e| e.to_string())?;
    let session = sessions
        .get(&id)
        .ok_or_else(|| "session not found".to_string())?;
    session.pty.resize(cols, rows)
}

#[tauri::command]
pub fn close_session(state: State<'_, SessionManager>, id: String) -> Result<(), String> {
    let mut sessions = state.sessions.lock().map_err(|e| e.to_string())?;
    if let Some(session) = sessions.remove(&id) {
        let _ = session.pty.kill();
    }
    Ok(())
}
