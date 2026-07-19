mod agent_support;
mod api;
pub mod llm;
pub use agent_support::UiLanguage;
mod pty;
mod session;
mod store;

use std::sync::Arc;
use std::time::Duration;

pub fn run() {
    // 读 .env（exe 旁或 cwd）
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let _ = dotenvy::from_path(dir.join(".env"));
        }
    }
    let _ = dotenvy::dotenv();
    let store = Arc::new(store::Store::load());
    let session_manager = session::SessionManager::new();

    // 独立线程 + tokio runtime 跑 axum HTTP 服务（托管前端 + 状态 API）
    let store_for_server = store.clone();
    let sessions_for_server = session_manager.clone();
    let (server_ready_tx, server_ready_rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");
        runtime.block_on(async move {
            api::serve(store_for_server, sessions_for_server, server_ready_tx).await;
        });
    });

    // 只接受本进程的 bind 成功信号。旧实例占用 8787 时不能把“端口可连接”
    // 误判为新服务已就绪，否则新窗口会继续加载旧版 dashboard/xterm。
    match server_ready_rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(())) => {}
        Ok(Err(error)) => panic!("{error}"),
        Err(error) => panic!("HTTP server did not become ready: {error}"),
    }

    tauri::Builder::default()
        .manage(session_manager)
        .invoke_handler(tauri::generate_handler![
            session::create_session,
            session::list_sessions,
            session::detach_session,
            session::rename_session,
            session::update_session_metadata,
            session::set_language,
            session::summarize_screen,
            session::pty_input,
            session::pty_resize,
            session::close_session,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
