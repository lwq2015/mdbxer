// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! Protobuf 动态解码：加载多个 .proto 文件（protox 纯 Rust 编译，无需 protoc），
//! 按用户选择的 message 类型把字节解码为 JSON 显示。
//!
//! 全局状态（进程单例）：已编译的类型池 + 已载入文件列表 + 当前 message 全名。
//! wire format 与随机二进制不可区分，故不进 Auto 猜测——只在用户显式选择
//! "proto" 排版时解码；未载入/未选类型/解码失败一律回退 UTF-8 文本。

use prost_reflect::{DescriptorPool, DynamicMessage};
use std::path::Path;
use std::sync::RwLock;

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

    let pool = POOL.read().unwrap();
    let desc = pool.as_ref()?.get_message_by_name(name)?;
    let msg = DynamicMessage::decode(desc, bytes).ok()?;
    serde_json::to_string_pretty(&msg).ok()
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
}
