// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 文件日志（零依赖）。
//!
//! - 等级：Error / Warn / Info / Debug，全局原子量，设置页可改，持久化在 config.json。
//! - 位置：exe 同目录 `log\`（创建失败——如装在 Program Files 无写权限——
//!   退化为 `%APPDATA%\mdbxer\log`）。
//! - 滚动：按月分文件 `mdbxer-YYYYMM.log`；单文件超过 [`MAX_FILE_SIZE`]
//!   （100MB）后拆分加编号 `mdbxer-YYYYMM.N.log`，跨月重新归零。
//! - 行格式：`2026-10-19 15:04:05.123 [INFO] 消息`。
//!
//! 用法：`crate::logging::info(&format!("打开 {path}"))`；
//! 首次写入时惰性初始化（目录解析 + 文件打开），无需显式 init。
//! 写盘失败退化为 stderr，绝不影响主流程。

use chrono::Local;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, Ordering};

/// 单个日志文件大小上限：超过后拆分加编号。
pub const MAX_FILE_SIZE: u64 = 100 * 1024 * 1024;

/// 日志等级（数值越大越啰嗦；写入判定：level <= 当前等级才落盘）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Level {
    /// 错误：影响功能的失败
    Error = 0,
    /// 警告：已自动回退但仍值得留痕的异常
    Warn = 1,
    /// 信息：常规运行轨迹（默认）
    #[default]
    Info = 2,
    /// 调试：内部细节
    Debug = 3,
}

impl Level {
    pub const ALL: [Level; 4] = [Level::Error, Level::Warn, Level::Info, Level::Debug];

    /// 配置持久化用代码：error / warn / info / debug。
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Error => "error",
            Level::Warn => "warn",
            Level::Info => "info",
            Level::Debug => "debug",
        }
    }

    /// 从持久化代码解析；未知值回退 Info。
    pub fn from_str(s: &str) -> Level {
        match s {
            "error" => Level::Error,
            "warn" => Level::Warn,
            "debug" => Level::Debug,
            _ => Level::Info,
        }
    }

    /// 行内等级标签。
    fn tag(self) -> &'static str {
        match self {
            Level::Error => "ERROR",
            Level::Warn => "WARN",
            Level::Info => "INFO",
            Level::Debug => "DEBUG",
        }
    }
}

static LEVEL: AtomicU8 = AtomicU8::new(Level::Info as u8);

/// 当前日志等级。
pub fn level() -> Level {
    match LEVEL.load(Ordering::Relaxed) {
        0 => Level::Error,
        1 => Level::Warn,
        3 => Level::Debug,
        _ => Level::Info,
    }
}

/// 切换日志等级（立即生效；持久化由调用方负责写 config）。
pub fn set_level(l: Level) {
    LEVEL.store(l as u8, Ordering::Relaxed);
}

/// 日志目录（设置页"打开日志目录"用）；目录可能尚不存在（尚未写入过日志）。
pub fn log_dir() -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent().map(|d| d.join("log")) {
            if dir.is_dir() {
                return Some(dir);
            }
        }
    }
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("mdbxer").join("log"))
}

/// 解析并创建日志目录：exe 同目录 `log\` 优先，失败退 `%APPDATA%\mdbxer\log`。
fn resolve_dir() -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent().map(|d| d.join("log")) {
            if std::fs::create_dir_all(&dir).is_ok() {
                return Some(dir);
            }
        }
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let dir = PathBuf::from(appdata).join("mdbxer").join("log");
        if std::fs::create_dir_all(&dir).is_ok() {
            return Some(dir);
        }
    }
    None
}

/// 月度文件名：无编号为基础名，滚动后加 `.N`。
fn target_path(dir: &Path, month: &str, index: u32) -> PathBuf {
    if index == 0 {
        dir.join(format!("mdbxer-{month}.log"))
    } else {
        dir.join(format!("mdbxer-{month}.{index}.log"))
    }
}

/// 一名写入者的状态：当前月、编号、句柄与已写字节数。
struct Writer {
    dir: PathBuf,
    /// 单文件上限（生产为 [`MAX_FILE_SIZE`]，测试用小值驱动滚动）
    limit: u64,
    /// 当前文件所属月份（"YYYYMM"），跨月时换新文件
    month: String,
    /// 当前文件的滚动编号（0 = 基础名）
    index: u32,
    file: Option<File>,
    /// 当前文件已写字节数（打开时从文件元数据取，写后累加）
    size: u64,
}

impl Writer {
    /// 按当前时刻写一行。
    fn append(&mut self, lv: Level, msg: &str) {
        let now = Local::now();
        self.append_with(
            lv,
            msg,
            &now.format("%Y%m").to_string(),
            &now.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
        );
    }

    /// 实际写入（月份与时间戳参数化，便于测试模拟跨月）。
    /// 滚动判定统一在"打开后按真实文件大小 + 本次行长"做，
    /// 因此跨进程重启后面对已满文件也能正确续滚编号。
    fn append_with(&mut self, lv: Level, msg: &str, month: &str, ts: &str) {
        if month != self.month {
            // 跨月：换新文件，编号归零
            self.month = month.to_string();
            self.index = 0;
            self.file = None;
            self.size = 0;
        }

        let line = format!("{ts} [{}] {msg}\n", lv.tag());
        let len = line.len() as u64;

        // 上限 64：防御 limit 异常（如 0）导致的死循环
        for _ in 0..64 {
            if self.file.is_none() {
                let path = target_path(&self.dir, &self.month, self.index);
                match OpenOptions::new().create(true).append(true).open(&path) {
                    Ok(f) => {
                        self.size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                        self.file = Some(f);
                    }
                    Err(e) => {
                        eprintln!("日志文件打开失败：{path:?}：{e}");
                        return;
                    }
                }
            }

            if self.size + len > self.limit {
                // 当前文件已满：关闭并切换到下一个编号
                self.file = None;
                self.size = 0;
                self.index += 1;
                continue;
            }

            if let Some(f) = self.file.as_mut() {
                if let Err(e) = f.write_all(line.as_bytes()).and_then(|()| f.flush()) {
                    eprintln!("日志写入失败：{e}");
                    self.file = None;
                    return;
                }
            }
            self.size += len;
            return;
        }
        eprintln!(
            "日志滚动异常：连续 64 个编号文件均已满（limit={}）",
            self.limit
        );
    }
}

/// 全局写入者；None = 尚未惰性初始化（首次写入时解析目录）。
static STATE: Mutex<Option<Writer>> = Mutex::new(None);

/// 等级过滤后落盘。目录/文件任何失败都退化为 stderr，不影响主流程。
fn write(lv: Level, msg: &str) {
    if (lv as u8) > LEVEL.load(Ordering::Relaxed) {
        return;
    }

    let Ok(mut guard) = STATE.lock() else {
        return;
    };
    if guard.is_none() {
        let Some(dir) = resolve_dir() else {
            eprintln!("[{}] {msg}", lv.tag());
            return;
        };
        let _ = guard.insert(Writer {
            dir,
            limit: MAX_FILE_SIZE,
            month: String::new(),
            index: 0,
            file: None,
            size: 0,
        });
    }
    if let Some(w) = guard.as_mut() {
        w.append(lv, msg);
    }
}

/// Error 级日志。
pub fn error(msg: &str) {
    write(Level::Error, msg);
}

/// Warn 级日志。
pub fn warn(msg: &str) {
    write(Level::Warn, msg);
}

/// Info 级日志。
pub fn info(msg: &str) {
    write(Level::Info, msg);
}

/// Debug 级日志。
pub fn debug(msg: &str) {
    write(Level::Debug, msg);
}

/// 把 panic 记入日志（Error 级，含位置）后转交原钩子，保留默认行为。
pub fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let loc = info
            .location()
            .map(|l| l.to_string())
            .unwrap_or_else(|| "未知位置".to_string());
        error(&format!("panic：{info}（{loc}）"));
        prev(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 目标文件名：月度基础名 + 滚动编号。
    #[test]
    fn target_file_naming() {
        let dir = Path::new(r"X:\log");
        assert_eq!(
            target_path(dir, "202610", 0),
            PathBuf::from(r"X:\log\mdbxer-202610.log")
        );
        assert_eq!(
            target_path(dir, "202610", 3),
            PathBuf::from(r"X:\log\mdbxer-202610.3.log")
        );
    }

    /// 等级与配置代码互转；未知值回退 Info。
    #[test]
    fn level_str_round_trip() {
        for l in Level::ALL {
            assert_eq!(Level::from_str(l.as_str()), l);
        }
        assert_eq!(Level::from_str("xxx"), Level::Info);
        assert_eq!(Level::default(), Level::Info);
    }

    /// 端到端：小 limit 下连续写入应滚动出多个编号文件，
    /// 跨月应换新基础文件并归零编号，重启后续写已满文件应继续加编号。
    #[test]
    fn write_rotates_and_switches_month() {
        let dir = std::env::temp_dir().join(format!("mdbxer_log_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // 行长约 91 字节（CJK 按 UTF-8 计 3 字节），limit 200：每文件装两行
        let mut w = Writer {
            dir: dir.clone(),
            limit: 200,
            month: "197001".to_string(),
            index: 0,
            file: None,
            size: 0,
        };
        for i in 0..3 {
            w.append_with(
                Level::Info,
                &format!("第一轮第 {i} 行 0123456789012345678901234567890123456789"),
                "197001",
                "1970-01-01 00:00:00.000",
            );
        }
        let base = target_path(&dir, "197001", 0);
        assert!(base.exists(), "基础文件应存在");
        assert!(
            target_path(&dir, "197001", 1).exists(),
            "滚动编号文件应存在"
        );
        assert_eq!(w.index, 1, "三行两行一档应滚到编号 1");

        // 跨月：换新基础文件，编号归零
        w.append_with(
            Level::Warn,
            "跨月后的一行",
            "197002",
            "1970-02-01 00:00:00.000",
        );
        assert_eq!(w.index, 0, "跨月编号应归零");
        assert!(target_path(&dir, "197002", 0).exists(), "新月文件应存在");

        // 模拟重启续写：新 Writer 首写就面对已满的基础文件，应滚动而非撑爆
        let mut w2 = Writer {
            dir: dir.clone(),
            limit: 200,
            month: "197002".to_string(),
            index: 0,
            file: None,
            size: 0,
        };
        for i in 0..3 {
            w2.append_with(
                Level::Error,
                &format!("重启续写第 {i} 行 0123456789012345678901234567890123456789"),
                "197002",
                "1970-02-01 00:00:01.000",
            );
        }
        assert_eq!(w2.index, 1, "基础文件已满应滚到编号 1");
        let full = std::fs::metadata(target_path(&dir, "197002", 0))
            .unwrap()
            .len();
        assert!(full <= 200, "满文件不应被撑爆：{full}");
        assert!(target_path(&dir, "197002", 1).exists(), "续滚文件应存在");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
