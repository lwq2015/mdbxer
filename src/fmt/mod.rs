// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 格式解析层：纯函数，仅依赖 std + chrono。

use std::sync::atomic::{AtomicBool, Ordering};

mod compress;
mod guess;
mod hexdump;
mod value;

pub use guess::{HintFlags, guess};
pub(crate) use compress::decompress;
pub(crate) use guess::{is_bson, is_cbor, is_msgpack, is_structured_json, is_toml, is_xml, is_yaml, magic_hint};
pub(crate) use hexdump::{
    addr_chars, addr_digits, hex_copy_selection, hex_line, hex_section_chars, mid_gap_index,
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
    /// Shift-JIS（日文传统编码）
    Sjis,
    /// Big5（繁体中文传统编码）
    Big5,
    /// Windows-1252（西欧语言传统编码，Latin-1 超集）
    Cp1252,
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
    /// MAC 地址（仅 6 字节；其他长度回退 hex）
    Mac,
    /// IP 地址（4 字节 IPv4 / 16 字节 IPv6；其他长度回退 hex）
    Ip,
    /// 时间戳（4 字节按 Unix 秒 / 8 字节自动判 Unix 秒·毫秒·FILETIME；其他长度回退 hex）
    Time,
    /// JSON 美化（非法 JSON 回退 UTF-8 文本）
    Json,
    /// BSON 文档 → 宽松扩展 JSON 显示（非法 BSON 回退 UTF-8 文本）
    Bson,
    /// MessagePack → JSON 显示（非法数据回退 UTF-8 文本）
    Msgpack,
    /// CBOR → JSON 显示（非法数据回退 UTF-8 文本）
    Cbor,
    /// Protobuf 动态解码为 JSON（需载入 .proto 并选择 message 类型；失败回退 UTF-8 文本）
    Proto,
    /// YAML 规范化重排（非法回退 UTF-8 文本）
    Yaml,
    /// TOML 规范化重排（非法回退 UTF-8 文本）
    Toml,
    /// XML 缩进重排（非法回退 UTF-8 文本）
    Xml,
}

impl DecodeMode {
    /// 下拉分组（成员与顺序固定，组名经 UI 层 i18n 动态显示）：
    /// 文本编码 / 数值 / 字节视图 / 标识与时间 / 结构化。
    pub const GROUPS: [&'static [DecodeMode]; 5] = [
        &[
            Self::Utf8,
            Self::Utf16Le,
            Self::Utf16Be,
            Self::Gb18030,
            Self::Cp1251,
            Self::Sjis,
            Self::Big5,
            Self::Cp1252,
        ],
        &[
            Self::I8,
            Self::I16,
            Self::I32,
            Self::I64,
            Self::U8,
            Self::U16,
            Self::U32,
            Self::U64,
            Self::F32,
            Self::F64,
        ],
        &[Self::Hex, Self::Dec, Self::Binary, Self::Base64],
        &[Self::Uuid, Self::Mac, Self::Ip, Self::Time],
        &[
            Self::Json,
            Self::Bson,
            Self::Msgpack,
            Self::Cbor,
            Self::Proto,
            Self::Yaml,
            Self::Toml,
            Self::Xml,
        ],
    ];

    /// 滚轮循环遍历的平铺序：Auto 置顶 + 分组顺序拼接
    /// （覆盖全部变体、无重复；下拉分组渲染见 ui::format_groups）。
    pub fn flat_all() -> Vec<DecodeMode> {
        std::iter::once(Self::Auto)
            .chain(Self::GROUPS.into_iter().flatten().copied())
            .collect()
    }

    /// 下拉框显示文本（"自动"随界面语言；类型名为国际通用写法不翻译）。
    pub fn label(self) -> String {
        match self {
            DecodeMode::Auto => crate::i18n::tr().mode_auto.to_string(),

            // 文本与编码
            DecodeMode::Utf8 => "utf8".to_string(),
            DecodeMode::Utf16Le => "utf16 (LE)".to_string(),
            DecodeMode::Utf16Be => "utf16 (BE)".to_string(),
            DecodeMode::Gb18030 => "gb18030".to_string(),
            DecodeMode::Cp1251 => "windows-1251".to_string(),
            DecodeMode::Sjis => "shift_jis".to_string(),
            DecodeMode::Big5 => "big5".to_string(),
            DecodeMode::Cp1252 => "windows-1252".to_string(),

            // 整数
            DecodeMode::I8 => "int8".to_string(),
            DecodeMode::I16 => "int16".to_string(),
            DecodeMode::I32 => "int32".to_string(),
            DecodeMode::I64 => "int64".to_string(),
            DecodeMode::U8 => "uint8".to_string(),
            DecodeMode::U16 => "uint16".to_string(),
            DecodeMode::U32 => "uint32".to_string(),
            DecodeMode::U64 => "uint64".to_string(),

            // 浮点
            DecodeMode::F32 => "float".to_string(),
            DecodeMode::F64 => "double".to_string(),

            // 进制/二进制展示
            DecodeMode::Hex => "hex".to_string(),
            DecodeMode::Dec => "dec".to_string(),
            DecodeMode::Binary => "binary".to_string(),

            // 编码文本类
            DecodeMode::Base64 => "base64".to_string(),
            DecodeMode::Uuid => "uuid".to_string(),
            DecodeMode::Mac => "mac".to_string(),
            DecodeMode::Ip => "ip".to_string(),
            DecodeMode::Time => "time".to_string(),
            DecodeMode::Json => "json".to_string(),
            DecodeMode::Bson => "bson".to_string(),
            DecodeMode::Msgpack => "msgpack".to_string(),
            DecodeMode::Cbor => "cbor".to_string(),
            DecodeMode::Proto => "proto".to_string(),
            DecodeMode::Yaml => "yaml".to_string(),
            DecodeMode::Toml => "toml".to_string(),
            DecodeMode::Xml => "xml".to_string(),
        }
    }

    /// 稳定字符串标识（配置持久化用）。
    pub fn as_str(self) -> &'static str {
        match self {
            DecodeMode::Auto => "auto",

            // 文本与编码
            DecodeMode::Utf8 => "utf8",
            DecodeMode::Utf16Le => "utf16le",
            DecodeMode::Utf16Be => "utf16be",
            DecodeMode::Gb18030 => "gb18030",
            DecodeMode::Cp1251 => "cp1251",
            DecodeMode::Sjis => "sjis",
            DecodeMode::Big5 => "big5",
            DecodeMode::Cp1252 => "cp1252",

            // 整数
            DecodeMode::I8 => "i8",
            DecodeMode::I16 => "i16",
            DecodeMode::I32 => "i32",
            DecodeMode::I64 => "i64",
            DecodeMode::U8 => "u8",
            DecodeMode::U16 => "u16",
            DecodeMode::U32 => "u32",
            DecodeMode::U64 => "u64",

            // 浮点
            DecodeMode::F32 => "f32",
            DecodeMode::F64 => "f64",

            // 进制/二进制展示
            DecodeMode::Hex => "hex",
            DecodeMode::Dec => "dec",
            DecodeMode::Binary => "bin",

            // 编码文本类
            DecodeMode::Base64 => "b64",
            DecodeMode::Uuid => "uuid",
            DecodeMode::Mac => "mac",
            DecodeMode::Ip => "ip",
            DecodeMode::Time => "time",
            DecodeMode::Json => "json",
            DecodeMode::Bson => "bson",
            DecodeMode::Msgpack => "msgpack",
            DecodeMode::Cbor => "cbor",
            DecodeMode::Proto => "proto",
            DecodeMode::Yaml => "yaml",
            DecodeMode::Toml => "toml",
            DecodeMode::Xml => "xml",
        }
    }

    /// 从 [`as_str`](Self::as_str) 标识解析；未知值回退 Auto。
    pub fn from_str(s: &str) -> DecodeMode {
        match s {
            // 文本与编码
            "utf8" => DecodeMode::Utf8,
            "utf16le" => DecodeMode::Utf16Le,
            "utf16be" => DecodeMode::Utf16Be,
            "gb18030" => DecodeMode::Gb18030,
            "cp1251" => DecodeMode::Cp1251,
            "sjis" => DecodeMode::Sjis,
            "big5" => DecodeMode::Big5,
            "cp1252" => DecodeMode::Cp1252,

            // 整数
            "i8" => DecodeMode::I8,
            "i16" => DecodeMode::I16,
            "i32" => DecodeMode::I32,
            "i64" => DecodeMode::I64,
            "u8" => DecodeMode::U8,
            "u16" => DecodeMode::U16,
            "u32" => DecodeMode::U32,
            "u64" => DecodeMode::U64,

            // 浮点
            "f32" => DecodeMode::F32,
            "f64" => DecodeMode::F64,

            // 进制/二进制展示
            "hex" => DecodeMode::Hex,
            "dec" => DecodeMode::Dec,
            "bin" => DecodeMode::Binary,

            // 编码文本类
            "b64" => DecodeMode::Base64,
            "uuid" => DecodeMode::Uuid,
            "mac" => DecodeMode::Mac,
            "ip" => DecodeMode::Ip,
            "time" => DecodeMode::Time,
            "json" => DecodeMode::Json,
            "bson" => DecodeMode::Bson,
            "msgpack" => DecodeMode::Msgpack,
            "cbor" => DecodeMode::Cbor,
            "proto" => DecodeMode::Proto,
            "yaml" => DecodeMode::Yaml,
            "toml" => DecodeMode::Toml,
            "xml" => DecodeMode::Xml,

            // 未知值回退 Auto
            _ => DecodeMode::Auto,
        }
    }

    /// 指定类型时"类型"列的显示标签（风格与 [`guess`] 的猜测标签一致：
    /// 多字节整数/浮点带字节序后缀）；[`DecodeMode::Auto`] 返回 None，
    /// 由调用方按字节内容自动猜测。
    pub fn forced_label(self, endian: Endian) -> Option<String> {
        let t = crate::i18n::tr();
        let sfx = endian.suffix();

        Some(match self {
            // Auto 交给调用方自动猜测
            DecodeMode::Auto => return None,

            // 文本与编码
            DecodeMode::Utf8 => t.guess_utf8.to_string(),
            DecodeMode::Utf16Le => t.guess_utf16_le.to_string(),
            DecodeMode::Utf16Be => t.guess_utf16_be.to_string(),
            DecodeMode::Gb18030 => "GB18030".to_string(),
            DecodeMode::Cp1251 => "Windows-1251".to_string(),
            DecodeMode::Sjis => "SHIFT_JIS".to_string(),
            DecodeMode::Big5 => "Big5".to_string(),
            DecodeMode::Cp1252 => "Windows-1252".to_string(),

            // 单字节整数无字节序后缀
            DecodeMode::I8 => "i8".to_string(),
            DecodeMode::U8 => "u8".to_string(),

            // 多字节整数/浮点随字节序加后缀
            DecodeMode::I16 => format!("i16 {sfx}"),
            DecodeMode::U16 => format!("u16 {sfx}"),
            DecodeMode::I32 => format!("i32 {sfx}"),
            DecodeMode::U32 => format!("u32 {sfx}"),
            DecodeMode::I64 => format!("i64 {sfx}"),
            DecodeMode::U64 => format!("u64 {sfx}"),
            DecodeMode::F32 => format!("f32 {sfx}"),
            DecodeMode::F64 => format!("f64 {sfx}"),

            // 进制/二进制展示
            DecodeMode::Hex => "hex".to_string(),
            DecodeMode::Dec => "dec".to_string(),
            DecodeMode::Binary => "binary".to_string(),

            // 编码文本类
            DecodeMode::Base64 => "base64".to_string(),
            DecodeMode::Uuid => "uuid".to_string(),
            DecodeMode::Mac => "mac".to_string(),
            DecodeMode::Ip => "ip".to_string(),
            DecodeMode::Time => "time".to_string(),
            DecodeMode::Json => "JSON".to_string(),
            DecodeMode::Bson => "BSON".to_string(),
            DecodeMode::Msgpack => "MSGPACK".to_string(),
            DecodeMode::Cbor => "CBOR".to_string(),
            DecodeMode::Proto => "PROTO".to_string(),
            DecodeMode::Yaml => "YAML".to_string(),
            DecodeMode::Toml => "TOML".to_string(),
            DecodeMode::Xml => "XML".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 全部变体清单（验证分组覆盖性与 label 完备性）
    const ALL: [DecodeMode; 35] = [
        DecodeMode::Auto,
        DecodeMode::Utf8,
        DecodeMode::Utf16Le,
        DecodeMode::Utf16Be,
        DecodeMode::Gb18030,
        DecodeMode::Cp1251,
        DecodeMode::Sjis,
        DecodeMode::Big5,
        DecodeMode::Cp1252,
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
        DecodeMode::Mac,
        DecodeMode::Ip,
        DecodeMode::Time,
        DecodeMode::Json,
        DecodeMode::Bson,
        DecodeMode::Msgpack,
        DecodeMode::Cbor,
        DecodeMode::Proto,
        DecodeMode::Yaml,
        DecodeMode::Toml,
        DecodeMode::Xml,
    ];

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
        assert_eq!(ALL.len(), 35);
    }

    #[test]
    fn decode_mode_flat_all_groups_cover_all() {
        let flat = DecodeMode::flat_all();

        // 全量：Auto 置顶，分组拼接恰好覆盖其余变体
        assert_eq!(flat.len(), ALL.len());
        assert_eq!(flat[0], DecodeMode::Auto);

        // 分组并集（不含 Auto）恰好等于 ALL 去掉 Auto：长度相等 + 每个变体
        // 都在——若有重复则长度必然超出，两条合起来推出无重复
        let grouped: Vec<_> = DecodeMode::GROUPS.into_iter().flatten().collect();
        assert_eq!(grouped.len(), ALL.len() - 1);
        assert!(ALL
            .iter()
            .filter(|m| **m != DecodeMode::Auto)
            .all(|m| grouped.contains(&m)));
        assert!(grouped.iter().all(|m| **m != DecodeMode::Auto));
    }

    #[test]
    fn decode_mode_as_str_from_str_round_trip() {
        for m in ALL {
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
    fn forced_label_none_for_auto_and_suffixed_for_multibyte() {
        // Auto 不产生指定标签（调用方退回自动猜测）
        assert_eq!(DecodeMode::Auto.forced_label(Endian::Little), None);

        // 单字节类型无字节序后缀
        assert_eq!(
            DecodeMode::U8.forced_label(Endian::Little).as_deref(),
            Some("u8")
        );

        // 多字节整数随字节序加后缀
        assert_eq!(
            DecodeMode::U32.forced_label(Endian::Little).as_deref(),
            Some("u32 LE")
        );
        assert_eq!(
            DecodeMode::U32.forced_label(Endian::Big).as_deref(),
            Some("u32 BE")
        );

        // 全部非 Auto 模式都有标签
        for m in ALL {
            if m != DecodeMode::Auto {
                assert!(m.forced_label(Endian::Little).is_some());
            }
        }
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
