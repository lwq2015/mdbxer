// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 应用配置：JSON 持久化到 %APPDATA%\mdbxer\config.json。
//!
//! 保存界面语言、深浅色主题、UI 偏好（页大小/字节序/排版/千位分隔），
//! 以及每个库的独立记录（最后打开的表、收藏表、收藏 Key；LRU 上限 50）。
//! 所有新字段均带 `#[serde(default)]`：旧版配置（仅 lang）可无缝升级。
//! 首次启动（无配置文件）时按系统区域设置自动选择语言。

use crate::i18n::Lang;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 每库记录的 LRU 上限。
pub const MAX_PER_DB: usize = 50;

#[derive(Serialize, Deserialize, Default)]
struct ConfigFile {
    /// 界面语言代码：zh / en / ru
    #[serde(default)]
    lang: Option<String>,
    /// 主题：dark / light（缺省 dark）
    #[serde(default)]
    theme: Option<String>,
    /// UI 偏好
    #[serde(default)]
    ui: UiPrefs,
    /// 每库记录（按最近使用排序，LRU 截断）
    #[serde(default)]
    per_db: Vec<PerDbRecord>,
}

/// 深浅色主题。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Theme {
    /// 深色（默认）
    #[default]
    Dark,
    /// 浅色
    Light,
}

impl Theme {
    pub fn as_str(self) -> &'static str {
        match self {
            Theme::Dark => "dark",
            Theme::Light => "light",
        }
    }

    /// 从持久化字符串解析；未知值回退 Dark。
    pub fn from_str(s: &str) -> Theme {
        match s {
            "light" => Theme::Light,
            _ => Theme::Dark,
        }
    }
}

/// UI 偏好（全部可选：旧配置缺字段时回退各自默认值）。
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct UiPrefs {
    /// 每页条数
    #[serde(default)]
    pub page_size: Option<usize>,
    /// true = 小端
    #[serde(default)]
    pub endian_le: Option<bool>,
    /// Key 排版（DecodeMode::as_str）
    #[serde(default)]
    pub key_mode: Option<String>,
    /// Value 排版（DecodeMode::as_str）
    #[serde(default)]
    pub val_mode: Option<String>,
    /// 整数千位分隔开关
    #[serde(default)]
    pub thousands_sep: Option<bool>,
    /// 单元格最多显示字符数
    #[serde(default)]
    pub cell_max: Option<usize>,
    /// hex dump：显示地址列
    #[serde(default)]
    pub show_addr: Option<bool>,
    /// hex dump：显示 HEX 列
    #[serde(default)]
    pub show_hex: Option<bool>,
    /// hex dump：显示 ASCII 列
    #[serde(default)]
    pub show_ascii: Option<bool>,
    /// hex dump 每行字节数（4/8/16/32）
    #[serde(default)]
    pub hex_width: Option<usize>,
    /// 左栏（表列表）可见
    #[serde(default)]
    pub left_visible: Option<bool>,
    /// 右栏（详情）可见
    #[serde(default)]
    pub detail_visible: Option<bool>,
    /// 左栏表列表排序（name_asc/name_desc/count_asc/count_desc）
    #[serde(default)]
    pub table_sort: Option<String>,
    /// 导出格式（csv/json）
    #[serde(default)]
    pub export_format: Option<String>,
}

/// 一个库的独立记录。
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct PerDbRecord {
    pub path: String,
    /// 最后打开的表名（None = 主表）
    #[serde(default)]
    pub last_table: Option<String>,
    /// 收藏的表（None 元素 = 主表）
    #[serde(default)]
    pub fav_tables: Vec<Option<String>>,
    /// 收藏的 Key
    #[serde(default)]
    pub fav_keys: Vec<FavKey>,
    /// 最近使用时间（unix 秒；LRU 排序依据）
    #[serde(default)]
    pub last_use: u64,
}

/// 一条收藏 Key。
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct FavKey {
    /// 表名（None = 主表）
    pub table: Option<String>,
    /// Key 字节的大写 hex
    pub key_hex: String,
    /// 备注（预留，当前为空串）
    #[serde(default)]
    pub note: String,
}

/// 配置文件路径：优先 `%APPDATA%\mdbxer\config.json`；
/// 无 %APPDATA%（非 Windows）时退化为 exe 旁的 `mdbxer-config.json`。
pub(crate) fn config_path() -> Option<PathBuf> {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return Some(PathBuf::from(appdata).join("mdbxer").join("config.json"));
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("mdbxer-config.json")))
}

/// 读整个配置；文件缺失/损坏时返回全默认（静默回退）。
/// per_db 的路径会做归一化并合并"同一目标的多写法"重复记录
/// （相对/绝对路径、大小写、斜杠方向等），下次写盘时自然落盘。
fn read_config() -> ConfigFile {
    if let Some(path) = config_path() {
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(mut cfg) = serde_json::from_str::<ConfigFile>(&text) {
                normalize_per_db(&mut cfg.per_db);
                return cfg;
            }
        }
    }
    ConfigFile::default()
}

/// 归一化 per_db 路径并合并重复项：收藏取并集（去重保序），
/// last_table 跟随更新的 last_use。
fn normalize_per_db(list: &mut Vec<PerDbRecord>) {
    let mut merged: Vec<PerDbRecord> = Vec::with_capacity(list.len());
    for rec in list.drain(..) {
        let norm = crate::history::normalize_path(&rec.path);
        match merged
            .iter_mut()
            .find(|r| crate::history::path_eq(&r.path, &norm))
        {
            Some(ex) => merge_per_db(ex, PerDbRecord { path: norm, ..rec }),
            None => merged.push(PerDbRecord { path: norm, ..rec }),
        }
    }
    *list = merged;
}

/// 把 src 合并进 dst：收藏并集去重，last_use/last_table 取更新者。
fn merge_per_db(dst: &mut PerDbRecord, src: PerDbRecord) {
    for t in src.fav_tables {
        if !dst.fav_tables.contains(&t) {
            dst.fav_tables.push(t);
        }
    }
    for k in src.fav_keys {
        if !dst
            .fav_keys
            .iter()
            .any(|x| x.table == k.table && x.key_hex == k.key_hex)
        {
            dst.fav_keys.push(k);
        }
    }
    if src.last_use > dst.last_use {
        dst.last_use = src.last_use;
        dst.last_table = src.last_table;
    }
}

/// 统一「读-改-写」：任何单项保存都不会丢其他字段；写盘失败静默忽略。
fn write_config(f: impl FnOnce(&mut ConfigFile)) {
    let Some(path) = config_path() else { return };
    let mut cfg = read_config();
    f(&mut cfg);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(&cfg) {
        let _ = std::fs::write(path, text);
    }
}

/// 启动时确定界面语言：配置文件优先；无配置则按系统区域设置猜测。
pub fn startup_lang() -> Lang {
    let code = read_config().lang;
    match code {
        Some(code) => Lang::from_code(&code),
        None => system_lang(),
    }
}

/// 保存语言选择（本次会话内立即生效由调用方负责）。
pub fn save_lang(lang: Lang) {
    write_config(|c| c.lang = Some(lang.code().to_string()));
}

/// 读取主题（缺省 Dark）。
pub fn load_theme() -> Theme {
    read_config()
        .theme
        .map(|s| Theme::from_str(&s))
        .unwrap_or_default()
}

/// 保存主题。
pub fn save_theme(theme: Theme) {
    write_config(|c| c.theme = Some(theme.as_str().to_string()));
}

/// 读取 UI 偏好。
pub fn load_ui_prefs() -> UiPrefs {
    read_config().ui
}

/// 保存 UI 偏好。
pub fn save_ui_prefs(p: &UiPrefs) {
    write_config(|c| c.ui = p.clone());
}

/// 读取某库的记录；无记录时返回以归一化路径初始化的默认记录。
pub fn load_per_db(path: &str) -> PerDbRecord {
    let norm = crate::history::normalize_path(path);
    read_config()
        .per_db
        .into_iter()
        .find(|r| crate::history::path_eq(&r.path, &norm))
        .unwrap_or_else(|| PerDbRecord {
            path: norm,
            ..Default::default()
        })
}

/// 保存（upsert）一库记录：last_use 刷为当前时间，按最近使用排序后截断到上限。
pub fn save_per_db(rec: &PerDbRecord) {
    let mut rec = rec.clone();
    rec.last_use = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    write_config(|c| upsert_per_db(&mut c.per_db, rec));
}

/// upsert + 按 last_use 降序 + LRU 截断（纯函数，便于测试）。
fn upsert_per_db(list: &mut Vec<PerDbRecord>, mut rec: PerDbRecord) {
    rec.path = crate::history::normalize_path(&rec.path);
    list.retain(|r| !crate::history::path_eq(&r.path, &rec.path));
    list.push(rec);
    list.sort_by(|a, b| b.last_use.cmp(&a.last_use));
    list.truncate(MAX_PER_DB);
}

/// 最近打开过的库路径（per_db 按 last_use 取最新一条）；无记录返回 None。
pub fn last_opened_db() -> Option<String> {
    read_config()
        .per_db
        .into_iter()
        .max_by_key(|r| r.last_use)
        .map(|r| r.path)
}

/// 按系统区域设置猜测语言：zh* → 中文，ru* → 俄语，其余 → 英语。
fn system_lang() -> Lang {
    let loc = system_locale_string().to_lowercase();
    if loc.starts_with("zh") {
        Lang::Zh
    } else if loc.starts_with("ru") {
        Lang::Ru
    } else {
        Lang::En
    }
}

/// 取系统区域标识，如 Windows 的 "zh-CN"、Unix 的 "ru_RU.UTF-8"。
fn system_locale_string() -> String {
    #[cfg(windows)]
    {
        // GetUserDefaultLocaleName：无需任何依赖的 Win32 API
        unsafe extern "system" {
            fn GetUserDefaultLocaleName(lp_locale_name: *mut u16, cch_name: i32) -> i32;
        }
        let mut buf = [0u16; 85]; // LOCALE_NAME_MAX_LENGTH
        let n = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
        if n > 0 {
            return String::from_utf16_lossy(&buf[..n as usize - 1]);
        }
    }
    // Unix: LC_ALL > LC_MESSAGES > LANG（形如 ru_RU.UTF-8）
    for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(v) = std::env::var(var) {
            if !v.is_empty() && v != "C" && v != "POSIX" {
                return v;
            }
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_config_with_only_lang_parses() {
        // 旧版配置（仅 lang）必须能解析，其余字段全默认
        let cfg: ConfigFile = serde_json::from_str(r#"{"lang":"en"}"#).unwrap();
        assert_eq!(cfg.lang.as_deref(), Some("en"));
        assert_eq!(cfg.theme, None);
        assert!(cfg.ui.page_size.is_none());
        assert!(cfg.per_db.is_empty());
    }

    #[test]
    fn corrupt_or_empty_config_falls_back() {
        let cfg: ConfigFile = serde_json::from_str("not json").unwrap_or_default();
        assert!(cfg.lang.is_none());
        let cfg: ConfigFile = serde_json::from_str("{}").unwrap();
        assert!(cfg.lang.is_none());
    }

    #[test]
    fn ui_prefs_round_trip() {
        let p = UiPrefs {
            page_size: Some(500),
            endian_le: Some(false),
            key_mode: Some("hex".to_string()),
            val_mode: Some("auto".to_string()),
            thousands_sep: Some(false),
            cell_max: Some(256),
            show_addr: Some(true),
            show_hex: Some(false),
            show_ascii: Some(true),
            hex_width: Some(16),
            left_visible: Some(false),
            detail_visible: Some(true),
            table_sort: Some("count_desc".to_string()),
            export_format: Some("json".to_string()),
        };
        let s = serde_json::to_string(&p).unwrap();
        let back: UiPrefs = serde_json::from_str(&s).unwrap();
        assert_eq!(back.page_size, Some(500));
        assert_eq!(back.endian_le, Some(false));
        assert_eq!(back.key_mode.as_deref(), Some("hex"));
        assert_eq!(back.val_mode.as_deref(), Some("auto"));
        assert_eq!(back.thousands_sep, Some(false));
        assert_eq!(back.cell_max, Some(256));
        assert_eq!(back.show_hex, Some(false));
        assert_eq!(back.hex_width, Some(16));
        assert_eq!(back.left_visible, Some(false));
        assert_eq!(back.detail_visible, Some(true));
        assert_eq!(back.table_sort.as_deref(), Some("count_desc"));
        assert_eq!(back.export_format.as_deref(), Some("json"));
    }

    #[test]
    fn ui_prefs_missing_fields_are_none() {
        let back: UiPrefs = serde_json::from_str("{}").unwrap();
        assert!(back.page_size.is_none());
        assert!(back.endian_le.is_none());
        assert!(back.key_mode.is_none());
        assert!(back.val_mode.is_none());
        assert!(back.thousands_sep.is_none());
        assert!(back.cell_max.is_none());
        assert!(back.show_addr.is_none());
        assert!(back.show_hex.is_none());
        assert!(back.show_ascii.is_none());
        assert!(back.hex_width.is_none());
        assert!(back.left_visible.is_none());
        assert!(back.detail_visible.is_none());
        assert!(back.table_sort.is_none());
        assert!(back.export_format.is_none());
    }

    #[test]
    fn theme_as_str_from_str_round_trip() {
        for t in [Theme::Dark, Theme::Light] {
            assert_eq!(Theme::from_str(t.as_str()), t);
        }
        assert_eq!(Theme::from_str("blue"), Theme::Dark);
        assert_eq!(Theme::default(), Theme::Dark);
    }

    #[test]
    fn upsert_replaces_same_path_and_orders_by_last_use() {
        let mut list = Vec::new();
        let rec = |path: &str, use_: u64| PerDbRecord {
            path: path.to_string(),
            last_use: use_,
            ..Default::default()
        };
        upsert_per_db(&mut list, rec("a", 1));
        upsert_per_db(&mut list, rec("b", 2));
        upsert_per_db(&mut list, rec("a", 3)); // 更新 a
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].path, crate::history::normalize_path("a")); // 最新在前
        assert_eq!(list[0].last_use, 3);
        assert_eq!(list[1].path, crate::history::normalize_path("b"));
    }

    #[test]
    fn upsert_truncates_to_max_per_db() {
        let mut list = Vec::new();
        for i in 0..(MAX_PER_DB + 10) {
            upsert_per_db(
                &mut list,
                PerDbRecord {
                    path: format!("db{i}"),
                    last_use: i as u64,
                    ..Default::default()
                },
            );
        }
        assert_eq!(list.len(), MAX_PER_DB);
        // 保留的是最新的 50 个（db10..db59），最旧在最末
        assert_eq!(
            list[0].path,
            crate::history::normalize_path(&format!("db{}", MAX_PER_DB + 9))
        );
        assert_eq!(
            list.last().unwrap().path,
            crate::history::normalize_path("db10")
        );
    }

    #[test]
    fn normalize_per_db_merges_equivalent_path_records() {
        let mut list = vec![
            PerDbRecord {
                path: "K:\\Data\\DB".to_string(),
                last_table: Some("t1".to_string()),
                fav_tables: vec![Some("a".to_string())],
                last_use: 100,
                ..Default::default()
            },
            PerDbRecord {
                path: "k:/data/db/".to_string(), // 同目标，另一种写法（不存在则词法归一）
                last_table: Some("t2".to_string()),
                fav_tables: vec![Some("b".to_string()), Some("a".to_string())],
                last_use: 200,
                ..Default::default()
            },
            PerDbRecord {
                path: "K:\\Other".to_string(),
                last_use: 10,
                ..Default::default()
            },
        ];
        normalize_per_db(&mut list);
        assert_eq!(list.len(), 2);
        let merged = list
            .iter()
            .find(|r| crate::history::path_eq(&r.path, "K:\\Data\\DB"))
            .unwrap();
        // 收藏并集去重保序
        assert_eq!(merged.fav_tables, vec![Some("a".to_string()), Some("b".to_string())]);
        // last_table 跟随更新的 last_use
        assert_eq!(merged.last_use, 200);
        assert_eq!(merged.last_table.as_deref(), Some("t2"));
    }

    #[test]
    fn fav_key_serde_round_trip() {
        let fk = FavKey {
            table: Some("kv".to_string()),
            key_hex: "00FF".to_string(),
            note: String::new(),
        };
        let s = serde_json::to_string(&fk).unwrap();
        let back: FavKey = serde_json::from_str(&s).unwrap();
        assert_eq!(back, fk);
        // note 缺省兼容
        let legacy: FavKey = serde_json::from_str(r#"{"table":null,"key_hex":"00FF"}"#).unwrap();
        assert_eq!(legacy.table, None);
        assert_eq!(legacy.note, "");
    }
}
