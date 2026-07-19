use crate::session::SessionManager;
use crate::store::Store;
use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::mpsc::SyncSender;
use std::sync::Arc;

#[derive(Clone)]
struct ApiState {
    store: Arc<Store>,
    sessions: SessionManager,
}

// 前端页面在编译时嵌入，单 exe 自包含
// dashboard.html 是前端的唯一源文件。不要从 dist/index.html 嵌入，
// 否则未手工复制到 dist 的终端修复在 Tauri 中会完全不生效。
const INDEX_HTML: &str = include_str!("../../dashboard.html");

pub async fn serve(
    store: Arc<Store>,
    sessions: SessionManager,
    ready: SyncSender<Result<(), String>>,
) {
    let state = ApiState { store, sessions };
    let app = Router::new()
        .route("/", get(index))
        .route("/dashboard.html", get(index))
        .route("/api/status", get(status))
        .route("/api/report", post(report))
        .route("/api/remove", get(remove))
        .route("/api/agent-event", post(agent_event))
        .with_state(state);

    let listener = match tokio::net::TcpListener::bind("127.0.0.1:8787").await {
        Ok(listener) => listener,
        Err(error) => {
            let message = format!("failed to bind 127.0.0.1:8787: {error}");
            let _ = ready.send(Err(message.clone()));
            eprintln!("{message}");
            return;
        }
    };
    let _ = ready.send(Ok(()));
    if let Err(error) = axum::serve(listener, app).await {
        eprintln!("HTTP server stopped: {error}");
    }
}

async fn index() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        INDEX_HTML,
    )
}

async fn status(State(state): State<ApiState>) -> impl IntoResponse {
    let value = state.store.all();
    (
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        Json(value),
    )
}

#[derive(Deserialize)]
struct ReportBody {
    task_id: String,
    node: String,
    cli: String,
    summary: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    history_line: Option<String>,
}

async fn report(State(state): State<ApiState>, Json(body): Json<ReportBody>) -> impl IntoResponse {
    let status = body.status.unwrap_or_else(|| "ok".to_string());
    state.store.report(
        body.task_id,
        body.node,
        body.cli,
        body.summary,
        status,
        body.history_line,
    );
    StatusCode::OK
}

#[derive(Deserialize)]
struct RemoveQuery {
    id: String,
}

async fn remove(State(state): State<ApiState>, Query(q): Query<RemoveQuery>) -> impl IntoResponse {
    state.store.remove(&q.id);
    StatusCode::OK
}

#[derive(Deserialize)]
struct AgentEventBody {
    dashboard_session_id: String,
    #[serde(default)]
    agent: Option<String>,
    event: String,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    summary_zh_cn: Option<String>,
    #[serde(default)]
    summary_en: Option<String>,
    #[serde(default)]
    agent_session_id: Option<String>,
    #[serde(default)]
    session_title: Option<String>,
}

#[derive(Serialize)]
struct AgentEventResponse {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

async fn agent_event(
    State(state): State<ApiState>,
    Json(body): Json<AgentEventBody>,
) -> impl IntoResponse {
    match state.sessions.report_agent_event(
        &body.dashboard_session_id,
        body.agent.as_deref(),
        &body.event,
        body.summary.as_deref(),
        body.summary_zh_cn.as_deref(),
        body.summary_en.as_deref(),
        body.agent_session_id.as_deref(),
        body.session_title.as_deref(),
    ) {
        Ok(()) => (
            StatusCode::OK,
            Json(AgentEventResponse {
                ok: true,
                error: None,
            }),
        ),
        Err(error) => (
            StatusCode::NOT_FOUND,
            Json(AgentEventResponse {
                ok: false,
                error: Some(error),
            }),
        ),
    }
}
