// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 表数据导出：后台线程流式写出 CSV / JSON。
//!
//! libmdbx 在同一进程内不允许二次打开同一环境（MDBX_BUSY），因此导出线程
//! 不能自行 open 环境。采用双通道流水线：
//! - UI 线程持有唯一环境句柄，按 `NeedBatch` 请求分批读取原始 KV（多值表
//!   逐值展开），经 `ExportBatch` 通道发给 worker；
//! - worker 线程只负责文件 IO 与 CSV/JSON 编码，`recv` 阻塞等批，不耗 CPU。
//! 用户中途关库时 UI 端 drop 发送端，worker 自然结束。

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::mpsc;

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

/// 一次导出任务的全部参数（不碰数据库，全部 Send）。
pub struct ExportJob {
    /// 该表是否多值表（决定 JSON 是否按 Key 分组）
    pub dup_sort: bool,
    pub key_mode: DecodeMode,
    pub val_mode: DecodeMode,
    pub endian: Endian,
    pub out_path: PathBuf,
    pub format: ExportFormat,
}

/// worker → UI：要下一批数据 / 完成 / 失败。
#[derive(Debug)]
pub enum ExportEvent {
    /// 请求下一批原始 KV（首批与续批同消息；UI 自行维护锚点）
    NeedBatch,
    /// 完成：共写出 n 条记录（多值表按值计）
    Done(usize),
    /// 失败
    Fail(String),
}

/// UI → worker：一批原始 KV，或中止信号。
pub enum ExportBatch {
    /// 一批记录；`has_more=false` 表示这是最后一批（rows 可为空 = 空表）
    Rows {
        /// 本批原始 (key, value) 记录，按遍历方向排列
        rows: Vec<(Vec<u8>, Vec<u8>)>,
        /// 是否还有后续批次
        has_more: bool,
    },
    /// 读批失败，中止导出
    Abort(String),
}

/// 启动后台写盘线程：返回（批次发送端, 事件接收端）。
pub fn start(job: ExportJob) -> (mpsc::Sender<ExportBatch>, mpsc::Receiver<ExportEvent>) {
    let (batch_tx, batch_rx) = mpsc::channel::<ExportBatch>();
    let (event_tx, event_rx) = mpsc::channel::<ExportEvent>();
    std::thread::spawn(move || {
        let msg = match run(&job, &event_tx, &batch_rx) {
            Ok(n) => ExportEvent::Done(n),
            Err(e) => ExportEvent::Fail(e),
        };
        let _ = event_tx.send(msg);
    });
    (batch_tx, event_rx)
}

/// worker 主体：建文件 → 循环要批、写批 → 收尾。
fn run(
    job: &ExportJob,
    events: &mpsc::Sender<ExportEvent>,
    batches: &mpsc::Receiver<ExportBatch>,
) -> Result<usize, String> {
    let file = File::create(&job.out_path).map_err(|e| e.to_string())?;
    let mut w = BufWriter::new(file);
    if job.format == ExportFormat::Csv {
        writeln!(w, "key,value").map_err(|e| e.to_string())?;
    } else {
        writeln!(w, "[").map_err(|e| e.to_string())?;
    }

    let dec = |b: &[u8], mode: DecodeMode| decode(b, mode, job.endian, usize::MAX);
    let mut n = 0usize;
    // JSON 跨批分组状态
    let mut json_started = false; // 是否已写出过第一个条目
    let mut json_cur_key: Option<Vec<u8>> = None;

    loop {
        if events.send(ExportEvent::NeedBatch).is_err() {
            return Err("UI closed".to_string());
        }
        match batches.recv() {
            Ok(ExportBatch::Rows { rows, has_more }) => {
                n += write_rows(
                    &mut w,
                    job,
                    &dec,
                    rows,
                    &mut json_started,
                    &mut json_cur_key,
                )?;
                if !has_more {
                    break;
                }
            }
            Ok(ExportBatch::Abort(e)) => return Err(e),
            // UI 端关闭（关库/退出）：静默结束，保留已写出内容
            Err(_) => return Err("aborted".to_string()),
        }
    }

    if job.format == ExportFormat::Json {
        if json_cur_key.is_some() {
            // 收尾最后一个多值分组
            writeln!(w, "]}}").map_err(|e| e.to_string())?;
        } else if json_started {
            writeln!(w).map_err(|e| e.to_string())?;
        }
        writeln!(w, "]").map_err(|e| e.to_string())?;
    }
    w.flush().map_err(|e| e.to_string())?;
    Ok(n)
}

/// 写出一批记录（CSV 逐行；JSON 普通表单条 / 多值表按 Key 连续分组，状态跨批延续）。
/// 返回写出的记录条数。
#[allow(clippy::too_many_arguments)]
fn write_rows(
    w: &mut impl Write,
    job: &ExportJob,
    dec: &impl Fn(&[u8], DecodeMode) -> String,
    rows: Vec<(Vec<u8>, Vec<u8>)>,
    json_started: &mut bool,
    json_cur_key: &mut Option<Vec<u8>>,
) -> Result<usize, String> {
    let mut n = 0usize;
    for (k, v) in rows {
        let key_text = dec(&k, job.key_mode);
        let val_text = dec(&v, job.val_mode);
        if job.format == ExportFormat::Csv {
            writeln!(w, "{},{}", csv_field(&key_text), csv_field(&val_text))
                .map_err(|e| e.to_string())?;
        } else {
            // JSON
            if job.dup_sort {
                // 多值表：{"key": k, "values": [v, ...]}，同 Key 连续分组（按原始字节判等）
                if json_cur_key.as_deref() != Some(k.as_slice()) {
                    if json_cur_key.is_some() {
                        writeln!(w, "]}},").map_err(|e| e.to_string())?;
                    }
                    *json_started = true;
                    write!(
                        w,
                        "  {{\"key\": {}, \"values\": [{}",
                        json_str(&key_text),
                        json_str(&val_text)
                    )
                    .map_err(|e| e.to_string())?;
                    *json_cur_key = Some(k);
                } else {
                    write!(w, ", {}", json_str(&val_text)).map_err(|e| e.to_string())?;
                }
            } else {
                if *json_started {
                    writeln!(w, ",").map_err(|e| e.to_string())?;
                }
                *json_started = true;
                write!(
                    w,
                    "  {{\"key\": {}, \"value\": {}}}",
                    json_str(&key_text),
                    json_str(&val_text)
                )
                .map_err(|e| e.to_string())?;
            }
        }
        n += 1;
    }
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

    fn job_at(path: &std::path::Path, format: ExportFormat, dup_sort: bool) -> ExportJob {
        ExportJob {
            dup_sort,
            key_mode: DecodeMode::Utf8,
            val_mode: DecodeMode::Utf8,
            endian: Endian::Little,
            out_path: path.to_path_buf(),
            format,
        }
    }

    /// 喂两批数据跑完整流水线，返回输出文件内容。
    fn pipeline(
        job: ExportJob,
        batches: Vec<ExportBatch>,
    ) -> (Result<usize, String>, String) {
        let (tx, rx) = mpsc::channel::<ExportBatch>();
        let (etx, erx) = mpsc::channel::<ExportEvent>();
        let path = job.out_path.clone();
        let handle = std::thread::spawn(move || run(&job, &etx, &rx));
        for b in batches {
            match erx.recv() {
                Ok(ExportEvent::NeedBatch) => {}
                other => panic!("unexpected event: {other:?}"),
            }
            tx.send(b).unwrap();
        }
        let result = handle.join().unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        (result, text)
    }

    #[test]
    fn csv_two_batches_stream() {
        let dir = std::env::temp_dir().join("mdbxer_export_test_csv");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.csv");
        let _ = std::fs::remove_file(&path);
        let job = job_at(&path, ExportFormat::Csv, false);
        let (result, text) = pipeline(
            job,
            vec![
                ExportBatch::Rows {
                    rows: vec![(b"a".to_vec(), b"1".to_vec())],
                    has_more: true,
                },
                ExportBatch::Rows {
                    rows: vec![
                        (b"b".to_vec(), b"2,3".to_vec()),
                        (b"c".to_vec(), b"x\"y".to_vec()),
                    ],
                    has_more: false,
                },
            ],
        );
        assert_eq!(result.unwrap(), 3);
        assert_eq!(text, "key,value\na,1\nb,\"2,3\"\nc,\"x\"\"y\"\n");
    }

    #[test]
    fn json_grouped_dup_spans_batches() {
        let dir = std::env::temp_dir().join("mdbxer_export_test_json_dup");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.json");
        let _ = std::fs::remove_file(&path);
        let job = job_at(&path, ExportFormat::Json, true);
        // 同一 Key 的三个值拆在两批，必须合并为一个 values 数组
        let (result, text) = pipeline(
            job,
            vec![
                ExportBatch::Rows {
                    rows: vec![
                        (b"k".to_vec(), b"v1".to_vec()),
                        (b"k".to_vec(), b"v2".to_vec()),
                    ],
                    has_more: true,
                },
                ExportBatch::Rows {
                    rows: vec![(b"k".to_vec(), b"v3".to_vec())],
                    has_more: false,
                },
            ],
        );
        assert_eq!(result.unwrap(), 3);
        assert_eq!(
            text,
            "[\n  {\"key\": \"k\", \"values\": [\"v1\", \"v2\", \"v3\"]}\n]\n"
        );
    }

    #[test]
    fn empty_table_produces_empty_json_array() {
        let dir = std::env::temp_dir().join("mdbxer_export_test_empty");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.json");
        let _ = std::fs::remove_file(&path);
        let job = job_at(&path, ExportFormat::Json, false);
        let (result, text) = pipeline(
            job,
            vec![ExportBatch::Rows {
                rows: vec![],
                has_more: false,
            }],
        );
        assert_eq!(result.unwrap(), 0);
        assert_eq!(text, "[\n]\n");
    }

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
