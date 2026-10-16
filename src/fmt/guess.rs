// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 字节内容的自动类型猜测。

use super::{Endian, value};

/// 猜测字节内容的类型，返回 (类型标签, 显示文本)。
/// 1 字节按 u8 解释（无字节序），多字节整数按 endian 解释。标签随当前界面语言。
pub fn guess(bytes: &[u8], endian: Endian) -> (String, String) {
    let t = crate::i18n::tr();

    // 空值：专有标签
    if bytes.is_empty() {
        return (
            t.guess_empty_label.to_string(),
            t.guess_empty_sym.to_string(),
        );
    }

    // 对象/数组形态的 JSON 优先识别为 JSON（在普通 UTF-8 之前，否则会被可打印
    // UTF-8 分支直接截走）。这里文本仍返回紧凑原文，pretty/着色交给详情层：
    // 表格 Value 是单行预览，若返回多行缩进会只剩一个 "{"。
    if is_structured_json(bytes) {
        // 已通过 serde 解析，必然是合法 UTF-8
        let s = std::str::from_utf8(bytes).unwrap_or_default();
        return ("JSON".to_string(), s.to_string());
    }

    // BSON 文档与 JSON 同级优先（都在普通 UTF-8 之前）。BSON 无魔数，靠
    // "前 4 字节长度自洽 + 0x00 终止 + 整体可解析"约束识别；文本仍返回紧凑
    // 扩展 JSON，pretty/着色交给详情层（与 JSON 同一套管线）。
    if is_bson(bytes) {
        if let Some(s) = value::bson_compact(bytes) {
            return ("BSON".to_string(), s);
        }
    }

    // 优先可打印 UTF-8 文本（允许 \t \n \r）
    if let Ok(s) = std::str::from_utf8(bytes) {
        if s.chars()
            .all(|c| !c.is_control() || matches!(c, '\t' | '\n' | '\r'))
        {
            return (t.guess_utf8.to_string(), s.to_string());
        }
    }

    // 再试 UTF-16：BOM 直接定字节序，无 BOM 按字节模式启发式
    if let Some(r) = try_utf16(bytes) {
        return r;
    }

    // 最后按 1/2/4/8 字节解释为整数，其余一律按二进制展示
    let sfx = endian.suffix();
    // match 保证长度后 try_into 必然成功
    match bytes.len() {
        1 => {
            // 单字节无字节序，标签不带 LE/BE；可打印 ASCII 已被上面的 UTF-8 分支截走
            ("u8".to_string(), value::with_sep(bytes[0].to_string()))
        }
        2 => {
            let arr: [u8; 2] = bytes.try_into().unwrap_or_else(|_| unreachable!());
            let v = match endian {
                Endian::Little => u16::from_le_bytes(arr),
                Endian::Big => u16::from_be_bytes(arr),
            };
            (format!("u16 {sfx}"), value::with_sep(v.to_string()))
        }
        4 => {
            let arr: [u8; 4] = bytes.try_into().unwrap_or_else(|_| unreachable!());
            let v = match endian {
                Endian::Little => u32::from_le_bytes(arr),
                Endian::Big => u32::from_be_bytes(arr),
            };
            (format!("u32 {sfx}"), value::with_sep(v.to_string()))
        }
        8 => {
            let arr: [u8; 8] = bytes.try_into().unwrap_or_else(|_| unreachable!());
            let v = match endian {
                Endian::Little => u64::from_le_bytes(arr),
                Endian::Big => u64::from_be_bytes(arr),
            };
            (format!("u64 {sfx}"), value::u64_text(v))
        }
        _ => (t.guess_binary.to_string(), value::hex_spaced(bytes)),
    }
}

/// 判断字节是否为"结构化"JSON（对象或数组），用于 Auto 自动识别与详情着色。
///
/// 仅当去掉前导 ASCII 空白后以 `{` 或 `[` 开头、且能被整体解析为 JSON 时成立。
/// 刻意排除 `123`、`true`、`"str"`、`null` 这类同样合法但属于标量的 JSON，
/// 否则普通数字串、单词都会被误判成 JSON。
pub fn is_structured_json(bytes: &[u8]) -> bool {
    let first = bytes
        .iter()
        .copied()
        .find(|c| !c.is_ascii_whitespace());
    matches!(first, Some(b'{') | Some(b'['))
        && serde_json::from_slice::<serde_json::Value>(bytes).is_ok()
}

/// 判断字节是否为完整 BSON 文档，用于 Auto 自动识别与详情着色。
///
/// BSON 无魔数，靠三层约束把误判压到可忽略：
/// 1. 至少 5 字节（最小 BSON 文档为 4 字节长度字段 + 1 字节终止符）；
/// 2. 末字节 0x00（文档终止符）；
/// 3. 前 4 字节小端 u32 恰好等于总长（BSON 文档首字段即自身总长）。
/// 满足后再交给 `bson::Document::from_reader` 做权威校验（类型字节、元素
/// 结构、CStr 字段名等）。随机二进制要同时命中这三层且解析成功，概率极低；
/// 普通整数/UTF-8 文本几乎不可能同时满足长度自洽与终止符。
pub fn is_bson(bytes: &[u8]) -> bool {
    bytes.len() >= 5
        && bytes[bytes.len() - 1] == 0
        && u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize == bytes.len()
        && bson::Document::from_reader(bytes).is_ok()
}

/// 尝试把字节识别为 UTF-16 文本。
///
/// - 开头 BOM（`FF FE` / `FE FF`）直接确定字节序；
/// - 无 BOM 时 LE/BE 各解一次：拒绝未配对代理、控制字符（`\t \n \r` 除外）与内嵌 NUL，
///   容忍单个结尾 NUL（Windows NUL 结尾串常见）；按可打印 ASCII 与字母占比打分选字节序。
///   无 ASCII 时要求至少 4 个字母字符，避免把 4 字节整数的两个 CJK 码位误判为文本。
fn try_utf16(bytes: &[u8]) -> Option<(String, String)> {
    let t = crate::i18n::tr();
    if bytes.len() < 4 || bytes.len() % 2 != 0 {
        return None;
    }

    // BOM 定序：入口已保证 len ≥ 4 且为偶，BOM 后必有余量且能切齐 u16 单元
    let bom_big = match &bytes[..2] {
        [0xFF, 0xFE] => Some(false),
        [0xFE, 0xFF] => Some(true),
        _ => None,
    };
    if let Some(big) = bom_big {
        let units = utf16_units(&bytes[2..], big);
        let text = utf16_validate(&units)?;
        let label = if big { t.guess_utf16_be } else { t.guess_utf16_le };
        return Some((label.to_string(), text));
    }

    // 无 BOM：两种字节序打分，优先 ASCII 含量高的；同分取 LE（Windows 数据占多数）
    let mut best: Option<(bool, u32, u32, String)> = None; // (big, ascii, alpha, text)
    for big in [false, true] {
        let units = utf16_units(bytes, big);
        let Some(text) = utf16_validate(&units) else {
            continue;
        };
        let chars: Vec<char> = text.chars().collect();
        if chars.len() < 2 {
            continue;
        }
        let mut ascii = 0u32;
        let mut alpha = 0u32;
        for c in &chars {
            if matches!(c, '\t' | '\n' | '\r' | ' '..='~') {
                ascii += 1;
            }
            if c.is_alphabetic() {
                alpha += 1;
            }
        }
        let n = chars.len() as u32;
        let ascii_ok = ascii >= 2 && ascii * 3 >= n * 2; // ASCII 占比 ≥ 2/3
        // 无 ASCII 的字母文本（如中文）。除字母数量外还要求文字体系一致：
        // 真实文本极少混用两套文字，而随机二进制（如 GBK 字节）按 UTF-16 误读
        // 常撞出 "生僻 CJK + 谚文音节" 这种跨体系组合——混入即不猜，宁显二进制。
        let cjk_ok = ascii + alpha >= n * 2 / 3 && alpha >= 4 && single_script(&chars);
        if ascii_ok || cjk_ok {
            let replace = best
                .as_ref()
                .is_some_and(|(_, a, al, _)| (ascii, alpha) > (*a, *al));
            if best.is_none() || replace {
                best = Some((big, ascii, alpha, text));
            }
        }
    }
    best.map(|(big, _, _, text)| {
        let label = if big { t.guess_utf16_be } else { t.guess_utf16_le };
        (label.to_string(), text)
    })
}

/// 按字节序把字节切成 u16 单元。
fn utf16_units(bytes: &[u8], big: bool) -> Vec<u16> {
    bytes
        .chunks_exact(2)
        .map(|c| {
            if big {
                u16::from_be_bytes([c[0], c[1]])
            } else {
                u16::from_le_bytes([c[0], c[1]])
            }
        })
        .collect()
}

/// 文字体系粗分（仅区分主要文字块，标点/数字/符号归 0 不参与判断）。
fn script_bucket(c: char) -> u8 {
    match c as u32 {
        0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0xF900..=0xFAFF => 1, // CJK 统一表意
        0xAC00..=0xD7AF | 0x1100..=0x11FF | 0x3130..=0x318F => 2, // 谚文
        0x0400..=0x052F => 3,                                     // 西里尔
        0x3040..=0x30FF => 4,                                     // 假名
        0x0600..=0x06FF | 0x0750..=0x077F => 5,                   // 阿拉伯
        0x00C0..=0x024F => 6,                                     // 拉丁扩展
        _ => 0,
    }
}

/// 字母字符是否同属一套文字体系（不同体系混排视为可疑）。
fn single_script(chars: &[char]) -> bool {
    let mut seen = 0u8;
    for c in chars {
        let b = script_bucket(*c);
        if b == 0 {
            continue;
        }
        if seen == 0 {
            seen = b;
        } else if seen != b {
            return false;
        }
    }
    true
}

/// 校验并解码 UTF-16 单元：拒绝未配对代理与控制字符（`\t \n \r` 除外）、内嵌 NUL；
/// 容忍并剥掉单个结尾 NUL。全部合法才返回文本。
fn utf16_validate(units: &[u16]) -> Option<String> {
    let mut n = units.len();
    if n > 0 && units[n - 1] == 0 {
        n -= 1;
    }

    let mut s = String::with_capacity(n);
    let mut i = 0;
    while i < n {
        let u = units[i];

        // 代理对解码
        let c = if (0xD800..=0xDBFF).contains(&u) {
            let lo = *units.get(i + 1)?;
            if !(0xDC00..=0xDFFF).contains(&lo) {
                return None;
            }
            i += 1;
            char::decode_utf16([u, lo])
                .next()?
                .ok()?
        } else if (0xDC00..=0xDFFF).contains(&u) || u == 0 {
            return None; // 低位代理 / 内嵌 NUL
        } else {
            char::from_u32(u as u32)?
        };

        // 控制字符拒绝
        if c.is_control() && !matches!(c, '\t' | '\n' | '\r') {
            return None;
        }

        // 拒绝私用区（PUA）：真实文本几乎不含 PUA，而 GBK/GB18030 等传统编码
        // 字节按 UTF-16 误读时经常落入 U+E000–U+F8FF（如 gbk_text 曾误判出
        // "譬需□□□□"），命中私用区即说明这更像二进制/其他编码，不猜 UTF-16。
        let cp = c as u32;
        if (0xE000..=0xF8FF).contains(&cp)
            || (0xF_0000..=0xF_FFFD).contains(&cp)
            || (0x10_0000..=0x10_FFFD).contains(&cp)
        {
            return None;
        }

        s.push(c);
        i += 1;
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fmt::Endian;

    #[test]
    fn guess_empty() {
        let (label, text) = guess(&[], Endian::Little);
        let t = crate::i18n::tr();
        assert_eq!(label, t.guess_empty_label);
        assert_eq!(text, t.guess_empty_sym);
    }

    #[test]
    fn guess_utf8_printable() {
        let (label, text) = guess(b"hello", Endian::Little);
        let t = crate::i18n::tr();
        assert_eq!(label, t.guess_utf8);
        assert_eq!(text, "hello");
    }

    #[test]
    fn guess_utf8_with_whitespace() {
        // \t \n \r 也算可打印
        let (label, _) = guess(b"a\tb\nc\r", Endian::Little);
        let t = crate::i18n::tr();
        assert_eq!(label, t.guess_utf8);
    }

    #[test]
    fn guess_utf8_control_rejected() {
        // 含 \0 控制字符，不走 UTF-8
        let (label, _) = guess(b"a\x00b", Endian::Little);
        let t = crate::i18n::tr();
        assert_ne!(label, t.guess_utf8);
    }

    #[test]
    fn guess_structured_json_object_and_array() {
        // 对象/数组形态 JSON：标签识别为 JSON，但文本保持紧凑原文（不 pretty）
        let raw = br#"{"a":1,"b":[2,3]}"#;
        let (label, text) = guess(raw, Endian::Little);
        assert_eq!(label, "JSON");
        assert_eq!(text.as_bytes(), raw.as_slice());
        assert!(!text.contains('\n'), "Auto 猜测不应返回多行 pretty");

        let (label2, _) = guess(b"[1,2,3]", Endian::Little);
        assert_eq!(label2, "JSON");

        // 允许前导空白
        let (label3, _) = guess(b"  {\"x\": 1}", Endian::Little);
        assert_eq!(label3, "JSON");

        // 中文键值
        let (label4, _) = guess(" {\"名\":\"值\"}".as_bytes(), Endian::Little);
        assert_eq!(label4, "JSON");
    }

    #[test]
    fn guess_json_scalars_are_not_special() {
        // 合法 JSON 标量不应被识别为 JSON：字符串/非整数字节串走 UTF-8
        let t = crate::i18n::tr();
        for scalar in [r#""abc""#, "123"] {
            let (label, _) = guess(scalar.as_bytes(), Endian::Little);
            assert_eq!(label, t.guess_utf8, "标量 {scalar} 不应判为 JSON");
        }
        // true/null 是 4 字节会落到 u32 分支，但同样绝不应是 JSON 标签
        for scalar in ["true", "null"] {
            let (label, _) = guess(scalar.as_bytes(), Endian::Little);
            assert_ne!(label, "JSON");
        }
    }

    #[test]
    fn guess_invalid_or_truncated_json_falls_back_to_utf8() {
        let t = crate::i18n::tr();
        // 以 { 开头但不可解析：仍是普通可打印 UTF-8
        let (label, _) = guess(b"{not json", Endian::Little);
        assert_eq!(label, t.guess_utf8);
        // 截断的对象：解析失败，回退 UTF-8
        let (label, _) = guess(br#"{"a": "#, Endian::Little);
        assert_eq!(label, t.guess_utf8);
    }

    #[test]
    fn structured_json_predicate() {
        assert!(is_structured_json(br#"{"a":1}"#));
        assert!(is_structured_json(b" [1, 2] "));
        assert!(!is_structured_json(b"123"));
        assert!(!is_structured_json(b"true"));
        assert!(!is_structured_json(br#""str""#));
        assert!(!is_structured_json(b"null"));
        assert!(!is_structured_json(b"{broken"));
        assert!(!is_structured_json(b""));
    }

    #[test]
    fn guess_bson_document() {
        // {"a": "hi"} 的 BSON：Auto 识别为 BSON，文本为紧凑扩展 JSON 单行
        let doc = bson::to_vec(&bson::doc! { "a": "hi" }).unwrap();
        assert!(is_bson(&doc));
        let (label, text) = guess(&doc, Endian::Little);
        assert_eq!(label, "BSON");
        assert_eq!(text, r#"{"a":"hi"}"#);
        assert!(!text.contains('\n'), "Auto 猜测 BSON 不应返回多行");
    }

    #[test]
    fn guess_bson_rich_types() {
        // 混合类型（整数/浮点/嵌套文档）：标签 BSON、文本可再次被 serde_json 解析
        let doc = bson::to_vec(&bson::doc! {
            "n": 42i32,
            "f": 1.5f64,
            "sub": { "ok": true },
        })
        .unwrap();
        let (label, text) = guess(&doc, Endian::Little);
        assert_eq!(label, "BSON");
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["n"], 42);
        assert_eq!(v["f"], 1.5);
        assert_eq!(v["sub"]["ok"], true);
    }

    #[test]
    fn guess_bson_empty_document() {
        // 最小 BSON：空文档 05 00 00 00 00
        let (label, _) = guess(&[0x05, 0x00, 0x00, 0x00, 0x00], Endian::Little);
        assert_eq!(label, "BSON");
    }

    #[test]
    fn bson_predicate_rejects_lookalikes() {
        // 长度字段与总长一致的 5 字节 \x05\x00\x00\x00\x00 是合法空文档
        assert!(is_bson(&[0x05, 0x00, 0x00, 0x00, 0x00]));
        // 长度自洽但末字节非 0 / 解析失败 / 长度不符：均不识别
        assert!(!is_bson(&[0x05, 0x00, 0x00, 0x00, 0xFF]));
        assert!(!is_bson(&[0x09, 0x00, 0x00, 0x00, 0x00]));
        assert!(!is_bson(&[0x00]));
        assert!(!is_bson(&[]));
        // 普通整数/文本/JSON 不被判为 BSON
        let int_le = [0x78u8, 0x56, 0x34, 0x12];
        assert!(!is_bson(&int_le));
        assert!(!is_bson(b"hello"));
        assert!(!is_bson(br#"{"a":1}"#));
    }

    #[test]
    fn guess_u16_le() {
        let bytes = [0x34, 0x12]; // LE 0x1234 = 4660
        let (label, text) = guess(&bytes, Endian::Little);
        assert!(label.contains("u16"));
        assert!(label.contains("LE"));
        // 4660 是 4 位数，不加千位分隔
        assert_eq!(text, "4660");
    }

    #[test]
    fn guess_u32_le() {
        let bytes = [0x78, 0x56, 0x34, 0x12]; // LE 0x12345678
        let (label, text) = guess(&bytes, Endian::Little);
        assert!(label.contains("u32"));
        assert!(label.contains("LE"));
        assert_eq!(text, "305,419,896");
    }

    #[test]
    fn guess_u64_be() {
        let bytes = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x2A]; // BE 42
        let (label, text) = guess(&bytes, Endian::Big);
        assert!(label.contains("u64"));
        assert!(label.contains("BE"));
        assert_eq!(text, "42");
    }

    #[test]
    fn guess_binary_for_other_lengths() {
        let bytes = [0xAB, 0xCD, 0xEF]; // 3 字节：非文本、非 2/4/8
        let (label, text) = guess(&bytes, Endian::Little);
        let t = crate::i18n::tr();
        assert_eq!(label, t.guess_binary);
        assert_eq!(text, "AB CD EF");
    }

    #[test]
    fn guess_u8_single_byte() {
        // 非法 UTF-8 的单字节按 u8 数值显示（0x80 = 128）
        let (label, text) = guess(&[0x80], Endian::Little);
        assert_eq!(label, "u8");
        assert_eq!(text, "128");
    }

    #[test]
    fn guess_single_printable_byte_is_utf8_not_u8() {
        // 可打印单字节优先走 UTF-8 文本，不落 u8 分支
        let (label, text) = guess(b"A", Endian::Little);
        let t = crate::i18n::tr();
        assert_eq!(label, t.guess_utf8);
        assert_eq!(text, "A");
    }

    #[test]
    fn guess_utf16le_bom() {
        // BOM + "Ab"（LE）
        let bytes = [0xFF, 0xFE, 0x41, 0x00, 0x62, 0x00];
        let (label, text) = guess(&bytes, Endian::Little);
        let t = crate::i18n::tr();
        assert_eq!(label, t.guess_utf16_le);
        assert_eq!(text, "Ab");
    }

    #[test]
    fn guess_utf16be_bom() {
        let bytes = [0xFE, 0xFF, 0x00, 0x41, 0x00, 0x62];
        let (label, text) = guess(&bytes, Endian::Little);
        let t = crate::i18n::tr();
        assert_eq!(label, t.guess_utf16_be);
        assert_eq!(text, "Ab");
    }

    #[test]
    fn guess_utf16le_no_bom() {
        // 无 BOM 的 "hello"（LE），典型 Windows 字符串
        let bytes = b"h\x00e\x00l\x00l\x00o\x00";
        let (label, text) = guess(bytes, Endian::Little);
        let t = crate::i18n::tr();
        assert_eq!(label, t.guess_utf16_le);
        assert_eq!(text, "hello");
    }

    #[test]
    fn guess_utf16le_trailing_nul() {
        let bytes = b"h\x00i\x00\x00\x00"; // NUL 结尾
        let (label, text) = guess(bytes, Endian::Little);
        let t = crate::i18n::tr();
        assert_eq!(label, t.guess_utf16_le);
        assert_eq!(text, "hi");
    }

    #[test]
    fn guess_utf16_cjk_no_bom() {
        // "中文测试"（LE）无 BOM，4 个字母字符门槛
        let s: Vec<u8> = "中文测试".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        let (label, text) = guess(&s, Endian::Little);
        let t = crate::i18n::tr();
        assert_eq!(label, t.guess_utf16_le);
        assert_eq!(text, "中文测试");
    }

    #[test]
    fn guess_utf16_surrogate_pair() {
        // "😀" U+1F600，代理对 D83D DE00（LE）
        let bytes = [0xFF, 0xFE, 0x3D, 0xD8, 0x00, 0xDE];
        let (label, text) = guess(&bytes, Endian::Little);
        let t = crate::i18n::tr();
        assert_eq!(label, t.guess_utf16_le);
        assert_eq!(text, "😀");
    }

    #[test]
    fn no_guess_utf16_for_interior_nul() {
        // 内嵌 NUL：更像整数/二进制，不猜 UTF-16
        let bytes = [0x41, 0x00, 0x00, 0x00, 0x42, 0x00]; // A NUL NUL B
        let (label, _) = guess(&bytes, Endian::Little);
        let t = crate::i18n::tr();
        assert_ne!(label, t.guess_utf16_le);
        assert_ne!(label, t.guess_utf16_be);
    }

    #[test]
    fn no_guess_utf16_for_u32_integer() {
        // 常见 u32（高两字节为 0）不被误判
        let bytes = [0x78, 0x56, 0x34, 0x12];
        let (label, _) = guess(&bytes, Endian::Little);
        let t = crate::i18n::tr();
        assert_ne!(label, t.guess_utf16_le);
        assert_ne!(label, t.guess_utf16_be);
        assert!(label.contains("u32"));
    }

    #[test]
    fn no_guess_utf16_two_cjk_units() {
        // 两个 CJK 码位 = 4 字节，可能是 u32 整数，无 BOM 不猜文本
        let bytes = [0x78, 0x56, 0x34, 0x12];
        let s: Vec<u8> = "中文".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        // 同样的字节模式不应被判为 UTF-16（4 字母门槛保护）
        debug_assert_eq!(s.len(), 4);
        let (label, _) = guess(&s, Endian::Little);
        let t = crate::i18n::tr();
        assert_ne!(label, t.guess_utf16_le);
        let _ = bytes;
    }

    #[test]
    fn no_guess_utf16_for_gbk_bytes() {
        // "GBK 简体中文" 的 GBK 字节按 UTF-16 LE 误读会得到 4 个字母字符
        // （CJK/西里尔/谚文）混 2 个私用区字符，曾误判为 UTF-16 显示乱码。
        // 私用区拒绝规则生效后应回退为二进制。
        let gbk = encoding_rs::GBK.encode("GBK 简体中文").0.into_owned();
        let (label, _) = guess(&gbk, Endian::Little);
        let t = crate::i18n::tr();
        assert_ne!(label, t.guess_utf16_le);
        assert_ne!(label, t.guess_utf16_be);
    }
}
