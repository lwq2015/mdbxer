// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 表数据导出：后台线程流式写出 CSV / JSON。
//!
//! 只读事务的生命周期绑定 `&Database`，无法跨线程传递，因此导出线程内
//! 重新以只读 + ACCEDE 方式打开环境（零写锁开销；用户中途关库不影响导出）。

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::mpsc;

use crate::db::{DbHandle, OpenMode};
use crate::fmt::{DecodeMode, Endian, decode};

/// 导出文件格式。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ExportFormat {
    /// CSV：表头 `key,value`；多值表每值一行
    #[default]
    Csv,
    /// JSON：普通表 `[{"key","value"}]`；多值表 `[{"key","values":[...]}]`
    Json,
}

impl ExportFormat {
    pub const ALL: [ExportFormat; 2] = [ExportFormat::Csv, ExportFormat::Json];

    /// 文件扩展名（保存对话框默认文件名用）。
    pub fn ext(self) -> &'static str {
        match self {
            ExportFormat::Csv => "csv",
            ExportFormat::Json => "json",
        }
    }

    /// 下拉框显示文本（格式名为国际通用写法，不翻译）。
    pub fn label(self) -> &'static str {
        match self {
            ExportFormat::Csv => "CSV",
            ExportFormat::Json => "JSON",
        }
    }
}

/// 一次导出任务的全部参数（全部字段 Send，整体移交后台线程）。
pub struct ExportJob {
    /// 数据库路径（文件或目录，由 open_mode 解释）
    pub db_path: PathBuf,
    pub open_mode: OpenMode,
    /// None = 主表
    pub table: Option<String>,
    pub dup_sort: bool,
    /// true = 降序导出（跟随界面全局遍历方向）
    pub sort_desc: bool,
    pub key_mode: DecodeMode,
    pub val_mode: DecodeMode,
    pub endian: Endian,
    pub out_path: PathBuf,
    pub format: ExportFormat,
}

/// 导出进度（经 mpsc 发回 UI 线程）。
pub enum ExportProgress {
    /// 已写出 n 条记录（多值表按值计）
    Progress(usize),
    /// 完成：共 n 条
    Done(usize),
    /// 失败
    Fail(String),
}

/// 启动后台导出线程，返回进度接收端。
pub fn start(job: ExportJob) -> mpsc::Receiver<ExportProgress> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let msg = match run(&job, &tx) {
            Ok(n) => ExportProgress::Done(n),
            Err(e) => ExportProgress::Fail(e),
        };
        let _ = tx.send(msg);
    });
    rx
}

/// 线程主体：打开环境 → 流式遍历 → 写出。返回写出的记录条数。
fn run(job: &ExportJob, tx: &mpsc::Sender<ExportProgress>) -> Result<usize, String> {
    let handle = DbHandle::open(&job.db_path, job.open_mode)?;
    let txn = handle.db.begin_ro_txn().map_err(|e| e.to_string())?;
    let table = txn
        .open_table(job.table.as_deref())
        .map_err(|e| e.to_string())?;
    let mut cursor = txn.cursor(&table).map_err(|e| e.to_string())?;

    let file = File::create(&job.out_path).map_err(|e| e.to_string())?;
    let mut w = BufWriter::new(file);

    let dec = |b: &[u8], mode: DecodeMode| decode(b, mode, job.endian, usize::MAX);
    let mut n = 0usize;

    // 统一升/降序遍历：(key, value) 流（多值表逐值展开）
    let mut item: Option<(Vec<u8>, Vec<u8>)> = if job.sort_desc {
        cursor.last()
    } else {
        cursor.first()
    }
    .map_err(|e| e.to_string())?;

    if job.format == ExportFormat::Csv {
        writeln!(w, "key,value").map_err(|e| e.to_string())?;
    } else {
        writeln!(w, "[").map_err(|e| e.to_string())?;
    }

    // JSON 多值表分组状态：当前 Key 原始字节（按字节判等，避免解码文本撞车）
    let mut json_first_entry = true;
    let mut json_cur_key: Option<Vec<u8>> = None;

    while let Some((k, v)) = item {
        let key_text = dec(&k, job.key_mode);
        let val_text = dec(&v, job.val_mode);
        match job.format {
            ExportFormat::Csv => {
                writeln!(w, "{},{}", csv_field(&key_text), csv_field(&val_text))
                    .map_err(|e| e.to_string())?;
            }
            ExportFormat::Json => {
                if job.dup_sort {
                    // 多值表：{"key": k, "values": [v, ...]}，同 Key 连续分组
                    if json_cur_key.as_deref() != Some(k.as_slice()) {
                        if json_cur_key.is_some() {
                            writeln!(w, "]}},").map_err(|e| e.to_string())?;
                        }
                        json_first_entry = false;
                        write!(
                            w,
                            "  {{\"key\": {}, \"values\": [{}",
                            json_str(&key_text),
                            json_str(&val_text)
                        )
                        .map_err(|e| e.to_string())?;
                        json_cur_key = Some(k);
                    } else {
                        write!(w, ", {}", json_str(&val_text)).map_err(|e| e.to_string())?;
                    }
                } else {
                    if !json_first_entry {
                        writeln!(w, ",").map_err(|e| e.to_string())?;
                    }
                    json_first_entry = false;
                    write!(
                        w,
                        "  {{\"key\": {}, \"value\": {}}}",
                        json_str(&key_text),
                        json_str(&val_text)
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
        }
        n += 1;
        if n % 1000 == 0 {
            let _ = tx.send(ExportProgress::Progress(n));
        }
        item = if job.sort_desc {
            cursor.prev()
        } else {
            cursor.next()
        }
        .map_err(|e| e.to_string())?;
    }

    if job.format == ExportFormat::Json {
        if json_cur_key.is_some() {
            // 收尾最后一个多值分组
            writeln!(w, "]}}").map_err(|e| e.to_string())?;
        } else if !json_first_entry {
            writeln!(w).map_err(|e| e.to_string())?;
        }
        writeln!(w, "]").map_err(|e| e.to_string())?;
    }
    w.flush().map_err(|e| e.to_string())?;
    Ok(n)
}

/// CSV 字段转义：含逗号/引号/换行时加引号并双写内部引号。
fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// JSON 字符串字面量（含引号与转义）。
fn json_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_field_plain() {
        assert_eq!(csv_field("abc"), "abc");
        assert_eq!(csv_field(""), "");
        assert_eq!(csv_field("1,234"), "\"1,234\"");
    }

    #[test]
    fn csv_field_quote_doubling() {
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn csv_field_newline() {
        assert_eq!(csv_field("a\nb"), "\"a\nb\"");
        assert_eq!(csv_field("a\rb"), "\"a\rb\"");
    }

    #[test]
    fn json_str_escapes() {
        assert_eq!(json_str("a\"b"), "\"a\\\"b\"");
        assert_eq!(json_str("换\n行"), "\"换\\n行\"");
        assert_eq!(json_str(""), "\"\"");
    }

    #[test]
    fn export_format_ext_label() {
        assert_eq!(ExportFormat::Csv.ext(), "csv");
        assert_eq!(ExportFormat::Json.ext(), "json");
        assert_eq!(ExportFormat::Csv.label(), "CSV");
        assert_eq!(ExportFormat::Json.label(), "JSON");
        assert_eq!(ExportFormat::ALL.len(), 2);
        assert_eq!(ExportFormat::default(), ExportFormat::Csv);
    }
}
