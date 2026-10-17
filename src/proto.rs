// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! Protobuf 动态解码：加载多个 .proto 文件（protox 纯 Rust 编译，无需 protoc），
//! 按用户选择的 message 类型把字节解码为 JSON 显示。
//!
//! 全局状态（进程单例）：已编译的类型池 + 已载入文件列表 + 当前 message 全名。
//! wire format 与随机二进制不可区分，故"未载入协议池"时不进 Auto 猜测；
//! 已载入后可全池试探解码（unknown 字段判别，见 [`guess_candidates`]），
//! 手动排版按 current 类型解码；未载入/未选类型/解码失败回退 UTF-8 文本。

use prost_reflect::{DescriptorPool, DynamicMessage};
use std::path::Path;
use std::sync::RwLock;

/// 参与自动猜测的字节上限：全池试探解码的代价随值大小线性增长，
/// 表格渲染每行都会调用；超过上限的值不猜（显示 hex，手动排版仍可看）。
pub const GUESS_MAX: usize = 2 * 1024;

/// 已编译的类型池（None = 未载入任何 .proto）。
static POOL: RwLock<Option<DescriptorPool>> = RwLock::new(None);

/// 已载入的 .proto 文件路径（config 持久化的内存镜像）。
static FILES: RwLock<Vec<String>> = RwLock::new(Vec::new());

/// 当前选择的 message 类型全名（如 "mypkg.MyMsg"，全局记忆）。
static CURRENT: RwLock<Option<String>> = RwLock::new(None);

/// 编译并载入多个 .proto 文件，整体替换现有类型池（失败保留旧池）。
/// include 搜索路径取各文件的父目录（去重）。返回池内 message 类型总数。
pub fn load_files<P: AsRef<Path>>(paths: &[P]) -> Result<usize, String> {
    if paths.is_empty() {
        return Err("no proto files".to_string());
    }

    // include 搜索路径：各文件的父目录，去重保序
    let mut includes: Vec<&Path> = Vec::new();
    for p in paths {
        let dir = p.as_ref().parent().unwrap_or_else(|| Path::new("."));
        if !includes.contains(&dir) {
            includes.push(dir);
        }
    }

    let fds = protox::compile(paths, includes).map_err(|e| e.to_string())?;
    let pool = DescriptorPool::from_file_descriptor_set(fds).map_err(|e| e.to_string())?;
    let n = pool.all_messages().count();

    *POOL.write().unwrap() = Some(pool);
    *FILES.write().unwrap() = paths
        .iter()
        .map(|p| p.as_ref().display().to_string())
        .collect();

    // 旧选择在新池中不存在时清掉，避免悬空
    if let Some(cur) = current() {
        if !message_names().iter().any(|n| *n == cur) {
            set_current(None);
        }
    }

    Ok(n)
}

/// 池内全部 message 类型全名（升序）。
pub fn message_names() -> Vec<String> {
    let pool = POOL.read().unwrap();

    match pool.as_ref() {
        Some(p) => {
            let mut names: Vec<String> =
                p.all_messages().map(|m| m.full_name().to_string()).collect();
            names.sort();
            names
        }
        None => Vec::new(),
    }
}

/// 当前 message 全名。
pub fn current() -> Option<String> {
    CURRENT.read().unwrap().clone()
}

/// 设置当前 message 全名（None = 清除）。
pub fn set_current(name: Option<String>) {
    *CURRENT.write().unwrap() = name;
}

/// 已载入的 .proto 文件列表。
pub fn files() -> Vec<String> {
    FILES.read().unwrap().clone()
}

/// 是否已载入协议池。
pub fn is_loaded() -> bool {
    POOL.read().unwrap().is_some()
}

/// 清空全部状态：类型池、文件列表、当前选择（清除库关联时调用）。
pub fn clear() {
    *POOL.write().unwrap() = None;
    FILES.write().unwrap().clear();
    *CURRENT.write().unwrap() = None;
}

/// 自动猜测候选：对池内全部 message 类型（跳过 map entry 内部类型）
/// 试探解码，返回 (类型全名, unknown 字段个数) 按 unknown 升序。
///
/// 原理：proto3 解码容忍未知字段——错误类型解码"成功"时字段几乎都落进
/// unknown；正确类型 unknown 为空。因此 unknown == 0 是强判别信号；
/// 全部候选都有 unknown 时（随机二进制/截断数据）调用方应放弃猜测。
pub fn guess_candidates(bytes: &[u8]) -> Vec<(String, usize)> {
    if bytes.is_empty() {
        return Vec::new();
    }

    let pool = POOL.read().unwrap();
    let Some(p) = pool.as_ref() else {
        return Vec::new();
    };

    let mut out: Vec<(String, usize)> = Vec::new();

    for m in p.all_messages() {
        if m.is_map_entry() {
            continue;
        }

        let Ok(msg) = DynamicMessage::decode(m.clone(), bytes) else {
            continue;
        };

        let unknown = msg.unknown_fields().count();
        out.push((m.full_name().to_string(), unknown));
    }

    out.sort_by_key(|(_, unknown)| *unknown);
    out
}

/// 按指定 message 全名解码为 DynamicMessage（内部共用）。
fn decode_msg(name: &str, bytes: &[u8]) -> Option<DynamicMessage> {
    if bytes.is_empty() {
        return None;
    }

    let pool = POOL.read().unwrap();
    let desc = pool.as_ref()?.get_message_by_name(name)?;
    DynamicMessage::decode(desc, bytes).ok()
}

/// 按当前 message 类型解码字节为 pretty JSON。
/// 未载入/未选类型/解码失败 → None（调用方回退 UTF-8 文本）。
pub fn decode_current(bytes: &[u8]) -> Option<String> {
    decode_with(&current()?, bytes)
}

/// 按指定 message 全名解码字节为 pretty JSON。
/// 字段名遵循 proto3 JSON mapping（lowerCamelCase），64 位整数转字符串。
pub fn decode_with(name: &str, bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return Some(String::new());
    }

    let msg = decode_msg(name, bytes)?;
    serde_json::to_string_pretty(&msg).ok()
}

/// 按指定 message 全名解码字节为紧凑 JSON（单行，表格 Auto 列预览用）。
pub fn decode_compact(name: &str, bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return Some(String::new());
    }

    let msg = decode_msg(name, bytes)?;
    serde_json::to_string(&msg).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 全局池被全部测试共享，proto 测试串行执行避免互相覆盖。
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn compile_load_decode_and_error_paths() {
        let _g = TEST_LOCK.lock().unwrap();

        let dir = std::env::temp_dir().join(format!("mdbxer_proto_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("t.proto");
        std::fs::write(
            &path,
            concat!(
                r#"syntax = "proto3"; package t;"#,
                "\n",
                r#"message Msg { int64 id = 1; string name = 2; repeated int32 nums = 3; }"#,
                "\n",
                r#"message Empty {}"#,
            ),
        )
        .unwrap();

        // 载入成功：类型总数 >= 2，全名可查
        let n = load_files(&[&path]).unwrap();
        assert!(n >= 2);

        let names = message_names();
        assert!(names.contains(&"t.Msg".to_string()), "names={names:?}");
        assert!(names.contains(&"t.Empty".to_string()));

        // field1 varint 42、field2 "hi"、field3 packed varint [1, 300]（len=3）
        // int64 按 proto3 JSON mapping 序列化为字符串（"42"），int32 保持数字
        let bytes = [0x08, 0x2A, 0x12, 0x02, b'h', b'i', 0x1A, 0x03, 0x01, 0xAC, 0x02];
        let json = decode_with("t.Msg", &bytes).unwrap();
        assert!(json.contains("\"id\": \"42\""), "json={json}");
        assert!(json.contains("\"name\": \"hi\""), "json={json}");
        assert!(json.contains("300"), "json={json}");

        // 当前选择 + decode_current 通路
        set_current(Some("t.Msg".to_string()));
        assert_eq!(decode_current(&bytes), Some(json));

        // 空字节 → 空文本
        assert_eq!(decode_with("t.Msg", b""), Some(String::new()));

        // 未知类型 / 截断的 varint → None
        assert!(decode_with("t.Nope", &bytes).is_none());
        assert!(decode_with("t.Msg", &[0x08]).is_none());
        assert!(decode_current(b"x").is_none()); // 未选类型时也是 None

        // 空文件列表
        assert!(load_files::<&Path>(&[]).is_err());

        // 载入失败保留旧池
        let bad = dir.join("bad.proto");
        std::fs::write(&bad, "this is not proto").unwrap();
        assert!(load_files(&[&bad]).is_err());
        assert!(message_names().contains(&"t.Msg".to_string()));

        set_current(None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn guess_candidates_and_auto_label() {
        let _g = TEST_LOCK.lock().unwrap();

        let dir = std::env::temp_dir().join(format!("mdbxer_proto_guess_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("g.proto");
        std::fs::write(
            &path,
            concat!(
                r#"syntax = "proto3"; package t2;"#,
                "\n",
                r#"message A { int32 x = 1; }"#,
                "\n",
                r#"message B { string y = 2; }"#,
            ),
        )
        .unwrap();
        load_files(&[&path]).unwrap();
        assert!(is_loaded());

        // field1 varint 42：A 全部已知（unknown=0）排第一；B 无 field1，
        // 字节落进 unknown（unknown>0）
        let bytes = [0x08, 0x2A];
        let cands = guess_candidates(&bytes);
        assert_eq!(cands[0], ("t2.A".to_string(), 0), "cands={cands:?}");
        assert!(cands.iter().any(|(n, u)| n == "t2.B" && *u > 0));

        // Auto 集成：fmt::guess 标签带类型全名，文本为紧凑 JSON
        let (label, text) = crate::fmt::guess(&bytes, crate::fmt::Endian::Little, &crate::fmt::HintFlags::default());
        assert_eq!(label, "PROTO · t2.A", "label={label}");
        assert_eq!(text, r#"{"x":42}"#, "text={text}");

        // 紧凑/pretty 两个出口
        assert_eq!(decode_compact("t2.A", &bytes).as_deref(), Some(r#"{"x":42}"#));
        assert!(decode_with("t2.A", &bytes).unwrap().contains("\n"));

        // 截断的 varint：解码失败 → 无候选；fmt::guess 不猜 proto
        assert!(guess_candidates(&[0x08]).is_empty());
        let (label2, _) = crate::fmt::guess(&[0x08], crate::fmt::Endian::Little, &crate::fmt::HintFlags::default());
        assert!(!label2.starts_with("PROTO"), "label2={label2}");

        // 空池（未载入时）——这里已载入，验证清理路径即可
        set_current(None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
