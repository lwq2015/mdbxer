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
use crate::ui::syntax::{FoldLine, append_colored, fold_lines, json_palette, syntax_tokens};

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
    let (text_h, _) = layout::card_content_heights(ui, app, ctx.window, &display_text, &budget);

    let guess_text = guess_text(app, ctx);
    let scroll_to = scroll_target(app, ctx);
    let hit_range = hit_range(app, ctx, &display_text);
    let lines = fold_line_list(syntax_kind, &display_text);
    let fold_active = lines.iter().any(|f| f.fold_end.is_some());
    let scroll_line = fold_scroll_line(fold_active, scroll_to, ctx, &display_text, &lines);

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
                syntax_kind.unwrap_or(DecodeMode::Json),
                &palette,
                hit_range,
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
            hit_range,
            scroll_to,
            text_h,
            line_h,
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
fn guess_text(app: &MdbxerApp, ctx: &CardCtx<'_>) -> String {
    let t = tr();
    if ctx.mode == DecodeMode::Auto {
        let (label, _) = fmt::guess(ctx.bytes, app.endian, &app.hint_value);
        // 附加解读只保留文件魔数（强信号、短标注）：地址/FILETIME 对
        // 任意字节几乎总命中，放猜测行是噪音且撑宽窗口
        match fmt::magic_hint(ctx.bytes) {
            Some(h) => t.guess_line(&format!("{label} · {h}"), ctx.bytes.len()),
            None => t.guess_line(&label, ctx.bytes.len()),
        }
    } else {
        t.bytes(ctx.bytes.len())
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

/// 文本区命中高亮区间（相对显示文本）：仅当"实际显示文本"与窗口字节 1:1
/// （即显示原文，长度相等且为合法 UTF-8）时可靠；hex/数值/pretty JSON 等
/// 重编码显示不映射，只滚动不高亮。
fn hit_range(app: &MdbxerApp, ctx: &CardCtx<'_>, display_text: &str) -> Option<(usize, usize)> {
    let (hit_abs, hit_len) = match ctx.side {
        Side::Key => &app.detail.key,
        Side::Value => &app.detail.val,
    }
    .blob_hit?;
    if hit_abs < ctx.off || hit_abs >= ctx.off + ctx.window.len() {
        return None;
    }
    if display_text.len() != ctx.window.len() || std::str::from_utf8(ctx.window).is_err() {
        return None;
    }
    let mut hs = hit_abs - ctx.off;
    let mut he = (hs + hit_len).min(display_text.len());
    while hs > 0 && !display_text.is_char_boundary(hs) {
        hs -= 1;
    }
    while he < display_text.len() && !display_text.is_char_boundary(he) {
        he += 1;
    }
    (hs < he).then_some((hs, he))
}

/// 折叠视图的行切分：语法着色激活且存在多行文本时才切；
/// 单行紧凑 JSON、纯文本、无块的多行文本不折叠，仍走 TextEdit 原路径。
fn fold_line_list(syntax: Option<DecodeMode>, text: &str) -> Vec<FoldLine> {
    match syntax {
        Some(_) if text.contains('\n') => fold_lines(text),
        _ => Vec::new(),
    }
}

/// 折叠视图滚动目标行：显示文本与窗口字节 1:1 时按字节精确定位行，
/// 重编码显示（pretty）按字节比例估算；命中行被祖先折叠遮盖时，
/// fold_view_body 渲染中自动展开覆盖它的祖先折叠。
fn fold_scroll_line(
    fold_active: bool,
    scroll_to: Option<usize>,
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
    let mut bp =
        if display_text.len() == ctx.window.len() && std::str::from_utf8(ctx.window).is_ok() {
            hit_rel
        } else {
            display_text.len() * hit_rel / ctx.window.len().max(1)
        };
    while bp > 0 && !display_text.is_char_boundary(bp) {
        bp -= 1;
    }
    Some(lines.partition_point(|fl| fl.start <= bp).saturating_sub(1))
}

/// TextEdit 渲染路径（无可折叠块时）：自定义 layouter 负责语法着色/命中高亮，
/// 并记录实际折行宽度供命中的滚动定位复用。
#[allow(clippy::too_many_arguments)]
fn text_edit_body(
    ui: &mut egui::Ui,
    ctx: &CardCtx<'_>,
    display_text: &str,
    syntax_kind: Option<DecodeMode>,
    hit_range: Option<(usize, usize)>,
    scroll_to: Option<usize>,
    text_h: f32,
    line_h: f32,
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
            let content_top = ui.cursor().min.y;
            // 记录 layouter 拿到的实际 wrap 宽度（与 TextEdit 渲染完全一致），
            // 供命中滚动定位复用——估算 wrap 会导致大文本折行次数与渲染不一致。
            // Rc 让 move 闭包与滚动定位共享同一个 Cell。
            let hit_wrap = std::rc::Rc::new(std::cell::Cell::new(None::<f32>));
            let hit_wrap_layouter = hit_wrap.clone();
            let te = egui::TextEdit::multiline(&mut text_owned)
                .font(egui::TextStyle::Monospace)
                .desired_width(f32::INFINITY)
                .desired_rows(1);
            // 无条件走自定义 layouter：即使无高亮/无语法，也要让 hit_wrap
            // 记录到 TextEdit 实际折行宽度。无高亮时其行为与 egui 默认 multiline
            // 一致（同 wrap、同 row_height、保留尾空白），故可安全无条件使用。
            let mut layouter = move |ui: &egui::Ui, s: &dyn egui::TextBuffer, wrap: f32| {
                hit_wrap_layouter.set(Some(wrap));
                layout_job(ui, s.as_str(), syntax_kind, hit_range, &palette, wrap)
            };
            let te_rect = ui.add(te.layouter(&mut layouter)).rect;
            if let Some(hit_abs) = scroll_to {
                scroll_to_hit(
                    ui,
                    te_rect,
                    content_top,
                    display_text,
                    hit_range,
                    hit_abs,
                    ctx,
                    &hit_wrap,
                    line_h,
                );
            }
        });
}

/// 按语法 token 与命中区间构造一行/整段的 layout job。
fn layout_job(
    ui: &egui::Ui,
    s: &str,
    syntax: Option<DecodeMode>,
    hit: Option<(usize, usize)>,
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
    if let Some(mode) = syntax {
        // s 即整段解码文本（合法结构化文本，pretty 多行），按格式分派词法分词器着色
        let toks = syntax_tokens(mode, s);
        append_colored(&mut job, s, &toks, base, palette, hit);
    } else if let Some((hs, he)) = hit {
        job.append(&s[..hs], 0.0, base.clone());
        job.append(
            &s[hs..he],
            0.0,
            egui::text::TextFormat {
                background: egui::Color32::from_rgba_unmultiplied(0xF5, 0xA6, 0x23, 0x66),
                ..base.clone()
            },
        );
        job.append(&s[he..], 0.0, base);
    } else {
        job.append(s, 0.0, base);
    }
    job.wrap.max_width = wrap;
    job.keep_trailing_whitespace = true;
    ui.fonts_mut(|f| f.layout_job(job))
}

/// 搜索命中：滚到命中所在行。必须与渲染/高亮使用**同一文本 display_text** 构图。
#[allow(clippy::too_many_arguments)]
fn scroll_to_hit(
    ui: &mut egui::Ui,
    te_rect: egui::Rect,
    content_top: f32,
    display_text: &str,
    hit_range: Option<(usize, usize)>,
    hit_abs: usize,
    ctx: &CardCtx<'_>,
    hit_wrap: &std::rc::Rc<std::cell::Cell<Option<f32>>>,
    line_h: f32,
) {
    if hit_abs < ctx.off || hit_abs >= ctx.off + ctx.window.len() {
        return;
    }
    // 命中字符偏移（基于显示文本，与高亮/折行同源）：1:1 时直接取高亮起点或
    // 字节转字符；非 1:1（lossy 重编码的二进制 blob）按字符比例近似——
    // 每行字数比字节数均匀，比字节比例更贴近视觉折行。
    let pc = if let Some((hs, _)) = hit_range {
        display_text[..hs].chars().count()
    } else if std::str::from_utf8(ctx.window).is_ok() {
        display_text.chars().take(hit_abs - ctx.off).count()
    } else {
        display_text.chars().count() * (hit_abs - ctx.off) / ctx.window.len().max(1)
    };
    let font_id = egui::TextStyle::Monospace.resolve(ui.style());
    // 与 TextEdit 渲染完全一致的 wrap（layouter 实际给的值）；
    // 仅当未走自定义 layouter 时才回退估算宽度。
    let wrap_w = hit_wrap.get().unwrap_or((te_rect.width() - 12.0).max(40.0));
    let galley = ui.fonts_mut(|f| {
        f.layout(
            display_text.to_string(),
            font_id,
            egui::Color32::WHITE,
            wrap_w,
        )
    });

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
        egui::pos2(te_rect.left(), content_top + 8.0 + row_y),
        egui::vec2(4.0, line_h),
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
    use super::row_for_char;

    #[test]
    fn row_lookup_counts_hard_newlines() {
        let rows = [(5, true), (5, true), (5, false)];
        assert_eq!(row_for_char(rows, 0), 0);
        assert_eq!(row_for_char(rows, 5), 0);
        assert_eq!(row_for_char(rows, 6), 1);
        assert_eq!(row_for_char(rows, 12), 2);
    }

    #[test]
    fn row_lookup_handles_soft_wraps() {
        let rows = [(4, false), (3, true), (2, false)];
        assert_eq!(row_for_char(rows, 3), 0);
        assert_eq!(row_for_char(rows, 4), 1);
        assert_eq!(row_for_char(rows, 8), 2);
    }
}
