// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 详情卡片语法着色：按显示文本的词法切 token 逐段着前景色。
//!
//! - 分词器：JSON 类（JSON/BSON/MSGPACK/CBOR/PROTO pretty）共用一套；
//!   YAML/TOML/XML 各有专用分词器，同色盘渲染。
//! - 搜索命中的琥珀色背景与语法色正交（TextFormat.color 与 background
//!   是两个字段），可叠加。
//! - 仅在对应解码模式且内容确实合法时启用，非法内容回退单色。

use crate::fmt::{self, DecodeMode};

/// 语法高亮使用的词法单元，记录其在原始 UTF-8 文本中的字节范围和语法类别。
///
/// `start..end` 采用半开区间，可直接用于字符串切片；分词及着色逻辑共用此范围，
/// 因而不会因多字节字符导致高亮位置偏移。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct JsonTok {
    /// token 在输入文本中的起始字节偏移（包含）。
    start: usize,
    /// token 在输入文本中的结束字节偏移（不包含）。
    end: usize,
    /// token 的语法类别，用于选择对应的主题颜色。
    kind: TokKind,
}

/// Display-coordinate ranges for search matches and the currently selected match.
#[derive(Default)]
pub(crate) struct SearchHighlights {
    pub(crate) ranges: Vec<(usize, usize)>,
    pub(crate) active: Option<(usize, usize)>,
}

/// 跨 JSON、YAML、TOML、XML 等格式复用的语法高亮类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum TokKind {
    /// 键（JSON 的 "k":、YAML/TOML 的 k: / k =、XML 标签名）
    Key,
    /// 字符串值（含 XML 属性值）
    Str,
    /// 数值（含 XML 属性名）
    Number,
    /// true / false / null（YAML 加 ~）
    Lit,
    /// 结构符号 { } [ ] : , < > = - 及节头
    Punct,
    /// 注释（YAML/TOML 的 #...，XML 的 <!--...--> 与处理指令）
    Comment,
}

impl TokKind {
    /// 对应 [json_palette] 数组的下标。
    fn palette_index(self) -> usize {
        match self {
            TokKind::Key => 0,
            TokKind::Str => 1,
            TokKind::Number => 2,
            TokKind::Lit => 3,
            TokKind::Punct => 4,
            TokKind::Comment => 5,
        }
    }
}

/// 判断该段文本是否启用语法着色；Some 返回 Auto 下 pretty 重解码应
/// 使用的格式（Json/Bson/Cbor/Msgpack/Xml 等）。
/// - 手动切到 Json：该段是任意合法 JSON（含标量）即着色；
/// - 手动 Bson：该段是完整 BSON 文档即着色（显示为 JSON pretty，复用 JSON 语法）；
/// - 手动 Yaml/Toml/Xml：内容能被对应解析器整体解析（与解码显示同口径）才着色；
/// - 手动 Proto：类型池按当前 message 解码成功（输出 proto3 JSON pretty）才着色，
///   未载入/未选类型/非法字节时显示 UTF-8 原文，不着色；
/// - Auto：完整 BSON、对象/数组形态的结构化 JSON、MSGPACK/CBOR/XML 时返回
///   对应格式，排除 `123`/`true`/`"s"`/`null` 等会被当普通文本的标量。
///
/// 大字段分段时只有能独立解析的段（通常是首页完整小值）着色；跨页截断的
/// 部分 JSON/BSON 解析失败，回退单色。
pub(crate) fn syntax_mode(mode: DecodeMode, window: &[u8]) -> Option<DecodeMode> {
    match mode {
        DecodeMode::Json => serde_json::from_slice::<serde_json::Value>(window)
            .is_ok()
            .then_some(DecodeMode::Json),
        DecodeMode::Bson => fmt::is_bson(window).then_some(DecodeMode::Bson),
        DecodeMode::Msgpack => fmt::is_msgpack(window).then_some(DecodeMode::Msgpack),
        DecodeMode::Cbor => fmt::is_cbor(window).then_some(DecodeMode::Cbor),
        DecodeMode::Yaml => serde_yaml::from_slice::<serde_yaml::Value>(window)
            .is_ok()
            .then_some(DecodeMode::Yaml),
        DecodeMode::Toml => std::str::from_utf8(window)
            .ok()
            .and_then(|s| toml::from_str::<toml::Value>(s).ok())
            .is_some()
            .then_some(DecodeMode::Toml),
        DecodeMode::Xml => fmt::is_xml(window).then_some(DecodeMode::Xml),
        DecodeMode::Proto => crate::proto::decode_current(window)
            .is_some()
            .then_some(DecodeMode::Proto),
        DecodeMode::Auto => {
            if fmt::is_bson(window) {
                Some(DecodeMode::Bson)
            } else if fmt::is_structured_json(window) {
                Some(DecodeMode::Json)
            } else if fmt::is_msgpack(window) {
                Some(DecodeMode::Msgpack)
            } else if fmt::is_cbor(window) {
                Some(DecodeMode::Cbor)
            } else if fmt::is_xml(window) {
                Some(DecodeMode::Xml)
            } else if fmt::is_yaml(window) {
                // 保守：能解析为映射/序列且形态像 YAML 才判，避免普通文本误判
                Some(DecodeMode::Yaml)
            } else if fmt::is_toml(window) {
                // 保守：顶层为表/数组且含 `=` 或 `[` 才判，避免裸标量误判
                Some(DecodeMode::Toml)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// 按着色格式分派词法分词器：JSON 类共用 json_tokens，其余用各自专用分词器。
pub(crate) fn syntax_tokens(mode: DecodeMode, s: &str) -> Vec<JsonTok> {
    match mode {
        DecodeMode::Yaml => yaml_tokens(s),
        DecodeMode::Toml => toml_tokens(s),
        DecodeMode::Xml => xml_tokens(s),
        _ => json_tokens(s),
    }
}

/// 对 pretty JSON 文本做轻量词法切分。
/// 不做语义校验（合法性已由调用方用 serde_json 判定），只按字符扫描。
fn json_tokens(s: &str) -> Vec<JsonTok> {
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let n = chars.len();
    let mut i = 0usize;
    let mut out = Vec::new();

    while i < n {
        let (start, c) = chars[i];
        match c {
            '"' => {
                let mut j = i + 1;
                while j < n {
                    let (_, cc) = chars[j];
                    if cc == '\\' {
                        j += 2; // 跳过转义对（如 \" \\ \n）
                        continue;
                    }
                    if cc == '"' {
                        j += 1; // 含结束引号
                        break;
                    }
                    j += 1;
                }
                let end = chars.get(j).map(|(p, _)| *p).unwrap_or(s.len());

                // 判断是否为键：其后第一个非空白字符是 ':'
                let mut k = j;
                while k < n && chars[k].1.is_whitespace() {
                    k += 1;
                }
                let kind = if k < n && chars[k].1 == ':' {
                    TokKind::Key
                } else {
                    TokKind::Str
                };
                out.push(JsonTok { start, end, kind });
                i = j;
            }

            '{' | '}' | '[' | ']' | ':' | ',' => {
                let end = chars.get(i + 1).map(|(p, _)| *p).unwrap_or(s.len());
                out.push(JsonTok {
                    start,
                    end,
                    kind: TokKind::Punct,
                });
                i += 1;
            }

            c if c.is_ascii_alphanumeric() || c == '-' => {
                let mut j = i + 1;
                while j < n {
                    let cc = chars[j].1;
                    if cc.is_ascii_alphanumeric() || matches!(cc, '+' | '-' | '.') {
                        j += 1;
                    } else {
                        break;
                    }
                }
                let end = chars.get(j).map(|(p, _)| *p).unwrap_or(s.len());
                let word = &s[start..end];
                let kind = match word {
                    "true" | "false" | "null" => TokKind::Lit,
                    _ => TokKind::Number,
                };
                out.push(JsonTok { start, end, kind });
                i = j;
            }

            // 空白 / 换行 / 缩进等：无需着色，跳过
            _ => i += 1,
        }
    }
    out
}

/// 对 YAML 文本做轻量词法切分（按行处理，跟踪块标量把内容行整行跳过）。
/// 不做语义校验（合法性已由调用方用 serde_yaml 判定），只按字符扫描；
/// 锚点/引用/标签等罕见语法不着色。
fn yaml_tokens(s: &str) -> Vec<JsonTok> {
    let mut out = Vec::new();

    // 块标量状态：Some(父行缩进) 时，缩进更深的行都是内容行，整行不着色
    let mut block_indent: Option<usize> = None;

    let mut pos = 0usize;
    for line in s.split_inclusive('\n') {
        let line_start = pos;
        pos += line.len();

        let body = line.strip_suffix('\n').unwrap_or(line);
        let trimmed = body.trim_start();
        let indent = body.len() - trimmed.len();
        let chars: Vec<(usize, char)> = body.char_indices().collect();
        let n = chars.len();

        // 块标量内容行：跳过着色；缩进回到父级及以下时块结束
        if let Some(parent) = block_indent {
            if indent > parent {
                continue;
            }
            block_indent = None;
        }

        // 整行注释
        if trimmed.starts_with('#') {
            out.push(JsonTok {
                start: line_start + indent,
                end: line_start + body.len(),
                kind: TokKind::Comment,
            });
            continue;
        }

        // 词后第一个非空白是 ':'（其后是空白或行尾）→ 该词是键
        let is_ws_or_end = |k: usize| k >= n || chars[k].1.is_whitespace();

        let mut i = indent;
        while i < n {
            let (start, c) = chars[i];
            match c {
                // 注释：行首或前面是空白
                '#' if i == indent || chars[i - 1].1.is_whitespace() => {
                    out.push(JsonTok {
                        start: line_start + start,
                        end: line_start + body.len(),
                        kind: TokKind::Comment,
                    });
                    break;
                }

                // 序列项 '- '（行首或前面是空白，其后是空白或行尾）
                '-' if is_ws_or_end(i + 1) && (i == indent || chars[i - 1].1.is_whitespace()) => {
                    out.push(JsonTok {
                        start: line_start + start,
                        end: line_start + start + 1,
                        kind: TokKind::Punct,
                    });
                    i += 1;
                }

                // 引号字符串（双引号含转义）；其后跟 ':' 则是键
                '"' | '\'' => {
                    let mut j = i + 1;
                    while j < n {
                        if c == '"' && chars[j].1 == '\\' {
                            j += 2;
                            continue;
                        }
                        if chars[j].1 == c {
                            j += 1;
                            break;
                        }
                        j += 1;
                    }
                    let end = chars.get(j).map(|(p, _)| *p).unwrap_or(body.len());

                    let mut k = j;
                    while k < n && chars[k].1.is_whitespace() {
                        k += 1;
                    }
                    let kind = if k < n && chars[k].1 == ':' && is_ws_or_end(k + 1) {
                        TokKind::Key
                    } else {
                        TokKind::Str
                    };
                    out.push(JsonTok {
                        start: line_start + start,
                        end: line_start + end,
                        kind,
                    });
                    i = j;
                }

                // 裸词：键名 / true|false|null|~ / 数字 / 块标量引导符
                c if !c.is_whitespace()
                    && !matches!(c, ',' | '[' | ']' | '{' | '}' | ':' | '#') =>
                {
                    let mut j = i + 1;
                    while j < n {
                        let cc = chars[j].1;
                        if cc.is_whitespace()
                            || matches!(cc, ',' | '[' | ']' | '{' | '}' | ':' | '#')
                        {
                            break;
                        }
                        j += 1;
                    }
                    let end = chars.get(j).map(|(p, _)| *p).unwrap_or(body.len());
                    let word = &body[start..end];

                    let mut k = j;
                    while k < n && chars[k].1.is_whitespace() {
                        k += 1;
                    }
                    let colon = k < n && chars[k].1 == ':' && is_ws_or_end(k + 1);

                    let numeric = word.starts_with(|ch: char| ch.is_ascii_digit())
                        || (word.starts_with('-')
                            && word.len() > 1
                            && word[1..].starts_with(|ch: char| ch.is_ascii_digit()));
                    let kind = if colon {
                        TokKind::Key
                    } else if matches!(word, "true" | "false" | "null" | "~") {
                        TokKind::Lit
                    } else if word.starts_with('|') || word.starts_with('>') {
                        // 块标量引导（| / |- / >+2 等）：着 Punct 并开启内容块
                        block_indent = Some(indent);
                        TokKind::Punct
                    } else if numeric {
                        TokKind::Number
                    } else {
                        // 普通裸词与锚点/引用/标签：不着色
                        i = j;
                        continue;
                    };
                    out.push(JsonTok {
                        start: line_start + start,
                        end: line_start + end,
                        kind,
                    });
                    i = j;
                }

                // 键值分隔冒号（含 flow 集合内的 a: b）
                ':' => {
                    out.push(JsonTok {
                        start: line_start + start,
                        end: line_start + start + 1,
                        kind: TokKind::Punct,
                    });
                    i += 1;
                }

                // flow 符号与空白
                _ => i += 1,
            }
        }
    }
    out
}

/// 对 TOML 文本做轻量词法切分（逐字符扫描，多行字符串跨行处理）。
/// 节头 `[table]`/`[[array]]` 仅在行首出现（值数组的 `[` 着 Punct）。
fn toml_tokens(s: &str) -> Vec<JsonTok> {
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let n = chars.len();
    let mut i = 0usize;
    let mut out = Vec::new();

    while i < n {
        let (start, c) = chars[i];
        match c {
            // 注释到行尾（i 是 chars 下标，扫到换行为止）
            '#' => {
                let mut j = i;
                while j < n && chars[j].1 != '\n' {
                    j += 1;
                }
                let end = chars.get(j).map(|(p, _)| *p).unwrap_or(s.len());
                out.push(JsonTok {
                    start,
                    end,
                    kind: TokKind::Comment,
                });
                i = j;
            }

            // 引号字符串：三引号（跨行）或单引号；其后跟 '=' 则是键
            '"' | '\'' => {
                let triple = chars.get(i + 1).map(|(_, x)| x) == Some(&c)
                    && chars.get(i + 2).map(|(_, x)| x) == Some(&c);
                let mut j = i + if triple { 3 } else { 1 };
                while j < n {
                    if c == '"' && chars[j].1 == '\\' {
                        j += 2;
                        continue;
                    }
                    if triple {
                        if chars[j].1 == c
                            && chars.get(j + 1).map(|(_, x)| x) == Some(&c)
                            && chars.get(j + 2).map(|(_, x)| x) == Some(&c)
                        {
                            j += 3;
                            break;
                        }
                    } else if chars[j].1 == c {
                        j += 1;
                        break;
                    }
                    j += 1;
                }
                let end = chars.get(j).map(|(p, _)| *p).unwrap_or(s.len());

                let mut k = j;
                while k < n && chars[k].1.is_whitespace() {
                    k += 1;
                }
                let kind = if k < n && chars[k].1 == '=' {
                    TokKind::Key
                } else {
                    TokKind::Str
                };
                out.push(JsonTok { start, end, kind });
                i = j;
            }

            // 节头（当前行内行首的方括号）：整体含括号按键色；值数组的 '[' 是 Punct
            '[' if s[..start]
                .rfind('\n')
                .map_or(s[..start].chars().all(char::is_whitespace), |p| {
                    s[p + 1..start].chars().all(char::is_whitespace)
                }) =>
            {
                let mut j = i + 1;
                if chars.get(j).map(|(_, x)| x) == Some(&'[') {
                    j += 1;
                }
                while j < n && chars[j].1 != ']' {
                    j += 1;
                }
                j += 1;
                if chars.get(j).map(|(_, x)| x) == Some(&']') {
                    j += 1; // [[array]] 双闭合
                }
                let end = chars.get(j).map(|(p, _)| *p).unwrap_or(s.len());
                out.push(JsonTok {
                    start,
                    end,
                    kind: TokKind::Key,
                });
                i = j;
            }

            '=' | ',' | '{' | '}' | '[' | ']' => {
                out.push(JsonTok {
                    start,
                    end: start + 1,
                    kind: TokKind::Punct,
                });
                i += 1;
            }

            // 裸词：键名（含点分）/ true|false / 数字与日期时间
            c if !c.is_whitespace() => {
                let mut j = i + 1;
                while j < n {
                    let cc = chars[j].1;
                    if cc.is_whitespace() || matches!(cc, ',' | '{' | '}' | '[' | ']' | '=' | '#') {
                        break;
                    }
                    j += 1;
                }
                let end = chars.get(j).map(|(p, _)| *p).unwrap_or(s.len());
                let word = &s[start..end];

                let mut k = j;
                while k < n && chars[k].1.is_whitespace() {
                    k += 1;
                }
                let is_key = k < n && chars[k].1 == '=';

                let numeric = word.starts_with(|ch: char| ch.is_ascii_digit())
                    || ((word.starts_with('-') || word.starts_with('+'))
                        && word.len() > 1
                        && word[1..].starts_with(|ch: char| ch.is_ascii_digit()));
                let kind = if is_key {
                    TokKind::Key
                } else if matches!(word, "true" | "false") {
                    TokKind::Lit
                } else if numeric {
                    TokKind::Number
                } else {
                    i = j;
                    continue;
                };
                out.push(JsonTok { start, end, kind });
                i = j;
            }

            _ => i += 1,
        }
    }
    out
}

/// 对 XML 文本做轻量词法切分：标签名/属性名/属性值/注释与处理指令；
/// 文本内容不着色。
fn xml_tokens(s: &str) -> Vec<JsonTok> {
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let n = chars.len();
    let mut i = 0usize;
    let mut out = Vec::new();

    while i < n {
        let (start, c) = chars[i];
        if c != '<' {
            i += 1;
            continue;
        }

        // 注释 / 处理指令 / DOCTYPE：整段到结束标记，着注释色
        let section_end = |open: &str, close: &str| -> Option<usize> {
            s[start..].starts_with(open).then(|| {
                s[start..]
                    .find(close)
                    .map_or(s.len(), |p| start + p + close.len())
            })
        };
        if let Some(end) = section_end("<!--", "-->")
            .or_else(|| section_end("<?", "?>"))
            .or_else(|| section_end("<!", ">"))
        {
            out.push(JsonTok {
                start,
                end,
                kind: TokKind::Comment,
            });
            // end 是字节偏移，i 用 chars 下标：换算后再继续
            i = s[..end].chars().count();
            continue;
        }

        // '<' 或 '</'（闭合前缀整体一个 token）
        let mut j = i + 1;
        if chars.get(j).map(|(_, x)| x) == Some(&'/') {
            out.push(JsonTok {
                start,
                end: start + 2,
                kind: TokKind::Punct,
            });
            j += 1;
        } else {
            out.push(JsonTok {
                start,
                end: start + 1,
                kind: TokKind::Punct,
            });
        }
        let name_start = j;
        while j < n && !chars[j].1.is_whitespace() && !matches!(chars[j].1, '>' | '/' | '?') {
            j += 1;
        }
        if j > name_start {
            // chars 下标换算字节偏移（中文标签名/前文含中文时两者不等）
            let end = chars.get(j).map(|(p, _)| *p).unwrap_or(s.len());
            out.push(JsonTok {
                start: chars[name_start].0,
                end,
                kind: TokKind::Key,
            });
        }

        // 属性区：属性名（Lit）/ = （Punct）/ 引号值（Str）/ > 或 /> 闭合
        while j < n {
            let (as0, ac) = chars[j];
            match ac {
                '>' => {
                    out.push(JsonTok {
                        start: as0,
                        end: as0 + 1,
                        kind: TokKind::Punct,
                    });
                    j += 1;
                    break;
                }
                '/' if chars.get(j + 1).map(|(_, x)| x) == Some(&'>') => {
                    out.push(JsonTok {
                        start: as0,
                        end: as0 + 2,
                        kind: TokKind::Punct,
                    });
                    j += 2;
                    break;
                }
                '=' => {
                    out.push(JsonTok {
                        start: as0,
                        end: as0 + 1,
                        kind: TokKind::Punct,
                    });
                    j += 1;
                }
                '"' | '\'' => {
                    let mut k = j + 1;
                    while k < n && chars[k].1 != ac {
                        k += 1;
                    }
                    let (end, next) = if k < n {
                        (chars[k].0 + 1, k + 1)
                    } else {
                        (s.len(), n)
                    };
                    out.push(JsonTok {
                        start: as0,
                        end,
                        kind: TokKind::Str,
                    });
                    j = next;
                }
                c if !c.is_whitespace() => {
                    let mut k = j + 1;
                    while k < n
                        && !chars[k].1.is_whitespace()
                        && !matches!(chars[k].1, '=' | '>' | '/')
                    {
                        k += 1;
                    }
                    let end = chars.get(k).map(|(p, _)| *p).unwrap_or(s.len());
                    out.push(JsonTok {
                        start: as0,
                        end,
                        kind: TokKind::Lit,
                    });
                    j = k;
                }
                _ => j += 1,
            }
        }
        i = j;
    }
    out
}

/// 语法配色（深/浅主题各一套），顺序对应 [TokKind] 6 个类别。
pub(crate) fn json_palette(dark: bool) -> [egui::Color32; 6] {
    if dark {
        [
            egui::Color32::from_rgb(0x7C, 0xAE, 0xFE), // Key  亮蓝
            egui::Color32::from_rgb(0x98, 0xC3, 0x79), // Str  柔绿
            egui::Color32::from_rgb(0xD1, 0x9A, 0x66), // Num  暖橙
            egui::Color32::from_rgb(0xC5, 0x86, 0xC0), // Lit  紫
            egui::Color32::from_rgb(0x8A, 0x8A, 0x8A), // Punct 灰
            egui::Color32::from_rgb(0x6A, 0x99, 0x55), // Comment 绿灰
        ]
    } else {
        [
            egui::Color32::from_rgb(0x05, 0x3F, 0xAE), // Key  深蓝
            egui::Color32::from_rgb(0x1A, 0x7F, 0x37), // Str  深绿
            egui::Color32::from_rgb(0xB2, 0x5C, 0x00), // Num  深橙
            egui::Color32::from_rgb(0x86, 0x2D, 0x9E), // Lit  深紫
            egui::Color32::from_rgb(0x77, 0x77, 0x77), // Punct 灰
            egui::Color32::from_rgb(0x55, 0x80, 0x3F), // Comment 橄榄绿
        ]
    }
}

/// 按语法 token 与可选命中区间，把文本切成若干段写入 `job`。
/// 命中区间 `[hs,he)` 内的文字额外加琥珀色背景（与 hex 视图同色半透明）。
pub(crate) fn append_colored(
    job: &mut egui::text::LayoutJob,
    s: &str,
    tokens: &[JsonTok],
    base: egui::text::TextFormat,
    palette: &[egui::Color32; 6],
    highlights: &SearchHighlights,
) {
    let mut fmt_for = |start: usize, end: usize, color: egui::Color32| {
        let format = egui::text::TextFormat {
            color,
            ..base.clone()
        };
        append_highlighted(job, &s[start..end], start, format, highlights);
    };
    let mut pos = 0usize;
    for tok in tokens {
        if tok.start > pos {
            fmt_for(pos, tok.start, base.color); // token 之间的空白/换行：默认色
        }
        fmt_for(tok.start, tok.end, palette[tok.kind.palette_index()]);
        pos = tok.end;
    }
    if pos < s.len() {
        fmt_for(pos, s.len(), base.color);
    }
}

fn append_highlighted(
    job: &mut egui::text::LayoutJob,
    text: &str,
    offset: usize,
    format: egui::text::TextFormat,
    highlights: &SearchHighlights,
) {
    let end = offset + text.len();
    let mut boundaries = vec![0, text.len()];
    for (start, stop) in highlights.ranges.iter().copied().chain(highlights.active) {
        let start = start.max(offset);
        let stop = stop.min(end);
        if start < stop {
            boundaries.push(start - offset);
            boundaries.push(stop - offset);
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    for pair in boundaries.windows(2) {
        let (start, stop) = (pair[0], pair[1]);
        if start == stop {
            continue;
        }
        let mut fragment_format = format.clone();
        if let Some(background) = highlight_background(offset + start, offset + stop, highlights) {
            fragment_format.background = background;
        }
        job.append(&text[start..stop], 0.0, fragment_format);
    }
}

/// 折叠视图的一行：字节区间 + 可折叠范围。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct FoldLine {
    /// 行字节起点（含行首缩进）
    pub(crate) start: usize,
    /// 行字节终点（不含换行符与行尾 \r）
    pub(crate) end: usize,
    /// 可折叠：折叠后隐藏下一行起至本行号 +1..=自身 fold_end 行号的行；
    /// None 表示该行无可折叠块
    pub(crate) fold_end: Option<usize>,
}

/// 行首缩进宽度（前导空格数；空白行为 usize::MAX，不截断上一行的折叠块）。
fn indent_of(line: &[u8]) -> usize {
    match line.iter().position(|b| !b.is_ascii_whitespace()) {
        Some(i) => i,
        // 空行/全空白行：视作无限深，不打断相邻更深层级的折叠块
        None => usize::MAX,
    }
}

/// 行内容（去掉行首缩进）是否以闭合标记开头：`}` `]` 或 XML 闭合标签 `</`。
/// 用于把折叠区间延伸到同缩进的闭合行（pretty JSON/XML），折叠后不残留孤儿闭合行。
fn starts_with_closer(line: &[u8], indent: usize) -> bool {
    let Some(rest) = line.get(indent..) else {
        return false;
    };
    rest.starts_with(b"}") || rest.starts_with(b"]") || rest.starts_with(b"</")
}

/// 按缩进计算每行的折叠区间，对 pretty JSON/XML/YAML/TOML 通用：
/// 某行之后存在更深缩进的块即可折叠，块尾为第一个缩进 ≤ 本行的行之前；
/// 若该行与本行同缩进且以闭合标记开头，则连闭合行一起隐藏。
/// 紧凑单行文本没有任何可折叠行。
pub(crate) fn fold_lines(s: &str) -> Vec<FoldLine> {
    let bytes = s.as_bytes();
    let mut starts: Vec<usize> = Vec::new();
    let mut ends: Vec<usize> = Vec::new();
    let mut indents: Vec<usize> = Vec::new();
    let mut start = 0usize;
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'\n' {
            let mut end = i;
            if end > start && bytes[end - 1] == b'\r' {
                end -= 1; // 行尾 \r（CRLF 文本）不计入行内容
            }
            starts.push(start);
            ends.push(end);
            indents.push(indent_of(&bytes[start..end]));
            start = i + 1;
        }
    }
    starts.push(start);
    ends.push({
        let mut end = bytes.len();
        if end > start && bytes[end - 1] == b'\r' {
            end -= 1;
        }
        end
    });
    indents.push(indent_of(&bytes[start..]));
    // 以 \n 结尾的文本：丢弃末尾幻影空行，避免闭合行多出"只藏一个空行"的折叠块，
    // 也避免折叠视图底部渲染一条多余的空行
    if s.ends_with('\n') {
        starts.pop();
        ends.pop();
        indents.pop();
    }

    let n = starts.len();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        // 第一个缩进 ≤ 本行的后续行 k：更深缩进块为 i+1..k-1（连续，
        // pretty 输出的块不会中途回到浅缩进再深入）
        let mut k = i + 1;
        while k < n && indents[k] > indents[i] {
            k += 1;
        }
        let fold_end = if k > i + 1 {
            // 块非空；块后同缩进行是闭合标记（}/]/</tag>）则连它一起隐藏
            if k < n
                && indents[k] == indents[i]
                && starts_with_closer(&bytes[starts[k]..ends[k]], indents[i])
            {
                Some(k)
            } else {
                Some(k - 1)
            }
        } else {
            None
        };
        out.push(FoldLine {
            start: starts[i],
            end: ends[i],
            fold_end,
        });
    }
    out
}

/// 折叠视图逐行着色：把整段文本的 token（绝对字节区间）裁剪到行区间
/// `[ls,le)` 后逐 token 写入行 job，token 间空白用默认色；
/// 可选命中区间（显示文本字节坐标）与 token 叠加琥珀背景，语义同 [`append_colored`]。
/// 跨行 token（如 YAML 块标量）按行裁剪，各行的所属段保持 token 类别色。
pub(crate) fn append_line_colored(
    job: &mut egui::text::LayoutJob,
    line: &str,
    toks: &[JsonTok],
    ls: usize,
    le: usize,
    base: egui::text::TextFormat,
    palette: &[egui::Color32; 6],
    highlights: &SearchHighlights,
) {
    let mut pos = ls;
    for tok in toks {
        let ts = tok.start.max(ls);
        let te = tok.end.min(le);
        if ts >= te {
            continue; // token 与本行不相交
        }
        if ts > pos {
            append_highlighted(job, &line[pos - ls..ts - ls], pos, base.clone(), highlights);
        }
        let mut f = base.clone();
        f.color = palette[tok.kind.palette_index()];
        append_highlighted(job, &line[ts - ls..te - ls], ts, f, highlights);
        pos = te;
    }
    if pos < le {
        append_highlighted(job, &line[pos - ls..le - ls], pos, base, highlights);
    }
}

fn highlight_background(
    start: usize,
    end: usize,
    highlights: &SearchHighlights,
) -> Option<egui::Color32> {
    let overlaps = |(hs, he): &(usize, usize)| start < *he && end > *hs;
    if highlights.active.as_ref().is_some_and(overlaps) {
        Some(egui::Color32::from_rgba_unmultiplied(
            0xF5, 0xA6, 0x23, 0x99,
        ))
    } else if highlights.ranges.iter().any(overlaps) {
        Some(egui::Color32::from_rgba_unmultiplied(
            0x45, 0x91, 0xD5, 0x55,
        ))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    // ── JSON 语法着色 ──────────────────────────────────────────────

    use super::{
        JsonTok, SearchHighlights, TokKind, append_colored, append_line_colored,
        highlight_background, json_tokens, syntax_mode,
    };

    fn highlighted_text(job: &egui::text::LayoutJob) -> Vec<&str> {
        job.sections
            .iter()
            .filter(|section| section.format.background != egui::Color32::TRANSPARENT)
            .map(|section| &job.text[section.byte_range.start.0..section.byte_range.end.0])
            .collect()
    }

    fn base_format() -> egui::text::TextFormat {
        egui::text::TextFormat::default()
    }

    #[test]
    fn plain_text_highlights_only_matching_ranges() {
        let text = "prefix needle and needle suffix";
        let highlights = SearchHighlights {
            ranges: vec![(7, 13), (18, 24)],
            active: Some((18, 24)),
        };
        let mut job = egui::text::LayoutJob::default();

        append_colored(
            &mut job,
            text,
            &[],
            base_format(),
            &[egui::Color32::WHITE; 6],
            &highlights,
        );

        assert_eq!(highlighted_text(&job), ["needle", "needle"]);
    }

    #[test]
    fn search_highlight_splits_json_string_token_at_match_boundaries() {
        let text = "\"request needle completed\"";
        let start = text.find("needle").unwrap();
        let highlights = SearchHighlights {
            ranges: vec![(start, start + "needle".len())],
            active: Some((start, start + "needle".len())),
        };
        let token = JsonTok {
            start: 0,
            end: text.len(),
            kind: TokKind::Str,
        };
        let mut job = egui::text::LayoutJob::default();

        append_colored(
            &mut job,
            text,
            &[token],
            base_format(),
            &[egui::Color32::WHITE; 6],
            &highlights,
        );

        assert_eq!(highlighted_text(&job), ["needle"]);
    }

    #[test]
    fn folded_line_highlight_splits_token_at_match_boundaries() {
        let text = "  \"request needle completed\"";
        let start = text.find("needle").unwrap();
        let highlights = SearchHighlights {
            ranges: vec![(start, start + "needle".len())],
            active: Some((start, start + "needle".len())),
        };
        let token = JsonTok {
            start: 2,
            end: text.len(),
            kind: TokKind::Str,
        };
        let mut job = egui::text::LayoutJob::default();

        append_line_colored(
            &mut job,
            text,
            &[token],
            0,
            text.len(),
            base_format(),
            &[egui::Color32::WHITE; 6],
            &highlights,
        );

        assert_eq!(highlighted_text(&job), ["needle"]);
    }
    use crate::fmt::DecodeMode;

    #[test]
    fn active_search_hit_uses_stronger_background_than_other_hits() {
        let highlights = SearchHighlights {
            ranges: vec![(0, 2), (4, 6)],
            active: Some((4, 6)),
        };
        let other_bg = highlight_background(0, 2, &highlights).unwrap();
        let active_bg = highlight_background(4, 6, &highlights).unwrap();
        assert_ne!(other_bg, active_bg);
    }

    /// 取 (文本片段, 类别) 列表，方便断言。
    fn tok_pairs(s: &str) -> Vec<(&str, TokKind)> {
        json_tokens(s)
            .into_iter()
            .map(|t| (&s[t.start..t.end], t.kind))
            .collect()
    }

    /// 构造一个最小合法 BSON 文档：{"a": "hi"}
    fn bson_sample() -> Vec<u8> {
        bson::to_vec(&bson::doc! { "a": "hi" }).expect("序列化 BSON 文档")
    }

    #[test]
    fn syntax_mode_formats() {
        let valid = br#"{"a":1}"#;
        // 手动 Json：合法 JSON 着色（格式为 Json）
        assert_eq!(syntax_mode(DecodeMode::Json, valid), Some(DecodeMode::Json));
        // Auto：对象/数组形态的合法 JSON 自动着色为 Json
        assert_eq!(syntax_mode(DecodeMode::Auto, valid), Some(DecodeMode::Json));
        assert_eq!(
            syntax_mode(DecodeMode::Auto, b"[1,2]"),
            Some(DecodeMode::Json)
        );
        // Auto：完整 BSON 文档自动着色为 Bson
        let doc = bson_sample();
        assert_eq!(syntax_mode(DecodeMode::Auto, &doc), Some(DecodeMode::Bson));
        // 手动 Bson：完整 BSON 文档着色（显示为 JSON pretty，复用 JSON 语法）
        assert_eq!(syntax_mode(DecodeMode::Bson, &doc), Some(DecodeMode::Bson));
        // 手动 Json 下 BSON 字节不是合法 JSON：不着色
        assert_eq!(syntax_mode(DecodeMode::Json, &doc), None);
        // Auto 下合法 JSON 标量不着色（避免普通数字/字符串被误判）
        assert_eq!(syntax_mode(DecodeMode::Auto, b"123"), None);
        assert_eq!(syntax_mode(DecodeMode::Auto, br#""s""#), None);
        // 非结构化模式：永不着色
        assert_eq!(syntax_mode(DecodeMode::Utf8, valid), None);
        // Bson 模式下非 BSON 内容（此处为 JSON 文本）：不着色
        assert_eq!(syntax_mode(DecodeMode::Bson, valid), None);
        // 非法内容不着色
        assert_eq!(syntax_mode(DecodeMode::Json, b"not json"), None);
        // 跨页截断的部分 JSON/BSON：Auto/Json 都不着色
        assert_eq!(syntax_mode(DecodeMode::Json, br#"{"a": "lon"#), None);
        assert_eq!(syntax_mode(DecodeMode::Auto, br#"{"a": "lon"#), None);
        // 截断的 BSON（长度字段无法自洽）：不着色
        assert_eq!(syntax_mode(DecodeMode::Auto, &doc[..doc.len() - 2]), None);
    }

    #[test]
    fn syntax_mode_proto() {
        // proto 类型池是全局单态：与 proto/guess 的测试共用锁串行，避免互相覆盖
        let _g = crate::proto::tests::TEST_LOCK.lock().unwrap();

        let dir = std::env::temp_dir().join(format!("mdbxer_syntax_proto_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.proto");
        std::fs::write(
            &path,
            concat!(
                r#"syntax = "proto3"; package t;"#,
                "\n",
                r#"message Msg { int64 id = 1; }"#,
            ),
        )
        .unwrap();
        crate::proto::load_files(&[&path]).unwrap();
        crate::proto::set_current(Some("t.Msg".to_string()));

        // field1 varint 42：类型池解码成功 → 着色（显示为 proto3 JSON pretty）
        let bytes = [0x08, 0x2A];
        assert_eq!(
            syntax_mode(DecodeMode::Proto, &bytes),
            Some(DecodeMode::Proto)
        );
        // 截断的 varint 解码失败 → 显示 UTF-8 原文，不着色
        assert_eq!(syntax_mode(DecodeMode::Proto, &[0x08]), None);
        // 未选类型 → 回退 UTF-8 原文，不着色
        crate::proto::set_current(None);
        assert_eq!(syntax_mode(DecodeMode::Proto, &bytes), None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn syntax_mode_config_formats() {
        // 手动 Yaml：合法 YAML 着色；Tab 缩进非法回退不着色
        let y = b"title: demo\nnums:\n  - 1\n  - 2\n";
        assert_eq!(syntax_mode(DecodeMode::Yaml, y), Some(DecodeMode::Yaml));
        assert_eq!(syntax_mode(DecodeMode::Yaml, b"a:\n\t- x\n"), None);

        // Auto：形态像 YAML（键/块序列、可整体解析）的才识别；普通句子不着色
        assert_eq!(
            syntax_mode(
                DecodeMode::Auto,
                b"app:\n  name: mdbx\n  debug: false\ntimeout: 30\n"
            ),
            Some(DecodeMode::Yaml)
        );
        assert_eq!(
            syntax_mode(DecodeMode::Auto, b"- a\n- b\n"),
            Some(DecodeMode::Yaml)
        );
        // 普通文本 / 含 ':' 的路径串 / 非法 YAML 都不判
        assert_eq!(syntax_mode(DecodeMode::Auto, b"hello world"), None);
        assert_eq!(syntax_mode(DecodeMode::Auto, b"C:\\Users\\x"), None);
        assert_eq!(syntax_mode(DecodeMode::Auto, b"a:\n\t- x\n"), None);

        // 手动 Toml：合法着色；非法不着色
        let tml = b"title = \"demo\"\n[owner]\nname = \"a\"\n";
        assert_eq!(syntax_mode(DecodeMode::Toml, tml), Some(DecodeMode::Toml));
        assert_eq!(syntax_mode(DecodeMode::Toml, b"title =\n"), None);

        // Auto：顶层为表/数组且含 `=` 或 `[` 才判 TOML；普通/裸标量不判
        assert_eq!(
            syntax_mode(
                DecodeMode::Auto,
                b"title = \"mdbx\"\n[db]\nmax_readers = 126\n"
            ),
            Some(DecodeMode::Toml)
        );
        assert_eq!(
            syntax_mode(DecodeMode::Auto, b"name = \"lwq\"\n"),
            Some(DecodeMode::Toml)
        );
        // 裸标量、无 '=' 的普通文本不判
        assert_eq!(
            syntax_mode(DecodeMode::Auto, b"hello = \"world\""),
            Some(DecodeMode::Toml)
        );
        assert_eq!(syntax_mode(DecodeMode::Auto, b"hello world"), None);
        // 非法 TOML（"title =" 缺值）不判
        assert_eq!(syntax_mode(DecodeMode::Auto, b"title =\n"), None);

        // 手动 Xml：合法（含根元素）着色；不配对/仅注释不着色
        assert_eq!(
            syntax_mode(DecodeMode::Xml, b"<a><b>1</b></a>"),
            Some(DecodeMode::Xml)
        );
        assert_eq!(syntax_mode(DecodeMode::Xml, b"<a><b></a>"), None);
        assert_eq!(syntax_mode(DecodeMode::Xml, b"<!-- c -->"), None);

        // Auto 命中 XML（JSON/BSON 等优先级更高，靠后兜底）
        assert_eq!(
            syntax_mode(DecodeMode::Auto, b"<root><x>1</x></root>"),
            Some(DecodeMode::Xml)
        );
        // Auto 下 YAML/TOML 复合形态（顶层表/映射）要识别
        assert_eq!(
            syntax_mode(DecodeMode::Auto, b"title: demo\nother: 1\n"),
            Some(DecodeMode::Yaml)
        );
        assert_eq!(
            syntax_mode(DecodeMode::Auto, b"title = \"x\"\nenable = true\n"),
            Some(DecodeMode::Toml)
        );
        // 但非映射/序列形态不识别：普通平铺文本解析成裸标量串，不进 Auto
        assert_eq!(syntax_mode(DecodeMode::Auto, b"hello world"), None);
    }

    #[test]
    fn yaml_tokens_keys_values_comments() {
        use super::{TokKind, yaml_tokens};

        let s = "# 配置\nname: \"demo\"\nnum: 42\nok: true\nlist:\n  - 1\n  - two\n";
        let toks = yaml_tokens(s);
        let pairs: Vec<(&str, TokKind)> =
            toks.iter().map(|t| (&s[t.start..t.end], t.kind)).collect();

        // 整行注释
        assert!(pairs.contains(&("# 配置", TokKind::Comment)));
        // 键与冒号
        assert!(pairs.contains(&("name", TokKind::Key)));
        assert!(pairs.contains(&("num", TokKind::Key)));
        assert!(pairs.contains(&("ok", TokKind::Key)));
        assert!(pairs.contains(&("list", TokKind::Key)));
        // 字符串/数字/布尔值
        assert!(pairs.contains(&("\"demo\"", TokKind::Str)));
        assert!(pairs.contains(&("42", TokKind::Number)));
        assert!(pairs.contains(&("true", TokKind::Lit)));
        assert!(pairs.contains(&("1", TokKind::Number)));
        // 序列项 '-'
        assert!(pairs.contains(&("-", TokKind::Punct)));
        // 普通裸词 "two" 不着色（无 token）
        assert!(!pairs.iter().any(|(t, _)| *t == "two"));
    }

    #[test]
    fn yaml_tokens_block_scalar_skips_content() {
        use super::{TokKind, yaml_tokens};

        // 块标量内容行（缩进更深）整行不着色：其中的 "k: v" 不当成键值对
        let s = "text: |\n  key: value\n  more\nafter: 1\n";
        let toks = yaml_tokens(s);
        let pairs: Vec<(&str, TokKind)> =
            toks.iter().map(|t| (&s[t.start..t.end], t.kind)).collect();

        assert!(pairs.contains(&("|", TokKind::Punct)));
        assert!(!pairs.iter().any(|(t, k)| *t == "key" && *k == TokKind::Key));
        assert!(!pairs.iter().any(|(t, _)| *t == "value"));
        // 块结束后恢复正常：after 是键、1 是数字
        assert!(pairs.contains(&("after", TokKind::Key)));
        assert!(pairs.contains(&("1", TokKind::Number)));
    }

    #[test]
    fn toml_tokens_sections_keys_values() {
        use super::{TokKind, toml_tokens};

        let s =
            "# 注释\n[server]\nhost = \"db.local\"\nport = 5432\nverbose = true\narr = [1, 2]\n";
        let toks = toml_tokens(s);
        let pairs: Vec<(&str, TokKind)> =
            toks.iter().map(|t| (&s[t.start..t.end], t.kind)).collect();

        assert!(pairs.contains(&("# 注释", TokKind::Comment)));
        assert!(pairs.contains(&("[server]", TokKind::Key)));
        assert!(pairs.contains(&("host", TokKind::Key)));
        assert!(pairs.contains(&("\"db.local\"", TokKind::Str)));
        assert!(pairs.contains(&("port", TokKind::Key)));
        assert!(pairs.contains(&("5432", TokKind::Number)));
        assert!(pairs.contains(&("verbose", TokKind::Key)));
        assert!(pairs.contains(&("true", TokKind::Lit)));
        // 值数组的方括号是 Punct（区别于行首节头）
        assert!(pairs.contains(&("[", TokKind::Punct)));
        assert!(pairs.contains(&("]", TokKind::Punct)));
    }

    #[test]
    fn xml_tokens_tags_attrs_comments() {
        use super::{TokKind, xml_tokens};

        let s = "<!-- 说明 --><root id=\"7\" ok='y'>文本</root>";
        let toks = xml_tokens(s);
        let pairs: Vec<(&str, TokKind)> =
            toks.iter().map(|t| (&s[t.start..t.end], t.kind)).collect();

        // 注释整段
        assert!(pairs.contains(&("<!-- 说明 -->", TokKind::Comment)));
        // 标签名（开/闭）
        assert!(pairs.contains(&("root", TokKind::Key)));
        // 属性名 / 等号 / 属性值
        assert!(pairs.contains(&("id", TokKind::Lit)));
        assert!(pairs.contains(&("=", TokKind::Punct)));
        assert!(pairs.contains(&("\"7\"", TokKind::Str)));
        assert!(pairs.contains(&("'y'", TokKind::Str)));
        // 括号与闭合
        assert!(pairs.contains(&("<", TokKind::Punct)));
        assert!(pairs.contains(&("</", TokKind::Punct)));
        assert!(pairs.contains(&(">", TokKind::Punct)));
        // 文本内容不着色
        assert!(!pairs.iter().any(|(t, _)| *t == "文本"));
    }

    #[test]
    fn json_tokens_classify_object() {
        // pretty: {\n  "name": "bob",\n  "age": 30\n}
        let s = serde_json::to_string_pretty(
            &serde_json::json!({"name": "bob", "age": 30, "ok": true, "x": null}),
        )
        .unwrap();
        let toks = tok_pairs(&s);
        // 键被识别为 Key（即使与值字符串外观相同）
        assert!(toks.contains(&("\"name\"", TokKind::Key)));
        assert!(toks.contains(&("\"age\"", TokKind::Key)));
        assert!(toks.contains(&("\"ok\"", TokKind::Key)));
        // 值字符串是 Str 而非 Key
        assert!(toks.contains(&("\"bob\"", TokKind::Str)));
        // 数字 / 字面量 / 标点
        assert!(toks.contains(&("30", TokKind::Number)));
        assert!(toks.contains(&("true", TokKind::Lit)));
        assert!(toks.contains(&("null", TokKind::Lit)));
        assert!(toks.contains(&("{", TokKind::Punct)));
        assert!(toks.contains(&("}", TokKind::Punct)));
        assert!(toks.contains(&(":", TokKind::Punct)));
        assert!(toks.contains(&(",", TokKind::Punct)));
        // false 也算字面量
        let s2 = serde_json::to_string_pretty(&serde_json::json!({"f": false})).unwrap();
        assert!(tok_pairs(&s2).contains(&("false", TokKind::Lit)));
    }

    #[test]
    fn json_tokens_arrays_and_numbers() {
        let s = "[1, -2, 3.5, 1e2]";
        let toks = tok_pairs(s);
        assert!(toks.contains(&("[", TokKind::Punct)));
        assert!(toks.contains(&("]", TokKind::Punct)));
        assert!(toks.contains(&("1", TokKind::Number)));
        assert!(toks.contains(&("-2", TokKind::Number)));
        assert!(toks.contains(&("3.5", TokKind::Number)));
        assert!(toks.contains(&("1e2", TokKind::Number)));
    }

    #[test]
    fn json_tokens_escaped_quote_stays_inside_string() {
        let s = r#""a\"b""#;
        let toks = json_tokens(s);
        // 整个字符串（含转义引号）是一个 token，不会被中间的 \" 截断
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].start, 0);
        assert_eq!(toks[0].end, s.len());
        assert_eq!(toks[0].kind, TokKind::Str);
    }

    #[test]
    fn json_tokens_intervals_cover_without_gap_overlap() {
        let s = serde_json::to_string_pretty(&serde_json::json!({"a": [1, 2], "b": "x"})).unwrap();
        let toks = json_tokens(&s);
        // 区间有序、不重叠；空白处允许有空隙
        let mut prev_end = 0usize;
        for t in &toks {
            assert!(t.start >= prev_end, "token 重叠或逆序");
            assert!(t.end > t.start, "token 非空");
            assert!(t.end <= s.len());
            prev_end = t.end;
        }
        // 所有非空白字符都应被某个 token 覆盖
        for (i, ch) in s.char_indices() {
            if !ch.is_whitespace() {
                assert!(
                    toks.iter().any(|t| i >= t.start && i < t.end),
                    "字符 {ch:?} @ {i} 未被任何 token 覆盖"
                );
            }
        }
    }

    #[test]
    fn json_tokens_utf8_boundaries_are_char_safe() {
        // 中文：字节区间必须落在字符边界上（切片不 panic）
        let s = serde_json::to_string_pretty(&serde_json::json!({"名": "值"})).unwrap();
        let toks = json_tokens(&s);
        for t in &toks {
            // 能正常切片即说明边界合法
            let _ = &s[t.start..t.end];
        }
        assert!(tok_pairs(&s).contains(&("\"名\"", TokKind::Key)));
        assert!(tok_pairs(&s).contains(&("\"值\"", TokKind::Str)));
    }

    #[test]
    fn json_tokens_complex_document() {
        // 紧凑手写 JSON：覆盖嵌套对象、对象/对象数组、空容器、各种数字写法、
        // bool/null、引号/反斜杠/控制符/unicode 转义、中文键值。分词器不依赖
        // pretty 格式，直接对原始文本切分；数字写法因此完全可控（不被序列化规范化）。
        let raw = concat!(
            r#"{"id":42,"neg":-7,"pi":3.14159,"ratio":1.5e-3,"zero":0,"#,
            r#""active":true,"disabled":false,"missing":null,"#,
            r#""tags":["db","gui","rust"],"empty_obj":{},"empty_arr":[],"#,
            r#""meta":{"version":"1.0.0","deep":[{"x":1},{"x":-2.5}],"#,
            r#""esc":"a\"b\\c\n\t\u4e2d"},"中文键":"值"}"#
        );
        // 先确认测试数据本身是合法 JSON
        assert!(serde_json::from_str::<serde_json::Value>(raw).is_ok());
        assert_eq!(
            syntax_mode(DecodeMode::Json, raw.as_bytes()),
            Some(DecodeMode::Json)
        );

        let toks = json_tokens(raw);
        let pairs = tok_pairs(raw);

        // ── 键 ─────────────────────────────────────────────
        for k in [
            "\"id\"",
            "\"neg\"",
            "\"pi\"",
            "\"ratio\"",
            "\"zero\"",
            "\"active\"",
            "\"disabled\"",
            "\"missing\"",
            "\"tags\"",
            "\"empty_obj\"",
            "\"empty_arr\"",
            "\"meta\"",
            "\"version\"",
            "\"deep\"",
            "\"x\"",
            "\"esc\"",
            "\"中文键\"",
        ] {
            assert!(pairs.contains(&(k, TokKind::Key)), "缺少键 {k}");
        }

        // ── 值字符串（含数组元素与嵌套值）──────────────────
        for v in ["\"db\"", "\"gui\"", "\"rust\"", "\"1.0.0\"", "\"值\""] {
            assert!(pairs.contains(&(v, TokKind::Str)), "缺少字符串值 {v}");
        }

        // ── 各种数字写法都应整体归为 Number ────────────────
        for num in ["42", "-7", "3.14159", "1.5e-3", "0", "1", "-2.5"] {
            assert!(
                pairs.contains(&(num, TokKind::Number)),
                "数字 {num} 未被识别为 Number"
            );
        }

        // ── 字面量 / 标点 ──────────────────────────────────
        assert!(pairs.contains(&("true", TokKind::Lit)));
        assert!(pairs.contains(&("false", TokKind::Lit)));
        assert!(pairs.contains(&("null", TokKind::Lit)));
        for p in ["{", "}", "[", "]", ":", ","] {
            assert!(
                pairs.iter().any(|(t, k)| *t == p && *k == TokKind::Punct),
                "缺少标点 {p}"
            );
        }
        // 空对象 {} / 空数组 [] 的括号成对出现
        assert!(
            pairs
                .windows(2)
                .any(|w| w[0] == ("{", TokKind::Punct) && w[1] == ("}", TokKind::Punct))
        );
        assert!(
            pairs
                .windows(2)
                .any(|w| w[0] == ("[", TokKind::Punct) && w[1] == ("]", TokKind::Punct))
        );

        // ── 转义字符串必须完整成一个 Str，不被内部 \" \\ 等截断 ──
        let esc = pairs
            .iter()
            .find(|(t, k)| *k == TokKind::Str && t.contains(r#"\u4e2d"#))
            .expect("应找到含 unicode 转义的字符串 token");
        assert!(
            esc.0.starts_with('"') && esc.0.ends_with('"'),
            "转义串应被引号完整包裹"
        );
        assert!(esc.0.contains(r#"\"#) && esc.0.contains(r#"\n"#));

        // ── 所有字符串/键 token 都以成对引号界定 ───────────
        for (txt, kind) in &pairs {
            if matches!(kind, TokKind::Key | TokKind::Str) {
                assert!(
                    txt.starts_with('"') && txt.ends_with('"'),
                    "字符串 token 不完整: {txt}"
                );
            }
        }

        // ── 区间有序、不重叠、不越界，且覆盖全部非空白字符 ──
        let mut prev_end = 0usize;
        for t in &toks {
            assert!(t.start >= prev_end);
            assert!(t.end > t.start && t.end <= raw.len());
            prev_end = t.end;
        }
        for (i, ch) in raw.char_indices() {
            if !ch.is_whitespace() {
                assert!(
                    toks.iter().any(|t| i >= t.start && i < t.end),
                    "字符 {ch:?} @ {i} 未被覆盖"
                );
            }
        }
    }

    // ── 折叠视图 ──────────────────────────────────────────────────

    use super::fold_lines;

    /// 汇总断言用的 (行文本, fold_end) 列表。
    fn fold_pairs(s: &str) -> Vec<(&str, Option<usize>)> {
        fold_lines(s)
            .into_iter()
            .zip(s.split('\n'))
            .map(|(fl, line)| (line, fl.fold_end))
            .collect()
    }

    #[test]
    fn fold_lines_json_pretty() {
        let s = concat!(
            "{\n",            // 0 可折叠 → 连闭合行 6 一起隐藏
            "  \"a\": 1,\n",  // 1
            "  \"b\": {\n",   // 2 可折叠 → 连闭合行 4 一起隐藏
            "    \"c\": 2\n", // 3
            "  },\n",         // 4 闭合行
            "  \"d\": []\n",  // 5 空数组不可折叠
            "}\n"             // 6 闭合行（末尾 \\n 的幻影空行不生成行）
        );
        let pairs = fold_pairs(s);
        assert_eq!(pairs[0], ("{", Some(6)));
        assert_eq!(pairs[1], ("  \"a\": 1,", None));
        assert_eq!(pairs[2], ("  \"b\": {", Some(4)));
        assert_eq!(pairs[3], ("    \"c\": 2", None));
        assert_eq!(pairs[4], ("  },", None));
        assert_eq!(pairs[5], ("  \"d\": []", None));
        assert_eq!(pairs[6], ("}", None));
        // 顶层对象内部（行 1..5）不延伸到顶层闭合行之外
        assert!(pairs.iter().all(|(_, fe)| fe.map_or(true, |e| e < 7)));
    }

    #[test]
    fn fold_lines_yaml_indent_block() {
        // 缩进式（YAML）：无闭合标记，块到第一个缩进 ≤ 的行之前
        let s = "a:\n  b: 1\n  c: 2\nd: 3\n";
        let pairs = fold_pairs(s);
        assert_eq!(pairs[0], ("a:", Some(2)));
        assert_eq!(pairs[1], ("  b: 1", None));
        assert_eq!(pairs[2], ("  c: 2", None));
        assert_eq!(pairs[3], ("d: 3", None));
    }

    #[test]
    fn fold_lines_yaml_blank_line_keeps_block() {
        // 块内空行不打断折叠块（视作无限深缩进）
        let s = "a:\n  b: 1\n\n  c: 2\nd: 3\n";
        let pairs = fold_pairs(s);
        assert_eq!(pairs[0], ("a:", Some(3)));
    }

    #[test]
    fn fold_lines_xml_closer_extends() {
        let s = concat!(
            "<root>\n",     // 0 → 连 </root> 行 5 一起隐藏
            "  <a>1</a>\n", // 1
            "  <b>\n",      // 2 → 连 </b> 行 4 一起隐藏
            "    2\n",      // 3
            "  </b>\n",     // 4
            "</root>\n"     // 5
        );
        let pairs = fold_pairs(s);
        assert_eq!(pairs[0], ("<root>", Some(5)));
        assert_eq!(pairs[1], ("  <a>1</a>", None));
        assert_eq!(pairs[2], ("  <b>", Some(4)));
        assert_eq!(pairs[3], ("    2", None));
        assert_eq!(pairs[4], ("  </b>", None));
        assert_eq!(pairs[5], ("</root>", None));
    }

    #[test]
    fn fold_lines_trailing_array() {
        // TOML/JSON 数组：闭合 ] 同缩进，一并隐藏
        let s = "items = [\n  1,\n  2,\n]\n";
        let pairs = fold_pairs(s);
        assert_eq!(pairs[0], ("items = [", Some(3)));
        assert_eq!(pairs[3], ("]", None));
    }

    #[test]
    fn fold_lines_no_fold_cases() {
        // 紧凑单行：无任何可折叠行
        assert!(
            fold_lines(r#"{"a":1}"#)
                .iter()
                .all(|f| f.fold_end.is_none())
        );
        // 深层块后没有更浅行（文件以深层块结尾）：折叠到最后一行
        let s = "a:\n  b:\n    c: 1\n";
        let pairs = fold_pairs(s);
        assert_eq!(pairs[0], ("a:", Some(2)));
        assert_eq!(pairs[1], ("  b:", Some(2)));
    }
}
