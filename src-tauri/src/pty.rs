use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

pub struct PtyHandle {
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    child: Arc<Mutex<Option<Box<dyn Child + Send + Sync>>>>,
}

impl PtyHandle {
    pub fn spawn(
        command: String,
        args: Vec<String>,
        dashboard_session_id: &str,
        cols: u16,
        rows: u16,
    ) -> Result<(Self, Box<dyn Read + Send>), String> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;

        let mut cmd = CommandBuilder::new(&command);
        // TUI 程序（opencode/codex 等）依赖 TERM 判断终端能力，必须设置
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("AGENT_DASHBOARD_SESSION_ID", dashboard_session_id);
        cmd.env(
            "AGENT_DASHBOARD_EVENT_URL",
            "http://127.0.0.1:8787/api/agent-event",
        );
        if let Ok(cwd) = std::env::current_dir() {
            cmd.env(
                "AGENT_DASHBOARD_HOOK",
                cwd.join("integrations").join("agent_dashboard_hook.py"),
            );
        }
        // 用户常在本面板的 pwsh 中进入 WSL 再启动 agent。通过 WSLENV 显式传递
        // 关联信息，WSL 内的 OpenCode/Claude Code/Codex hook 仍能找到对应卡片。
        let mut wslenv = std::env::var("WSLENV").unwrap_or_default();
        for name in [
            "AGENT_DASHBOARD_SESSION_ID",
            "AGENT_DASHBOARD_EVENT_URL",
            "AGENT_DASHBOARD_HOOK/p",
        ] {
            if !wslenv.split(':').any(|item| item == name) {
                if !wslenv.is_empty() {
                    wslenv.push(':');
                }
                wslenv.push_str(name);
            }
        }
        cmd.env("WSLENV", wslenv);
        for a in args {
            cmd.arg(a);
        }

        let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        // 释放 slave，否则 reader 永远读不到 EOF
        drop(pair.slave);

        let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        let master = pair.master;

        Ok((
            Self {
                writer: Mutex::new(writer),
                master: Mutex::new(master),
                child: Arc::new(Mutex::new(Some(child))),
            },
            reader,
        ))
    }

    pub fn write(&self, data: &[u8]) -> Result<(), String> {
        let mut w = self.writer.lock().map_err(|e| e.to_string())?;
        w.write_all(data).map_err(|e| e.to_string())?;
        w.flush().map_err(|e| e.to_string())
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), String> {
        let m = self.master.lock().map_err(|e| e.to_string())?;
        m.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())
    }

    pub fn kill(&self) -> Result<(), String> {
        let mut c = self.child.lock().map_err(|e| e.to_string())?;
        if let Some(child) = c.as_mut() {
            let _ = child.kill();
        }
        Ok(())
    }
}
