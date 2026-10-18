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

/// 一个词法 token：字节区间（相对传入文本）+ 类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct JsonTok {
    start: usize,
    end: usize,
    kind: TokKind,
}

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
/// - 手动 Yaml/Toml/Xml：内容能被对应解析器整体解析（与解码显示同口径）才着色；
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
    hit: Option<(usize, usize)>,
) {
    let hit_bg = egui::Color32::from_rgba_unmultiplied(0xF5, 0xA6, 0x23, 0x66);
    let mut fmt_for = |start: usize, end: usize, color: egui::Color32| {
        let mut f = egui::text::TextFormat {
            color,
            ..base.clone()
        };
        if let Some((hs, he)) = hit
            && start < he
            && end > hs
        {
            f.background = hit_bg;
        }
        job.append(&s[start..end], 0.0, f);
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

#[cfg(test)]
mod tests {
    // ── JSON 语法着色 ──────────────────────────────────────────────

    use super::{TokKind, json_tokens, syntax_mode};
    use crate::fmt::DecodeMode;

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
        // 手动 Json 下 BSON 字节不是合法 JSON：不着色
        assert_eq!(syntax_mode(DecodeMode::Json, &doc), None);
        // Auto 下合法 JSON 标量不着色（避免普通数字/字符串被误判）
        assert_eq!(syntax_mode(DecodeMode::Auto, b"123"), None);
        assert_eq!(syntax_mode(DecodeMode::Auto, br#""s""#), None);
        // 非 Json/Bson/Auto 模式：永不着色
        assert_eq!(syntax_mode(DecodeMode::Utf8, valid), None);
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
    fn syntax_mode_config_formats() {
        // 手动 Yaml：合法 YAML 着色；Tab 缩进非法回退不着色
        let y = b"title: demo\nnums:\n  - 1\n  - 2\n";
        assert_eq!(syntax_mode(DecodeMode::Yaml, y), Some(DecodeMode::Yaml));
        assert_eq!(syntax_mode(DecodeMode::Yaml, b"a:\n\t- x\n"), None);

        // Auto：形态像 YAML（键/块序列、可整体解析）的才识别；普通句子不着色
        assert_eq!(
            syntax_mode(DecodeMode::Auto, b"app:\n  name: mdbx\n  debug: false\ntimeout: 30\n"),
            Some(DecodeMode::Yaml)
        );
        assert_eq!(syntax_mode(DecodeMode::Auto, b"- a\n- b\n"), Some(DecodeMode::Yaml));
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
            syntax_mode(DecodeMode::Auto, b"title = \"mdbx\"\n[db]\nmax_readers = 126\n"),
            Some(DecodeMode::Toml)
        );
        assert_eq!(syntax_mode(DecodeMode::Auto, b"name = \"lwq\"\n"), Some(DecodeMode::Toml));
        // 裸标量、无 '=' 的普通文本不判
        assert_eq!(syntax_mode(DecodeMode::Auto, b"hello = \"world\""), Some(DecodeMode::Toml));
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
}
