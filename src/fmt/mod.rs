//! 格式解析层：纯函数，仅依赖 std + chrono。

mod guess;
mod hexdump;
mod value;

pub use guess::guess;
pub use hexdump::{DEFAULT_HEX_WIDTH, HEX_WIDTHS, PAGE_BYTES, hex_dump};
pub use value::decode;

/// 多字节整数的字节序（默认小端，可切换为大端）。
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

    pub fn label(self) -> &'static str {
        match self {
            Endian::Little => "小端 LE",
            Endian::Big => "大端 BE",
        }
    }

    pub fn suffix(self) -> &'static str {
        match self {
            Endian::Little => "LE",
            Endian::Big => "BE",
        }
    }
}

/// 字节显示格式。
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

    pub fn label(self) -> &'static str {
        match self {
            DecodeMode::Auto => "自动",
            DecodeMode::Utf8 => "utf8",
            DecodeMode::Utf16Le => "utf16 (LE)",
            DecodeMode::Utf16Be => "utf16 (BE)",
            DecodeMode::I8 => "int8",
            DecodeMode::I16 => "int16",
            DecodeMode::I32 => "int32",
            DecodeMode::I64 => "int64",
            DecodeMode::U8 => "uint8",
            DecodeMode::U16 => "uint16",
            DecodeMode::U32 => "uint32",
            DecodeMode::U64 => "uint64",
            DecodeMode::F32 => "float",
            DecodeMode::F64 => "double",
            DecodeMode::Hex => "hex",
            DecodeMode::Dec => "dec",
            DecodeMode::Binary => "binary",
        }
    }
}
