use chrono::Local;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const STALE_SECONDS: f64 = 180.0;
const MAX_HISTORY: usize = 50;

#[derive(Clone, Serialize, Deserialize)]
pub struct Entry {
    pub node: String,
    pub cli: String,
    pub summary: String,
    pub status: String,
    pub last_seen: f64,
    pub updated_at: f64,
    pub history: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub stale: Option<bool>,
}

pub struct Store {
    data: Mutex<HashMap<String, Entry>>,
    path: PathBuf,
}

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn hhmmss() -> String {
    Local::now().format("%H:%M:%S").to_string()
}

fn default_path() -> PathBuf {
    config_dir().join("status.json")
}

// 状态文件放在 exe 旁边，便于便携运行
fn config_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.to_path_buf();
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

impl Store {
    pub fn load() -> Self {
        let path = default_path();
        let data = fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<HashMap<String, Entry>>(&s).ok())
            .unwrap_or_default();
        Self {
            data: Mutex::new(data),
            path,
        }
    }

    pub fn report(
        &self,
        task_id: String,
        node: String,
        cli: String,
        summary: String,
        status: String,
        history_line: Option<String>,
    ) {
        let mut data = self.data.lock().unwrap();
        let now = now_ts();
        let prev = data.get(&task_id);
        let summary_changed = prev.map_or(true, |p| p.summary != summary);
        let mut entry = prev.cloned().unwrap_or_else(|| Entry {
            node: String::new(),
            cli: String::new(),
            summary: String::new(),
            status: String::new(),
            last_seen: 0.0,
            updated_at: 0.0,
            history: Vec::new(),
            stale: None,
        });
        entry.node = node;
        entry.cli = cli;
        entry.summary = summary.clone();
        entry.status = status;
        entry.last_seen = now;
        entry.stale = None;
        if summary_changed {
            entry.updated_at = now;
            let line = history_line.unwrap_or(summary);
            entry.history.push(format!("[{}] {}", hhmmss(), line));
            if entry.history.len() > MAX_HISTORY {
                let extra = entry.history.len() - MAX_HISTORY;
                entry.history.drain(0..extra);
            }
        }
        data.insert(task_id, entry);
        self.persist(&data);
    }

    pub fn all(&self) -> serde_json::Value {
        let mut data = self.data.lock().unwrap();
        let now = now_ts();
        for e in data.values_mut() {
            if e.status == "ok" && now - e.last_seen > STALE_SECONDS {
                e.status = "warn".to_string();
                e.stale = Some(true);
            }
        }
        let mut map = serde_json::Map::new();
        for (k, v) in data.iter() {
            map.insert(k.clone(), serde_json::to_value(v).unwrap());
        }
        serde_json::Value::Object(map)
    }

    pub fn remove(&self, task_id: &str) {
        let mut data = self.data.lock().unwrap();
        data.remove(task_id);
        self.persist(&data);
    }

    fn persist(&self, data: &HashMap<String, Entry>) {
        let tmp = self.path.with_extension("tmp");
        if let Ok(json) = serde_json::to_string_pretty(data) {
            if fs::write(&tmp, json).is_ok() {
                let _ = fs::rename(&tmp, &self.path);
            }
        }
    }
}
