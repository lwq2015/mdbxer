// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 折叠视图：替代 TextEdit 的自绘只读文本渲染（TextEdit 无法隐藏部分行）。
//!
//! 逐行按语法 token 着色（复用 [`crate::ui::syntax`] 的分词与色盘，命中背景
//! 与 TextEdit 路径同色同语义），行首 gutter 画折叠三角（▼ 展开 / ▶ 折叠）；
//! 折叠后隐藏子行，行尾以弱色显示"… N 行"摘要。命中行被折叠遮盖时自动展开
//! 覆盖它的祖先折叠并滚动居中。
//!
//! 正文支持拖拽选择与复制（与 hex 视图同交互）：按下定锚点、拖动更新选区端点
//! （可跨行），右键复制所选并取消选择，Ctrl+C 复制选区文本，单击正文/Esc
//! 清除选区；折叠摘要后缀不参与选择。

use super::state::Side;
use super::text_fp;
use crate::fmt::DecodeMode;
use crate::i18n::tr;
use crate::ui::syntax::{FoldLine, JsonTok, SearchHighlights, append_line_colored, syntax_tokens};

/// 行首折叠三角区域宽度。
const TRIANGLE_GUTTER: f32 = 18.0;
/// 行号与折叠三角区域之间的留白。
const LINE_NUMBER_PADDING: f32 = 8.0;

/// 在独立滚动区中绘制文本折叠视图。
///
/// 展示语法高亮、折叠三角、可选行号和文本选区；根据内容指纹恢复折叠状态，
/// 返回本次渲染后的内容指纹与折叠行号集合，供调用方持久化。
///
/// `highlights` 包含显示文本中的匹配区间与当前命中；
/// `scroll_line` 是需要滚动到可见区的逻辑行号；
/// `stored` 和 `sel` 分别承载上次折叠状态与当前选区。
#[allow(clippy::too_many_arguments)]
pub(super) fn show(
    ui: &mut egui::Ui,
    side: Side,
    text: &str,
    lines: &[FoldLine],
    show_line_numbers: bool,
    mode: DecodeMode,
    palette: &[egui::Color32; 6],
    highlights: &SearchHighlights,
    scroll_line: Option<usize>,
    stored: &Option<(u64, Vec<usize>)>,
    sel: &mut Option<(usize, usize)>,
    max_height: f32,
    reset_scroll: bool,
) -> (u64, Vec<usize>) {
    // 与 TextEdit 路径同一 ScrollArea（id_salt 相同，滚动位置在两种视图间延续）
    let mut area = egui::ScrollArea::vertical()
        .id_salt(("detail_text_scroll", side.is_key()))
        .auto_shrink([false, true])
        .max_height(max_height);
    if reset_scroll {
        // 换行后回到顶部：滚动位置按固定 Id 持久化，不复位会沿用上一行
        area = area.vertical_scroll_offset(0.0);
    }
    area.show(ui, |ui| {
        FoldView::new(
            ui,
            side,
            text,
            lines,
            show_line_numbers,
            mode,
            palette,
            highlights,
        )
        .run(ui, stored, scroll_line, sel)
    })
    .inner
}

/// 折叠视图拖拽选区的过程状态（ctx temp，跨帧保存在卡片组件 id 下）。
#[derive(Clone, Copy)]
struct FoldDrag {
    /// 起手锚点（显示文本字节偏移）
    anchor: usize,
    /// 指针是否真的移出过锚点——区分"单击"（清选区）与"拖选"
    moved: bool,
}

/// 单个逻辑行的绘制数据与交互响应。
struct LineView {
    /// 行占用的布局矩形，也是行级鼠标交互的命中区域。
    rect: egui::Rect,
    /// 该行的点击、拖拽与右键事件状态。
    resp: egui::Response,
    /// 已排版的行内容，供绘制、字符命中和选区背景计算共用。
    galley: std::sync::Arc<egui::Galley>,
}

/// 单次折叠视图绘制期间共享的输入、样式和交互状态。
struct FoldView<'a> {
    /// 当前卡片是否为 Key，用于隔离 Key/Value 的交互 ID。
    is_key: bool,
    /// 完整显示文本，行切片与选区偏移均以其字节位置为基准。
    text: &'a str,
    /// 语法分析生成的逻辑行及其可折叠范围。
    lines: &'a [FoldLine],
    /// 是否在每个逻辑文本行前绘制行号。
    show_line_numbers: bool,
    /// 文本中的语法 token，用于逐行生成着色排版。
    toks: Vec<JsonTok>,
    /// 各语法 token 对应的主题颜色。
    palette: &'a [egui::Color32; 6],
    /// 搜索命中的显示文本字符区间。
    /// 搜索命中与当前命中的显示文本字符区间。
    highlights: &'a SearchHighlights,
    /// 正文的基础字体和前景色格式。
    base: egui::text::TextFormat,
    /// 折叠摘要使用的弱化文本格式。
    weak: egui::text::TextFormat,
    /// 与 TextEdit/HexView 对齐的单行高度。
    line_h: f32,
    /// 行首三角、行号占用的总宽度。
    gutter_width: f32,
    /// 当前卡片折叠文本选择状态的临时数据 ID。
    sel_id: egui::Id,
    /// 最近交互视图标记的共享 ID，用于分配全局快捷键。
    owner_id: egui::Id,
    /// 绘制选区字符背景时使用的半透明颜色。
    sel_bg: egui::Color32,
    /// 当前折叠的逻辑行索引集合。
    folded: std::collections::BTreeSet<usize>,
}

impl<'a> FoldView<'a> {
    /// 根据当前文本、语法和显示选项初始化一次折叠视图。
    fn new(
        ui: &mut egui::Ui,
        side: Side,
        text: &'a str,
        lines: &'a [FoldLine],
        show_line_numbers: bool,
        mode: DecodeMode,
        palette: &'a [egui::Color32; 6],
        highlights: &'a SearchHighlights,
    ) -> Self {
        let font_id = egui::TextStyle::Monospace.resolve(ui.style());
        // 行高与 Hex 视图一致（不额外加 extra_text_line_spacing），使同窗口内
        // 文本区和 hex 区行高对齐、总高度吻合，避免右栏下方多出/不足。
        let line_h = ui.fonts_mut(|f| f.row_height(&font_id));
        let gutter_width = if show_line_numbers {
            let digits = lines.len().max(1).ilog10() + 1;
            let digit_width = ui.fonts_mut(|f| f.glyph_width(&font_id, '0'));
            TRIANGLE_GUTTER + digits as f32 * digit_width + LINE_NUMBER_PADDING
        } else {
            TRIANGLE_GUTTER
        };
        // 逐行 allocate 会带入 egui 的 item_spacing.y（默认 3px），使折叠视图行距
        // 比 TextEdit/hex（单个 widget 内部排版、纯 line_h）大。归零后三条渲染路径
        // 行距统一，card_content_heights 按 line_h 的 galley 估算也与实际吻合。
        ui.spacing_mut().item_spacing.y = 0.0;
        let base = egui::text::TextFormat {
            font_id,
            color: ui.visuals().text_color(),
            line_height: Some(line_h),
            ..Default::default()
        };
        let weak = egui::text::TextFormat {
            color: ui.visuals().weak_text_color(),
            ..base.clone()
        };
        Self {
            is_key: side.is_key(),
            text,
            lines,
            show_line_numbers,
            toks: syntax_tokens(mode, text),
            palette,
            highlights,
            base,
            weak,
            line_h,
            gutter_width,
            sel_id: egui::Id::new(("mdbxer_fold_sel", side.is_key())),
            owner_id: egui::Id::new("mdbxer_sel_owner"),
            sel_bg: with_alpha(ui.visuals().selection.bg_fill, 150),
            folded: std::collections::BTreeSet::new(),
        }
    }

    /// 绘制所有未被祖先折叠隐藏的行，并处理折叠、选区、滚动和键盘事件。
    ///
    /// 返回内容指纹及更新后的折叠行索引，供外层写回卡片状态。
    fn run(
        mut self,
        ui: &mut egui::Ui,
        stored: &Option<(u64, Vec<usize>)>,
        scroll_line: Option<usize>,
        sel: &mut Option<(usize, usize)>,
    ) -> (u64, Vec<usize>) {
        let fp = text_fp(self.text);
        self.load_folded(stored, scroll_line);

        // 本帧起手时是否处于拖拽中（滚动帧端点的更新要延迟到内容滚到位）
        let drag = ui
            .ctx()
            .data(|d| d.get_temp::<Option<FoldDrag>>(self.sel_id))
            .flatten();
        let wrap_w = (ui.available_width() - self.gutter_width - 12.0).max(60.0);
        let view = ui.clip_rect();
        let content_top = ui.cursor().top();
        let dt = ui.input(|i| i.stable_dt).min(0.05);
        let mut hide_until = 0usize; // 行号 < hide_until 的行被祖先折叠遮盖，跳过渲染

        for i in 0..self.lines.len() {
            let fl = self.lines[i];
            if i < hide_until {
                continue;
            }
            let is_folded = self.folded.contains(&i);
            let lv = self.render_line(ui, i, fl, is_folded, wrap_w);
            self.handle_pointer(ui, i, fl, &lv, sel, drag);

            // 正文悬停显示文本光标（gutter/三角区域保持各自光标）
            let in_body = |p: egui::Pos2| p.x >= lv.rect.left() + self.gutter_width;
            if lv.resp.hovered() && ui.input(|i| i.pointer.hover_pos()).is_none_or(in_body) {
                ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::Text);
            }
            if lv.resp.hovered()
                && ui.input(|i| i.pointer.any_pressed())
                && ui
                    .input(|i| i.pointer.interact_pos())
                    .is_some_and(|p| p.x >= lv.rect.left() + TRIANGLE_GUTTER)
            {
                ui.ctx()
                    .data_mut(|d| d.insert_temp(self.owner_id, Some(self.sel_id)));
            }

            self.paint_selection(ui, fl, &lv, sel);
            if self.show_line_numbers {
                ui.painter().text(
                    egui::pos2(lv.rect.left() + self.gutter_width - 4.0, lv.rect.top()),
                    egui::Align2::RIGHT_TOP,
                    (i + 1).to_string(),
                    self.base.font_id.clone(),
                    with_alpha(ui.visuals().hyperlink_color, 128),
                );
            }
            ui.painter().galley(
                lv.rect.left_top() + egui::vec2(self.gutter_width, 0.0),
                lv.galley.clone(),
                self.base.color,
            );

            if fl.fold_end.is_some() {
                self.draw_triangle(ui, i, is_folded, &lv);
            }

            if scroll_line == Some(i) {
                ui.scroll_to_rect(lv.rect, Some(egui::Align::Center));
            }
            if is_folded {
                hide_until = fl.fold_end.unwrap_or(i) + 1;
            }
        }

        self.auto_scroll(ui, content_top, view, drag, dt);
        self.handle_keys(ui, sel);
        (fp, self.folded.into_iter().collect())
    }

    /// 折叠状态初始化：指纹不符即全展开；命中行祖先自动展开。
    fn load_folded(&mut self, stored: &Option<(u64, Vec<usize>)>, scroll_line: Option<usize>) {
        let fp = text_fp(self.text);
        self.folded = match stored {
            Some((f, set)) if *f == fp => set.iter().copied().collect(),
            _ => std::collections::BTreeSet::new(),
        };
        // 命中行祖先自动展开：所有行号更小且折叠区间覆盖命中行的可折叠行
        if let Some(h) = scroll_line {
            self.folded
                .retain(|&i| !(i < h && self.lines[i].fold_end.is_some_and(|fe| fe >= h)));
        }
    }

    /// 渲染一行（正文 + 折叠摘要），返回占位矩形与交互响应。
    fn render_line(
        &self,
        ui: &mut egui::Ui,
        i: usize,
        fl: FoldLine,
        is_folded: bool,
        wrap_w: f32,
    ) -> LineView {
        let t = tr();
        let line_str = &self.text[fl.start..fl.end];
        let mut job = egui::text::LayoutJob::default();
        append_line_colored(
            &mut job,
            line_str,
            &self.toks,
            fl.start,
            fl.end,
            self.base.clone(),
            self.palette,
            self.highlights,
        );
        if is_folded {
            let hidden = fl.fold_end.unwrap_or(i) - i;
            job.append(
                &format!("  {}", t.fold_summary(hidden)),
                0.0,
                self.weak.clone(),
            );
        }
        job.wrap.max_width = wrap_w;
        // 保留行尾空白：保证 glyphs 数与字符数一致，选区字符偏移累计才不错位
        job.keep_trailing_whitespace = true;
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        let row_h = galley.size().y.max(self.line_h);
        let (rect, resp) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), row_h),
            egui::Sense::click_and_drag(),
        );
        LineView { rect, resp, galley }
    }

    /// 正文拖拽选区与右键复制；drag 事件只发给起手行，跨行端点更新用全局指针状态。
    fn handle_pointer(
        &self,
        ui: &mut egui::Ui,
        i: usize,
        fl: FoldLine,
        lv: &LineView,
        sel: &mut Option<(usize, usize)>,
        drag: Option<FoldDrag>,
    ) {
        let in_body = |p: egui::Pos2| p.x >= lv.rect.left() + self.gutter_width;
        let press_pos = lv.resp.interact_pointer_pos();
        if lv.resp.double_clicked_by(egui::PointerButton::Primary) && press_pos.is_some_and(in_body)
        {
            let pointer = press_pos.unwrap();
            let local = pointer - egui::vec2(lv.rect.left() + self.gutter_width, lv.rect.top());
            let char_index = galley_char_offset(&lv.galley, local, self.line_chars(fl));
            let line = &self.text[fl.start..fl.end];
            let (start, end) = word_byte_range(line, char_index);
            *sel = Some((fl.start + start, fl.start + end));
        }

        if self.show_line_numbers
            && lv.resp.clicked_by(egui::PointerButton::Primary)
            && let Some(pointer) = press_pos
            && pointer.x >= lv.rect.left() + TRIANGLE_GUTTER
            && pointer.x < lv.rect.left() + self.gutter_width
        {
            let line_end = self
                .lines
                .get(i + 1)
                .map_or(self.text.len(), |next| next.start);
            *sel = Some((fl.start, line_end));
        }

        if lv.resp.drag_started() && press_pos.is_some_and(in_body) {
            let lp =
                press_pos.unwrap() - egui::vec2(lv.rect.left() + self.gutter_width, lv.rect.top());
            let anchor = fl.start
                + char_to_byte(
                    &self.text[fl.start..fl.end],
                    galley_char_offset(&lv.galley, lp, self.line_chars(fl)),
                );
            ui.ctx().data_mut(|d| {
                d.insert_temp(
                    self.sel_id,
                    Some(FoldDrag {
                        anchor,
                        moved: false,
                    }),
                );
                d.insert_temp(self.owner_id, Some(self.sel_id));
            });
        }

        if let Some(dg) = drag
            && ui.input(|i| i.pointer.primary_down())
            && let Some(p) = ui.input(|i| i.pointer.hover_pos())
            && lv.rect.top() <= p.y
            && p.y < lv.rect.bottom()
            && in_body(p)
        {
            let lp = p - egui::vec2(lv.rect.left() + self.gutter_width, lv.rect.top());
            let line_str = &self.text[fl.start..fl.end];
            let cur = fl.start
                + char_to_byte(
                    line_str,
                    galley_char_offset(&lv.galley, lp, self.line_chars(fl)),
                );
            let moved = dg.moved || cur != dg.anchor;
            ui.ctx().data_mut(|d| {
                d.insert_temp(
                    self.sel_id,
                    Some(FoldDrag {
                        anchor: dg.anchor,
                        moved,
                    }),
                )
            });
            if moved {
                *sel = Some((dg.anchor, cur));
            }
        }

        if lv.resp.drag_stopped() {
            // 单击正文（没拖动）不保留选区，等同放置光标
            if !lv.resp.double_clicked_by(egui::PointerButton::Primary)
                && let Some(FoldDrag { moved: false, .. }) = ui
                    .ctx()
                    .data(|d| d.get_temp::<Option<FoldDrag>>(self.sel_id))
                    .flatten()
            {
                *sel = None;
            }
            ui.ctx()
                .data_mut(|d| d.remove_temp::<Option<FoldDrag>>(self.sel_id));
        }

        if lv.resp.clicked_by(egui::PointerButton::Primary)
            && press_pos.is_some_and(in_body)
            && !lv.resp.double_clicked_by(egui::PointerButton::Primary)
            && sel.is_some()
        {
            *sel = None;
        }

        // 右键单击正文：有选区时复制并取消选择（右键 = 复制 + 取消所选）
        if lv.resp.secondary_clicked()
            && press_pos.is_some_and(in_body)
            && let Some((a, b)) = sel.take()
        {
            let (lo, hi) = (a.min(b), a.max(b));
            ui.ctx().copy_text(self.text[lo..hi].to_owned());
        }
    }

    /// 选区背景：每个视觉行画连续底色（先于文字绘制；摘要后缀不参与选择）。
    fn paint_selection(
        &self,
        ui: &egui::Ui,
        fl: FoldLine,
        lv: &LineView,
        sel: &Option<(usize, usize)>,
    ) {
        let Some((a, b)) = *sel else { return };
        let lo = a.min(b).max(fl.start);
        let hi = a.max(b).min(fl.end);
        if lo >= hi {
            return;
        }
        let line_str = &self.text[fl.start..fl.end];
        let cs = line_str[..lo - fl.start].chars().count();
        let ce = cs + line_str[lo - fl.start..hi - fl.start].chars().count();
        let origin = lv.rect.left_top() + egui::vec2(self.gutter_width, 0.0);
        let mut char_off = 0usize;
        for prow in &lv.galley.rows {
            let row_char_start = char_off;
            let row_char_end = row_char_start + prow.row.glyphs.len();
            let selected_start = cs.max(row_char_start);
            let selected_end = ce.min(row_char_end);
            if selected_start < selected_end {
                let first = &prow.row.glyphs[selected_start - row_char_start];
                let last = &prow.row.glyphs[selected_end - row_char_start - 1];
                let left = first.pos.x.min(last.pos.x);
                let right =
                    (first.pos.x + first.advance_width).max(last.pos.x + last.advance_width);
                let rect = egui::Rect::from_min_max(
                    origin + prow.pos.to_vec2() + egui::vec2(left, 0.0),
                    origin + prow.pos.to_vec2() + egui::vec2(right, prow.row.size.y),
                );
                ui.painter().rect_filled(rect, 0.0, self.sel_bg);
            }
            char_off += prow.char_count_including_newline().0;
        }
    }

    /// 行首折叠三角：▼=展开 ▶=已折叠；点击切换折叠状态。
    fn draw_triangle(&mut self, ui: &mut egui::Ui, i: usize, is_folded: bool, lv: &LineView) {
        let c = ui.visuals().weak_text_color();
        let x = lv.rect.left() + 4.0;
        let y = lv.rect.top() + (self.line_h - 8.0) * 0.5;
        let (pts, tri_rect) = if is_folded {
            (
                vec![
                    egui::pos2(x + 2.0, y),
                    egui::pos2(x + 8.0, y + 4.0),
                    egui::pos2(x + 2.0, y + 8.0),
                ],
                egui::Rect::from_min_size(egui::pos2(x, y - 2.0), egui::vec2(14.0, 12.0)),
            )
        } else {
            (
                vec![
                    egui::pos2(x, y + 2.0),
                    egui::pos2(x + 8.0, y + 2.0),
                    egui::pos2(x + 4.0, y + 8.0),
                ],
                egui::Rect::from_min_size(egui::pos2(x - 2.0, y), egui::vec2(12.0, 14.0)),
            )
        };
        let resp = ui.interact(
            tri_rect,
            egui::Id::new(("fold_tri", self.is_key, i)),
            egui::Sense::click(),
        );
        if resp.clicked() {
            if is_folded {
                self.folded.remove(&i);
            } else {
                self.folded.insert(i);
            }
        }
        resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        ui.painter()
            .add(egui::Shape::convex_polygon(pts, c, egui::Stroke::NONE));
    }

    /// 拖到视口上下边缘自动滚动：使选区可以一直延伸到当前不可见的部分
    /// （与 hex 视图同交互）。速度按指针深入边缘的距离比例递增（二次方——
    /// 贴边缓慢精修，越出边缘越远越快；上限约 60 行高/秒）；内容本身没超出
    /// 该方向则不滚。滚动帧端点不更新，下一帧内容滚到位后由行命中逻辑延伸。
    fn auto_scroll(
        &self,
        ui: &mut egui::Ui,
        content_top: f32,
        view: egui::Rect,
        drag: Option<FoldDrag>,
        dt: f32,
    ) {
        if drag.is_none() || !ui.input(|i| i.pointer.primary_down()) {
            return;
        }
        let Some(p) = ui.input(|i| i.pointer.hover_pos()) else {
            return;
        };
        let margin = 28.0;
        let max_speed = self.line_h * crate::ui::hexview::EDGE_SCROLL_MAX_ROWS_PER_SECOND;
        if content_top < view.top() - 1.0 && p.y < view.top() + margin {
            let factor =
                crate::ui::hexview::edge_scroll_speed_fraction(view.top() + margin - p.y, margin);
            // egui 约定 scroll delta 正值 = 向上看
            ui.scroll_with_delta(egui::vec2(0.0, max_speed * factor * dt));
        } else if ui.cursor().top() > view.bottom() + 1.0 && p.y > view.bottom() - margin {
            let factor = crate::ui::hexview::edge_scroll_speed_fraction(
                p.y - (view.bottom() - margin),
                margin,
            );
            ui.scroll_with_delta(egui::vec2(0.0, -max_speed * factor * dt));
        }
    }

    /// 键盘：本视图为最近选择交互目标时，Ctrl+C 复制选区文本、Esc 清除。
    /// 多行多 widget 无法共享一个焦点 id，改用"最近按下"的视图标记（与 hex
    /// 视图共用 owner id）判断快捷键归属，避免两个视图同时响应互相覆盖剪贴板。
    fn handle_keys(&self, ui: &mut egui::Ui, sel: &mut Option<(usize, usize)>) {
        let owner = ui
            .ctx()
            .data(|d| d.get_temp::<Option<egui::Id>>(self.owner_id))
            .flatten();
        if owner != Some(self.sel_id) {
            return;
        }
        let (select_all, copy, escape) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::A) && i.modifiers.command,
                i.events
                    .iter()
                    .any(|e| matches!(e, egui::Event::Copy | egui::Event::Cut)),
                i.key_pressed(egui::Key::Escape),
            )
        });
        if select_all {
            *sel = Some((0, self.text.len()));
        }
        if copy && let Some((a, b)) = *sel {
            let (lo, hi) = (a.min(b), a.max(b));
            ui.ctx().copy_text(self.text[lo..hi].to_owned());
        }
        if escape {
            *sel = None;
        }
    }

    /// 行内字符数（选区字符偏移的 clamp 上界）。
    fn line_chars(&self, fl: FoldLine) -> usize {
        self.text[fl.start..fl.end].chars().count()
    }
}

/// 主题色 → 指定 alpha 的半透明版（折叠视图选区背景，与 hex 视图同法同色）。
fn with_alpha(c: egui::Color32, a: u8) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

/// 第 `n_chars` 个字符的字节偏移（等于行字符数时为行尾）。
fn char_to_byte(s: &str, n_chars: usize) -> usize {
    s.char_indices().nth(n_chars).map_or(s.len(), |(b, _)| b)
}

fn word_byte_range(text: &str, char_index: usize) -> (usize, usize) {
    let cursor = egui::text::CCursor::new(char_index);
    let start = egui::text_selection::text_cursor_state::ccursor_previous_word(text, cursor)
        .index
        .0;
    let end = egui::text_selection::text_cursor_state::ccursor_next_word(text, cursor)
        .index
        .0;
    (char_to_byte(text, start), char_to_byte(text, end))
}

/// 指针相对行 galley 左上角的位置 → galley 全文字符偏移（逐视觉行累计，自动处理
/// 折行），再 clamp 到 `line_chars`：折叠摘要后缀不计入选区，落在摘要上视作正文行尾。
fn galley_char_offset(galley: &egui::Galley, p: egui::Pos2, line_chars: usize) -> usize {
    if galley.rows.is_empty() {
        return 0;
    }
    let idx = galley
        .rows
        .iter()
        .position(|r| p.y >= r.pos.y && p.y < r.pos.y + r.row.size.y)
        .unwrap_or(galley.rows.len() - 1);
    let mut off: usize = galley.rows[..idx]
        .iter()
        .map(|r| r.char_count_including_newline().0)
        .sum();
    let row = &galley.rows[idx];
    off += row.row.char_at(p.x - row.pos.x).0;
    off.min(line_chars)
}

#[cfg(test)]
mod tests {
    use super::word_byte_range;

    #[test]
    fn double_click_word_range_uses_utf8_byte_offsets() {
        let text = "键 needle-amber 完成";
        let range = word_byte_range(text, 5);
        assert_eq!(&text[range.0..range.1], "needle");
    }
}
