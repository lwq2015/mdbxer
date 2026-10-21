// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 按指定格式把字节解码为显示文本。

use super::{DecodeMode, Endian, guess, guess_key};

/// 空数据占位文案（随界面语言）。
fn empty_text() -> String {
    crate::i18n::tr().empty.to_string()
}

/// 补零标注（随界面语言）。
fn pad_note() -> &'static str {
    crate::i18n::tr().padded
}

/// 给数字字符串加千位分隔符（如 `1625981420` → `1,625,981,420`，
/// `1.62598142e20` → `1.62598142e20` 整数部分不变（太短）等）。
/// 受全局开关控制，关闭时原样返回。
pub(crate) fn with_sep(s: String) -> String {
    if !super::thousands_sep() {
        return s;
    }
    // 浮点数：只给整数部分加逗号，小数/指数后缀保留
    if let Some(pos) = s.find(['.', 'e', 'E']) {
        let int_part = &s[..pos];
        let suffix = &s[pos..];
        return format!("{}{}", sep_int(int_part), suffix);
    }
    // 纯整数（含负号）
    sep_int(&s)
}

/// 给纯整数部分（可带负号）加千位分隔符；非纯数字或太短则原样返回。
fn sep_int(s: &str) -> String {
    let (neg, digits) = match s.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", s),
    };

    if digits.len() < 5 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return s.to_string();
    }

    let mut out = String::with_capacity(s.len() + digits.len() / 3);
    out.push_str(neg);
    let rem = digits.len() % 3;
    for (i, c) in digits.char_indices() {
        if i > 0 && i % 3 == rem {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// 按 mode 解码 bytes，多字节整数按 endian 解释，
/// 结果最多 max_chars 个字符（超出截断并加省略号）。
///
/// 字节数少于类型宽度时按零扩展补齐（小端补尾部、大端补头部，数值等价），
/// 并在结果后标注补零说明；空数据显示空占位；超长取前 N 字节。
/// 占位文案随当前界面语言。附注（Auto 猜测的时间/地址/魔数解读）按 hints 开关。
pub fn decode(
    bytes: &[u8],
    mode: DecodeMode,
    endian: Endian,
    max_chars: usize,
    hints: &super::HintFlags,
) -> String {
    decode_impl(bytes, mode, endian, max_chars, hints, false)
}

/// Key 版解码：Auto 模式不做特殊格式猜测（JSON/BSON/PROTO 等只可能出现在
/// 数据载荷里，短 key 极易被凑巧的结构误判），仅文本/UTF-16/整数/时间等
/// 基础解释与 hex；手动指定的模式原样生效。
pub fn decode_key(
    bytes: &[u8],
    mode: DecodeMode,
    endian: Endian,
    max_chars: usize,
    hints: &super::HintFlags,
) -> String {
    decode_impl(bytes, mode, endian, max_chars, hints, true)
}

fn decode_impl(
    bytes: &[u8],
    mode: DecodeMode,
    endian: Endian,
    max_chars: usize,
    hints: &super::HintFlags,
    is_key: bool,
) -> String {
    // 整数分支：零扩展补齐；fmt 闭包把数值转显示文本（普通整数加千位分隔，
    // u32/u64 换成带时间戳附注的版本）
    macro_rules! int {
        ($ty:ty, $fmt:expr) => {{
            let n = std::mem::size_of::<$ty>();
            if bytes.is_empty() {
                empty_text()
            } else {
                let take = bytes.len().min(n);
                let mut arr = [0u8; { std::mem::size_of::<$ty>() }];
                match endian {
                    Endian::Little => arr[..take].copy_from_slice(&bytes[..take]),
                    Endian::Big => arr[n - take..].copy_from_slice(&bytes[..take]),
                }
                let v = match endian {
                    Endian::Little => <$ty>::from_le_bytes(arr),
                    Endian::Big => <$ty>::from_be_bytes(arr),
                };
                let s = ($fmt)(v);
                if bytes.len() < n {
                    format!("{}{}", s, pad_note())
                } else {
                    s
                }
            }
        }};
    }

    // 浮点分支：同样零扩展补齐
    macro_rules! float {
        ($ty:ty) => {{
            let n = std::mem::size_of::<$ty>();
            if bytes.is_empty() {
                empty_text()
            } else {
                let take = bytes.len().min(n);
                let mut arr = [0u8; { std::mem::size_of::<$ty>() }];
                match endian {
                    Endian::Little => arr[..take].copy_from_slice(&bytes[..take]),
                    Endian::Big => arr[n - take..].copy_from_slice(&bytes[..take]),
                }
                let v = match endian {
                    Endian::Little => <$ty>::from_le_bytes(arr),
                    Endian::Big => <$ty>::from_be_bytes(arr),
                };
                // Rust 浮点 Display 不用科学计数法：极小值（如补零后的 denormal）
                // 会展开成几百位 0.000…，超长时改用科学计数法
                let s = format!("{v}");
                let s = if s.len() > 24 { format!("{v:e}") } else { s };
                if bytes.len() < n {
                    format!("{}{}", with_sep(s), pad_note())
                } else {
                    with_sep(s)
                }
            }
        }};
    }

    let s = match mode {
        DecodeMode::Auto => {
            if is_key {
                guess_key(bytes, endian, hints).1
            } else {
                guess(bytes, endian, hints).1
            }
        }
        DecodeMode::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
        DecodeMode::Utf16Le => utf16_text(bytes, false),
        DecodeMode::Utf16Be => utf16_text(bytes, true),
        DecodeMode::Gb18030 => legacy_text(bytes, encoding_rs::GB18030),
        DecodeMode::Cp1251 => legacy_text(bytes, encoding_rs::WINDOWS_1251),
        DecodeMode::Sjis => legacy_text(bytes, encoding_rs::SHIFT_JIS),
        DecodeMode::Big5 => legacy_text(bytes, encoding_rs::BIG5),
        DecodeMode::Cp1252 => legacy_text(bytes, encoding_rs::WINDOWS_1252),
        DecodeMode::I8 => int!(i8, |v: i8| with_sep(v.to_string())),
        DecodeMode::U8 => int!(u8, |v: u8| with_sep(v.to_string())),
        DecodeMode::I16 => int!(i16, |v: i16| with_sep(v.to_string())),
        DecodeMode::I32 => int!(i32, |v: i32| with_sep(v.to_string())),
        DecodeMode::I64 => int!(i64, |v: i64| with_sep(v.to_string())),
        DecodeMode::U16 => int!(u16, |v: u16| with_sep(v.to_string())),
        DecodeMode::U32 => int!(u32, |v: u32| u32_text(v, hints.unix_ts)),
        DecodeMode::U64 => int!(u64, |v: u64| u64_text(v, hints.unix_ts, hints.filetime)),
        DecodeMode::F32 => float!(f32),
        DecodeMode::F64 => float!(f64),
        DecodeMode::Hex => hex_spaced(bytes),
        DecodeMode::Dec => dec_spaced(bytes),
        DecodeMode::Binary => binary_bits(bytes),
        DecodeMode::Base64 => base64_text(bytes),
        DecodeMode::Uuid => uuid_text(bytes),
        DecodeMode::Mac => mac_text(bytes),
        DecodeMode::Ip => ip_text(bytes),
        DecodeMode::Time => time_text(bytes, endian),
        DecodeMode::Json => json_text(bytes),
        DecodeMode::Bson => bson_text(bytes),
        DecodeMode::Msgpack => msgpack_text(bytes),
        DecodeMode::Cbor => cbor_text(bytes),
        // Protobuf：按全局类型池与当前 message 解码；未载入/失败回退 UTF-8 文本
        DecodeMode::Proto => crate::proto::decode_current(bytes)
            .unwrap_or_else(|| String::from_utf8_lossy(bytes).into_owned()),
        DecodeMode::Yaml => yaml_text(bytes),
        DecodeMode::Toml => toml_text(bytes),
        DecodeMode::Xml => xml_text(bytes),
    };
    truncate_chars(s, max_chars)
}

/// 空格分隔的大写十六进制（如 `2A 2B 2C`）。
pub fn hex_spaced(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn dec_spaced(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| b.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

/// 每字节显示为 8 位二进制，空格分隔。
fn binary_bits(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:08b}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn utf16_text(bytes: &[u8], big_endian: bool) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| {
            if big_endian {
                u16::from_be_bytes([c[0], c[1]])
            } else {
                u16::from_le_bytes([c[0], c[1]])
            }
        })
        .collect();
    String::from_utf16_lossy(&units)
}

/// 传统单/双字节编码文本（GB18030、Windows-1251 等）。
/// encoding_rs 对未定义字节输出 U+FFFD 替换符，与 UTF-8 lossy 行为一致。
fn legacy_text(bytes: &[u8], enc: &'static encoding_rs::Encoding) -> String {
    enc.decode(bytes).0.into_owned()
}

/// u32 显示文本；按开关落在 Unix 秒域时附本地日期（与 u64 同一套域判定，
/// u32 上限 4.29e9 与域 1e9..1e10 相交段即 2001~2106 年）。FILETIME 域
/// 从 110e15 起，远超 u32 上限，无需判定。
pub fn u32_text(v: u32, unix_ts: bool) -> String {
    let num = with_sep(v.to_string());
    let date = if unix_ts {
        unix_to_local(v as u64)
    } else {
        None
    };
    match date {
        Some(d) => format!("{num} → {d}"),
        None => num,
    }
}

/// u64 显示文本；按开关落在 Unix 时间戳（秒/毫秒）或 FILETIME 合理区间时
/// 附本地日期。毫秒级值保留 3 位小数（`.xxx`），秒级值只到秒。数字部分带千位分隔。
pub fn u64_text(v: u64, unix_ts: bool, filetime: bool) -> String {
    let num = with_sep(v.to_string());
    let date = if unix_ts { unix_to_local(v) } else { None }
        .or_else(|| if filetime { filetime_to_local(v) } else { None });
    match date {
        Some(date) => format!("{num} → {date}"),
        None => num,
    }
}

/// 识别秒（1e9..1e10）或毫秒（1e12..1e13）级 Unix 时间戳，格式化为本地时间；
/// 毫秒保留 3 位小数（`.000`），秒级只到秒。
fn unix_to_local(v: u64) -> Option<String> {
    if (1_000_000_000..10_000_000_000).contains(&v) {
        let dt = chrono::DateTime::from_timestamp(v as i64, 0)?;
        Some(
            dt.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S (%Z%:z)")
                .to_string(),
        )
    } else if (1_000_000_000_000..10_000_000_000_000).contains(&v) {
        // 毫秒：余数转成纳秒传入，格式串里 %.3f 显示 .xxx
        let dt =
            chrono::DateTime::from_timestamp((v / 1000) as i64, (v % 1000 * 1_000_000) as u32)?;
        Some(
            dt.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S%.3f (%Z%:z)")
                .to_string(),
        )
    } else {
        None
    }
}

/// FILETIME 合理域（Windows 1601 epoch、100ns 单位）：约 1949~2124 年。
/// 随机 u64 落入此域的概率约 0.17%，作为附加解读可忽略。
/// 与上面 Unix 秒/毫秒域完全不相交，同一值不会双重附注。
pub(crate) const FILETIME_MIN: u64 = 110_000_000_000_000_000;
pub(crate) const FILETIME_MAX: u64 = 140_000_000_000_000_000;

/// FILETIME → 本地时间文本（毫秒精度，与 Unix 毫秒时间戳同一格式）。
pub(crate) fn filetime_to_local(v: u64) -> Option<String> {
    if !(FILETIME_MIN..FILETIME_MAX).contains(&v) {
        return None;
    }

    // 100ns 单位 → (秒, 纳秒)，再减 1601→1970 的 11_644_473_600 秒
    let secs = (v / 10_000_000) as i64 - 11_644_473_600;
    let nanos = (v % 10_000_000) as u32 * 100;
    let dt = chrono::DateTime::from_timestamp(secs, nanos)?;
    Some(
        dt.with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M:%S%.3f (%Z%:z)")
            .to_string(),
    )
}

fn truncate_chars(s: String, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s;
    }
    let mut out: String = s.chars().take(max_chars).collect();
    out.push('…');
    out
}

/// RFC 4648 标准 Base64 编码（含填充）。
fn base64_text(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }

    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);

    // 每 3 字节编码为 4 个输出字符，末尾不足 3 字节用 '=' 填充
    for chunk in bytes.chunks(3) {
        let mut buf = [0u8; 3];
        for (i, &b) in chunk.iter().enumerate() {
            buf[i] = b;
        }
        out.push(TABLE[(buf[0] >> 2) as usize] as char);
        out.push(TABLE[(((buf[0] & 0x03) << 4) | (buf[1] >> 4)) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(((buf[1] & 0x0F) << 2) | (buf[2] >> 6)) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(buf[2] & 0x3F) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// UUID：仅 16 字节时格式化为 8-4-4-4-12 小写 hex；否则回退 hex_spaced。
fn uuid_text(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }

    if bytes.len() != 16 {
        let t = crate::i18n::tr();
        return format!("{} {}", hex_spaced(bytes), t.uuid_bad_len);
    }

    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15],
    )
}

/// MAC 地址（仅 6 字节）；其他长度回退 hex + 长度提示。
fn mac_text(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }

    if bytes.len() != 6 {
        let t = crate::i18n::tr();
        return format!("{} {}", hex_spaced(bytes), t.mac_bad_len);
    }

    format!(
        "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5]
    )
}

/// IP 地址（4 字节 IPv4 / 16 字节 IPv6）；其他长度回退 hex + 长度提示。
/// IPv6 用 std Display（RFC 5952 `::` 压缩、小写）。
fn ip_text(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }

    match bytes.len() {
        4 => format!("{}.{}.{}.{}", bytes[0], bytes[1], bytes[2], bytes[3]),
        16 => {
            let arr: [u8; 16] = bytes.try_into().unwrap_or_else(|_| unreachable!());
            std::net::Ipv6Addr::from(arr).to_string()
        }
        _ => {
            let t = crate::i18n::tr();
            format!("{} {}", hex_spaced(bytes), t.ip_bad_len)
        }
    }
}

/// 时间戳（4 字节按 Unix 秒 / 8 字节自动判 Unix 秒·毫秒·FILETIME）；
/// 其他长度回退 hex + 长度提示。u32 秒全域（1970~2106）都有效，直接转；
/// u64 走 [`u64_text`]（三域判定，域外只显示数字）。
fn time_text(bytes: &[u8], endian: Endian) -> String {
    if bytes.is_empty() {
        return String::new();
    }

    match bytes.len() {
        4 => {
            let arr: [u8; 4] = bytes.try_into().unwrap_or_else(|_| unreachable!());
            let v = match endian {
                Endian::Little => u32::from_le_bytes(arr),
                Endian::Big => u32::from_be_bytes(arr),
            };
            // u32 秒全域 1970~2106 均有效，from_timestamp 实际不会为 None
            let dt = chrono::DateTime::from_timestamp(v as i64, 0)
                .unwrap_or(chrono::DateTime::UNIX_EPOCH);
            format!(
                "{} → {}",
                with_sep(v.to_string()),
                dt.with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M:%S (%Z%:z)")
            )
        }
        8 => {
            let arr: [u8; 8] = bytes.try_into().unwrap_or_else(|_| unreachable!());
            let v = match endian {
                Endian::Little => u64::from_le_bytes(arr),
                Endian::Big => u64::from_be_bytes(arr),
            };
            // 手动 time 格式：用户明确要看时间，附注必开（不受 hint 开关影响）
            u64_text(v, true, true)
        }
        _ => {
            let t = crate::i18n::tr();
            format!("{} {}", hex_spaced(bytes), t.time_bad_len)
        }
    }
}

/// 回退文本：空数据 → 空串；否则 UTF-8 lossy（非法字节显示 U+FFFD）。
fn utf8_fallback(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        String::new()
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// 通用 pretty JSON 输出：parse 成功 → pretty；失败 → UTF-8 文本回退。
/// JSON/BSON/MSGPACK/CBOR 四种 pretty 解码模式共用同一套回退语义。
fn json_pretty_text(
    bytes: &[u8],
    parse: impl FnOnce(&[u8]) -> Option<serde_json::Value>,
) -> String {
    match parse(bytes) {
        Some(v) => serde_json::to_string_pretty(&v).unwrap_or_else(|_| utf8_fallback(bytes)),
        None => utf8_fallback(bytes),
    }
}

/// 通用单行紧凑 JSON：parse 成功 → 紧凑文本；失败 → None（表格 Auto 列预览用）。
fn json_compact(
    bytes: &[u8],
    parse: impl FnOnce(&[u8]) -> Option<serde_json::Value>,
) -> Option<String> {
    parse(bytes).and_then(|v| serde_json::to_string(&v).ok())
}

/// JSON 美化：合法 JSON → pretty；非法 → UTF-8 文本。
fn json_text(bytes: &[u8]) -> String {
    json_pretty_text(bytes, |b| serde_json::from_slice(b).ok())
}

/// BSON 转宽松扩展 JSON：合法文档 → pretty；非法 → UTF-8 文本。
/// 宽松（relaxed）扩展 JSON 下日期/整数等以自然形式显示，最接近源码可读形态。
pub(crate) fn bson_text(bytes: &[u8]) -> String {
    json_pretty_text(bytes, bson_json_value)
}

/// 紧凑版 BSON → 扩展 JSON（表格 Auto 单行预览用）。
pub(crate) fn bson_compact(bytes: &[u8]) -> Option<String> {
    json_compact(bytes, bson_json_value)
}

/// 把字节按 BSON 文档解析并转成 serde_json 值（宽松扩展 JSON）。
/// 非 BSON 字节返回 None；空字节同样按解析失败处理。
fn bson_json_value(bytes: &[u8]) -> Option<serde_json::Value> {
    if bytes.is_empty() {
        return None;
    }

    let doc = bson::Document::from_reader(bytes).ok()?;
    bson::Bson::Document(doc).into_relaxed_extjson().into()
}

// ── MessagePack ──

/// rmpv Value → serde_json Value。类型映射：binary/ext 转 base64 字符串
/// （与 proto3 JSON mapping 的 bytes 处理一致），非字符串键转 JSON 文本。
fn msgpack_to_json(v: rmpv::Value) -> serde_json::Value {
    use rmpv::Value as V;

    match v {
        V::Nil => serde_json::Value::Null,
        V::Boolean(b) => serde_json::Value::Bool(b),
        V::Integer(i) => match (i.as_i64(), i.as_u64()) {
            (Some(n), _) => serde_json::json!(n),
            (None, Some(n)) => serde_json::json!(n),
            // as_i64/as_u64 覆盖 Integer 全部取值，到这里理论不可达
            _ => serde_json::Value::Null,
        },
        V::F32(f) => serde_json::json!(f),
        V::F64(f) => serde_json::json!(f),
        V::String(s) => serde_json::json!(s.as_str().unwrap_or_default()),
        V::Binary(b) => serde_json::json!(base64_text(&b)),
        V::Array(a) => serde_json::Value::Array(a.into_iter().map(msgpack_to_json).collect()),
        V::Map(m) => {
            let mut obj = serde_json::Map::new();

            for (k, v) in m {
                let key = match k {
                    V::String(s) => s.as_str().unwrap_or_default().to_string(),
                    other => msgpack_to_json(other).to_string(),
                };
                obj.insert(key, msgpack_to_json(v));
            }

            serde_json::Value::Object(obj)
        }
        V::Ext(t, d) => serde_json::json!({ format!("_ext{t}"): base64_text(&d) }),
    }
}

/// MessagePack 解析为 serde_json Value：整体必须恰好解析为一个值并消耗完。
fn msgpack_json_value(bytes: &[u8]) -> Option<serde_json::Value> {
    if bytes.is_empty() {
        return None;
    }

    let mut cur: &[u8] = bytes;
    let v = rmpv::decode::read_value(&mut cur).ok()?;
    cur.is_empty().then(|| msgpack_to_json(v))
}

/// MessagePack → pretty JSON；非法回退 UTF-8 文本。
pub(crate) fn msgpack_text(bytes: &[u8]) -> String {
    json_pretty_text(bytes, msgpack_json_value)
}

/// MessagePack → 单行紧凑 JSON（表格 Auto 列用）；非法返回 None。
pub(crate) fn msgpack_compact(bytes: &[u8]) -> Option<String> {
    json_compact(bytes, msgpack_json_value)
}

// ── CBOR ──

/// ciborium Value → serde_json Value。类型映射与 MessagePack 一致：
/// bytes 转 base64，非字符串键转 JSON 文本，tag 包一层 `_tagN` 对象。
fn cbor_to_json(v: ciborium::value::Value) -> serde_json::Value {
    use ciborium::value::Value as V;

    // match 分支按枚举声明顺序；枚举为 #[non_exhaustive]，末尾需通配兜底
    match v {
        V::Integer(i) => {
            // serde_json Number 只支持 64 位；ciborium 的 Integer 内部为 i128
            // 但仅公开 64 位转换，仍溢出时从 Debug 输出（"Integer(x)"）剥壳保真
            match i64::try_from(i) {
                Ok(v) => serde_json::json!(v),
                Err(_) => match u64::try_from(i) {
                    Ok(v) => serde_json::json!(v),
                    Err(_) => {
                        let dbg = format!("{i:?}");
                        serde_json::json!(&dbg[8..dbg.len() - 1])
                    }
                },
            }
        }
        V::Bytes(b) => serde_json::json!(base64_text(&b)),
        V::Float(f) => serde_json::json!(f),
        V::Text(s) => serde_json::json!(s),
        V::Bool(b) => serde_json::Value::Bool(b),
        V::Null => serde_json::Value::Null,
        V::Tag(t, boxed) => serde_json::json!({ format!("_tag{t}"): cbor_to_json(*boxed) }),
        V::Array(a) => serde_json::Value::Array(a.into_iter().map(cbor_to_json).collect()),
        V::Map(m) => {
            let mut obj = serde_json::Map::new();

            for (k, v) in m {
                let key = match k {
                    V::Text(s) => s,
                    other => cbor_to_json(other).to_string(),
                };
                obj.insert(key, cbor_to_json(v));
            }

            serde_json::Value::Object(obj)
        }
        _ => serde_json::Value::Null,
    }
}

/// CBOR 解析为 serde_json Value：整体必须恰好解析为一个值并消耗完。
fn cbor_json_value(bytes: &[u8]) -> Option<serde_json::Value> {
    if bytes.is_empty() {
        return None;
    }

    let mut cur: &[u8] = bytes;
    let v: ciborium::value::Value = ciborium::de::from_reader(&mut cur).ok()?;
    cur.is_empty().then(|| cbor_to_json(v))
}

/// CBOR → pretty JSON；非法回退 UTF-8 文本。
pub(crate) fn cbor_text(bytes: &[u8]) -> String {
    json_pretty_text(bytes, cbor_json_value)
}

/// CBOR → 单行紧凑 JSON（表格 Auto 列用）；非法返回 None。
pub(crate) fn cbor_compact(bytes: &[u8]) -> Option<String> {
    json_compact(bytes, cbor_json_value)
}

// ── YAML / TOML / XML ──

/// YAML 规范化重排：合法 YAML → 归一化文本；非法 → UTF-8 文本。
/// 纯文本标量也是合法 YAML，会按 YAML 值重排（如 `123` → `123`）。
fn yaml_text(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }

    match std::str::from_utf8(bytes) {
        Ok(s) => match serde_yaml::from_str::<serde_yaml::Value>(s) {
            Ok(v) => serde_yaml::to_string(&v)
                .map(|t| t.trim_end().to_string())
                .unwrap_or_else(|_| s.to_string()),
            Err(_) => s.to_string(),
        },
        Err(_) => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// TOML 规范化重排：合法 TOML（顶层必须是表）→ pretty；非法 → UTF-8 文本。
fn toml_text(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }

    match std::str::from_utf8(bytes) {
        Ok(s) => match toml::from_str::<toml::Value>(s) {
            Ok(v) => toml::to_string_pretty(&v).unwrap_or_else(|_| s.to_string()),
            Err(_) => s.to_string(),
        },
        Err(_) => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// XML 缩进重排：合法 XML → 两空格缩进（单一文本子节点内联）；非法 → UTF-8 文本。
fn xml_text(bytes: &[u8]) -> String {
    xml_pretty(bytes).unwrap_or_else(|| utf8_fallback(bytes))
}

/// 解析 XML 并缩进重排。仅接受 UTF-8 输入（roxmltree 接口限制）；
/// 解析失败或无根元素返回 None。注释与处理指令节点保留；XML 声明被
/// roxmltree 作为文档信息消费，不产出节点、不保留。
fn xml_pretty(bytes: &[u8]) -> Option<String> {
    let s = std::str::from_utf8(bytes).ok()?;
    let doc = roxmltree::Document::parse(s).ok()?;

    let mut out = String::new();
    xml_write_children(doc.root(), 0, &mut out);
    let trimmed = out.trim_end();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// 递归输出 node 的子节点（node 自身不输出，便于从 Root 起步）。
/// 纯空白文本节点视为原文排版噪音跳过；文本统一 trim 后转义重排。
fn xml_write_children(node: roxmltree::Node, depth: usize, out: &mut String) {
    let indent = "  ".repeat(depth);

    for child in node.children() {
        match child.node_type() {
            roxmltree::NodeType::Element => {
                out.push_str(&indent);
                out.push('<');
                out.push_str(child.tag_name().name());
                for attr in child.attributes() {
                    out.push_str(&format!(
                        " {}=\"{}\"",
                        attr.name(),
                        xml_escape_attr(attr.value())
                    ));
                }

                let children: Vec<_> = child.children().collect();
                if children.len() == 1 && children[0].is_text() {
                    // 单一文本子节点内联：<x>hi</x>
                    let text = children[0].text().unwrap_or_default().trim();
                    if text.is_empty() {
                        out.push_str("/>\n");
                    } else {
                        out.push('>');
                        out.push_str(&xml_escape_text(text));
                        out.push_str(&format!("</{}>\n", child.tag_name().name()));
                    }
                } else if children.is_empty() {
                    out.push_str("/>\n");
                } else {
                    out.push_str(">\n");
                    xml_write_children(child, depth + 1, out);
                    out.push_str(&indent);
                    out.push_str(&format!("</{}>\n", child.tag_name().name()));
                }
            }
            roxmltree::NodeType::Text => {
                let text = child.text().unwrap_or_default().trim();
                if !text.is_empty() {
                    out.push_str(&indent);
                    out.push_str(&xml_escape_text(text));
                    out.push('\n');
                }
            }
            roxmltree::NodeType::Comment => {
                out.push_str(&indent);
                out.push_str(&format!("<!--{}-->\n", child.text().unwrap_or_default()));
            }
            roxmltree::NodeType::PI => {
                let data = child.text().unwrap_or_default();
                if data.is_empty() {
                    out.push_str(&format!("{indent}<?{}?>\n", child.tag_name().name()));
                } else {
                    out.push_str(&format!(
                        "{indent}<?{} {}?>\n",
                        child.tag_name().name(),
                        data
                    ));
                }
            }
            // Root 自身不作为子节点出现，仅为穷举兜底
            roxmltree::NodeType::Root => {}
        }
    }
}

/// 文本节点转义（还原 roxmltree 解码掉的实体）。
fn xml_escape_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// 属性值转义：文本转义之外再加引号。
fn xml_escape_attr(s: &str) -> String {
    xml_escape_text(s).replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 默认附注开关的 decode 快捷方式：多数测试不关心附注。
    #[allow(dead_code)]
    fn decode(bytes: &[u8], mode: DecodeMode, endian: Endian, max_chars: usize) -> String {
        super::decode(
            bytes,
            mode,
            endian,
            max_chars,
            &crate::fmt::HintFlags::default(),
        )
    }

    use crate::fmt::{DecodeMode, Endian, set_thousands_sep};

    fn setup() {
        // 测试统一打开千位分隔，避免全局状态污染断言
        set_thousands_sep(true);
    }

    #[test]
    fn hex_spaced_basic() {
        assert_eq!(hex_spaced(&[0x2A, 0x2B, 0x2C]), "2A 2B 2C");
        assert_eq!(hex_spaced(&[]), "");
        assert_eq!(hex_spaced(&[0x00, 0xFF]), "00 FF");
    }

    #[test]
    fn decode_utf8() {
        setup();
        let s = decode(b"hello", DecodeMode::Utf8, Endian::Little, 100);
        assert_eq!(s, "hello");
        // 非 UTF-8 字节走 lossy
        let s = decode(&[0xFF, 0xFE], DecodeMode::Utf8, Endian::Little, 100);
        assert!(s.contains('\u{FFFD}'));
    }

    #[test]
    fn decode_u8() {
        setup();
        assert_eq!(decode(&[255], DecodeMode::U8, Endian::Little, 100), "255");
        assert_eq!(decode(&[128], DecodeMode::I8, Endian::Little, 100), "-128");
    }

    #[test]
    fn decode_u16_le_be() {
        setup();
        let bytes = [0x34, 0x12]; // LE=0x1234(4660), BE=0x3412(13330)
        // 4 位数不加千位分隔
        assert_eq!(decode(&bytes, DecodeMode::U16, Endian::Little, 100), "4660");
        assert_eq!(decode(&bytes, DecodeMode::U16, Endian::Big, 100), "13,330");
    }

    #[test]
    fn decode_u32_le() {
        setup();
        let bytes = [0x78, 0x56, 0x34, 0x12]; // 0x12345678
        assert_eq!(
            decode(&bytes, DecodeMode::U32, Endian::Little, 100),
            "305,419,896"
        );
    }

    #[test]
    fn decode_u64_le() {
        setup();
        let bytes = [0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(decode(&bytes, DecodeMode::U64, Endian::Little, 100), "1");
    }

    #[test]
    fn decode_i32_negative() {
        setup();
        let bytes = (-1i32).to_le_bytes();
        assert_eq!(decode(&bytes, DecodeMode::I32, Endian::Little, 100), "-1");
    }

    #[test]
    fn decode_short_bytes_zero_extend() {
        setup();
        // 1 字节当 u32 解：小端补尾部零 → 数值不变，标注补零
        let s = decode(&[0x05], DecodeMode::U32, Endian::Little, 100);
        assert!(s.starts_with("5"));
        assert!(s.ends_with(crate::i18n::tr().padded));
        // 大端补头部零 → 同样数值 5
        let s = decode(&[0x05], DecodeMode::U32, Endian::Big, 100);
        assert!(s.starts_with("5"));
    }

    #[test]
    fn decode_empty() {
        setup();
        let t = crate::i18n::tr();
        use DecodeMode::*;

        // 整数/浮点模式：空数据返回通用空占位文案
        let numeric = [I8, U8, I16, U16, I32, U32, I64, U64, F32, F64];
        for mode in numeric {
            let s = decode(&[], mode, Endian::Little, 100);
            assert_eq!(s, t.empty, "mode={:?}", mode);
        }

        // 字符串/进制模式：空数据返回空串（无内容可展示）
        let textual = [
            Utf8, Utf16Le, Utf16Be, Gb18030, Cp1251, Sjis, Big5, Cp1252, Hex, Dec, Binary, Yaml,
            Toml, Xml,
        ];
        for mode in textual {
            let s = decode(&[], mode, Endian::Little, 100);
            assert_eq!(s, "", "mode={:?}", mode);
        }

        // Auto 模式：走 guess，返回 guess_empty_sym
        let s = decode(&[], Auto, Endian::Little, 100);
        assert_eq!(s, t.guess_empty_sym);
    }

    #[test]
    fn decode_msgpack_pretty() {
        // {"a":"hi"}
        let bytes = [0x81, 0xa1, b'a', 0xa2, b'h', b'i'];
        assert_eq!(
            decode(&bytes, DecodeMode::Msgpack, Endian::Little, 100),
            "{\n  \"a\": \"hi\"\n}"
        );
        // 非法数据回退 UTF-8 原文
        assert_eq!(
            decode(b"hi", DecodeMode::Msgpack, Endian::Little, 100),
            "hi"
        );
    }

    #[test]
    fn decode_cbor_pretty() {
        setup();
        // {1: 42}：整数键转文本，uint8 头 0x18 + 0x2A
        let bytes = [0xA1, 0x01, 0x18, 0x2A];
        assert_eq!(
            decode(&bytes, DecodeMode::Cbor, Endian::Little, 100),
            "{\n  \"1\": 42\n}"
        );
        // {"a": true}：text(1) 头 0x61
        let bytes = [0xA1, 0x61, b'a', 0xF5];
        assert_eq!(
            decode(&bytes, DecodeMode::Cbor, Endian::Little, 100),
            "{\n  \"a\": true\n}"
        );
        // 非法数据回退 UTF-8 原文
        assert_eq!(decode(b"hi", DecodeMode::Cbor, Endian::Little, 100), "hi");
    }

    #[test]
    fn decode_hex_mode() {
        setup();
        assert_eq!(
            decode(&[0xAB, 0xCD], DecodeMode::Hex, Endian::Little, 100),
            "AB CD"
        );
    }

    #[test]
    fn decode_gb18030() {
        setup();
        // "中文" GBK 字节（GB18030 超集兼容）
        let bytes = [0xD6, 0xD0, 0xCE, 0xC4];
        assert_eq!(
            decode(&bytes, DecodeMode::Gb18030, Endian::Little, 100),
            "中文"
        );
    }

    #[test]
    fn decode_cp1251() {
        setup();
        // "Привет" 的 Windows-1251 字节
        let bytes = [0xCF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2];
        assert_eq!(
            decode(&bytes, DecodeMode::Cp1251, Endian::Little, 100),
            "Привет"
        );
    }

    #[test]
    fn decode_dec_mode() {
        setup();
        assert_eq!(
            decode(&[10, 20], DecodeMode::Dec, Endian::Little, 100),
            "10 20"
        );
    }

    #[test]
    fn decode_binary_mode() {
        setup();
        assert_eq!(
            decode(&[0b1010_1010], DecodeMode::Binary, Endian::Little, 100),
            "10101010"
        );
    }

    #[test]
    fn decode_truncate_chars() {
        setup();
        let long = "a".repeat(50);
        let s = decode(long.as_bytes(), DecodeMode::Utf8, Endian::Little, 10);
        assert_eq!(s.chars().count(), 11); // 10 字符 + 省略号
        assert!(s.ends_with('…'));
    }

    #[test]
    fn with_sep_integer() {
        set_thousands_sep(true);
        assert_eq!(with_sep("1625981420".to_string()), "1,625,981,420");
        assert_eq!(with_sep("-1625981420".to_string()), "-1,625,981,420");
        // 少于 5 位不加
        assert_eq!(with_sep("1234".to_string()), "1234");
        // 关闭开关
        set_thousands_sep(false);
        assert_eq!(with_sep("1625981420".to_string()), "1625981420");
        set_thousands_sep(true);
    }

    #[test]
    fn with_sep_float_only_int_part() {
        set_thousands_sep(true);
        // 整数部分 < 5 位，原样
        assert_eq!(with_sep("3.14".to_string()), "3.14");
        // 指数后缀保留
        assert_eq!(with_sep("1.5e10".to_string()), "1.5e10");
    }

    #[test]
    fn u32_text_timestamp() {
        setup();

        // u32 秒级时间戳（ts_seconds_u32 样本值）：附本地日期
        let v = 1_759_000_000u32;
        let s = u32_text(v, true);
        assert!(s.starts_with("1,759,000,000 → "), "s={s}");
        assert!(s.contains("2025"), "s={s}");

        // 域外小值不附日期；开关关闭只显示数字
        assert_eq!(u32_text(42, true), "42");
        assert_eq!(u32_text(v, false), "1,759,000,000");

        // U32 排版同样附注（与 Auto 的 u32 分支同一来源）
        let d = decode(
            &v.to_le_bytes(),
            DecodeMode::U32,
            super::Endian::Little,
            100,
        );
        assert_eq!(d, s);
    }

    #[test]
    fn u64_text_timestamp() {
        setup();
        // 秒级时间戳：2024-01-01 00:00:00 UTC
        let v = 1_704_067_200u64;
        let s = u64_text(v, true, true);
        assert!(s.starts_with("1,704,067,200 → "));
        assert!(s.contains("2024"));
        // 非时间戳范围：只显示数字
        let s = u64_text(42, true, true);
        assert_eq!(s, "42");
        // 开关关闭：时间戳只显示数字
        assert_eq!(u64_text(v, false, false), "1,704,067,200");
    }

    #[test]
    fn u64_text_filetime() {
        setup();
        // 2026-01-01 00:00:00 UTC 的 FILETIME：与 Unix 秒/毫秒同一套附日期显示
        let v = (1_767_225_600u64 + 11_644_473_600) * 10_000_000;
        let s = u64_text(v, true, true);
        assert!(s.starts_with("134,116,992,000,000,000 → "), "s={s}");
        assert!(s.contains("2026"), "s={s}");

        // Unix 秒/毫秒域与 FILETIME 域不相交：普通时间戳不受影响
        assert!(u64_text(1_704_067_200, true, true).contains("2024"));
        // 域外大数只显示数字
        assert_eq!(
            u64_text(u64::MAX, true, true),
            with_sep(u64::MAX.to_string())
        );
    }

    #[test]
    fn decode_f32() {
        setup();
        let bytes = 1.5f32.to_le_bytes();
        let s = decode(&bytes, DecodeMode::F32, Endian::Little, 100);
        assert_eq!(s, "1.5");
    }

    #[test]
    fn decode_f64() {
        setup();
        let bytes = 2.5f64.to_le_bytes();
        let s = decode(&bytes, DecodeMode::F64, Endian::Little, 100);
        assert_eq!(s, "2.5");
    }

    #[test]
    fn decode_utf16_le() {
        setup();
        // "AB" 的 UTF-16LE
        let bytes = [0x41, 0x00, 0x42, 0x00];
        assert_eq!(
            decode(&bytes, DecodeMode::Utf16Le, Endian::Little, 100),
            "AB"
        );
    }

    #[test]
    fn decode_utf16_be() {
        setup();
        let bytes = [0x00, 0x41, 0x00, 0x42];
        assert_eq!(
            decode(&bytes, DecodeMode::Utf16Be, Endian::Little, 100),
            "AB"
        );
    }

    #[test]
    fn decode_base64() {
        setup();
        assert_eq!(
            decode(b"hello", DecodeMode::Base64, Endian::Little, 100),
            "aGVsbG8="
        );
        assert_eq!(
            decode(b"f", DecodeMode::Base64, Endian::Little, 100),
            "Zg=="
        );
        assert_eq!(
            decode(b"fo", DecodeMode::Base64, Endian::Little, 100),
            "Zm8="
        );
        assert_eq!(
            decode(b"foo", DecodeMode::Base64, Endian::Little, 100),
            "Zm9v"
        );

        // 空输入返回空串
        assert_eq!(decode(&[], DecodeMode::Base64, Endian::Little, 100), "");
    }

    #[test]
    fn decode_uuid_16_bytes() {
        setup();
        let bytes: [u8; 16] = [
            0x55, 0x0e, 0x84, 0x00, 0xe2, 0x9b, 0x41, 0xd4, 0xa7, 0x16, 0x44, 0x66, 0x55, 0x44,
            0x00, 0x00,
        ];
        assert_eq!(
            decode(&bytes, DecodeMode::Uuid, Endian::Little, 100),
            "550e8400-e29b-41d4-a716-446655440000"
        );
    }

    #[test]
    fn decode_uuid_wrong_len() {
        setup();
        let t = crate::i18n::tr();
        // 15 字节：回退 hex + 标注
        let bytes = [0xABu8; 15];
        let s = decode(&bytes, DecodeMode::Uuid, Endian::Little, 500);
        assert!(s.contains("AB AB"));
        assert!(s.contains(t.uuid_bad_len));
        // 17 字节同样回退
        let bytes = [0x01u8; 17];
        assert!(decode(&bytes, DecodeMode::Uuid, Endian::Little, 500).contains(t.uuid_bad_len));
        // 空输入为空串
        assert_eq!(decode(&[], DecodeMode::Uuid, Endian::Little, 100), "");
    }

    #[test]
    fn decode_mac_ip_time() {
        setup();
        let t = crate::i18n::tr();

        // MAC：6 字节大写冒号格式；不符长度回退 hex + 标注
        let mac = [0x00u8, 0x1A, 0x2B, 0x3C, 0x4D, 0x5E];
        assert_eq!(
            decode(&mac, DecodeMode::Mac, Endian::Little, 100),
            "00:1A:2B:3C:4D:5E"
        );
        let s = decode(&[0xABu8; 5], DecodeMode::Mac, Endian::Little, 100);
        assert!(s.contains(t.mac_bad_len));

        // IP：4 字节 IPv4 / 16 字节 IPv6（RFC 5952 压缩）；不符回退
        assert_eq!(
            decode(&[192, 168, 1, 1], DecodeMode::Ip, Endian::Little, 100),
            "192.168.1.1"
        );
        let v6 = [
            0x20u8, 0x01, 0x0D, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01,
        ];
        assert_eq!(
            decode(&v6, DecodeMode::Ip, Endian::Little, 100),
            "2001:db8::1"
        );
        let s = decode(&[0u8; 8], DecodeMode::Ip, Endian::Little, 100);
        assert!(s.contains(t.ip_bad_len));

        // 时间：4 字节 Unix 秒（LE）；8 字节自动判秒/毫秒/FILETIME；不符回退
        let ts = 1_704_067_200u32; // 2024-01-01 00:00:00 UTC
        let s = decode(&ts.to_le_bytes(), DecodeMode::Time, Endian::Little, 100);
        assert!(s.starts_with("1,704,067,200 → "), "s={s}");
        assert!(s.contains("2024"), "s={s}");

        // 8 字节 FILETIME 走三域判定（u64_text 同一套）
        let ft = (1_767_225_600u64 + 11_644_473_600) * 10_000_000;
        let s = decode(&ft.to_le_bytes(), DecodeMode::Time, Endian::Little, 100);
        assert!(s.contains("2026"), "s={s}");

        // 8 字节域外大数：只显示数字
        assert_eq!(
            decode(
                &u64::MAX.to_le_bytes(),
                DecodeMode::Time,
                Endian::Little,
                100
            ),
            with_sep(u64::MAX.to_string())
        );

        let s = decode(&[0u8; 6], DecodeMode::Time, Endian::Little, 100);
        assert!(s.contains(t.time_bad_len));

        // 空输入为空串
        assert_eq!(decode(&[], DecodeMode::Mac, Endian::Little, 100), "");
        assert_eq!(decode(&[], DecodeMode::Ip, Endian::Little, 100), "");
        assert_eq!(decode(&[], DecodeMode::Time, Endian::Little, 100), "");
    }

    #[test]
    fn decode_json_pretty() {
        setup();
        let s = decode(
            br#"{"a":1,"b":[2,3]}"#,
            DecodeMode::Json,
            Endian::Little,
            500,
        );
        assert!(s.contains('\n'), "pretty JSON 应多行: {s}");
        assert!(s.contains("\"a\": 1"));
        // 非法 JSON 回退 UTF-8 文本
        let s = decode(b"not json", DecodeMode::Json, Endian::Little, 100);
        assert_eq!(s, "not json");
        // 空输入为空串
        assert_eq!(decode(&[], DecodeMode::Json, Endian::Little, 100), "");
    }

    #[test]
    fn decode_bson_pretty() {
        setup();
        let doc = bson::to_vec(&bson::doc! { "a": "hi", "n": 3i32 }).unwrap();
        let s = decode(&doc, DecodeMode::Bson, Endian::Little, 500);
        // 宽松扩展 JSON pretty 多行，与 Json 模式同款缩进
        assert!(s.contains('\n'), "pretty BSON 应多行: {s}");
        assert!(s.contains("\"a\" : \"hi\"") || s.contains("\"a\": \"hi\""));
        assert!(s.contains("3"));
        // 非法 BSON 回退 UTF-8 文本
        let s = decode(b"not bson", DecodeMode::Bson, Endian::Little, 100);
        assert_eq!(s, "not bson");
        // 空输入为空串
        assert_eq!(decode(&[], DecodeMode::Bson, Endian::Little, 100), "");
    }

    #[test]
    fn decode_legacy_encodings() {
        setup();
        // "日本語" 的 Shift-JIS 字节（用 encoding_rs 生成，保证编码正确）
        let (b, _, _) = encoding_rs::SHIFT_JIS.encode("日本語");
        assert_eq!(decode(&b, DecodeMode::Sjis, Endian::Little, 100), "日本語");

        // "中文" 的 Big5 字节
        let (b, _, _) = encoding_rs::BIG5.encode("中文");
        assert_eq!(decode(&b, DecodeMode::Big5, Endian::Little, 100), "中文");

        // "Café" 的 Windows-1252 字节（0xE9 = é）
        assert_eq!(
            decode(
                &[0x43, 0x61, 0x66, 0xE9],
                DecodeMode::Cp1252,
                Endian::Little,
                100
            ),
            "Café"
        );

        // 无法映射的字节输出 U+FFFD，与 UTF-8 lossy 行为一致
        let s = decode(&[0x81], DecodeMode::Sjis, Endian::Little, 100);
        assert!(s.contains('\u{FFFD}'));
    }

    #[test]
    fn decode_yaml() {
        setup();
        // 合法 YAML → 归一化重排（列表展开为块序列）
        let s = decode(b"a: 1\nb: [x, y]", DecodeMode::Yaml, Endian::Little, 500);
        assert!(s.contains("a: 1"), "s={s}");
        assert!(s.contains("- x"), "s={s}");
        assert!(s.contains("- y"), "s={s}");

        // 缩进含 Tab 是非法 YAML → 回退 UTF-8 原文
        let raw = "a:\n\t- 1";
        let s = decode(raw.as_bytes(), DecodeMode::Yaml, Endian::Little, 100);
        assert_eq!(s, raw);
    }

    #[test]
    fn decode_toml() {
        setup();
        // 合法 TOML → 规范化重排
        let s = decode(b"b = \"x\"\na = 1", DecodeMode::Toml, Endian::Little, 500);
        assert!(s.contains("a = 1"), "s={s}");
        assert!(s.contains("b = \"x\""), "s={s}");

        // 非法 TOML 回退 UTF-8 原文
        assert_eq!(decode(b"a =", DecodeMode::Toml, Endian::Little, 100), "a =");
    }

    #[test]
    fn decode_xml() {
        setup();
        // 合法 XML → 两空格缩进重排，单一文本子节点内联
        let raw: &[u8] = br#"<root a="1"><x>hi</x><y/></root>"#;
        let s = decode(raw, DecodeMode::Xml, Endian::Little, 500);
        assert!(s.contains("<root a=\"1\">"), "s={s}");
        assert!(s.contains("  <x>hi</x>"), "s={s}");
        assert!(s.contains("  <y/>"), "s={s}");
        assert!(s.trim_end().ends_with("</root>"), "s={s}");

        // 注释保留（XML 声明被 roxmltree 作为文档信息消费，不产出节点）
        let raw: &[u8] = b"<a><!--note--></a>";
        let s = decode(raw, DecodeMode::Xml, Endian::Little, 500);
        assert!(s.contains("<!--note-->"), "s={s}");

        // 文本实体转义还原
        let raw: &[u8] = br#"<t>a &lt; b &amp; c</t>"#;
        let s = decode(raw, DecodeMode::Xml, Endian::Little, 500);
        assert!(s.contains("<t>a &lt; b &amp; c</t>"), "s={s}");

        // 非法 XML 回退 UTF-8 原文
        assert_eq!(
            decode(b"<a><b>", DecodeMode::Xml, Endian::Little, 100),
            "<a><b>"
        );
    }
}
