// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 格式解析层：纯函数，仅依赖 std + chrono。

use std::sync::atomic::{AtomicBool, Ordering};

mod guess;
mod hexdump;
mod value;

pub use guess::guess;
pub(crate) use hexdump::{
    ADDR_CHARS, hex_copy_selection, hex_line, hex_section_chars, mid_gap_index,
};
pub use hexdump::{DEFAULT_HEX_WIDTH, HEX_WIDTHS, PAGE_BYTES};
pub use value::decode;

/// 整数千位分隔开关（默认开）：1,625,981,420 vs 1625981420。
/// 全局开关而非 decode 参数——decode/guess 调用点太多，且这是纯显示偏好。
static THOUSANDS_SEP: AtomicBool = AtomicBool::new(true);

/// 设置整数千位分隔开关。
pub fn set_thousands_sep(on: bool) {
    THOUSANDS_SEP.store(on, Ordering::Relaxed);
}

/// 当前是否启用整数千位分隔。
pub fn thousands_sep() -> bool {
    THOUSANDS_SEP.load(Ordering::Relaxed)
}

/// 多字节整数的字节序。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Endian {
    /// 小端（MDBX INTEGER_KEY 的本机字节序，默认）
    #[default]
    Little,
    /// 大端
    Big,
}

impl Endian {
    pub const ALL: [Endian; 2] = [Endian::Little, Endian::Big];

    /// 下拉框显示文本（随界面语言）。
    pub fn label(self) -> String {
        let t = crate::i18n::tr();
        match self {
            Endian::Little => t.endian_le.to_string(),
            Endian::Big => t.endian_be.to_string(),
        }
    }

    pub fn suffix(self) -> &'static str {
        match self {
            Endian::Little => "LE",
            Endian::Big => "BE",
        }
    }
}

/// 字节显示/解码格式。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DecodeMode {
    /// 按内容自动猜测
    #[default]
    Auto,
    Utf8,
    Utf16Le,
    Utf16Be,
    /// GB18030（GBK/GB2312 超集，简体中文传统编码）
    Gb18030,
    /// Windows-1251（西里尔字母，俄语传统编码）
    Cp1251,
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    Hex,
    Dec,
    Binary,
    /// Base64 编码文本（RFC 4648 标准字母表，带填充）
    Base64,
    /// UUID（仅 16 字节；其他长度回退 hex）
    Uuid,
    /// JSON 美化（非法 JSON 回退 UTF-8 文本）
    Json,
}

impl DecodeMode {
    pub const ALL: [DecodeMode; 22] = [
        DecodeMode::Auto,
        DecodeMode::Utf8,
        DecodeMode::Utf16Le,
        DecodeMode::Utf16Be,
        DecodeMode::Gb18030,
        DecodeMode::Cp1251,
        DecodeMode::I8,
        DecodeMode::I16,
        DecodeMode::I32,
        DecodeMode::I64,
        DecodeMode::U8,
        DecodeMode::U16,
        DecodeMode::U32,
        DecodeMode::U64,
        DecodeMode::F32,
        DecodeMode::F64,
        DecodeMode::Hex,
        DecodeMode::Dec,
        DecodeMode::Binary,
        DecodeMode::Base64,
        DecodeMode::Uuid,
        DecodeMode::Json,
    ];

    /// 下拉框显示文本（"自动"随界面语言；类型名为国际通用写法不翻译）。
    pub fn label(self) -> String {
        match self {
            DecodeMode::Auto => crate::i18n::tr().mode_auto.to_string(),
            DecodeMode::Utf8 => "utf8".to_string(),
            DecodeMode::Utf16Le => "utf16 (LE)".to_string(),
            DecodeMode::Utf16Be => "utf16 (BE)".to_string(),
            DecodeMode::Gb18030 => "gb18030".to_string(),
            DecodeMode::Cp1251 => "windows-1251".to_string(),
            DecodeMode::I8 => "int8".to_string(),
            DecodeMode::I16 => "int16".to_string(),
            DecodeMode::I32 => "int32".to_string(),
            DecodeMode::I64 => "int64".to_string(),
            DecodeMode::U8 => "uint8".to_string(),
            DecodeMode::U16 => "uint16".to_string(),
            DecodeMode::U32 => "uint32".to_string(),
            DecodeMode::U64 => "uint64".to_string(),
            DecodeMode::F32 => "float".to_string(),
            DecodeMode::F64 => "double".to_string(),
            DecodeMode::Hex => "hex".to_string(),
            DecodeMode::Dec => "dec".to_string(),
            DecodeMode::Binary => "binary".to_string(),
            DecodeMode::Base64 => "base64".to_string(),
            DecodeMode::Uuid => "uuid".to_string(),
            DecodeMode::Json => "json".to_string(),
        }
    }

    /// 稳定字符串标识（配置持久化用）。
    pub fn as_str(self) -> &'static str {
        match self {
            DecodeMode::Auto => "auto",
            DecodeMode::Utf8 => "utf8",
            DecodeMode::Utf16Le => "utf16le",
            DecodeMode::Utf16Be => "utf16be",
            DecodeMode::Gb18030 => "gb18030",
            DecodeMode::Cp1251 => "cp1251",
            DecodeMode::I8 => "i8",
            DecodeMode::I16 => "i16",
            DecodeMode::I32 => "i32",
            DecodeMode::I64 => "i64",
            DecodeMode::U8 => "u8",
            DecodeMode::U16 => "u16",
            DecodeMode::U32 => "u32",
            DecodeMode::U64 => "u64",
            DecodeMode::F32 => "f32",
            DecodeMode::F64 => "f64",
            DecodeMode::Hex => "hex",
            DecodeMode::Dec => "dec",
            DecodeMode::Binary => "bin",
            DecodeMode::Base64 => "b64",
            DecodeMode::Uuid => "uuid",
            DecodeMode::Json => "json",
        }
    }

    /// 从 [`as_str`](Self::as_str) 标识解析；未知值回退 Auto。
    pub fn from_str(s: &str) -> DecodeMode {
        match s {
            "utf8" => DecodeMode::Utf8,
            "utf16le" => DecodeMode::Utf16Le,
            "utf16be" => DecodeMode::Utf16Be,
            "gb18030" => DecodeMode::Gb18030,
            "cp1251" => DecodeMode::Cp1251,
            "i8" => DecodeMode::I8,
            "i16" => DecodeMode::I16,
            "i32" => DecodeMode::I32,
            "i64" => DecodeMode::I64,
            "u8" => DecodeMode::U8,
            "u16" => DecodeMode::U16,
            "u32" => DecodeMode::U32,
            "u64" => DecodeMode::U64,
            "f32" => DecodeMode::F32,
            "f64" => DecodeMode::F64,
            "hex" => DecodeMode::Hex,
            "dec" => DecodeMode::Dec,
            "bin" => DecodeMode::Binary,
            "b64" => DecodeMode::Base64,
            "uuid" => DecodeMode::Uuid,
            "json" => DecodeMode::Json,
            _ => DecodeMode::Auto,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endian_suffix() {
        assert_eq!(Endian::Little.suffix(), "LE");
        assert_eq!(Endian::Big.suffix(), "BE");
    }

    #[test]
    fn endian_all_contains_two() {
        assert_eq!(Endian::ALL.len(), 2);
        assert!(Endian::ALL.contains(&Endian::Little));
        assert!(Endian::ALL.contains(&Endian::Big));
    }

    #[test]
    fn endian_default_is_little() {
        assert_eq!(Endian::default(), Endian::Little);
    }

    #[test]
    fn endian_label_follows_lang() {
        let prev = crate::i18n::lang();
        crate::i18n::set_lang(crate::i18n::Lang::En);
        let t = crate::i18n::tr();
        assert_eq!(Endian::Little.label(), t.endian_le);
        assert_eq!(Endian::Big.label(), t.endian_be);
        crate::i18n::set_lang(crate::i18n::Lang::Zh);
        let t = crate::i18n::tr();
        assert_eq!(Endian::Little.label(), t.endian_le);
        crate::i18n::set_lang(prev);
    }

    #[test]
    fn decode_mode_all_complete() {
        assert_eq!(DecodeMode::ALL.len(), 22);
    }

    #[test]
    fn decode_mode_as_str_from_str_round_trip() {
        for m in DecodeMode::ALL {
            assert_eq!(DecodeMode::from_str(m.as_str()), m);
        }
        // 未知值回退 Auto
        assert_eq!(DecodeMode::from_str(""), DecodeMode::Auto);
        assert_eq!(DecodeMode::from_str("???"), DecodeMode::Auto);
    }

    #[test]
    fn decode_mode_default_is_auto() {
        assert_eq!(DecodeMode::default(), DecodeMode::Auto);
    }

    #[test]
    fn decode_mode_labels_are_type_names_except_auto() {
        let prev = crate::i18n::lang();
        crate::i18n::set_lang(crate::i18n::Lang::Zh);
        // Auto 随语言翻译
        assert_eq!(DecodeMode::Auto.label(), crate::i18n::tr().mode_auto);
        // 其余是固定类型名
        assert_eq!(DecodeMode::U8.label(), "uint8");
        assert_eq!(DecodeMode::I32.label(), "int32");
        assert_eq!(DecodeMode::Hex.label(), "hex");
        crate::i18n::set_lang(prev);
    }

    #[test]
    fn thousands_sep_toggle() {
        let prev = thousands_sep();
        set_thousands_sep(false);
        assert!(!thousands_sep());
        set_thousands_sep(true);
        assert!(thousands_sep());
        set_thousands_sep(prev);
    }
}
