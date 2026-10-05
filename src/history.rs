// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 打开历史记录：JSON 持久化到 %APPDATA%\mdbxer\history.json。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const MAX_ENTRIES: usize = 20;

/// 一条历史记录。
#[derive(Serialize, Deserialize, Clone)]
pub struct HistoryEntry {
    /// 数据库路径（文件或目录）
    pub path: String,
    /// "auto" | "file" | "dir"
    pub mode: String,
    /// Unix 秒
    pub last_open: u64,
}

/// 历史记录集合；`file` 为 None 时仅存在于内存（不写盘）。
pub struct History {
    /// 最近打开在前，最多 MAX_ENTRIES 条
    pub entries: Vec<HistoryEntry>,
    /// 持久化文件路径；取不到 %APPDATA% 也无 exe 目录时为 None
    file: Option<PathBuf>,
}

impl History {
    /// 从磁盘加载；文件缺失或 JSON 损坏时返回空历史。
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
    /// `mode` 取 [`crate::db::OpenMode::as_str`]。
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

    /// 删除第 `index` 条并写盘；越界时忽略。
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

/// 存储路径：优先 `%APPDATA%\mdbxer\history.json`；
/// 无 %APPDATA%（非 Windows）时退化为 exe 旁的 `mdbxer-history.json`。
fn storage_path() -> Option<PathBuf> {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return Some(PathBuf::from(appdata).join("mdbxer").join("history.json"));
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("mdbxer-history.json")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_history() -> (History, std::path::PathBuf) {
        let ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("mdbxer_test_{ns}"));
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("history.json");
        let _ = fs::remove_file(&file);
        (
            History {
                entries: Vec::new(),
                file: Some(file.clone()),
            },
            file,
        )
    }

    #[test]
    fn add_appends_to_front() {
        let (mut h, _) = temp_history();
        h.add("/a", "file");
        h.add("/b", "dir");
        assert_eq!(h.entries.len(), 2);
        assert_eq!(h.entries[0].path, "/b");
        assert_eq!(h.entries[1].path, "/a");
    }

    #[test]
    fn add_dedup_same_path_moves_to_front() {
        let (mut h, _) = temp_history();
        h.add("/a", "file");
        h.add("/b", "dir");
        h.add("/a", "dir"); // 重新打开 /a，mode 也应更新
        assert_eq!(h.entries.len(), 2);
        assert_eq!(h.entries[0].path, "/a");
        assert_eq!(h.entries[0].mode, "dir");
        assert_eq!(h.entries[1].path, "/b");
    }

    #[test]
    fn add_truncates_to_max_entries() {
        let (mut h, _) = temp_history();
        for i in 0..(MAX_ENTRIES + 5) {
            h.add(&format!("/p{i}"), "file");
        }
        assert_eq!(h.entries.len(), MAX_ENTRIES);
        // 最新的在最前
        assert_eq!(h.entries[0].path, format!("/p{}", MAX_ENTRIES + 4));
    }

    #[test]
    fn remove_valid_index() {
        let (mut h, _) = temp_history();
        h.add("/a", "file");
        h.add("/b", "file");
        h.remove(0);
        assert_eq!(h.entries.len(), 1);
        assert_eq!(h.entries[0].path, "/a");
    }

    #[test]
    fn remove_out_of_bounds_is_noop() {
        let (mut h, _) = temp_history();
        h.add("/a", "file");
        h.remove(5); // 越界，忽略
        assert_eq!(h.entries.len(), 1);
        h.remove(0);
        h.remove(0); // 已空，忽略
        assert!(h.entries.is_empty());
    }

    #[test]
    fn add_persists_to_disk() {
        let (mut h, file) = temp_history();
        h.add("/a", "file");
        h.add("/b", "dir");
        // 磁盘文件应存在且可反序列化
        let text = fs::read_to_string(&file).expect("file should be written");
        let loaded: Vec<HistoryEntry> = serde_json::from_str(&text).expect("valid json");
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].path, "/b");
        assert_eq!(loaded[1].path, "/a");
    }

    #[test]
    fn remove_persists_to_disk() {
        let (mut h, file) = temp_history();
        h.add("/a", "file");
        h.add("/b", "file");
        h.remove(1); // 删 /a
        let text = fs::read_to_string(&file).unwrap();
        let loaded: Vec<HistoryEntry> = serde_json::from_str(&text).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].path, "/b");
    }

    #[test]
    fn history_entry_serialization_round_trip() {
        let entry = HistoryEntry {
            path: "/test/db".to_string(),
            mode: "auto".to_string(),
            last_open: 1234567890,
        };
        let json = serde_json::to_string(&entry).unwrap();
        let back: HistoryEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.path, entry.path);
        assert_eq!(back.mode, entry.mode);
        assert_eq!(back.last_open, entry.last_open);
    }
}
