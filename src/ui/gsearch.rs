// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 跨表 Value 搜索：逐帧分批扫描库里所有表，命中项在浮动窗口列出，
//! 点击命中项切换到对应表并定位到该 Key。

use super::{MdbxerApp, Status, ValueSearchQuery};
use crate::db;
use crate::fmt::{DecodeMode, decode};
use crate::i18n::tr;

/// 每帧扫描的记录条数（UI 线程分批，保持界面流畅）
const BATCH: usize = 500;
/// 命中条数上限（超出只提示截断，不再累积）
const MAX_HITS: usize = 500;
/// 命中项预览字符数
const PREVIEW_CHARS: usize = 200;

/// 一条命中记录。
pub struct Hit {
    /// 所在表的下标（跳转时用）
    table_idx: usize,
    /// 表显示名（列表展示用）
    table_label: String,
    /// 原始 Key（跳转时用）
    key: Vec<u8>,
    /// Key/Value 预览文本
    key_text: String,
    val_text: String,
}

/// 跨表搜索状态。
pub struct State {
    /// 搜索词及匹配语义
    query: ValueSearchQuery,
    /// 下一张待扫描的表
    next_table: usize,
    /// 当前表的续读锚点（None = 该表首批）
    anchor: Option<db::RawAnchor>,
    pub hits: Vec<Hit>,
    /// 已扫描的原始记录条数
    pub scanned: usize,
    pub done: bool,
    pub truncated: bool,
    /// 正在扫描的表显示名（进度显示）
    current_table: String,
}

/// 用搜索框当前内容发起跨表搜索（替换上一次结果）。
pub fn start(app: &mut MdbxerApp) {
    let t = tr();
    if app.db.is_none() {
        return;
    }
    let input = app.search_input.trim().to_string();
    if input.is_empty() {
        app.status = Status::warn(t.search_empty.to_string());
        return;
    }
    let query = match ValueSearchQuery::parse(&input, app.search_case, app.search_regex) {
        Ok(q) => q,
        Err(e) => {
            app.status = Status::error(t.key_search_bad(&e));
            return;
        }
    };
    app.add_search_term(&input);
    app.gsearch = Some(State {
        query,
        next_table: 0,
        anchor: None,
        hits: Vec::new(),
        scanned: 0,
        done: app.db.as_ref().is_none_or(|d| d.tables.is_empty()),
        truncated: false,
        current_table: String::new(),
    });
}

/// 每帧推进一批扫描；未完成时请求重绘以继续。
pub fn poll(app: &mut MdbxerApp, ctx: &egui::Context) {
    let Some(mut st) = app.gsearch.take() else {
        return;
    };
    if st.done {
        app.gsearch = Some(st);
        return;
    }
    let Some(dbh) = app.db.as_ref() else {
        st.done = true;
        app.gsearch = Some(st);
        return;
    };
    if st.next_table >= dbh.tables.len() {
        st.done = true;
        app.gsearch = Some(st);
        return;
    }

    let table = &dbh.tables[st.next_table];
    st.current_table = table.display();
    let name = table.name.clone();
    let dup = table.dup_sort;
    let batch = db::fetch_raw_batch(
        &dbh.db,
        name.as_deref(),
        dup,
        crate::db::Direction::Forward,
        st.anchor.as_ref(),
        st.anchor.is_some(),
        BATCH,
    );
    let batch = match batch {
        Ok(b) => b,
        Err(e) => {
            app.status = Status::error(e);
            st.done = true;
            app.gsearch = Some(st);
            return;
        }
    };

    let hint = app.hint_value;
    let hint_key = app.hint_key;
    let endian = app.endian;
    let val_mode = app.val_mode;
    let key_mode = app.key_mode;
    for (k, v) in &batch.rows {
        let matched = match &st.query {
            ValueSearchQuery::Bytes(_) => st.query.matches_bytes(v),
            // 文本：同时按 Auto 与当前排版解码匹配（与全表搜索一致）
            _ => {
                let auto_text = decode(v, DecodeMode::Auto, endian, 65536, &hint);
                let mode_text = decode(v, val_mode, endian, 65536, &hint);
                st.query.matches_text(&auto_text) || st.query.matches_text(&mode_text)
            }
        };
        if !matched {
            continue;
        }
        if st.hits.len() < MAX_HITS {
            st.hits.push(Hit {
                table_idx: st.next_table,
                table_label: table.display(),
                key: k.clone(),
                key_text: crate::fmt::decode_key(k, key_mode, endian, PREVIEW_CHARS, &hint_key),
                val_text: decode(v, DecodeMode::Auto, endian, PREVIEW_CHARS, &hint),
            });
        } else {
            st.truncated = true;
        }
    }
    st.scanned += batch.rows.len();

    if batch.has_more {
        st.anchor = batch.rows.last().cloned();
    } else {
        // 本表扫完，转到下一张表
        st.anchor = None;
        st.next_table += 1;
    }
    let done = st.done;
    app.gsearch = Some(st);
    if !done {
        ctx.request_repaint();
    }
}

/// 结果窗口（Some = 打开）。
pub fn show(ui: &mut egui::Ui, app: &mut MdbxerApp) {
    let t = tr();
    let Some(st) = app.gsearch.as_ref() else {
        return;
    };

    let status = if st.done {
        t.gsearch_done(st.scanned, st.hits.len())
    } else {
        t.gsearch_scanning(&st.current_table, st.scanned, st.hits.len())
    };
    let truncated = st.truncated;
    let hits = &st.hits;

    let mut open = true;
    let mut input = app.search_input.clone();
    let mut rescan = false;
    let mut export = false;
    let mut jump: Option<(usize, Vec<u8>)> = None;

    egui::Window::new(t.gsearch_title)
        .id(egui::Id::new("mdbxer_gsearch"))
        .open(&mut open)
        .default_width(620.0)
        .resizable(true)
        .show(ui.ctx(), |ui| {
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut input)
                        .id_salt("gsearch_input")
                        .desired_width(320.0)
                        .hint_text(t.toolbar_search_hint),
                );
                if ui.button(t.gsearch_start).clicked() {
                    rescan = true;
                }
                if !hits.is_empty() && ui.button(t.gsearch_export).clicked() {
                    export = true;
                }
            });
            ui.separator();
            ui.weak(status);
            if truncated {
                ui.label(
                    egui::RichText::new(t.gsearch_truncated).color(ui.visuals().warn_fg_color),
                );
            }
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(420.0)
                .show(ui, |ui| {
                    for hit in hits {
                        let text =
                            format!("{}  {}  {}", hit.table_label, hit.key_text, hit.val_text);
                        if ui
                            .add(egui::Button::new(text).wrap())
                            .on_hover_text(format!("{}\n{}", hit.key_text, hit.val_text))
                            .clicked()
                        {
                            jump = Some((hit.table_idx, hit.key.clone()));
                        }
                    }
                });
        });

    if input != app.search_input {
        app.search_input = input;
    }
    if !open {
        app.gsearch = None;
    } else if rescan {
        start(app);
    } else if export {
        export_hits(app);
    } else if let Some((idx, key)) = jump {
        app.jump_to_hit(idx, &key);
    }
}

/// 把命中项导出为 JSONL（经保存对话框）。
fn export_hits(app: &mut MdbxerApp) {
    let Some(st) = app.gsearch.as_ref() else {
        return;
    };
    let mut out = String::new();
    for hit in &st.hits {
        let line = serde_json::json!({
            "table": hit.table_label,
            "key": hit.key_text,
            "value": hit.val_text,
        });
        out.push_str(&line.to_string());
        out.push('\n');
    }
    let n = st.hits.len();
    if app.save_bytes("search_hits.jsonl", out.as_bytes(), "jsonl") {
        let t = tr();
        app.status = Status::info(t.gsearch_exported(n));
    }
}
