// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 应用配置：JSON 持久化到 %APPDATA%\mdbxer\config.json。
//!
//! 当前只保存界面语言；首次启动（无配置文件）时按系统区域设置自动选择。

use crate::i18n::Lang;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Default)]
struct ConfigFile {
    /// 界面语言代码：zh / en / ru
    #[serde(default)]
    lang: Option<String>,
}

/// 配置文件路径：优先 `%APPDATA%\mdbxer\config.json`；
/// 无 %APPDATA%（非 Windows）时退化为 exe 旁的 `mdbxer-config.json`。
fn config_path() -> Option<PathBuf> {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return Some(PathBuf::from(appdata).join("mdbxer").join("config.json"));
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("mdbxer-config.json")))
}

/// 启动时确定界面语言：配置文件优先；无配置则按系统区域设置猜测。
pub fn startup_lang() -> Lang {
    if let Some(path) = config_path() {
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(cfg) = serde_json::from_str::<ConfigFile>(&text) {
                if let Some(code) = cfg.lang {
                    return Lang::from_code(&code);
                }
            }
        }
    }
    system_lang()
}

/// 保存语言选择；写盘失败静默忽略（本次会话内仍然生效）。
pub fn save_lang(lang: Lang) {
    let Some(path) = config_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let cfg = ConfigFile {
        lang: Some(lang.code().to_string()),
    };
    if let Ok(text) = serde_json::to_string_pretty(&cfg) {
        let _ = std::fs::write(path, text);
    }
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
