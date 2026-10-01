// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 格式解析层：纯函数，仅依赖 std + chrono。

use std::sync::atomic::{AtomicBool, Ordering};

mod guess;
mod hexdump;
mod value;

pub use guess::guess;
pub use hexdump::{DEFAULT_HEX_WIDTH, HEX_WIDTHS, PAGE_BYTES, hex_dump};
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
}

impl DecodeMode {
    pub const ALL: [DecodeMode; 17] = [
        DecodeMode::Auto,
        DecodeMode::Utf8,
        DecodeMode::Utf16Le,
        DecodeMode::Utf16Be,
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
    ];

    /// 下拉框显示文本（"自动"随界面语言；类型名为国际通用写法不翻译）。
    pub fn label(self) -> String {
        match self {
            DecodeMode::Auto => crate::i18n::tr().mode_auto.to_string(),
            DecodeMode::Utf8 => "utf8".to_string(),
            DecodeMode::Utf16Le => "utf16 (LE)".to_string(),
            DecodeMode::Utf16Be => "utf16 (BE)".to_string(),
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
        }
    }
}
