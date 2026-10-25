// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 卡片导航行：分段翻页、大字段搜索、多值导航与值内容搜索。

use super::super::dup::DupCtx;
use super::super::state::{DetailState, Side};
use crate::i18n::tr;
use crate::ui::{MdbxerApp, SearchContext, Status};
use libmdbx::{Database, NoWriteMap};

/// 单行内容搜索的操作（⬇ = 下一个 / ⬆ = 上一个）。
#[derive(Clone, Copy, PartialEq, Eq)]
enum SearchCmd {
    None,
    Prev,
    Next,
}

/// 大字段分段导航：◀ / ▶ + 偏移跳转输入 + 段号信息。
pub(super) fn seg_row(
    ui: &mut egui::Ui,
    app: &mut MdbxerApp,
    side: Side,
    total: usize,
    off: usize,
) {
    let t = tr();
    ui.horizontal(|ui| {
        // 按钮/输入框固定在最左：段号与偏移文本长度会变，放前面会挤动按钮
        if ui
            .add_enabled(off > 0, egui::Button::new("◀"))
            .on_hover_text(t.seg_prev_tip)
            .clicked()
        {
            app.detail.seg_step(side, total, -1);
        }
        if ui
            .add_enabled(off + crate::fmt::PAGE_BYTES < total, egui::Button::new("▶"))
            .on_hover_text(t.seg_next_tip)
            .clicked()
        {
            app.detail.seg_step(side, total, 1);
        }
        let resp = ui.add(
            egui::TextEdit::singleline(match side {
                Side::Key => &mut app.detail.key.seg_input,
                Side::Value => &mut app.detail.val.seg_input,
            })
            .desired_width(84.0)
            .hint_text(t.seg_input_hint),
        );
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            app.status = Status::leveled(app.detail.seg_jump(side, total));
        }
        let seg_no = off / crate::fmt::PAGE_BYTES + 1;
        let seg_cnt = total.div_ceil(crate::fmt::PAGE_BYTES);
        ui.weak(t.seg_line(seg_no, seg_cnt, off, total));
    });
}

/// 大字段内容搜索行：文本按 UTF-8，hex(...)/0x.../binary(...)/dec(...) 按字节，回绕。
pub(super) fn blob_search_row(ui: &mut egui::Ui, app: &mut MdbxerApp, side: Side, bytes: &[u8]) {
    ui.horizontal(|ui| {
        let cmd = {
            let input = match side {
                Side::Key => &mut app.detail.key.blob_input,
                Side::Value => &mut app.detail.val.blob_input,
            };
            search_row(ui, input)
        };
        if cmd != SearchCmd::None {
            app.last_search = blob_context(side);
        }
        match cmd {
            SearchCmd::Prev => {
                app.status = Status::leveled(app.detail.blob_search(side, bytes, false));
            }
            SearchCmd::Next => {
                app.status = Status::leveled(app.detail.blob_search(side, bytes, true));
            }
            SearchCmd::None => {}
        }
        // VSCode 风格命中计数：当前第 n 个 / 共 m 个
        if let Some((n, m)) = match side {
            Side::Key => app.detail.key.blob_matches,
            Side::Value => app.detail.val.blob_matches,
        } {
            ui.weak(format!("{n}/{m}"));
        }
    });
}

/// 多值导航（仅 Value 卡片、多值表）：两行——翻页/跳转、值内容搜索。
pub(super) fn dup_rows(ui: &mut egui::Ui, app: &mut MdbxerApp, key: &[u8]) {
    let Some(table_name) = app.cur_table().map(|tbl| tbl.name.clone()) else {
        return;
    };
    let Some(dbh) = app.db.as_ref() else { return };

    let detail = &mut app.detail;
    let status = &mut app.status;
    let last = &mut app.last_search;
    let db = &dbh.db;
    let table = table_name.as_deref();

    dup_nav_row(ui, detail, status, db, table, key);
    dup_search_row(ui, detail, status, last, db, table, key);
    ui.add_space(2.0);
}

/// 多值翻页行：首/上页/上一个 + 下一个/下页/末 + 序号跳转。
fn dup_nav_row(
    ui: &mut egui::Ui,
    detail: &mut DetailState,
    status: &mut Status,
    db: &Database<NoWriteMap>,
    table: Option<&str>,
    key: &[u8],
) {
    let t = tr();
    let (idx, total) = (detail.dup.index, detail.dup.total);
    let dctx = DupCtx { db, table, key };
    ui.horizontal(|ui| {
        if action_button(ui, idx > 0, "⏮", t.dup_tip_first) {
            report(status, detail.dup_goto(&dctx, 0));
        }
        if action_button(ui, idx > 0, "⏪", t.dup_tip_prev100) {
            report(status, detail.dup_page_step(&dctx, -1));
        }
        if action_button(ui, idx > 0, "◀", t.dup_tip_prev) {
            report(status, detail.dup_step(&dctx, -1));
        }
        if action_button(ui, idx + 1 < total, "▶", t.dup_tip_next) {
            report(status, detail.dup_step(&dctx, 1));
        }
        if action_button(ui, idx + 1 < total, "⏩", t.dup_tip_next100) {
            report(status, detail.dup_page_step(&dctx, 1));
        }
        if action_button(ui, idx + 1 < total, "⏭", t.dup_tip_last) {
            report(status, detail.dup_goto(&dctx, total.saturating_sub(1)));
        }
        ui.label(t.goto);
        let resp = ui.add(
            egui::TextEdit::singleline(&mut detail.dup.jump_input)
                .desired_width(48.0)
                .hint_text("#"),
        );
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            *status = Status::leveled(detail.dup_jump(&dctx));
        }
    });
}

/// 多值搜索行：按值内容查找并跳转，命中后焦点保留以便连续 Enter。
fn dup_search_row(
    ui: &mut egui::Ui,
    detail: &mut DetailState,
    status: &mut Status,
    last: &mut SearchContext,
    db: &Database<NoWriteMap>,
    table: Option<&str>,
    key: &[u8],
) {
    let dctx = DupCtx { db, table, key };
    ui.horizontal(|ui| {
        let cmd = search_row(ui, &mut detail.dup.search_input);
        match cmd {
            SearchCmd::Prev => {
                *last = SearchContext::DetailDup;
                *status = Status::leveled(detail.dup_search(&dctx, false));
            }
            SearchCmd::Next => {
                *last = SearchContext::DetailDup;
                *status = Status::leveled(detail.dup_search(&dctx, true));
            }
            SearchCmd::None => {}
        }
    });
}

/// 共用内容搜索行：Enter=下一个、Shift/Ctrl+Enter=上一个、Esc=清空输入；
/// 回车后自动要回焦点，便于连续 Enter 逐个命中。
/// ⬇(下一个)在左、⬆(上一个)在右：主操作（Enter）优先的习惯顺序。
fn search_row(ui: &mut egui::Ui, input: &mut String) -> SearchCmd {
    let t = tr();
    let resp = ui
        .add(
            egui::TextEdit::singleline(input)
                .desired_width(178.0)
                .hint_text(t.search_hint),
        )
        .on_hover_text(t.search_hint);
    let key_enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
    let (ctrl, shift) = ui.input(|i| (i.modifiers.ctrl, i.modifiers.shift));
    // 无修饰 Enter 会 surrender focus（lost_focus）；带修饰 Enter 焦点仍在（has_focus）。
    let enter = key_enter && (resp.lost_focus() || resp.has_focus());
    // Esc：清除搜索输入。egui 无修饰 Esc 帧首收走焦点，用 lost_focus 判据。
    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        input.clear();
    }

    let next =
        ui.button("⬇").on_hover_text(t.search_next_tip).clicked() || (enter && !shift && !ctrl);
    let prev =
        ui.button("⬆").on_hover_text(t.search_prev_tip).clicked() || (enter && (shift || ctrl));

    // egui singleline 回车默认 surrender_focus，焦点离开后连续回车无法继续搜下一个
    // 命中——搜索完立刻把焦点要回此输入框
    if enter {
        resp.request_focus();
    }

    if next {
        SearchCmd::Next
    } else if prev {
        SearchCmd::Prev
    } else {
        SearchCmd::None
    }
}

/// 带 hover 提示的按钮，返回是否点击。
fn action_button(ui: &mut egui::Ui, enabled: bool, label: &str, tip: &str) -> bool {
    ui.add_enabled(enabled, egui::Button::new(label))
        .on_hover_text(tip)
        .clicked()
}

/// 多值操作失败（跨页加载）时把错误写到状态栏。
fn report(status: &mut Status, res: Result<(), String>) {
    if let Err(e) = res {
        *status = Status::error(e);
    }
}

/// 卡片角色对应的搜索上下文（F3/Shift+F3 继续哪个搜索）。
fn blob_context(side: Side) -> SearchContext {
    if side.is_key() {
        SearchContext::DetailKeyBlob
    } else {
        SearchContext::DetailValBlob
    }
}
