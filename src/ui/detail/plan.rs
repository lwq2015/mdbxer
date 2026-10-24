// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 卡片显示决议：把"解码模式 + 当前段字节"归纳成实际显示文本与着色格式。
//!
//! 特殊格式解析统一抽象成 [`FormatProbe`]：结构化、压缩穿透、Protobuf 猜型
//! 各自独立实现，由 [`plan_display`] 按既有优先级合成结果。

use super::export::proto_auto_pretty;
use super::state::Side;
use crate::fmt::{self, DecodeMode, Endian, HintFlags};
use crate::ui::syntax::syntax_mode;

/// 特殊格式探测的输入。
pub(crate) struct ProbeCtx<'a> {
    /// 当前段窗口字节
    pub bytes: &'a [u8],
    /// 卡片角色
    pub side: Side,
    /// 当前解码模式
    pub mode: DecodeMode,
}

/// 命中后显示文本的来源。
pub(crate) enum DisplaySource {
    /// 对探测时的窗口字节按该格式重解码
    Window(DecodeMode),
    /// 对解压后的内层字节按该格式重解码
    Inner { bytes: Vec<u8>, mode: DecodeMode },
    /// 已生成好的显示文本
    Ready(String),
}

/// 一次特殊格式命中的结果：显示文本来源 + 语法着色格式。
pub(crate) struct Detected {
    /// 显示文本怎么来
    pub source: DisplaySource,
    /// 着色使用哪种格式
    pub syntax: DecodeMode,
}

/// 特殊格式解析接口：一种"原始字节 → 更适合阅读的展示形式"的转换策略。
/// 返回 None 表示本策略不适用或未命中。
pub(crate) trait FormatProbe {
    /// 探测当前窗口。
    fn probe(&self, ctx: &ProbeCtx<'_>) -> Option<Detected>;
}

/// 结构化格式（JSON/BSON/MSGPACK/CBOR/XML/YAML/TOML）：显示时 pretty 重解码。
pub(crate) struct StructuredProbe;

impl FormatProbe for StructuredProbe {
    fn probe(&self, ctx: &ProbeCtx<'_>) -> Option<Detected> {
        if ctx.mode != DecodeMode::Auto || !ctx.side.allows_guess() {
            return None;
        }
        let mode = syntax_mode(DecodeMode::Auto, ctx.bytes)?;
        Some(Detected {
            source: DisplaySource::Window(mode),
            syntax: mode,
        })
    }
}

/// 压缩容器：解压穿透，按内层结构化格式显示。
pub(crate) struct CompressionProbe;

impl FormatProbe for CompressionProbe {
    fn probe(&self, ctx: &ProbeCtx<'_>) -> Option<Detected> {
        if ctx.mode != DecodeMode::Auto || !ctx.side.allows_guess() {
            return None;
        }
        let (_, inner) = fmt::decompress(ctx.bytes)?;
        let mode = syntax_mode(DecodeMode::Auto, &inner)?;
        Some(Detected {
            source: DisplaySource::Inner { bytes: inner, mode },
            syntax: mode,
        })
    }
}

/// Protobuf 猜型：Auto + 已载入协议池时按命中的 message 输出 proto3 JSON。
pub(crate) struct ProtoProbe;

impl FormatProbe for ProtoProbe {
    fn probe(&self, ctx: &ProbeCtx<'_>) -> Option<Detected> {
        if !ctx.side.allows_guess() {
            return None;
        }
        let text = proto_auto_pretty(ctx.mode, ctx.bytes)?;
        Some(Detected {
            source: DisplaySource::Ready(text),
            // proto3 JSON mapping 的输出就是 JSON 文本，着色复用 JSON 语法
            syntax: DecodeMode::Json,
        })
    }
}

/// 卡片文本区的显示决议。
pub(crate) struct DisplayPlan {
    /// 实际显示文本（可能为 pretty 重编码）
    pub text: String,
    /// 语法着色格式；None = 不着色
    pub syntax: Option<DecodeMode>,
}

/// 生成卡片文本区的显示决议。
///
/// - 手动模式：显示文本即当前模式解码结果，按内容合法性决定着色；
/// - Auto + Key：不做特殊格式猜测，原样显示、不着色；
/// - Auto + Value：按压缩 → proto → 结构的显示优先级选择增强，
///   着色优先级则是结构 → 压缩 → proto（既有行为，勿合并成一次遍历）。
pub(crate) fn plan_display(
    mode: DecodeMode,
    side: Side,
    window: &[u8],
    fallback: &str,
    endian: Endian,
    hints: &HintFlags,
) -> DisplayPlan {
    if mode != DecodeMode::Auto {
        return DisplayPlan {
            text: fallback.to_string(),
            syntax: syntax_mode(mode, window),
        };
    }

    if !side.allows_guess() {
        return DisplayPlan {
            text: fallback.to_string(),
            syntax: None,
        };
    }

    let ctx = ProbeCtx {
        bytes: window,
        side,
        mode,
    };
    let structured = StructuredProbe.probe(&ctx);
    let compressed = CompressionProbe.probe(&ctx);
    let proto = ProtoProbe.probe(&ctx);

    let syntax = structured
        .as_ref()
        .map(|d| d.syntax)
        .or(compressed.as_ref().map(|d| d.syntax))
        .or(proto.as_ref().map(|d| d.syntax));

    let chosen = compressed.or(proto).or(structured);
    let text = match chosen.map(|d| d.source) {
        Some(DisplaySource::Window(m)) => decode_pretty(window, m, endian, hints),
        Some(DisplaySource::Inner { bytes, mode }) => decode_pretty(&bytes, mode, endian, hints),
        Some(DisplaySource::Ready(text)) => text,
        None => fallback.to_string(),
    };

    DisplayPlan { text, syntax }
}

/// pretty 重解码：宽松上限（格式化后字符数可能数倍于字节数）。
fn decode_pretty(bytes: &[u8], mode: DecodeMode, endian: Endian, hints: &HintFlags) -> String {
    let cap = bytes.len().saturating_mul(4) + 256;
    fmt::decode(bytes, mode, endian, cap, hints)
}

#[cfg(test)]
mod tests {
    use super::{FormatProbe, ProbeCtx, ProtoProbe, StructuredProbe, plan_display};
    use crate::fmt::{DecodeMode, Endian, HintFlags};
    use crate::ui::detail::state::Side;

    const JSON: &[u8] = br#"{"a":1,"b":[2,3]}"#;

    fn plan(
        mode: DecodeMode,
        side: Side,
        bytes: &[u8],
        text: &str,
    ) -> (String, Option<DecodeMode>) {
        let p = plan_display(
            mode,
            side,
            bytes,
            text,
            Endian::Little,
            &HintFlags::default(),
        );
        (p.text, p.syntax)
    }

    #[test]
    fn auto_value_pretty_prints_structured_json() {
        let (text, syntax) = plan(DecodeMode::Auto, Side::Value, JSON, "{\"a\":1,\"b\":[2,3]}");
        assert_eq!(syntax, Some(DecodeMode::Json));
        assert!(text.contains('\n'));
        assert!(text.contains("\"a\": 1"));
    }

    #[test]
    fn auto_key_keeps_raw_text_and_plain_color() {
        let raw = "{\"a\":1,\"b\":[2,3]}";
        let (text, syntax) = plan(DecodeMode::Auto, Side::Key, JSON, raw);
        assert_eq!(text, raw);
        assert_eq!(syntax, None);
    }

    #[test]
    fn manual_mode_keeps_decoded_text_and_enables_syntax() {
        let raw = "{\"a\": 1}";
        let (text, syntax) = plan(DecodeMode::Json, Side::Value, JSON, raw);
        assert_eq!(text, raw);
        assert_eq!(syntax, Some(DecodeMode::Json));
    }

    #[test]
    fn manual_mode_with_invalid_payload_stays_plain() {
        let raw = "not json";
        let (text, syntax) = plan(DecodeMode::Json, Side::Value, b"not json", raw);
        assert_eq!(text, raw);
        assert_eq!(syntax, None);
    }

    #[test]
    fn auto_plain_text_is_untouched() {
        let (text, syntax) = plan(DecodeMode::Auto, Side::Value, b"hello", "hello");
        assert_eq!(text, "hello");
        assert_eq!(syntax, None);
    }

    #[test]
    fn structured_probe_rejects_key_and_manual_mode() {
        let json = JSON;
        let key_ctx = ProbeCtx {
            bytes: json,
            side: Side::Key,
            mode: DecodeMode::Auto,
        };
        assert!(StructuredProbe.probe(&key_ctx).is_none());

        let manual_ctx = ProbeCtx {
            bytes: json,
            side: Side::Value,
            mode: DecodeMode::Json,
        };
        assert!(StructuredProbe.probe(&manual_ctx).is_none());
    }

    #[test]
    fn proto_probe_requires_auto_and_loaded_pool() {
        // 未载入协议池时恒不命中（无论字节内容）
        let ctx = ProbeCtx {
            bytes: JSON,
            side: Side::Value,
            mode: DecodeMode::Auto,
        };
        assert!(ProtoProbe.probe(&ctx).is_none());
    }
}
