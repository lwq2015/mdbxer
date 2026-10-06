// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 交互式十六进制视图（Notepad++ Hex-Editor 风格）。
//!
//! 自绘实现（非 TextEdit），支持：
//! - 悬停整行高亮（跨 地址/HEX/ASCII 三列）；
//! - 悬停单个字节时 HEX 与 ASCII 两侧联动高亮；
//! - 鼠标拖拽按字节选择，HEX 与 ASCII 选区联动、可跨多行；
//! - Ctrl+C 复制所选字节的十六进制（空格分隔），Esc 清除选区。
//!
//! 大段数据（可能数千行）只绘制与命中测试与裁剪区相交的行。

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::fmt::{ADDR_CHARS, hex_line, mid_gap_index};

/// Galley 缓存：内容指纹变化时整体作废。
#[derive(Default, Clone)]
struct GalleyCache {
    tag: u64,
    rows: HashMap<usize, Arc<egui::Galley>>,
}

/// 拖拽过程状态（锚点字节序号 + 是否实际移动过 + 起手列）。
#[derive(Clone, Copy)]
struct Drag {
    anchor: usize,
    moved: bool,
    col: DragCol,
}

/// 拖拽起手列：决定 Ctrl+C 复制十六进制还是 ASCII 原文。
#[derive(Clone, Copy, Default, PartialEq)]
enum DragCol {
    #[default]
    Hex,
    Ascii,
}

/// 渲染几何（点坐标）。
struct Geom {
    x0: f32,
    top: f32,
    cw: f32,
    rh: f32,
    hex_x: f32,
    hex_w: f32,
    ascii_x: f32,
    ascii_w: f32,
}

impl Geom {
    /// 命中测试：返回（行号, 行内字节序号, 起手列），落在地址列/空隙/补齐区返回 None。
    fn hit(&self, p: egui::Pos2, n: usize, chunk_len: usize) -> Option<(usize, usize, DragCol)> {
        if p.x < self.x0 || p.y < self.top {
            return None;
        }
        let row = ((p.y - self.top) / self.rh).floor() as isize;
        if row < 0 {
            return None;
        }
        let row = row as usize;
        if row == usize::MAX || chunk_len == 0 {
            return None;
        }

        // HEX 列优先（与 ASCII 列在布局上不重叠）
        if self.hex_w > 0.0 && p.x >= self.hex_x && p.x < self.hex_x + self.hex_w {
            let mut raw = ((p.x - self.hex_x) / self.cw).floor() as isize;
            if let Some(mid) = mid_gap_index(n) {
                // mid 处是额外空隙；其后的字符坐标整体后移 1
                if raw == (3 * mid) as isize {
                    return None;
                }
                if raw > (3 * mid) as isize {
                    raw -= 1;
                }
            }
            if raw < 0 {
                return None;
            }
            let (bi, frac) = (raw as usize / 3, raw as usize % 3);
            // 每字节只有前两个字符（两个 hex 数字）可点，第 3 个是空格
            (bi < chunk_len && frac < 2).then_some((row, bi, DragCol::Hex))
        } else if self.ascii_w > 0.0 && p.x >= self.ascii_x && p.x < self.ascii_x + self.ascii_w {
            let bi = ((p.x - self.ascii_x) / self.cw).floor() as isize;
            (bi >= 0 && (bi as usize) < chunk_len).then_some((row, bi as usize, DragCol::Ascii))
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
                self.hex_x + (3 * i) as f32 * self.cw + gap,
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
pub(crate) fn hex_view(
    ui: &mut egui::Ui,
    id: egui::Id,
    bytes: &[u8],
    width: usize,
    show_addr: bool,
    show_hex: bool,
    show_ascii: bool,
    base_offset: usize,
    sel: &mut Option<(usize, usize)>,
) -> egui::Response {
    let n = width.max(1);
    let rows = bytes.len().div_ceil(n).max(1);
    let font_id = egui::TextStyle::Monospace.resolve(ui.style());
    let (cw, rh) = ui.fonts_mut(|f| (f.glyph_width(&font_id, '0'), f.row_height(&font_id)));

    let addr_w = if show_addr {
        ADDR_CHARS as f32 * cw
    } else {
        0.0
    };
    let hex_w = if show_hex {
        crate::fmt::hex_section_chars(n) as f32 * cw
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
        hex_x: rect.left() + addr_w,
        hex_w,
        ascii_x: rect.left() + addr_w + hex_w,
        ascii_w,
    };

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
            ui.visuals().dark_mode,
            ui.ctx().pixels_per_point().to_bits(),
        )
            .hash(&mut h);
        bytes.hash(&mut h);
        h.finish()
    };
    let text_color = ui.visuals().widgets.noninteractive.fg_stroke.color;

    let hit_at = |p: egui::Pos2| -> Option<(usize, usize, DragCol)> {
        let (r, i, col) = geom.hit(p, n, n)?;
        if r >= rows {
            return None;
        }
        let chunk_len = (bytes.len() - r * n).min(n);
        if i >= chunk_len {
            return None;
        }
        Some((r, i, col))
    };

    // ── 指针交互：按下定锚点、拖拽更新选区（HEX/ASCII 高亮联动，
    //    但复制格式由"起手列"决定：HEX 起手复制十六进制，ASCII 起手复制原文）──
    let col_id = id.with("copy_col");
    let pressed = resp.drag_started();
    if pressed {
        resp.request_focus();
        if let Some(p) = resp.interact_pointer_pos() {
            match hit_at(p) {
                Some((r, i, col)) => {
                    ui.ctx().data_mut(|d| {
                        d.insert_temp(col_id, col);
                        d.insert_temp(
                            id,
                            Some(Drag {
                                anchor: r * n + i,
                                moved: false,
                                col,
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
        if let (Some(drag), Some(p)) = (anchor, resp.interact_pointer_pos()) {
            if let Some((r, i, _)) = hit_at(p) {
                let cur = r * n + i;
                let moved = drag.moved || cur != drag.anchor;
                ui.ctx().data_mut(|d| {
                    d.insert_temp(
                        id,
                        Some(Drag {
                            anchor: drag.anchor,
                            moved,
                            col: drag.col,
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
    // 右键一律清除选区（Esc 的兜底：egui begin_pass 会先于帧逻辑清掉焦点）
    if resp.secondary_clicked() {
        *sel = None;
        ui.ctx().data_mut(|d| d.remove_temp::<DragCol>(col_id));
    }

    // ── 键盘：聚焦时 Ctrl+C 按起手列复制（HEX=按显示行换行的十六进制，
    //    ASCII=字节原文连续文本），Esc 清除选区 ──
    // 注意：egui 在每帧 begin_pass 处理原始事件时，一旦发现无修饰 Esc 会立即
    // 清空 focused_widget（memory/mod.rs Focus::begin_pass），所以这一帧
    // focused() 已经是 None。要用 resp.lost_focus()（比较上一帧焦点）+
    // key_pressed(Escape) 才能识别"焦点在本组件时按下了 Esc"。
    let escape_pressed = ui.input(|i| i.key_pressed(egui::Key::Escape));
    let focused = ui.ctx().memory(|m| m.focused() == Some(id));
    if focused || (escape_pressed && resp.lost_focus()) {
        let copy = ui.input(|i| {
            i.events
                .iter()
                .any(|e| matches!(e, egui::Event::Copy | egui::Event::Cut))
        });
        if copy {
            if let Some((a, b)) = *sel {
                let (lo, hi) = (a.min(b), a.max(b).min(bytes.len() - 1));
                let chunk = &bytes[lo..=hi];
                let col = ui
                    .ctx()
                    .data(|d| d.get_temp::<DragCol>(col_id))
                    .unwrap_or_default();
                let text = match col {
                    DragCol::Hex => crate::fmt::hex_copy_selection(bytes, lo, hi, n),
                    DragCol::Ascii => {
                        // 忠实于字节：连续拼接，不凭空插入换行；非 UTF-8 用替换字符
                        String::from_utf8_lossy(chunk).into_owned()
                    }
                };
                ui.ctx().copy_text(text);
            }
        }
        if escape_pressed && resp.lost_focus() {
            *sel = None;
            ui.ctx().data_mut(|d| d.remove_temp::<DragCol>(col_id));
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

    // 悬停字节（未拖拽时用 hover 位置；拖拽中用指针位置联动）
    let pointer = if resp.dragged() {
        resp.interact_pointer_pos()
    } else {
        resp.hover_pos()
    };
    let hover = pointer.and_then(hit_at);
    if hover.is_some() {
        ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::Text);
    }

    let sel_norm = sel.map(|(a, b)| (a.min(b), a.max(b)));

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

    for r in vis_first..vis_last {
        let row_top = rect.top() + r as f32 * rh;
        let row_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left(), row_top),
            egui::vec2(rect.width(), rh),
        );

        // 1) 整行悬停背景
        if hover.is_some_and(|(hr, _, _)| hr == r) {
            painter.rect_filled(row_rect, 0.0, row_bg);
        }

        let chunk_len = (bytes.len() - r * n).min(n);
        for i in 0..chunk_len {
            let g = r * n + i;
            let selected = sel_norm.is_some_and(|(lo, hi)| g >= lo && g <= hi);
            let hovered = hover.is_some_and(|(hr, hi, _)| hr == r && hi == i);

            // 2) 选区 / 悬停字节背景（HEX 与 ASCII 联动）
            let v_inset = egui::vec2(0.0, 1.0);
            if selected {
                if show_hex {
                    painter.rect_filled(geom.hex_cell(n, r, i).shrink2(v_inset), 1.5, sel_bg);
                }
                if show_ascii {
                    painter.rect_filled(geom.ascii_cell(r, i).shrink2(v_inset), 1.5, sel_bg);
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

        // 3) 文本（最后画，压在背景之上）
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
                );
                ui.fonts_mut(|f| f.layout_no_wrap(line, font_id.clone(), text_color))
            })
            .clone();
        painter.galley(egui::pos2(rect.left(), row_top), galley, text_color);
    }
    ui.ctx().data_mut(|d| d.insert_temp(id, cache));

    resp.on_hover_text(crate::i18n::tr().hex_hint)
}
