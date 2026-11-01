// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 交互式十六进制视图（Notepad++ Hex-Editor 风格）。
//!
//! 自绘实现（非 TextEdit），支持：
//! - 悬停整行高亮（跨 地址/HEX/ASCII 三列）；
//! - 悬停单个字节时 HEX 与 ASCII 两侧联动高亮；
//! - 鼠标拖拽按字节选择，HEX 与 ASCII 选区联动、可跨多行；单击单个字节即选中该字节；
//! - 悬停在选区内字节上时，用 tooltip 逐行列出各类型按当前字节序的解释（类型 + 值）；
//! - Ctrl+C 同时复制所选字节的十六进制与 ASCII 原文（按显示行对齐），Esc 清除选区。
//!
//! 大段数据（可能数千行）只绘制与命中测试与裁剪区相交的行。

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::fmt::{addr_chars, addr_digits, hex_line, hex_section_chars, mid_gap_index};
use crate::ui::syntax::SearchHighlights;

pub(crate) const EDGE_SCROLL_MAX_ROWS_PER_SECOND: f32 = 60.0;
const EDGE_SCROLL_MAX_DEPTH: f32 = 4.0;
const COPY_TOAST_DURATION: Duration = Duration::from_secs(2);

fn show_copy_toast(ctx: &egui::Context, id: egui::Id) {
    let toast_id = id.with("copy_toast");
    let Some((shown_at, pos)) = ctx.data(|d| d.get_temp::<(Instant, egui::Pos2)>(toast_id)) else {
        return;
    };
    let remaining = COPY_TOAST_DURATION.saturating_sub(shown_at.elapsed());
    if remaining.is_zero() {
        ctx.data_mut(|d| d.remove::<Instant>(toast_id));
        return;
    }

    ctx.request_repaint_after(remaining);
    egui::Area::new(toast_id.with("area"))
        .order(egui::Order::Foreground)
        .fixed_pos(pos + egui::vec2(12.0, 12.0))
        .constrain(true)
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.add(
                    egui::Label::new(crate::i18n::tr().copied_selection)
                        .wrap_mode(egui::TextWrapMode::Extend),
                );
            });
        });
}

pub(crate) fn edge_scroll_speed_fraction(distance: f32, margin: f32) -> f32 {
    let depth = (distance / margin).clamp(0.0, EDGE_SCROLL_MAX_DEPTH);
    (depth / EDGE_SCROLL_MAX_DEPTH).powi(2)
}

/// Galley 缓存：内容指纹变化时整体作废。
#[derive(Default, Clone)]
struct GalleyCache {
    /// 内容/配置/主题指纹（见 hex_view 内 tag 计算），不符时整表作废重建
    tag: u64,
    /// 行号 → 已排版的行文本，仅缓存可视区及前后少量缓冲行
    rows: HashMap<usize, Arc<egui::Galley>>,
}

/// 拖拽过程状态。
#[derive(Clone, Copy)]
struct Drag {
    /// 起手锚点字节全局序号，选区另一端跟随指针
    anchor: usize,
    /// 指针是否真的移出过锚点字节——区分"单击"（清选区）与"拖选"
    moved: bool,
}

/// 渲染几何（点坐标）：三列布局与字节单元格换算的基准。
struct Geom {
    /// 内容区左缘 = 地址列起点
    x0: f32,
    /// 内容区顶缘 = 第 0 行顶部
    top: f32,
    /// 等宽字体单字符宽度
    cw: f32,
    /// 行高
    rh: f32,
    /// 每字节在 HEX 列占的字符数（紧凑 2 / 常规 3）
    per: f32,
    /// HEX 列左缘（地址列右侧）
    hex_x: f32,
    /// HEX 列总宽（含字节间空格与 mid gap）
    hex_w: f32,
    /// ASCII 列左缘
    ascii_x: f32,
    /// ASCII 列总宽
    ascii_w: f32,
}

impl Geom {
    /// 命中测试：返回（行号, 行内字节序号），落在地址列/空隙/补齐区返回 None。
    fn hit(&self, p: egui::Pos2, n: usize, chunk_len: usize) -> Option<(usize, usize)> {
        if p.x < self.x0 || p.y < self.top {
            return None;
        }
        let row = ((p.y - self.top) / self.rh).floor() as isize;
        if row < 0 {
            return None;
        }
        let row = row as usize;
        if chunk_len == 0 {
            return None;
        }

        // HEX 列优先（与 ASCII 列在布局上不重叠）
        if self.hex_w > 0.0 && p.x >= self.hex_x && p.x < self.hex_x + self.hex_w {
            let mut raw = ((p.x - self.hex_x) / self.cw).floor() as isize;
            let per = self.per as isize;
            if let Some(mid) = mid_gap_index(n) {
                // mid 处是额外空隙；其后的字符坐标整体后移 1
                if raw == per * mid as isize {
                    return None;
                }
                if raw > per * mid as isize {
                    raw -= 1;
                }
            }
            if raw < 0 {
                return None;
            }
            let (bi, frac) = (raw / per, raw % per);
            // 每字节只有两位 hex 数字可点：常规格式第 3 位是空格，紧凑格式两位全是数字
            ((bi as usize) < chunk_len && frac < 2).then_some((row, bi as usize))
        } else if self.ascii_w > 0.0 && p.x >= self.ascii_x && p.x < self.ascii_x + self.ascii_w {
            let bi = ((p.x - self.ascii_x) / self.cw).floor() as isize;
            ((bi as usize) < chunk_len).then_some((row, bi as usize))
        } else {
            None
        }
    }

    /// 某行内第 i 字节在 HEX 列的两位数字矩形。
    fn hex_cell(&self, n: usize, row: usize, i: usize) -> egui::Rect {
        let gap = if mid_gap_index(n).is_some_and(|m| i >= m) {
            self.cw
        } else {
            0.0
        };
        egui::Rect::from_min_size(
            egui::pos2(
                self.hex_x + (self.per * i as f32) * self.cw + gap,
                self.top + row as f32 * self.rh,
            ),
            egui::vec2(2.0 * self.cw, self.rh),
        )
    }

    /// 某行内第 i 字节在 ASCII 列的字符矩形。
    fn ascii_cell(&self, row: usize, i: usize) -> egui::Rect {
        egui::Rect::from_min_size(
            egui::pos2(
                self.ascii_x + i as f32 * self.cw,
                self.top + row as f32 * self.rh,
            ),
            egui::vec2(self.cw, self.rh),
        )
    }
}

/// 主题色 → 指定 alpha 的半透明版（行悬停/字节悬停/选区背景的低透明度叠加）。
fn with_alpha(c: egui::Color32, a: u8) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

fn line_byte_range(row: usize, width: usize, byte_len: usize) -> Option<(usize, usize)> {
    let start = row.checked_mul(width)?;
    if width == 0 || start >= byte_len {
        return None;
    }
    Some((start, (start + width).min(byte_len) - 1))
}

/// Key/Value 两个 hex 视图的全局稳定组件 id（不依赖 ui 层级），
/// 便于全局快捷键判断焦点是否在 hex 视图内。
pub(crate) fn view_id(is_key: bool) -> egui::Id {
    egui::Id::new(("mdbxer_hex_view", is_key))
}

/// 选区字节按各数值类型的解释结果（`(标签, 值)`，供悬停 tooltip 逐行展示）。
///
/// 整数/浮点按类型宽度分块解释（与 VS 的字节解释一致）：能容纳整个选区的
/// 最小宽度决定最大类型（1 字节 → 到 u8/i8，2 字节 → 到 u16/i16，4 字节 → 到
/// u32/i32/f32，8 字节 → 到 u64/i64/f64）；更窄的类型把选区按该宽度逐块切分，
/// 每块一个值（如 3 字节的 u16 得到 2 个值），末块不足宽度时零扩展补全并标注
/// 补零。多字节类型按 `endian` 解释并在标签后标注字节序；值间用分号分隔
/// （千位分隔符本身用逗号，混用会难以分辨），多字节类型每行最多 8 个值，
/// 单字节类型最多 16 个值，超出以 `…` 省略。
/// 标识与时间只在长度精确匹配时解释（16 字节 → uuid/ipv6，6 字节 → mac，
/// 4 字节 → ipv4/time，8 字节 → time）。末尾附 ASCII（非可打印字节显示为 `.`）
/// 与 UTF-8 文本解释，只取选区前 64 字节，超出以 `…` 截断。
fn type_interpretations(bytes: &[u8], endian: crate::fmt::Endian) -> Vec<(String, String)> {
    use crate::fmt::{Endian, with_sep};

    let mut rows: Vec<(String, String)> = Vec::new();
    if bytes.is_empty() {
        return rows;
    }
    let sfx = endian.suffix();
    // 每行最多展示的数值个数，超出部分以 `…` 省略；
    // 单字节类型值短，可多放一些（正好覆盖一行 16 字节）
    const MAX_VALUES: usize = 8;
    const MAX_BYTES: usize = 16;
    let note = crate::i18n::tr().padded;

    // 能容纳整个选区的最大类型宽度（超过 8 字节按 8 字节封顶），
    // 更窄的类型把选区按各自宽度分块，给出多个值
    let width = [1usize, 2, 4, 8]
        .into_iter()
        .find(|&w| w >= bytes.len().min(8))
        .unwrap_or(8);

    // 单字节类型：每字节一个值，无字节序后缀
    macro_rules! byte_row {
        ($label:literal, $ty:ty) => {{
            let mut vals: Vec<String> = Vec::new();
            for (k, b) in bytes.iter().enumerate() {
                if k == MAX_BYTES {
                    vals.push("…".to_string());
                    break;
                }
                vals.push(with_sep((*b as $ty).to_string()));
            }
            rows.push(($label.to_string(), vals.join("; ")));
        }};
    }
    byte_row!("u8", u8);
    byte_row!("i8", i8);

    // 多字节整数：按宽度切块，末块不足时零扩展（小端补尾部、大端补头部）
    macro_rules! int_row {
        ($label:literal, $ty:ty) => {{
            let n = std::mem::size_of::<$ty>();
            if width >= n {
                let mut vals: Vec<String> = Vec::new();
                for (k, chunk) in bytes.chunks(n).enumerate() {
                    if k == MAX_VALUES {
                        vals.push("…".to_string());
                        break;
                    }
                    let mut arr = [0u8; std::mem::size_of::<$ty>()];
                    match endian {
                        Endian::Little => arr[..chunk.len()].copy_from_slice(chunk),
                        Endian::Big => arr[n - chunk.len()..].copy_from_slice(chunk),
                    }
                    let v = match endian {
                        Endian::Little => <$ty>::from_le_bytes(arr),
                        Endian::Big => <$ty>::from_be_bytes(arr),
                    };
                    let mut s = with_sep(v.to_string());
                    if chunk.len() < n {
                        s.push_str(note);
                    }
                    vals.push(s);
                }
                rows.push((format!("{} {sfx}", $label), vals.join("; ")));
            }
        }};
    }
    int_row!("u16", u16);
    int_row!("i16", i16);
    int_row!("u32", u32);
    int_row!("i32", i32);
    int_row!("u64", u64);
    int_row!("i64", i64);

    // 浮点：与整数同样按宽度切块
    macro_rules! float_row {
        ($label:literal, $ty:ty) => {{
            let n = std::mem::size_of::<$ty>();
            if width >= n {
                let mut vals: Vec<String> = Vec::new();
                for (k, chunk) in bytes.chunks(n).enumerate() {
                    if k == MAX_VALUES {
                        vals.push("…".to_string());
                        break;
                    }
                    let mut arr = [0u8; std::mem::size_of::<$ty>()];
                    match endian {
                        Endian::Little => arr[..chunk.len()].copy_from_slice(chunk),
                        Endian::Big => arr[n - chunk.len()..].copy_from_slice(chunk),
                    }
                    let v = match endian {
                        Endian::Little => <$ty>::from_le_bytes(arr),
                        Endian::Big => <$ty>::from_be_bytes(arr),
                    };
                    // 与解码显示一致：极小值 Display 会展开成几百位，改用科学计数法
                    let s = format!("{v}");
                    let mut s = if s.len() > 24 { format!("{v:e}") } else { s };
                    s = with_sep(s);
                    if chunk.len() < n {
                        s.push_str(note);
                    }
                    vals.push(s);
                }
                rows.push((format!("{} {sfx}", $label), vals.join("; ")));
            }
        }};
    }
    float_row!("f32", f32);
    float_row!("f64", f64);

    // 标识与时间：长度必须与类型要求一致，直接复用解码层同一套格式化
    // （uuid/mac/ip 无有效长度时只回退 hex，这里靠长度过滤掉）
    let ident = |mode: crate::fmt::DecodeMode| {
        crate::fmt::decode(
            bytes,
            mode,
            endian,
            usize::MAX,
            &crate::fmt::HintFlags::default(),
        )
    };
    match bytes.len() {
        16 => {
            rows.push(("uuid".to_string(), ident(crate::fmt::DecodeMode::Uuid)));
            rows.push(("ip".to_string(), ident(crate::fmt::DecodeMode::Ip)));
        }
        6 => rows.push(("mac".to_string(), ident(crate::fmt::DecodeMode::Mac))),
        4 => {
            rows.push(("ip".to_string(), ident(crate::fmt::DecodeMode::Ip)));
            rows.push(("time".to_string(), ident(crate::fmt::DecodeMode::Time)));
        }
        8 => rows.push(("time".to_string(), ident(crate::fmt::DecodeMode::Time))),
        _ => {}
    }

    // 文本解释：可打印 ASCII 原样、其余显示为 '.'（与 ASCII 列口径一致）
    const TEXT_CAP: usize = 64;
    let shown = &bytes[..bytes.len().min(TEXT_CAP)];
    let tail = if bytes.len() > shown.len() { "…" } else { "" };
    let ascii: String = shown
        .iter()
        .map(|&b| {
            if (0x20..=0x7E).contains(&b) {
                b as char
            } else {
                '.'
            }
        })
        .collect();
    rows.push(("ASCII".to_string(), format!("{ascii}{tail}")));
    rows.push((
        "utf8".to_string(),
        format!("{}{tail}", String::from_utf8_lossy(shown)),
    ));
    rows
}

/// 悬停选区时的类型解释提示：逐行「类型 = 值」。
fn show_type_tooltip(ui: &mut egui::Ui, bytes: &[u8], endian: crate::fmt::Endian) {
    egui::Grid::new(ui.id().with("hex_sel_types"))
        .num_columns(2)
        .spacing([12.0, 2.0])
        .show(ui, |ui| {
            for (label, value) in type_interpretations(bytes, endian) {
                ui.add(egui::Label::new(egui::RichText::new(label).monospace()));
                ui.add(egui::Label::new(egui::RichText::new(value).monospace()));
                ui.end_row();
            }
        });
}

/// 交互式 hex 视图。
///
/// - `id`：组件唯一 id（Key/Value 卡片各一）；
/// - `base_offset`：本段在原始字节中的起始偏移（地址列用）；
/// - `endian`：多字节类型解释所用的字节序（与全局设置一致）；
/// - `sel`：当前选区（字节全局序号，端点顺序无关）；内容切换由调用方负责清空。
/// - `highlights`：内容搜索匹配区间及当前命中（字节全局序号 start, len）；
///   当前命中使用更醒目的颜色；与选区重叠时让位于选区。
/// - `scroll_to`：需要滚动到可见的命中字节（全局偏移，卡片搜索命中后由调用方一次性传入）。
pub(crate) fn hex_view(
    ui: &mut egui::Ui,
    id: egui::Id,
    bytes: &[u8],
    width: usize,
    show_addr: bool,
    show_line_numbers: bool,
    show_hex: bool,
    show_ascii: bool,
    compact: bool,
    base_offset: usize,
    endian: crate::fmt::Endian,
    sel: &mut Option<(usize, usize)>,
    highlights: &SearchHighlights,
    scroll_to: Option<usize>,
) -> egui::Response {
    let font_id = egui::TextStyle::Monospace.resolve(ui.style());
    let (cw, rh) = ui.fonts_mut(|f| (f.glyph_width(&font_id, '0'), f.row_height(&font_id)));

    // 地址位数随数据大小变化（同一 dump 内所有行统一位数保证对齐）
    let digits = addr_digits(base_offset, bytes.len());
    let addr_w = if show_addr {
        addr_chars(digits) as f32 * cw
    } else {
        0.0
    };
    // 每行字节数固定为工具栏所选值（4/8/16）：右栏最小宽度按当前配置所需
    // 行宽钳制（detail/mod.rs 的 size_range），拖窄到下限即停，
    // 整行始终完整可见，这里无需再按可用宽度缩减列数。
    let n = width.max(1);
    let rows = bytes.len().div_ceil(n).max(1);
    let line_number_w = if show_line_numbers {
        (rows.ilog10() + 2) as f32 * cw
    } else {
        0.0
    };
    let hex_w = if show_hex {
        hex_section_chars(n, compact) as f32 * cw
    } else {
        0.0
    };
    let ascii_w = if show_ascii { n as f32 * cw } else { 0.0 };
    let content_w = line_number_w + addr_w + hex_w + ascii_w;

    let total_h = rows as f32 * rh;
    let wanted = egui::vec2(ui.available_width().max(content_w), total_h);
    // 显式绑定固定 id：request_focus / 焦点判断 / 临时数据全部指向同一 id
    let (_space_id, rect) = ui.allocate_space(wanted);
    let resp = ui.interact(rect, id, egui::Sense::click_and_drag());

    let geom = Geom {
        x0: rect.left() + line_number_w,
        top: rect.top(),
        cw,
        rh,
        // 每字节在 HEX 列占的字符数（紧凑 2 / 常规 3），数字两位恒为 2
        per: if compact { 2.0 } else { 3.0 },
        hex_x: rect.left() + line_number_w + addr_w,
        hex_w,
        ascii_x: rect.left() + line_number_w + addr_w + hex_w,
        ascii_w,
    };
    // 搜索命中后自动滚动：命中字节在本段内时，把该行滚到可视区（调用方一次性传入）
    if let Some(g) = scroll_to {
        if g >= base_offset && g < base_offset + bytes.len() {
            let row = (g - base_offset) / n;
            let y = geom.top + row as f32 * rh;
            ui.scroll_to_rect(
                egui::Rect::from_min_size(egui::pos2(rect.left(), y), egui::vec2(rect.width(), rh)),
                Some(egui::Align::Center),
            );
        }
    }

    // 内容/配置/主题指纹：变了就丢弃 galley 缓存
    let tag = {
        let mut h = DefaultHasher::new();
        (
            base_offset,
            bytes.len(),
            width,
            show_addr,
            show_line_numbers,
            show_hex,
            show_ascii,
            compact,
            ui.visuals().dark_mode,
            ui.ctx().pixels_per_point().to_bits(),
        )
            .hash(&mut h);
        bytes.hash(&mut h);
        h.finish()
    };
    // HEX/ASCII 文本基色：取非交互控件前景色，不随 hover/选中态变化，
    // 保证选区高亮叠加时字节文字颜色稳定（高亮只改背景不改前景）
    let text_color = ui.visuals().widgets.noninteractive.fg_stroke.color;

    // 指针坐标 → (行号, 行内字节序号)；落在行外空白、mid gap 或超出
    // 本段实际数据（末行截断部分）时返回 None，交互一律以此为准
    let hit_at = |p: egui::Pos2| -> Option<(usize, usize)> {
        let (r, i) = geom.hit(p, n, n)?;
        if r >= rows {
            return None;
        }
        let chunk_len = (bytes.len() - r * n).min(n);
        if i >= chunk_len {
            return None;
        }
        Some((r, i))
    };
    let line_number_at = |pointer: egui::Pos2| {
        show_line_numbers
            && pointer.x >= rect.left()
            && pointer.x < geom.x0
            && pointer.y >= rect.top()
            && pointer.y < rect.bottom()
    };
    if resp.hovered() && resp.hover_pos().is_some_and(line_number_at) {
        ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
    }

    // ── 指针交互：按下定锚点、拖拽更新选区（HEX/ASCII 高亮联动，
    //    复制时 HEX 与 ASCII 同时输出，与起手列无关）──
    let pressed = resp.drag_started();
    if pressed {
        resp.request_focus();
        // 标记最近选择交互视图：折叠文本视图（detail.rs fold_view_body）与之共用
        // 该标记决定 Ctrl+C/Esc 归属，避免两个视图同时响应互相覆盖剪贴板
        ui.ctx()
            .data_mut(|d| d.insert_temp(egui::Id::new("mdbxer_sel_owner"), Some(id)));
        if let Some(p) = resp.interact_pointer_pos() {
            if !line_number_at(p) {
                match hit_at(p) {
                    Some((r, i)) => {
                        ui.ctx().data_mut(|d| {
                            d.insert_temp(
                                id,
                                Some(Drag {
                                    anchor: r * n + i,
                                    moved: false,
                                }),
                            )
                        });
                    }
                    None => {
                        *sel = None;
                        ui.ctx()
                            .data_mut(|d| d.insert_temp::<Option<Drag>>(id, None));
                    }
                }
            } else {
                ui.ctx()
                    .data_mut(|d| d.insert_temp::<Option<Drag>>(id, None));
            }
        }
    }
    if resp.dragged() {
        let anchor = ui.ctx().data(|d| d.get_temp::<Option<Drag>>(id)).flatten();
        if let (Some(drag), Some(mut p)) = (anchor, resp.interact_pointer_pos()) {
            // 拖到视口上下边缘自动滚动，使选区可以一直延伸到不可见部分。
            // 速度按指针深入边缘的距离比例递增；内容本身没超出该方向则不滚。
            let view = ui.clip_rect();
            let margin = 28.0;
            let dt = ui.input(|i| i.stable_dt).min(0.05);
            // 二次曲线：靠近触发边缘时慢，指针越过边缘越远越快，最多 60 行/秒。
            let max_speed = rh * EDGE_SCROLL_MAX_ROWS_PER_SECOND;
            let mut edge = false;
            if rect.top() < view.top() - 1.0 && p.y < view.top() + margin {
                // egui 约定 scroll delta 正值 = 向上看
                let factor = edge_scroll_speed_fraction(view.top() + margin - p.y, margin);
                ui.scroll_with_delta(egui::vec2(0.0, max_speed * factor * dt));
                p.y = view.top() + 1.0;
                edge = true;
            } else if rect.bottom() > view.bottom() + 1.0 && p.y > view.bottom() - margin {
                let factor = edge_scroll_speed_fraction(p.y - (view.bottom() - margin), margin);
                ui.scroll_with_delta(egui::vec2(0.0, -max_speed * factor * dt));
                p.y = view.bottom() - 2.0;
                edge = true;
            }
            // 纵向已夹到边缘时，横向也夹回内容区，保证能命中最边缘的字节
            if edge {
                p.x = p.x.clamp(rect.left() + 1.0, rect.left() + content_w - 1.0);
            }
            if let Some((r, i)) = hit_at(p) {
                let cur = r * n + i;
                let moved = drag.moved || cur != drag.anchor;
                ui.ctx().data_mut(|d| {
                    d.insert_temp(
                        id,
                        Some(Drag {
                            anchor: drag.anchor,
                            moved,
                        }),
                    )
                });
                if moved {
                    *sel = Some((drag.anchor, cur));
                }
            }
        }
    }
    if resp.drag_stopped() {
        // 未移动即松开（含按住稍久才松手，egui 会判成拖拽）也视为单击该字节，
        // 建立只含该字节的选区，保证 1 字节同样能选中/看类型解释；
        // 普通快速单击不产生拖拽记录，走下面的 clicked 分支
        if let Some(Drag {
            anchor,
            moved: false,
        }) = ui.ctx().data(|d| d.get_temp::<Option<Drag>>(id)).flatten()
        {
            *sel = Some((anchor, anchor));
            resp.request_focus();
            ui.ctx()
                .data_mut(|d| d.insert_temp(egui::Id::new("mdbxer_sel_owner"), Some(id)));
        }
        ui.ctx().data_mut(|d| d.remove_temp::<Option<Drag>>(id));
    }
    let clicked = resp.clicked_by(egui::PointerButton::Primary);
    let line_number_clicked = clicked && resp.interact_pointer_pos().is_some_and(line_number_at);
    if line_number_clicked
        && let Some(pointer) = resp.interact_pointer_pos()
        && !bytes.is_empty()
    {
        let row = ((pointer.y - rect.top()) / rh).floor() as usize;
        if let Some(range) = line_byte_range(row, n, bytes.len()) {
            *sel = Some(range);
            resp.request_focus();
            ui.ctx()
                .data_mut(|d| d.insert_temp(egui::Id::new("mdbxer_sel_owner"), Some(id)));
        }
    } else if clicked
        && !bytes.is_empty()
        && let Some((r, i)) = resp.interact_pointer_pos().and_then(hit_at)
    {
        // 单击单个字节：建立只含该字节的选区（悬停其上即可看类型解释），
        // 也便于只复制这一个字节
        let g = r * n + i;
        *sel = Some((g, g));
        resp.request_focus();
        ui.ctx()
            .data_mut(|d| d.insert_temp(egui::Id::new("mdbxer_sel_owner"), Some(id)));
    } else if clicked && sel.is_some() {
        // 单击地址列/空隙/空白处：清除选区
        *sel = None;
    }
    // 右键：有选区时复制并取消选择（右键 = 复制 + 取消所选）；
    // 单击空白/Esc 仍取消选择
    let rmb_pos = resp.interact_pointer_pos();
    let rmb_sel = if resp.secondary_clicked() {
        sel.take()
    } else {
        None
    };
    let rmb_copy = rmb_sel.is_some();

    // ── 键盘：聚焦时 Ctrl+C 复制（HEX 与 ASCII 同行，按显示列对齐），Esc 清除 ──
    // 注意：egui 在每帧 begin_pass 处理原始事件时，一旦发现无修饰 Esc 会立即
    // 清空 focused_widget（memory/mod.rs Focus::begin_pass），所以这一帧
    // focused() 已经是 None。要用 resp.lost_focus()（比较上一帧焦点）+
    // key_pressed(Escape) 才能识别"焦点在本组件时按下了 Esc"。
    let escape_pressed = ui.input(|i| i.key_pressed(egui::Key::Escape));
    let focused = ui.ctx().memory(|m| m.focused() == Some(id));
    if focused || (escape_pressed && resp.lost_focus()) || rmb_copy {
        let copy = rmb_copy
            || ui.input(|i| {
                i.events
                    .iter()
                    .any(|e| matches!(e, egui::Event::Copy | egui::Event::Cut))
            });
        if copy {
            if let Some((a, b)) = rmb_sel.or(*sel) {
                let (lo, hi) = (a.min(b), a.max(b).min(bytes.len() - 1));
                // 地址/HEX/ASCII 跟随界面开关输出，按显示行/列对齐（见 hexdump::hex_copy_selection）
                ui.ctx().copy_text(crate::fmt::hex_copy_selection(
                    bytes,
                    lo,
                    hi,
                    n,
                    show_addr,
                    show_hex,
                    show_ascii,
                    base_offset,
                    digits,
                    compact,
                ));
                if rmb_copy {
                    ui.ctx().data_mut(|d| {
                        d.insert_temp(
                            id.with("copy_toast"),
                            (Instant::now(), rmb_pos.unwrap_or(rect.center())),
                        );
                    });
                }
            }
        }
        if escape_pressed && resp.lost_focus() {
            *sel = None;
        }
    }

    show_copy_toast(ui.ctx(), id);

    // ── 绘制（仅裁剪区内可见行）──
    let painter = ui.painter();
    let clip = painter.clip_rect().intersect(rect);
    let vis_first =
        (((clip.top() - rect.top()) / rh).floor() as isize).clamp(0, rows as isize) as usize;
    let vis_last =
        (((clip.bottom() - rect.top()) / rh).ceil() as isize).clamp(0, rows as isize) as usize;

    let vis = ui.visuals();
    let row_bg = with_alpha(vis.widgets.hovered.bg_fill, 40);
    let byte_bg = with_alpha(vis.selection.bg_fill, 80);
    let sel_bg = with_alpha(vis.selection.bg_fill, 150);
    // 搜索命中：琥珀色（固定色，深浅主题下都与蓝色选区可区分）
    let active_hit_bg = egui::Color32::from_rgba_unmultiplied(0xF5, 0xA6, 0x23, 0x99);
    let other_hit_bg = egui::Color32::from_rgba_unmultiplied(0x45, 0x91, 0xD5, 0x55);

    // 悬停字节（未拖拽时用 hover 位置；拖拽中用指针位置联动）
    let pointer = if resp.dragged() {
        resp.interact_pointer_pos()
    } else {
        resp.hover_pos()
    };
    let hover_row = pointer
        .filter(|p| rect.contains(*p))
        .map(|p| ((p.y - rect.top()) / rh).floor() as usize)
        .filter(|&r| r < rows);
    let hover = pointer.and_then(hit_at);
    if hover.is_some() {
        ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::Text);
    }

    let sel_norm = sel.map(|(a, b)| (a.min(b), a.max(b)));
    // 搜索命中（全局偏移）裁剪到本段，转为段内闭区间
    let normalize_hit = |(s, l): &(usize, usize)| {
        let g_hi = s.saturating_add(*l);
        let seg_end = base_offset + bytes.len();
        if g_hi <= base_offset || *s >= seg_end {
            return None;
        }
        let lo = s.saturating_sub(base_offset);
        let hi = g_hi.min(seg_end) - base_offset - 1;
        Some((lo, hi))
    };
    let hit_norm: Vec<_> = highlights.ranges.iter().filter_map(normalize_hit).collect();
    let active_hit_norm = highlights.active.as_ref().and_then(normalize_hit);

    // galley 缓存：本帧取一次、帧末写回
    let mut cache = ui
        .ctx()
        .data(|d| d.get_temp::<GalleyCache>(id))
        .unwrap_or_default();
    if cache.tag != tag {
        cache = GalleyCache {
            tag,
            rows: HashMap::new(),
        };
    }
    let cache_first = vis_first.saturating_sub(2);
    let cache_last = vis_last.saturating_add(2).min(rows);
    cache
        .rows
        .retain(|&r, _| (cache_first..cache_last).contains(&r));

    for r in vis_first..vis_last {
        let row_top = rect.top() + r as f32 * rh;
        let row_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left(), row_top),
            egui::vec2(rect.width(), rh),
        );

        // 1) 整行悬停背景
        if hover_row == Some(r) {
            painter.rect_filled(row_rect, 0.0, row_bg);
        }

        if show_line_numbers {
            painter.text(
                egui::pos2(geom.x0 - 4.0, row_top),
                egui::Align2::RIGHT_TOP,
                (r + 1).to_string(),
                font_id.clone(),
                with_alpha(ui.visuals().hyperlink_color, 128),
            );
        }

        let chunk_len = (bytes.len() - r * n).min(n);
        for i in 0..chunk_len {
            let g = r * n + i;
            let selected = sel_norm.is_some_and(|(lo, hi)| g >= lo && g <= hi);
            let active_hit_now = active_hit_norm.is_some_and(|(lo, hi)| g >= lo && g <= hi);
            let hit_now = hit_norm.iter().any(|&(lo, hi)| g >= lo && g <= hi);
            let hovered = hover.is_some_and(|(hr, hi)| hr == r && hi == i);

            // 2) 选区 / 搜索命中 / 悬停字节背景（HEX 与 ASCII 联动）；选区优先于命中
            let v_inset = egui::vec2(0.0, 1.0);
            if selected {
                if show_hex {
                    painter.rect_filled(geom.hex_cell(n, r, i).shrink2(v_inset), 1.5, sel_bg);
                }
                if show_ascii {
                    painter.rect_filled(geom.ascii_cell(r, i).shrink2(v_inset), 1.5, sel_bg);
                }
            } else if active_hit_now {
                if show_hex {
                    painter.rect_filled(
                        geom.hex_cell(n, r, i).shrink2(v_inset),
                        1.5,
                        active_hit_bg,
                    );
                }
                if show_ascii {
                    painter.rect_filled(geom.ascii_cell(r, i).shrink2(v_inset), 1.5, active_hit_bg);
                }
            } else if hit_now {
                if show_hex {
                    painter.rect_filled(geom.hex_cell(n, r, i).shrink2(v_inset), 1.5, other_hit_bg);
                }
                if show_ascii {
                    painter.rect_filled(geom.ascii_cell(r, i).shrink2(v_inset), 1.5, other_hit_bg);
                }
            } else if hovered {
                if show_hex {
                    painter.rect_filled(geom.hex_cell(n, r, i).shrink2(v_inset), 1.5, byte_bg);
                }
                if show_ascii {
                    painter.rect_filled(geom.ascii_cell(r, i).shrink2(v_inset), 1.5, byte_bg);
                }
            }
        }

        // 3) 文本（最后画，压在背景之上）。地址列弱化半透明、
        //    HEX/ASCII 数据列保持原色，用 LayoutJob 两段着色
        let galley = cache
            .rows
            .entry(r)
            .or_insert_with(|| {
                let chunk = &bytes[r * n..(r * n + chunk_len).min(bytes.len())];
                let line = hex_line(
                    chunk,
                    n,
                    show_addr,
                    show_hex,
                    show_ascii,
                    base_offset + r * n,
                    digits,
                    compact,
                );
                // 整行均为 ASCII（地址 + HEX + ASCII 列），按字节切出地址段
                let addr_len = if show_addr { digits + 2 } else { 0 };
                let mut job = egui::text::LayoutJob::default();
                if addr_len > 0 {
                    let addr_color = with_alpha(text_color, 0x99);
                    job.append(
                        &line[..addr_len],
                        0.0,
                        egui::TextFormat::simple(font_id.clone(), addr_color),
                    );
                }
                job.append(
                    &line[addr_len..],
                    0.0,
                    egui::TextFormat::simple(font_id.clone(), text_color),
                );
                ui.fonts_mut(|f| f.layout_job(job))
            })
            .clone();
        painter.galley(egui::pos2(geom.x0, row_top), galley, text_color);
    }
    ui.ctx().data_mut(|d| d.insert_temp(id, cache));

    // egui 在按下/拖拽期间只把"被拖拽控件"标记为 hovered（interaction.rs），指针移出后
    // 提示仍会跟随指针显示、盖住其他面板区域；仅悬停且未拖拽时才显示提示。
    // 必须用 at_pointer 变体：本控件 allocate 的是全部内容的高度（数千像素），
    // 默认锚定控件矩形的提示会被钳制到窗口底部、横向跑到左下方。
    //
    // 指针落在选区内的字节上时显示类型解释（各数值类型按当前字节序 + 文本），
    // 其余位置显示整控件的操作提示。
    let sel_tip = sel_norm.and_then(|(lo, hi)| {
        let (r, i) = hover?;
        let g = r * n + i;
        if bytes.is_empty() || g < lo || g > hi {
            return None;
        }
        let lo = lo.min(bytes.len() - 1);
        let hi = hi.min(bytes.len() - 1);
        Some(&bytes[lo..=hi])
    });
    let resp = if resp.hovered() && !resp.dragged() {
        match sel_tip {
            Some(sel_bytes) => {
                resp.on_hover_ui_at_pointer(|ui| show_type_tooltip(ui, sel_bytes, endian))
            }
            None => resp.on_hover_text_at_pointer(crate::i18n::tr().hex_hint),
        }
    } else {
        resp
    };
    resp
}

#[cfg(test)]
mod tests {
    use super::{Geom, edge_scroll_speed_fraction, line_byte_range, type_interpretations};
    use crate::fmt::{Endian, hex_section_chars};

    fn geom(width: usize, compact: bool, show_ascii: bool) -> Geom {
        let cw = 10.0;
        let addr_w = 20.0;
        let hex_w = hex_section_chars(width, compact) as f32 * cw;
        Geom {
            x0: 0.0,
            top: 0.0,
            cw,
            rh: 10.0,
            per: if compact { 2.0 } else { 3.0 },
            hex_x: addr_w,
            hex_w,
            ascii_x: addr_w + hex_w,
            ascii_w: if show_ascii { width as f32 * cw } else { 0.0 },
        }
    }

    #[test]
    fn hit_maps_hex_ascii_and_rows_to_same_byte() {
        let g = geom(8, false, true);
        assert_eq!(g.hit(egui::pos2(25.0, 5.0), 8, 8), Some((0, 0)));
        assert_eq!(g.hit(egui::pos2(185.0, 5.0), 8, 8), Some((0, 5)));
        assert_eq!(g.hit(egui::pos2(g.ascii_x + 55.0, 5.0), 8, 8), Some((0, 5)));
        assert_eq!(g.hit(egui::pos2(25.0, 15.0), 8, 8), Some((1, 0)));
    }

    #[test]
    fn hit_rejects_address_gaps_and_missing_bytes() {
        let g = geom(8, false, true);
        assert_eq!(g.hit(egui::pos2(10.0, 5.0), 8, 8), None);
        assert_eq!(g.hit(egui::pos2(45.0, 5.0), 8, 8), None);
        assert_eq!(g.hit(egui::pos2(140.0, 5.0), 8, 8), None);
        assert_eq!(g.hit(egui::pos2(111.0, 5.0), 8, 3), None);
    }

    #[test]
    fn line_selection_covers_visible_row_and_clips_partial_last_row() {
        assert_eq!(line_byte_range(0, 16, 40), Some((0, 15)));
        assert_eq!(line_byte_range(1, 16, 40), Some((16, 31)));
        assert_eq!(line_byte_range(2, 16, 40), Some((32, 39)));
        assert_eq!(line_byte_range(3, 16, 40), None);
        assert_eq!(line_byte_range(0, 0, 4), None);
    }

    #[test]
    fn type_interpretations_chunk_narrower_types() {
        // 两字节选区：u8/i8 每字节一个值，更宽类型不显示
        let le = type_interpretations(&[1, 2], Endian::Little);
        let be = type_interpretations(&[1, 2], Endian::Big);
        assert!(le.contains(&("u8".to_string(), "1; 2".to_string())));
        assert!(le.contains(&("u16 LE".to_string(), "513".to_string())));
        assert!(be.contains(&("u16 BE".to_string(), "258".to_string())));
        assert!(!le.iter().any(|(l, _)| l.starts_with("u32")));
        assert!(!le.iter().any(|(l, _)| l.starts_with("f32")));

        // 文本解释：非可打印字节显示为 '.'（与 ASCII 列口径一致）
        assert!(le.contains(&("ASCII".to_string(), "..".to_string())));
        assert!(le.contains(&("utf8".to_string(), "\u{1}\u{2}".to_string())));
    }

    #[test]
    fn type_interpretations_single_byte_shows_u8_only() {
        // 单字节选区：u8/i8 有值，u16 及以上跳过
        let rows = type_interpretations(&[0xFF], Endian::Little);
        assert!(rows.contains(&("u8".to_string(), "255".to_string())));
        assert!(rows.contains(&("i8".to_string(), "-1".to_string())));
        assert!(!rows.iter().any(|(l, _)| l.starts_with("u16")));
    }

    #[test]
    fn type_interpretations_byte_rows_show_up_to_16_values() {
        let u8_row = |rows: &[(String, String)]| -> String {
            rows.iter()
                .find(|(l, _)| l == "u8")
                .map(|(_, v)| v.clone())
                .unwrap()
        };
        // 16 字节：u8 全给 16 个值，不省略
        let sixteen = type_interpretations(&[1u8; 16], Endian::Little);
        assert_eq!(u8_row(&sixteen).split("; ").count(), 16);
        assert!(!u8_row(&sixteen).contains('…'));

        // 21 字节：u8 只给前 16 个，末尾以省略号收尾
        let long = type_interpretations(&[1u8; 21], Endian::Little);
        let row = u8_row(&long);
        assert_eq!(row.split("; ").count(), 17);
        assert!(row.ends_with('…'));
    }

    #[test]
    fn type_interpretations_four_and_eight_bytes() {
        // 4 字节：u8 四个值、u16 两个值，u32/i32 与 f32 各一个，8 字节类型跳过
        let le = type_interpretations(&[0x00, 0x00, 0x80, 0x3F], Endian::Little);
        assert!(le.contains(&("u8".to_string(), "0; 0; 128; 63".to_string())));
        // 第二块 0x3F80 = 16256（千位分隔开关影响具体文本，只看前缀）
        assert!(
            le.iter()
                .any(|(l, v)| l == "u16 LE" && v.starts_with("0; 16"))
        );
        assert!(le.contains(&("f32 LE".to_string(), "1".to_string())));
        assert!(le.iter().any(|(l, _)| l == "u32 LE"));
        assert!(!le.iter().any(|(l, _)| l.starts_with("u64")));

        // 8 字节：u16 四个值、u32 两个值，u64/i64 与 f64 各一个
        let eight = type_interpretations(&[0u8; 8], Endian::Little);
        assert!(eight.contains(&("u16 LE".to_string(), "0; 0; 0; 0".to_string())));
        assert!(eight.contains(&("u32 LE".to_string(), "0; 0".to_string())));
        assert!(eight.iter().any(|(l, _)| l == "u64 LE"));
        assert!(eight.iter().any(|(l, _)| l == "f64 LE"));

        // 可打印文本：ASCII 与 UTF-8 原样输出
        let text = type_interpretations(b"AB", Endian::Little);
        assert!(text.contains(&("ASCII".to_string(), "AB".to_string())));
        assert!(text.contains(&("utf8".to_string(), "AB".to_string())));
    }

    #[test]
    fn type_interpretations_include_identifiers_and_time() {
        // 4 字节：IPv4 与 Unix 秒
        let four = type_interpretations(&[1, 2, 3, 4], Endian::Little);
        assert!(four.contains(&("ip".to_string(), "1.2.3.4".to_string())));
        assert!(four.iter().any(|(l, _)| l == "time"));

        // 6 字节：MAC
        let six = type_interpretations(&[0x00, 0x1A, 0x2B, 0x3C, 0x4D, 0x5E], Endian::Little);
        assert!(six.contains(&("mac".to_string(), "00:1A:2B:3C:4D:5E".to_string())));

        // 16 字节：UUID 与 IPv6
        let sixteen = type_interpretations(&[0u8; 16], Endian::Little);
        assert!(sixteen.contains(&(
            "uuid".to_string(),
            "00000000-0000-0000-0000-000000000000".to_string()
        )));
        assert!(sixteen.contains(&("ip".to_string(), "::".to_string())));

        // 8 字节：时间（Unix 秒/毫秒或 FILETIME）
        let eight = type_interpretations(&[0u8; 8], Endian::Little);
        assert!(eight.iter().any(|(l, _)| l == "time"));

        // 3 字节：u8 三个值、u16 两块（末块补零）、更宽类型只按 4 字节补零解释，
        // 标识类仍需精确长度（不出现）
        let three = type_interpretations(&[1, 2, 3], Endian::Little);
        let note = crate::i18n::tr().padded;
        assert!(three.contains(&("u8".to_string(), "1; 2; 3".to_string())));
        assert!(
            three
                .iter()
                .any(|(l, v)| l == "u16 LE" && v.starts_with("513; 3") && v.ends_with(note))
        );
        assert!(three.iter().any(|(l, _)| l == "u32 LE"));
        assert!(!three.iter().any(|(l, _)| l.starts_with("u64")));
        assert!(
            !three
                .iter()
                .any(|(l, _)| matches!(l.as_str(), "ip" | "mac" | "time"))
        );
    }

    #[test]
    fn type_interpretations_pad_short_selection_to_type_width() {
        let note = crate::i18n::tr().padded;

        // 3 字节 → 补零成 4 字节（小端补尾部、大端补头部）
        let le = type_interpretations(&[0, 0, 0], Endian::Little);
        let be = type_interpretations(&[0, 0, 0], Endian::Big);
        assert!(
            le.iter()
                .any(|(l, v)| l == "u32 LE" && v.starts_with('0') && v.contains(note))
        );
        assert!(
            be.iter()
                .any(|(l, v)| l == "u32 BE" && v.starts_with('0') && v.contains(note))
        );

        // 5 字节 → 补零成 8 字节
        let five = type_interpretations(&[0u8; 5], Endian::Little);
        assert!(
            five.iter()
                .any(|(l, v)| l == "u64 LE" && v.starts_with('0') && v.contains(note))
        );

        // 选区恰好等于类型宽度时不标注补零
        let full = type_interpretations(&[0u8; 4], Endian::Little);
        assert!(!full.iter().any(|(_, v)| v.contains(note)));
    }

    #[test]
    fn hit_accounts_for_compact_mid_gap() {
        let g = geom(8, true, false);
        assert_eq!(g.hit(egui::pos2(100.0, 5.0), 8, 8), None);
        assert_eq!(g.hit(egui::pos2(111.0, 5.0), 8, 8), Some((0, 4)));
    }

    #[test]
    fn edge_scroll_accelerates_with_pointer_distance_and_is_capped() {
        let near = edge_scroll_speed_fraction(28.0, 28.0);
        let farther = edge_scroll_speed_fraction(56.0, 28.0);
        let farthest = edge_scroll_speed_fraction(112.0, 28.0);
        assert!(near > 0.0);
        assert!(farther > near);
        assert!(farthest > farther);
        assert_eq!(farthest, 1.0);
        assert_eq!(edge_scroll_speed_fraction(200.0, 28.0), 1.0);
    }
}
