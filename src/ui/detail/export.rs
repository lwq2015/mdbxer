// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 卡片内容的复制/另存：完整文本解码、格式猜测与推荐扩展名。
//! 纯函数为主，仅 [`save_proto_binding`] 需要应用状态。

use super::state::Side;
use crate::fmt::{self, DecodeMode};
use crate::ui::MdbxerApp;
use crate::ui::syntax::syntax_mode;

/// 复制按钮用：完整解码文本（不受 cell_max 截断）；Key 不做特殊格式猜测。
pub(super) fn text_of(
    bytes: &[u8],
    mode: DecodeMode,
    endian: fmt::Endian,
    hints: &fmt::HintFlags,
    side: Side,
) -> String {
    if side.is_key() {
        fmt::decode_key(bytes, mode, endian, usize::MAX, hints)
    } else {
        fmt::decode(bytes, mode, endian, usize::MAX, hints)
    }
}

/// 保存当前库的 Protobuf 关联表（.proto 文件列表 + message 选择），
/// 打开该库时自动恢复。未打开库（无宿主可挂）时跳过。
pub(super) fn save_proto_binding(app: &MdbxerApp, msg: Option<&str>) {
    if let Some(p) = &app.opened_path {
        crate::config::save_proto_binding(p, &crate::proto::files(), msg);
    }
}

/// Auto 模式 + 已载入协议池：全池试探解码（unknown 字段判别），命中时
/// 返回按该类型的 proto3 JSON pretty 文本（着色层复用 JSON 语法渲染）。
/// 未载入池/非 Auto/超长/无 0-unknown 候选时返回 None，走原文本管线。
pub(super) fn proto_auto_pretty(mode: DecodeMode, bytes: &[u8]) -> Option<String> {
    if mode != DecodeMode::Auto
        || !crate::proto::is_loaded()
        || bytes.len() > crate::proto::GUESS_MAX
    {
        return None;
    }

    crate::proto::guess_candidates(bytes)
        .into_iter()
        .find(|(_, unknown)| *unknown == 0)
        .and_then(|(name, _)| crate::proto::decode_with(&name, bytes))
}

/// 魔数格式名 → 推荐文件扩展名。
fn magic_ext(m: &str) -> &'static str {
    match m {
        "JPEG" => "jpg",
        "PNG" => "png",
        "GIF" => "gif",
        "WebP" => "webp",
        "WAV" => "wav",
        "MP3" => "mp3",
        "Ogg" => "ogg",
        "FLAC" => "flac",
        "ZIP" => "zip",
        "7z" => "7z",
        "RAR" => "rar",
        "PDF" => "pdf",
        "MP4" => "mp4",
        "ICO" => "ico",
        "SQLite" => "db",
        _ => "bin",
    }
}

/// 结构化解码模式 → (扩展名, 菜单标签)；JSON 类（含 BSON/MSGPACK/CBOR）
/// 统一落 JSON。
fn structured_ext(mode: DecodeMode) -> (&'static str, &'static str) {
    match mode {
        DecodeMode::Xml => ("xml", "XML"),
        DecodeMode::Yaml => ("yaml", "YAML"),
        DecodeMode::Toml => ("toml", "TOML"),
        _ => ("json", "JSON"),
    }
}

/// 内容嗅探：推荐文件扩展名（结构化 → 魔数 → 兜底 bin）。
fn sniff_ext(bytes: &[u8]) -> &'static str {
    if let Some(mode) = syntax_mode(DecodeMode::Auto, bytes) {
        return structured_ext(mode).0;
    }
    if let Some(m) = fmt::magic_hint(bytes) {
        return magic_ext(m);
    }
    "bin"
}

/// 另存菜单"猜测格式"候选：与 Auto 显示管线同优先级（压缩穿透 →
/// 结构化 pretty → PROTO → 二进制魔数），返回 (标签, 内容, 推荐扩展名)；
/// 无命中返回 None（菜单不出现该项）。
pub(super) fn guess_save(bytes: &[u8]) -> Option<(String, Vec<u8>, &'static str)> {
    // 压缩容器：存解压后的内层，标签穿透内层猜测（如 "gzip → JSON"）
    if let Some((algo, inner)) = fmt::decompress(bytes) {
        let (label, _) = fmt::guess(&inner, fmt::Endian::Little, &fmt::HintFlags::default());
        let ext = sniff_ext(&inner);
        return Some((format!("{algo} → {label}"), inner, ext));
    }

    // 结构化格式：存 pretty 文本（BSON/MSGPACK/CBOR 统一转 JSON）
    if let Some(mode) = syntax_mode(DecodeMode::Auto, bytes) {
        let (ext, label) = structured_ext(mode);
        let text = fmt::decode(
            bytes,
            mode,
            fmt::Endian::Little,
            usize::MAX,
            &fmt::HintFlags::default(),
        );
        return Some((format!("{label} pretty"), text.into_bytes(), ext));
    }

    // PROTO：存 proto3 JSON 文本
    if let Some(text) = proto_auto_pretty(DecodeMode::Auto, bytes) {
        return Some(("PROTO · JSON".to_string(), text.into_bytes(), "json"));
    }

    // 二进制魔数：原始字节配正确扩展名（JPEG/ZIP 等本体即该格式）
    if let Some(m) = fmt::magic_hint(bytes) {
        let ext = magic_ext(m);
        return Some((m.to_string(), bytes.to_vec(), ext));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{guess_save, magic_ext, sniff_ext, structured_ext};
    use crate::fmt::DecodeMode;

    #[test]
    fn magic_ext_maps_known_formats() {
        assert_eq!(magic_ext("JPEG"), "jpg");
        assert_eq!(magic_ext("ZIP"), "zip");
        assert_eq!(magic_ext("SQLite"), "db");
        assert_eq!(magic_ext("未知格式"), "bin");
    }

    #[test]
    fn structured_ext_falls_back_to_json() {
        assert_eq!(structured_ext(DecodeMode::Xml), ("xml", "XML"));
        assert_eq!(structured_ext(DecodeMode::Yaml), ("yaml", "YAML"));
        assert_eq!(structured_ext(DecodeMode::Toml), ("toml", "TOML"));
        assert_eq!(structured_ext(DecodeMode::Bson), ("json", "JSON"));
        assert_eq!(structured_ext(DecodeMode::Cbor), ("json", "JSON"));
    }

    #[test]
    fn sniff_ext_prefers_structured_then_magic() {
        assert_eq!(sniff_ext(br#"{"a":1}"#), "json");
        // PNG 魔数：并非结构化，落到魔数分支
        let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        assert_eq!(sniff_ext(&png), "png");
        assert_eq!(sniff_ext(b"\x01\x02\x03"), "bin");
    }

    #[test]
    fn guess_save_renders_structured_pretty() {
        let (label, data, ext) = guess_save(br#"{"a":1}"#).expect("JSON 应命中");
        assert_eq!(label, "JSON pretty");
        assert_eq!(ext, "json");
        let text = String::from_utf8(data).expect("pretty 文本应为 UTF-8");
        assert!(text.contains('\n'));
    }

    #[test]
    fn guess_save_keeps_raw_bytes_for_magic() {
        let png = [
            0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0x0D, b'I', b'H', b'D', b'R',
        ];
        let (label, data, ext) = guess_save(&png).expect("PNG 应命中");
        assert_eq!(label, "PNG");
        assert_eq!(ext, "png");
        assert_eq!(data, png);
    }

    #[test]
    fn guess_save_returns_none_for_plain_text() {
        assert!(guess_save(b"hello world").is_none());
    }
}
