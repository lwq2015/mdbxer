// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 打开历史记录：JSON 持久化到 %APPDATA%\mdbxer\history.json。

use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

const MAX_ENTRIES: usize = 20;

/// 归一化路径用于存储与比较：
/// - 相对路径相对当前工作目录转绝对；
/// - 已存在的路径 canonicalize（解析符号链接、`..`、Windows 盘符大小写）；
/// - 不存在的路径做词法清理（折叠 `.`/`..`、重复分隔符），不要求路径存在；
/// - 去掉 Windows canonicalize 产生的 `\\?\` / `\\?\UNC\`  verbatim 前缀。
pub(crate) fn normalize_path(path: &str) -> String {
    let p = Path::new(path);
    let abs = if p.is_relative() {
        match std::env::current_dir() {
            Ok(cwd) => cwd.join(p),
            Err(_) => return path.to_string(),
        }
    } else {
        p.to_path_buf()
    };
    let resolved = std::fs::canonicalize(&abs).unwrap_or_else(|_| lexical_normalize(&abs));
    let mut s = resolved.to_string_lossy().into_owned();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        s = format!(r"\\{rest}");
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        s = rest.to_string();
    }
    s
}

/// 不依赖文件系统存在的词法归一化（折叠 CurDir、回退 ParentDir）。
fn lexical_normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        p.to_path_buf()
    } else {
        out
    }
}

/// 归一化后的比较键：Windows 忽略盘符大小写、斜杠方向与尾部分隔符。
pub(crate) fn path_key(s: &str) -> String {
    let n = normalize_path(s);
    #[cfg(windows)]
    {
        n.trim_end_matches(['\\', '/'])
            .replace('/', "\\")
            .to_ascii_lowercase()
    }
    #[cfg(not(windows))]
    {
        n
    }
}

/// 两个（已或未归一化的）路径是否指向同一目标。
pub(crate) fn path_eq(a: &str, b: &str) -> bool {
    path_key(a) == path_key(b)
}

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
    /// 加载时归一化所有路径并合并"同一目标的多写法"重复项（相对/绝对、
    /// 大小写、斜杠方向、`.`/`..`），有改动则回写一次（迁移旧数据）。
    pub fn load() -> Self {
        let file = storage_path();
        if let Some(f) = &file {
            if let Ok(text) = std::fs::read_to_string(f) {
                if let Ok(entries) = serde_json::from_str::<Vec<HistoryEntry>>(&text) {
                    let mut hist = Self {
                        entries: Vec::with_capacity(entries.len()),
                        file: file.clone(),
                    };
                    let mut changed = false;
                    // entries 按最近打开在前；重复时保留靠前者（更新的 last_open）。
                    for e in entries {
                        let norm = normalize_path(&e.path);
                        if norm != e.path {
                            changed = true;
                        }
                        match hist
                            .entries
                            .iter_mut()
                            .find(|x| path_key(&x.path) == path_key(&norm))
                        {
                            Some(existing) => {
                                changed = true;
                                if e.last_open > existing.last_open {
                                    existing.last_open = e.last_open;
                                    existing.mode = e.mode;
                                }
                            }
                            None => hist.entries.push(HistoryEntry {
                                path: norm,
                                ..e
                            }),
                        }
                    }
                    if changed {
                        hist.save();
                    }
                    return hist;
                }
            }
        }
        Self {
            entries: Vec::new(),
            file,
        }
    }

    /// 记录一次打开：路径归一化后按同一目标去重置顶，最多保留 MAX_ENTRIES 条。
    /// `mode` 取 [`crate::db::OpenMode::as_str`]。
    pub fn add(&mut self, path: &str, mode: &str) {
        let norm = normalize_path(path);
        self.entries.retain(|e| !path_eq(&e.path, &norm));
        let last_open = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.entries.insert(
            0,
            HistoryEntry {
                path: norm,
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

    /// 按路径删除匹配条目（打开失败、路径已不存在时自动清理），写盘。
    /// 按归一化后的同一目标比较：相对/绝对、Windows 盘符大小写、斜杠方向
    /// 与尾部分隔符的差异都视为同一路径。返回是否删除了条目。
    pub fn remove_path(&mut self, path: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| !path_eq(&e.path, path));
        if self.entries.len() != before {
            self.save();
            true
        } else {
            false
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
pub(crate) fn storage_path() -> Option<PathBuf> {
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
        assert_eq!(h.entries[0].path, normalize_path("/b"));
        assert_eq!(h.entries[1].path, normalize_path("/a"));
    }

    #[test]
    fn add_dedup_same_path_moves_to_front() {
        let (mut h, _) = temp_history();
        h.add("/a", "file");
        h.add("/b", "dir");
        h.add("/a", "dir"); // 重新打开 /a，mode 也应更新
        assert_eq!(h.entries.len(), 2);
        assert_eq!(h.entries[0].path, normalize_path("/a"));
        assert_eq!(h.entries[0].mode, "dir");
        assert_eq!(h.entries[1].path, normalize_path("/b"));
    }

    #[test]
    fn add_dedup_equivalent_path_forms() {
        let (mut h, _) = temp_history();
        // 真实存在的目录：含 ./ 的写法与规范写法必须视为同一条
        let ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!("mdbxer_hist_dup_{ns}"));
        fs::create_dir_all(base.join("sub")).unwrap();
        let p1 = base.join("sub");
        let p2 = base.join(".").join("sub");
        assert_ne!(p1.to_string_lossy(), p2.to_string_lossy());
        h.add(p1.to_str().unwrap(), "dir");
        h.add(p2.to_str().unwrap(), "dir");
        assert_eq!(h.entries.len(), 1);
        assert_eq!(h.entries[0].path, normalize_path(p1.to_str().unwrap()));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn add_truncates_to_max_entries() {
        let (mut h, _) = temp_history();
        for i in 0..(MAX_ENTRIES + 5) {
            h.add(&format!("/p{i}"), "file");
        }
        assert_eq!(h.entries.len(), MAX_ENTRIES);
        // 最新的在最前
        assert_eq!(
            h.entries[0].path,
            normalize_path(&format!("/p{}", MAX_ENTRIES + 4))
        );
    }

    #[test]
    fn remove_valid_index() {
        let (mut h, _) = temp_history();
        h.add("/a", "file");
        h.add("/b", "file");
        h.remove(0);
        assert_eq!(h.entries.len(), 1);
        assert_eq!(h.entries[0].path, normalize_path("/a"));
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
    fn remove_path_deletes_match_and_reports() {
        let (mut h, _) = temp_history();
        h.add("/a", "file");
        h.add("/b", "dir");
        assert!(h.remove_path("/b"));
        assert_eq!(h.entries.len(), 1);
        assert_eq!(h.entries[0].path, normalize_path("/a"));
        assert!(!h.remove_path("/missing"));
        assert_eq!(h.entries.len(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn remove_path_case_and_slash_insensitive_on_windows() {
        let (mut h, _) = temp_history();
        h.add(r"C:\Data\DB", "file");
        // 小写盘符 + 正斜杠 + 尾部分隔符，仍应命中
        assert!(h.remove_path(r"c:/data/DB/"));
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
        assert_eq!(loaded[0].path, normalize_path("/b"));
        assert_eq!(loaded[1].path, normalize_path("/a"));
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
        assert_eq!(loaded[0].path, normalize_path("/b"));
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
