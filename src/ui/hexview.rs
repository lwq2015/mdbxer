// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 交互式十六进制视图（Notepad++ Hex-Editor 风格）。
//!
//! 自绘实现（非 TextEdit），支持：
//! - 悬停整行高亮（跨 地址/HEX/ASCII 三列）；
//! - 悬停单个字节时 HEX 与 ASCII 两侧联动高亮；
//! - 鼠标拖拽按字节选择，HEX 与 ASCII 选区联动、可跨多行；
//! - Ctrl+C 同时复制所选字节的十六进制与 ASCII 原文（按显示行对齐），Esc 清除选区。
//!
//! 大段数据（可能数千行）只绘制与命中测试与裁剪区相交的行。

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::fmt::{addr_chars, addr_digits, hex_line, hex_section_chars, mid_gap_index};

pub(crate) const EDGE_SCROLL_MAX_ROWS_PER_SECOND: f32 = 60.0;
const EDGE_SCROLL_MAX_DEPTH: f32 = 4.0;

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

/// Key/Value 两个 hex 视图的全局稳定组件 id（不依赖 ui 层级），
/// 便于全局快捷键判断焦点是否在 hex 视图内。
pub(crate) fn view_id(is_key: bool) -> egui::Id {
    egui::Id::new(("mdbxer_hex_view", is_key))
}

/// 交互式 hex 视图。
///
/// - `id`：组件唯一 id（Key/Value 卡片各一）；
/// - `base_offset`：本段在原始字节中的起始偏移（地址列用）；
/// - `sel`：当前选区（字节全局序号，端点顺序无关）；内容切换由调用方负责清空。
/// - `hit`：内容搜索命中区间（字节全局序号 start, len），琥珀色高亮；与选区重叠时让位于选区。
/// - `scroll_to`：需要滚动到可见的命中字节（全局偏移，卡片搜索命中后由调用方一次性传入）。
pub(crate) fn hex_view(
    ui: &mut egui::Ui,
    id: egui::Id,
    bytes: &[u8],
    width: usize,
    show_addr: bool,
    show_hex: bool,
    show_ascii: bool,
    compact: bool,
    base_offset: usize,
    sel: &mut Option<(usize, usize)>,
    hit: Option<(usize, usize)>,
    scroll_to: Option<usize>,
) -> egui::Response {
    let n = width.max(1);
    let rows = bytes.len().div_ceil(n).max(1);
    let font_id = egui::TextStyle::Monospace.resolve(ui.style());
    let (cw, rh) = ui.fonts_mut(|f| (f.glyph_width(&font_id, '0'), f.row_height(&font_id)));

    // 地址位数随数据大小变化（同一 dump 内所有行统一位数保证对齐）
    let digits = addr_digits(base_offset, bytes.len());
    let addr_w = if show_addr {
        addr_chars(digits) as f32 * cw
    } else {
        0.0
    };
    let hex_w = if show_hex {
        hex_section_chars(n, compact) as f32 * cw
    } else {
        0.0
    };
    let ascii_w = if show_ascii { n as f32 * cw } else { 0.0 };
    let content_w = addr_w + hex_w + ascii_w;

    let total_h = rows as f32 * rh;
    let wanted = egui::vec2(ui.available_width().max(content_w), total_h);
    // 显式绑定固定 id：request_focus / 焦点判断 / 临时数据全部指向同一 id
    let (_space_id, rect) = ui.allocate_space(wanted);
    let resp = ui.interact(rect, id, egui::Sense::click_and_drag());

    let geom = Geom {
        x0: rect.left(),
        top: rect.top(),
        cw,
        rh,
        // 每字节在 HEX 列占的字符数（紧凑 2 / 常规 3），数字两位恒为 2
        per: if compact { 2.0 } else { 3.0 },
        hex_x: rect.left() + addr_w,
        hex_w,
        ascii_x: rect.left() + addr_w + hex_w,
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
        // 单击（没拖动）不保留选区，等同放置光标
        if let Some(Drag { moved: false, .. }) =
            ui.ctx().data(|d| d.get_temp::<Option<Drag>>(id)).flatten()
        {
            *sel = None;
        }
        ui.ctx().data_mut(|d| d.remove_temp::<Option<Drag>>(id));
    }
    // 右键：有选区时复制并取消选择（右键 = 复制 + 取消所选）；
    // 单击空白/Esc 仍取消选择
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
            }
        }
        if escape_pressed && resp.lost_focus() {
            *sel = None;
        }
    }

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
    let hit_bg = egui::Color32::from_rgba_unmultiplied(0xF5, 0xA6, 0x23, 0x66);

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
    let hit_norm = hit.and_then(|(s, l)| {
        let g_hi = s + l;
        let seg_end = base_offset + bytes.len();
        if g_hi <= base_offset || s >= seg_end {
            return None;
        }
        let lo = s.saturating_sub(base_offset);
        let hi = g_hi.min(seg_end) - base_offset - 1;
        Some((lo, hi))
    });

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

        let chunk_len = (bytes.len() - r * n).min(n);
        for i in 0..chunk_len {
            let g = r * n + i;
            let selected = sel_norm.is_some_and(|(lo, hi)| g >= lo && g <= hi);
            let hit_now = hit_norm.is_some_and(|(lo, hi)| g >= lo && g <= hi);
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
            } else if hit_now {
                if show_hex {
                    painter.rect_filled(geom.hex_cell(n, r, i).shrink2(v_inset), 1.5, hit_bg);
                }
                if show_ascii {
                    painter.rect_filled(geom.ascii_cell(r, i).shrink2(v_inset), 1.5, hit_bg);
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
        painter.galley(egui::pos2(rect.left(), row_top), galley, text_color);
    }
    ui.ctx().data_mut(|d| d.insert_temp(id, cache));

    // egui 在按下/拖拽期间只把"被拖拽控件"标记为 hovered（interaction.rs），指针移出后
    // 提示仍会跟随指针显示、盖住其他面板区域；仅悬停且未拖拽时才显示本提示。
    // 必须用 at_pointer 变体：本控件 allocate 的是全部内容的高度（数千像素），
    // 默认锚定控件矩形的提示会被钳制到窗口底部、横向跑到左下方。
    let resp = if resp.hovered() && !resp.dragged() {
        resp.on_hover_text_at_pointer(crate::i18n::tr().hex_hint)
    } else {
        resp
    };
    resp
}

#[cfg(test)]
mod tests {
    use super::{Geom, edge_scroll_speed_fraction};
    use crate::fmt::hex_section_chars;

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
