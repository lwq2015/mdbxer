//! 打开历史记录：JSON 持久化到 %APPDATA%\mdbxer\history.json。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const MAX_ENTRIES: usize = 20;

#[derive(Serialize, Deserialize, Clone)]
pub struct HistoryEntry {
    pub path: String,
    /// "auto" | "file" | "dir"
    pub mode: String,
    /// Unix 秒
    pub last_open: u64,
}

pub struct History {
    pub entries: Vec<HistoryEntry>,
    file: Option<PathBuf>,
}

impl History {
    pub fn load() -> Self {
        let file = storage_path();
        if let Some(f) = &file {
            if let Ok(text) = std::fs::read_to_string(f) {
                if let Ok(entries) = serde_json::from_str(&text) {
                    return Self { entries, file };
                }
            }
        }
        Self {
            entries: Vec::new(),
            file,
        }
    }

    /// 记录一次打开：同路径去重置顶，最多保留 MAX_ENTRIES 条。
    pub fn add(&mut self, path: &str, mode: &str) {
        self.entries.retain(|e| e.path != path);
        let last_open = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.entries.insert(
            0,
            HistoryEntry {
                path: path.to_string(),
                mode: mode.to_string(),
                last_open,
            },
        );
        self.entries.truncate(MAX_ENTRIES);
        self.save();
    }

    pub fn remove(&mut self, index: usize) {
        if index < self.entries.len() {
            self.entries.remove(index);
            self.save();
        }
    }

    /// 写盘失败静默忽略（历史退化为本次会话内存记录）。
    fn save(&self) {
        if let Some(f) = &self.file {
            if let Some(dir) = f.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(text) = serde_json::to_string_pretty(&self.entries) {
                let _ = std::fs::write(f, text);
            }
        }
    }
}

fn storage_path() -> Option<PathBuf> {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return Some(PathBuf::from(appdata).join("mdbxer").join("history.json"));
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("mdbxer-history.json")))
}
