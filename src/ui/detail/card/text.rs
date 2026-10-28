// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 卡片文本区：显示决议、折叠视图 / TextEdit 两条渲染路径、命中滚动定位。

use super::super::fold;
use super::super::layout;
use super::super::plan::plan_display;
use super::CardCtx;
use crate::fmt::{self, DecodeMode};
use crate::i18n::tr;
use crate::ui::MdbxerApp;
use crate::ui::detail::state::Side;
use crate::ui::detail::text_fp;
use crate::ui::syntax::{
    FoldLine, SearchHighlights, append_colored, fold_lines, json_palette, syntax_tokens,
};

/// 渲染文本区（含折叠头、猜测行、内容滚动）。
pub(super) fn show(ui: &mut egui::Ui, app: &mut MdbxerApp, ctx: &CardCtx<'_>) {
    let t = tr();
    let plan = plan_display(
        ctx.mode,
        ctx.side,
        ctx.window,
        ctx.text,
        app.endian,
        &app.hint_value,
    );
    let display_text = plan.text;
    let syntax_kind = plan.syntax;

    // 行高与 Hex 视图一致（不加 extra_text_line_spacing）：与 fold_view_body /
    // TextEdit layouter 的实测行高保持一致。
    let line_h = mono_line_height(ui);
    let budget = layout::HeightBudget {
        line_h,
        content_budget: (ctx.budget - ctx.fixed).max(80.0),
        text_open: ctx.text_open,
        hex_open: ctx.hex_open,
    };
    let every_display_row = layout::number_text_display_rows(app, ctx.side, ctx.bytes);
    let (text_h, _) = layout::card_content_heights(
        ui,
        app,
        ctx.window,
        &display_text,
        &budget,
        every_display_row,
    );

    let (guess_text, binary_guess) = guess_text(app, ctx);
    let scroll_to = scroll_target(app, ctx);
    let highlights = hit_ranges(
        app,
        ctx,
        &display_text,
        binary_guess,
        syntax_kind,
        app.detail.highlight_all_matches,
    );
    let lines = fold_line_list(syntax_kind, &display_text);
    let fold_active = lines.iter().any(|f| f.fold_end.is_some());
    let show_line_numbers = app.detail.show_text_line_numbers;
    let scroll_line = fold_scroll_line(
        fold_active,
        scroll_to,
        highlights.active,
        ctx,
        &display_text,
        &lines,
    );

    let card = match ctx.side {
        Side::Key => &app.detail.key,
        Side::Value => &app.detail.val,
    };
    let stored = card.fold.clone();
    let sel_fp = text_fp(&display_text);
    let mut sel = match card.fold_sel {
        Some((f, Some(r))) if f == sel_fp => Some(r),
        _ => None,
    };
    let mut fold_writeback: Option<(u64, Vec<usize>)> = None;

    egui::collapsing_header::CollapsingState::load_with_default_open(
        ui.ctx(),
        egui::Id::new(("detail_text", ctx.side.is_key(), true)),
        true,
    )
    .show_header(ui, |ui| {
        // 与 CollapsingHeader 默认一致：标题用 Button 样式文本
        ui.add(
            egui::Label::new(
                egui::RichText::new(t.section_text).text_style(egui::TextStyle::Button),
            )
            .selectable(false),
        );
        ui.weak(guess_text);
    })
    .body(|ui| {
        if fold_active {
            // 折叠视图：自绘逐行渲染 + 行首折叠三角（替代 TextEdit）。
            // 与 TextEdit 路径同一 ScrollArea 约束（id_salt 相同，滚动位置
            // 在两种视图间延续）；返回渲染后的折叠状态（含点击切换与命中
            // 自动展开祖先），借 fold_writeback 带出闭包、结束后写回。
            let palette = json_palette(ui.visuals().dark_mode);
            fold_writeback = Some(fold::show(
                ui,
                ctx.side,
                &display_text,
                &lines,
                show_line_numbers,
                syntax_kind.unwrap_or(DecodeMode::Json),
                &palette,
                &highlights,
                scroll_line,
                &stored,
                &mut sel,
                text_h,
            ));
            return;
        }
        // display_text 已按当前模式算好（Auto 命中结构化格式时为 pretty，
        // 其余为原文）；高度估算也用它，二者一致。
        text_edit_body(
            ui,
            ctx,
            &display_text,
            syntax_kind,
            &highlights,
            scroll_to,
            text_h,
            line_h,
            show_line_numbers,
            every_display_row,
        );
    });

    // 折叠状态写回：渲染中可能因点击折叠三角或命中自动展开祖先而变化；
    // 内容指纹随状态一起存，换值/换段/换模式后指纹不符即整体重置为全展开
    if let Some(wb) = fold_writeback {
        let card = match ctx.side {
            Side::Key => &mut app.detail.key,
            Side::Value => &mut app.detail.val,
        };
        card.fold = Some(wb);
        // 折叠视图本帧渲染过：选区状态同步写回（无选区时存 None）
        card.fold_sel = Some((sel_fp, sel));
    }
}

/// 文本区标题右侧的猜测/字节数信息。
fn guess_text(app: &MdbxerApp, ctx: &CardCtx<'_>) -> (String, bool) {
    let t = tr();
    if ctx.mode == DecodeMode::Auto {
        let (label, _) = fmt::guess(ctx.bytes, app.endian, &app.hint_value);
        let binary = label == t.guess_binary;
        // 附加解读只保留文件魔数（强信号、短标注）：地址/FILETIME 对
        // 任意字节几乎总命中，放猜测行是噪音且撑宽窗口
        let text = match fmt::magic_hint(ctx.bytes) {
            Some(h) => t.guess_line(&format!("{label} · {h}"), ctx.bytes.len()),
            None => t.guess_line(&label, ctx.bytes.len()),
        };
        (text, binary)
    } else {
        (t.bytes(ctx.bytes.len()), false)
    }
}

/// 搜索命中联动：文本区也滚到命中附近。hex 展开时只看不取（留给 hex 区消费），
/// hex 折叠时由文本区消费，避免滚动目标滞留导致每帧抢滚动。
fn scroll_target(app: &mut MdbxerApp, ctx: &CardCtx<'_>) -> Option<usize> {
    if ctx.hex_open {
        match ctx.side {
            Side::Key => &app.detail.key,
            Side::Value => &app.detail.val,
        }
        .blob_scroll
    } else {
        app.detail.blob_scroll_take(ctx.side)
    }
}

/// 将当前段内的搜索命中映射到显示文本坐标；pretty JSON 按原始出现序号映射。
fn hit_ranges(
    app: &MdbxerApp,
    ctx: &CardCtx<'_>,
    display_text: &str,
    binary_guess: bool,
    syntax_kind: Option<DecodeMode>,
    highlight_all: bool,
) -> SearchHighlights {
    let card = match ctx.side {
        Side::Key => &app.detail.key,
        Side::Value => &app.detail.val,
    };
    let active = card.blob_hit.and_then(|(hit_abs, hit_len)| {
        if hit_abs < ctx.off || hit_abs >= ctx.off + ctx.window.len() {
            return None;
        }
        map_hit_range(
            ctx,
            display_text,
            binary_guess,
            syntax_kind,
            hit_abs - ctx.off,
            hit_len,
        )
    });
    let hit_query = card.blob_query.as_deref();
    if !highlight_all {
        return SearchHighlights {
            ranges: active.into_iter().collect(),
            active,
        };
    }

    let Some(query) = hit_query else {
        return SearchHighlights {
            ranges: active.into_iter().collect(),
            active,
        };
    };
    if query.is_empty() || query.len() > ctx.window.len() {
        return SearchHighlights {
            ranges: active.into_iter().collect(),
            active,
        };
    }

    let raw_hits = super::super::search::find_all_hits(ctx.window, query, ctx.off);
    let mut hits = Vec::new();
    for (abs_start, byte_len) in raw_hits {
        if let Some(range) = map_hit_range(
            ctx,
            display_text,
            binary_guess,
            syntax_kind,
            abs_start - ctx.off,
            byte_len,
        ) {
            hits.push(range);
        }
    }
    if let Some(active) = active
        && !hits.contains(&active)
    {
        hits.push(active);
    }
    SearchHighlights {
        ranges: hits,
        active,
    }
}

fn map_hit_range(
    ctx: &CardCtx<'_>,
    display_text: &str,
    binary_guess: bool,
    syntax_kind: Option<DecodeMode>,
    byte_start: usize,
    byte_len: usize,
) -> Option<(usize, usize)> {
    let byte_format = match ctx.mode {
        DecodeMode::Hex => Some(ByteFormat::Hex),
        DecodeMode::Binary => Some(ByteFormat::Binary),
        DecodeMode::Dec => Some(ByteFormat::Decimal),
        DecodeMode::Auto if binary_guess => Some(ByteFormat::Hex),
        _ => None,
    };
    if let Some(format) = byte_format {
        return byte_range_in_byte_text(display_text, ctx.window, byte_start, byte_len, format);
    }
    if display_text.as_bytes() != ctx.window {
        if syntax_kind == Some(DecodeMode::Json) {
            let end = byte_start.checked_add(byte_len)?;
            return remapped_text_hit_range(ctx.window, byte_start, end, display_text);
        }
        return None;
    }
    let mut hs = byte_start;
    let mut he = hs.checked_add(byte_len)?.min(display_text.len());
    while hs > 0 && !display_text.is_char_boundary(hs) {
        hs -= 1;
    }
    while he < display_text.len() && !display_text.is_char_boundary(he) {
        he += 1;
    }
    (hs < he).then_some((hs, he))
}

/// 把压缩 JSON 中的 UTF-8 子串命中映射到 pretty JSON 中相同子串的对应出现位置。
fn remapped_text_hit_range(
    source: &[u8],
    hit_start: usize,
    hit_end: usize,
    display_text: &str,
) -> Option<(usize, usize)> {
    let matched = std::str::from_utf8(source.get(hit_start..hit_end)?).ok()?;
    if matched.is_empty() {
        return None;
    }

    let mut occurrence = 0;
    let mut scan = 0;
    while scan < hit_start {
        let Some(relative) = source[scan..]
            .windows(matched.len())
            .position(|window| window == matched.as_bytes())
        else {
            break;
        };
        let start = scan + relative;
        if start >= hit_start {
            break;
        }
        occurrence += 1;
        scan = start + matched.len();
    }

    display_text
        .match_indices(matched)
        .nth(occurrence)
        .map(|(start, text)| (start, start + text.len()))
}

#[derive(Clone, Copy)]
enum ByteFormat {
    Hex,
    Binary,
    Decimal,
}

/// 把段内字节区间映射到按字节分隔的 HEX/Binary/Dec 文本字符区间。
fn byte_range_in_byte_text(
    display_text: &str,
    bytes: &[u8],
    byte_start: usize,
    byte_count: usize,
    format: ByteFormat,
) -> Option<(usize, usize)> {
    if byte_count == 0 || byte_start >= bytes.len() {
        return None;
    }
    let byte_end = byte_start.saturating_add(byte_count).min(bytes.len());
    let token_width = |byte: u8| match format {
        ByteFormat::Hex => 2,
        ByteFormat::Binary => 8,
        ByteFormat::Decimal => byte.to_string().len(),
    };
    let char_offset = |end: usize| {
        bytes[..end]
            .iter()
            .map(|&byte| token_width(byte))
            .sum::<usize>()
            .saturating_add(end.saturating_sub(1))
    };
    let start = bytes[..byte_start]
        .iter()
        .map(|&byte| token_width(byte))
        .sum::<usize>()
        .saturating_add(byte_start);
    let end = char_offset(byte_end);
    let text_len = display_text.len();
    if start >= text_len {
        return None;
    }
    let end = end.min(text_len);
    (start < end).then_some((start, end))
}

/// 折叠视图的行切分：语法着色激活且存在多行文本时才切；
/// 单行紧凑 JSON、纯文本、无块的多行文本不折叠，仍走 TextEdit 原路径。
fn fold_line_list(syntax: Option<DecodeMode>, text: &str) -> Vec<FoldLine> {
    match syntax {
        Some(_) if text.contains('\n') => fold_lines(text),
        _ => Vec::new(),
    }
}

/// 折叠视图滚动优先使用命中高亮映射后的显示文本偏移；无法映射时，
/// 再按窗口字节偏移估算。命中行被祖先折叠遮盖时会自动展开祖先。
fn fold_scroll_line(
    fold_active: bool,
    scroll_to: Option<usize>,
    active_hit: Option<(usize, usize)>,
    ctx: &CardCtx<'_>,
    display_text: &str,
    lines: &[FoldLine],
) -> Option<usize> {
    if !fold_active {
        return None;
    }
    let hit_abs = scroll_to?;
    if hit_abs < ctx.off || hit_abs >= ctx.off + ctx.window.len() {
        return None;
    }
    let hit_rel = hit_abs - ctx.off;
    let estimated =
        if display_text.len() == ctx.window.len() && std::str::from_utf8(ctx.window).is_ok() {
            hit_rel
        } else {
            display_text.len() * hit_rel / ctx.window.len().max(1)
        };
    let display_offset = active_hit
        .filter(|(start, end)| start < end && *end <= display_text.len())
        .map_or(estimated, |(start, _)| start);
    fold_line_for_display_offset(display_text, lines, display_offset)
}

fn fold_line_for_display_offset(
    display_text: &str,
    lines: &[FoldLine],
    mut display_offset: usize,
) -> Option<usize> {
    if lines.is_empty() {
        return None;
    }
    display_offset = display_offset.min(display_text.len());
    while display_offset > 0 && !display_text.is_char_boundary(display_offset) {
        display_offset -= 1;
    }
    Some(
        lines
            .partition_point(|line| line.start <= display_offset)
            .saturating_sub(1),
    )
}

/// TextEdit 渲染路径（无可折叠块时）：自定义 layouter 负责语法着色/命中高亮；
/// 命中滚动定位直接使用其实际 galley。
#[allow(clippy::too_many_arguments)]
fn text_edit_body(
    ui: &mut egui::Ui,
    ctx: &CardCtx<'_>,
    display_text: &str,
    syntax_kind: Option<DecodeMode>,
    highlights: &SearchHighlights,
    scroll_to: Option<usize>,
    text_h: f32,
    line_h: f32,
    show_line_numbers: bool,
    every_display_row: bool,
) {
    // TextEdit 自身无内部滚动、高度随内容无限增长，故必须包一层 ScrollArea；
    // + 12 ≈ TextEdit frame 上下内边距，保证整数行内容恰好不滚
    let mut text_owned = display_text.to_string();
    let palette = json_palette(ui.visuals().dark_mode);
    egui::ScrollArea::vertical()
        .id_salt(("detail_text_scroll", ctx.side.is_key()))
        .auto_shrink([false, true])
        .max_height(text_h)
        .show(ui, |ui| {
            // TextEdit 与命中高亮共用同一 layouter；滚动定位直接读取 TextEdit
            // 实际生成的 galley，避免重新估算折行。
            let te = egui::TextEdit::multiline(&mut text_owned)
                .font(egui::TextStyle::Monospace)
                .desired_width(f32::INFINITY)
                .desired_rows(1);
            // 无条件走自定义 layouter：即使无高亮/无语法，也要让滚动定位拿到
            // TextEdit 实际生成的 galley。无高亮时其行为与 egui 默认 multiline
            // 一致（同 wrap、同 row_height、保留尾空白），故可安全无条件使用。
            let mut layouter = move |ui: &egui::Ui, s: &dyn egui::TextBuffer, wrap: f32| {
                layout_job(ui, s.as_str(), syntax_kind, highlights, &palette, wrap)
            };
            let gutter_width = show_line_numbers
                .then(|| line_number_gutter_width(ui, display_text, every_display_row));
            let mut output = if let Some(gutter_width) = gutter_width {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let gutter_right = ui.cursor().left() + gutter_width - 4.0;
                    ui.add_space(gutter_width);
                    let output = te.layouter(&mut layouter).show(ui);
                    paint_line_numbers(
                        ui,
                        &output.galley,
                        output.galley_pos,
                        gutter_right,
                        every_display_row,
                    );
                    output
                })
                .inner
            } else {
                te.layouter(&mut layouter).show(ui)
            };
            let line_selection = gutter_width.and_then(|gutter_width| {
                let gutter_rect = egui::Rect::from_min_max(
                    egui::pos2(
                        output.galley_pos.x - gutter_width + 4.0,
                        output.galley_pos.y,
                    ),
                    egui::pos2(
                        output.galley_pos.x,
                        output.galley_pos.y + output.galley.size().y,
                    ),
                );
                let response = ui.interact(
                    gutter_rect,
                    output.response.id.with("line_number_gutter"),
                    egui::Sense::click(),
                );
                if response.hovered() {
                    ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
                }
                if !response.clicked_by(egui::PointerButton::Primary) {
                    return None;
                }
                let pointer = response.interact_pointer_pos()?;
                if every_display_row {
                    visual_row_char_range(
                        output.galley.rows.iter().map(|row| {
                            (
                                row.pos.y,
                                row.row.size.y,
                                row.char_count_including_newline().0,
                            )
                        }),
                        pointer.y - output.galley_pos.y,
                    )
                } else {
                    let char_index = output
                        .galley
                        .cursor_from_pos(pointer - output.galley_pos)
                        .index
                        .0;
                    Some(line_char_range(display_text, char_index))
                }
            });
            let te_response = &output.response;
            if let Some(hit_abs) = scroll_to {
                scroll_to_hit(
                    ui,
                    output.galley_pos,
                    &output.galley,
                    highlights.active,
                    hit_abs,
                    ctx,
                );
            }
            text_edge_auto_scroll(ui, te_response, line_h);
            if let Some((start, end)) = line_selection {
                output.response.request_focus();
                output
                    .state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::two(
                        egui::text::CCursor::new(start),
                        egui::text::CCursor::new(end),
                    )));
                output.state.store(ui.ctx(), output.response.id);
            }
        });
}

fn line_char_range(text: &str, char_index: usize) -> (usize, usize) {
    let char_count = text.chars().count();
    let char_index = char_index.min(char_count);
    let start = text
        .chars()
        .take(char_index)
        .enumerate()
        .filter_map(|(index, ch)| (ch == '\n').then_some(index + 1))
        .last()
        .unwrap_or(0);
    let end = text
        .chars()
        .enumerate()
        .skip(char_index)
        .find_map(|(index, ch)| (ch == '\n').then_some(index + 1))
        .unwrap_or(char_count);
    (start, end)
}

fn visual_row_char_range(
    rows: impl IntoIterator<Item = (f32, f32, usize)>,
    y: f32,
) -> Option<(usize, usize)> {
    let mut char_start = 0;
    for (row_y, row_height, char_count) in rows {
        if y >= row_y && y < row_y + row_height {
            return (char_count > 0).then_some((char_start, char_start + char_count));
        }
        char_start += char_count;
    }
    None
}

fn line_number_gutter_width(ui: &egui::Ui, text: &str, every_display_row: bool) -> f32 {
    let font_id = egui::TextStyle::Monospace.resolve(ui.style());
    let digit_width = ui.fonts_mut(|fonts| fonts.glyph_width(&font_id, '0'));
    let digits = if every_display_row {
        let available_width = (ui.available_width() - 16.0).max(digit_width);
        let chars = text.chars().count();
        let mut digits = 1;
        loop {
            let text_width = (available_width - digits as f32 * digit_width - 8.0).max(digit_width);
            let chars_per_row = (text_width / digit_width).floor().max(1.0) as usize;
            let rows = chars.div_ceil(chars_per_row).max(1);
            let next_digits = rows.ilog10() + 1;
            if next_digits == digits {
                break digits;
            }
            digits = next_digits;
        }
    } else {
        let line_count = text.bytes().filter(|&b| b == b'\n').count() + 1;
        line_count.ilog10() + 1
    };
    digits as f32 * digit_width + 8.0
}

fn paint_line_numbers(
    ui: &egui::Ui,
    galley: &egui::Galley,
    galley_pos: egui::Pos2,
    right: f32,
    every_display_row: bool,
) {
    let font_id = egui::TextStyle::Monospace.resolve(ui.style());
    let color = with_alpha(ui.visuals().hyperlink_color, 128);
    for (number, y) in line_number_positions(
        galley
            .rows
            .iter()
            .map(|row| (row.pos.y, row.ends_with_newline)),
        every_display_row,
    ) {
        ui.painter().text(
            egui::pos2(right, galley_pos.y + y),
            egui::Align2::RIGHT_TOP,
            number.to_string(),
            font_id.clone(),
            color,
        );
    }
}

fn with_alpha(color: egui::Color32, alpha: u8) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

fn line_number_positions(
    rows: impl Iterator<Item = (f32, bool)>,
    every_display_row: bool,
) -> Vec<(usize, f32)> {
    if every_display_row {
        rows.enumerate()
            .map(|(index, (y, _))| (index + 1, y))
            .collect()
    } else {
        hard_line_number_positions(rows)
    }
}

fn hard_line_number_positions(rows: impl IntoIterator<Item = (f32, bool)>) -> Vec<(usize, f32)> {
    let mut positions = Vec::new();
    let mut line_number = 1;
    let mut starts_line = true;
    for (y, ends_with_newline) in rows {
        if starts_line {
            positions.push((line_number, y));
        }
        if ends_with_newline {
            line_number += 1;
            starts_line = true;
        } else {
            starts_line = false;
        }
    }
    positions
}

/// TextEdit 原生拖选路径的边缘自动滚动，与自绘文本/hex 视图使用相同速度曲线。
fn text_edge_auto_scroll(ui: &mut egui::Ui, response: &egui::Response, line_h: f32) {
    if !response.dragged() || !ui.input(|i| i.pointer.primary_down()) {
        return;
    }
    let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) else {
        return;
    };
    let view = ui.clip_rect();
    let rect = response.rect;
    let margin = 28.0;
    let dt = ui.input(|i| i.stable_dt).min(0.05);
    let max_speed = line_h * crate::ui::hexview::EDGE_SCROLL_MAX_ROWS_PER_SECOND;

    if rect.top() < view.top() - 1.0 && pointer.y < view.top() + margin {
        let factor =
            crate::ui::hexview::edge_scroll_speed_fraction(view.top() + margin - pointer.y, margin);
        ui.scroll_with_delta(egui::vec2(0.0, max_speed * factor * dt));
    } else if rect.bottom() > view.bottom() + 1.0 && pointer.y > view.bottom() - margin {
        let factor = crate::ui::hexview::edge_scroll_speed_fraction(
            pointer.y - (view.bottom() - margin),
            margin,
        );
        ui.scroll_with_delta(egui::vec2(0.0, -max_speed * factor * dt));
    }
}

/// 按语法 token 与命中区间构造一行/整段的 layout job。
fn layout_job(
    ui: &egui::Ui,
    s: &str,
    syntax: Option<DecodeMode>,
    highlights: &SearchHighlights,
    palette: &[egui::Color32; 6],
    wrap: f32,
) -> std::sync::Arc<egui::Galley> {
    let font_id = egui::TextStyle::Monospace.resolve(ui.style());
    // 与 Hex 视图行高一致：不加 extra_text_line_spacing，同窗口文本/hex 行高对齐
    let line_height = ui.fonts_mut(|f| f.row_height(&font_id));
    let base = egui::text::TextFormat {
        font_id,
        color: ui.visuals().text_color(),
        line_height: Some(line_height),
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::default();
    let tokens = syntax
        .map(|mode| syntax_tokens(mode, s))
        .unwrap_or_default();
    append_colored(&mut job, s, &tokens, base, palette, highlights);
    job.wrap.max_width = wrap;
    job.keep_trailing_whitespace = true;
    ui.fonts_mut(|f| f.layout_job(job))
}

/// 搜索命中：滚到命中所在行。必须与渲染/高亮使用**同一文本 display_text** 构图。
fn scroll_to_hit(
    ui: &mut egui::Ui,
    galley_pos: egui::Pos2,
    galley: &egui::Galley,
    active_hit: Option<(usize, usize)>,
    hit_abs: usize,
    ctx: &CardCtx<'_>,
) {
    if hit_abs < ctx.off || hit_abs >= ctx.off + ctx.window.len() {
        return;
    }
    // 命中字符偏移（基于显示文本，与高亮/折行同源）：1:1 时直接取高亮起点或
    // 字节转字符；非 1:1（lossy 重编码的二进制 blob）按字符比例近似——
    // 每行字数比字节数均匀，比字节比例更贴近视觉折行。
    let display_text = galley.text();
    let pc = if let Some((hs, _)) = active_hit {
        display_text[..hs].chars().count()
    } else if std::str::from_utf8(ctx.window).is_ok() {
        display_text.chars().take(hit_abs - ctx.off).count()
    } else {
        display_text.chars().count() * (hit_abs - ctx.off) / ctx.window.len().max(1)
    };
    // Galley 不把硬换行作为 glyph 保存，需额外计入 ends_with_newline，
    // 否则文本越靠后，滚动目标就会逐行向后漂移。
    let row_idx = row_for_char(
        galley
            .rows
            .iter()
            .map(|row| (row.glyphs.len(), row.ends_with_newline)),
        pc,
    );
    let row_y = galley.rows[row_idx].rect().top();
    let rect = egui::Rect::from_min_size(
        egui::pos2(galley_pos.x, galley_pos.y + row_y),
        egui::vec2(4.0, galley.rows[row_idx].rect().height()),
    );
    ui.scroll_to_rect(rect, Some(egui::Align::Center));
}

fn row_for_char(rows: impl IntoIterator<Item = (usize, bool)>, char_offset: usize) -> usize {
    let mut char_start = 0usize;
    let mut last_idx = 0usize;
    for (idx, (glyph_count, ends_with_newline)) in rows.into_iter().enumerate() {
        last_idx = idx;
        let row_chars = glyph_count + usize::from(ends_with_newline);
        if char_start + row_chars > char_offset {
            return idx;
        }
        char_start += row_chars;
    }
    last_idx
}

/// 等宽字体行高（文本区与 hex 区统一）。
fn mono_line_height(ui: &egui::Ui) -> f32 {
    ui.fonts_mut(|f| f.row_height(&egui::TextStyle::Monospace.resolve(ui.style())))
}

#[cfg(test)]
mod tests {
    use super::{
        ByteFormat, byte_range_in_byte_text, fold_line_for_display_offset,
        hard_line_number_positions, line_char_range, line_number_positions,
        remapped_text_hit_range, row_for_char, visual_row_char_range,
    };
    use crate::fmt::{DecodeMode, Endian, HintFlags, decode};

    #[test]
    fn row_lookup_counts_hard_newlines() {
        let rows = [(5, true), (5, true), (5, false)];
        assert_eq!(row_for_char(rows, 0), 0);
        assert_eq!(row_for_char(rows, 5), 0);
        assert_eq!(row_for_char(rows, 6), 1);
        assert_eq!(row_for_char(rows, 12), 2);
    }

    #[test]
    fn line_number_selection_includes_newline_and_handles_utf8() {
        let text = "键一\nsecond\nlast";
        let (start, end) = line_char_range(text, 1);
        assert_eq!(
            &text.chars().collect::<Vec<_>>()[start..end],
            ['键', '一', '\n']
        );
        let (start, end) = line_char_range(text, 5);
        assert_eq!(
            &text.chars().collect::<Vec<_>>()[start..end],
            "second\n".chars().collect::<Vec<_>>()
        );
        let (start, end) = line_char_range(text, usize::MAX);
        assert_eq!(
            &text.chars().collect::<Vec<_>>()[start..end],
            ['l', 'a', 's', 't']
        );
    }

    #[test]
    fn binary_line_number_selects_only_the_clicked_visual_row() {
        let rows = [(0.0, 16.0, 24), (16.0, 16.0, 24), (32.0, 16.0, 8)];
        assert_eq!(visual_row_char_range(rows, 20.0), Some((24, 48)));
        assert_eq!(visual_row_char_range(rows, 40.0), Some((48, 56)));
        assert_eq!(visual_row_char_range(rows, 60.0), None);
    }

    #[test]
    fn row_lookup_handles_soft_wraps() {
        let rows = [(4, false), (3, true), (2, false)];
        assert_eq!(row_for_char(rows, 3), 0);
        assert_eq!(row_for_char(rows, 4), 1);
        assert_eq!(row_for_char(rows, 8), 2);
    }

    #[test]
    fn line_numbers_ignore_soft_wrap_rows() {
        let rows = [(0.0, false), (16.0, true), (32.0, true), (48.0, false)];
        assert_eq!(
            hard_line_number_positions(rows),
            [(1, 0.0), (2, 32.0), (3, 48.0)]
        );
    }

    #[test]
    fn hex_display_rows_number_soft_wraps() {
        let rows = [(0.0, false), (16.0, false), (32.0, false)];
        assert_eq!(
            line_number_positions(rows.into_iter(), true),
            [(1, 0.0), (2, 16.0), (3, 32.0)]
        );
    }

    #[test]
    fn binary_search_hit_maps_to_hex_text_characters() {
        let bytes: Vec<u8> = (0u8..=255).cycle().take(4096).collect();
        let text = decode(
            &bytes,
            DecodeMode::Hex,
            Endian::Little,
            bytes.len() * 3,
            &HintFlags::default(),
        );
        let start = bytes
            .windows(4)
            .position(|window| window == [0x51, 0x52, 0x53, 0x54])
            .unwrap();
        let range = byte_range_in_byte_text(&text, &bytes, start, 4, ByteFormat::Hex).unwrap();
        assert_eq!(&text[range.0..range.1], "51 52 53 54");
    }

    #[test]
    fn binary_and_decimal_search_hits_map_to_visible_text() {
        let bytes = [0x51, 0x00, 0xFF, 0x04];
        let binary = decode(
            &bytes,
            DecodeMode::Binary,
            Endian::Little,
            100,
            &HintFlags::default(),
        );
        let decimal = decode(
            &bytes,
            DecodeMode::Dec,
            Endian::Little,
            100,
            &HintFlags::default(),
        );
        let binary_range =
            byte_range_in_byte_text(&binary, &bytes, 1, 2, ByteFormat::Binary).unwrap();
        let decimal_range =
            byte_range_in_byte_text(&decimal, &bytes, 1, 2, ByteFormat::Decimal).unwrap();
        assert_eq!(&binary[binary_range.0..binary_range.1], "00000000 11111111");
        assert_eq!(&decimal[decimal_range.0..decimal_range.1], "0 255");
    }

    #[test]
    fn byte_text_hit_clips_at_visible_segment_end() {
        let bytes = [0xFE, 0xFF];
        let text = "FE FF";
        assert_eq!(
            byte_range_in_byte_text(text, &bytes, 1, 4, ByteFormat::Hex),
            Some((3, 5))
        );
    }

    #[test]
    fn pretty_json_search_hit_maps_to_matching_occurrence() {
        let source = br#"{"first":22,"second":22}"#;
        let display = "{\n  \"first\": 22,\n  \"second\": 22\n}";
        let second_hit = source
            .windows(2)
            .rposition(|window| window == b"22")
            .unwrap();
        let range = remapped_text_hit_range(source, second_hit, second_hit + 2, display).unwrap();
        let expected = display.rfind("22").unwrap();
        assert_eq!(range, (expected, expected + 2));
    }

    #[test]
    fn pretty_json_second_22_hit_scrolls_to_122_line() {
        let source = br#"{"binNos":[22,122]}"#;
        let display = "{\n  \"binNos\": [\n    22,\n    122\n  ]\n}";
        let lines = super::fold_line_list(Some(DecodeMode::Json), display);
        let hits = super::super::super::search::find_all_hits(source, b"22", 0);
        assert_eq!(hits.len(), 2);

        let second = hits[1];
        let range =
            remapped_text_hit_range(source, second.0, second.0 + second.1, display).unwrap();
        let line = fold_line_for_display_offset(display, &lines, range.0).unwrap();

        assert!(display[lines[line].start..lines[line].end].contains("122"));
    }

    #[test]
    fn pretty_json_all_search_hits_map_to_visible_text() {
        let source = br#"{"first":22,"second":22}"#;
        let display = "{\n  \"first\": 22,\n  \"second\": 22\n}";
        let ranges: Vec<_> = super::super::super::search::find_all_hits(source, b"22", 0)
            .into_iter()
            .filter_map(|(start, len)| remapped_text_hit_range(source, start, start + len, display))
            .collect();
        assert_eq!(ranges.len(), 2);
        assert!(
            ranges
                .iter()
                .all(|&(start, end)| &display[start..end] == "22")
        );
    }

    #[test]
    fn pretty_json_search_does_not_highlight_unrepresented_query() {
        let source = br#"{"value":22,"next":1}"#;
        let display = "{\n  \"value\": 22\n}";
        let start = source.windows(2).position(|window| window == b",\"");
        assert_eq!(
            start.and_then(|start| remapped_text_hit_range(source, start, start + 2, display)),
            None
        );
    }
}
