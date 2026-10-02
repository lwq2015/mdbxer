// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 中间 "表统计" 与 "环境信息" 页签：渲染 db::stats 返回的键值行。

use super::{MdbxerApp, Status};
use crate::db;
use crate::i18n::tr;

/// "表统计"页签：按选中表缓存，点"刷新"时清缓存重读。
pub fn show_table_stat(ui: &mut egui::Ui, app: &mut MdbxerApp) {
    let t = tr();
    let Some(index) = app.selected_table else {
        ui.label(t.select_table);
        return;
    };
    if app.stat_cache.as_ref().map(|(i, _)| *i) != Some(index) {
        let Some(dbh) = &app.db else { return };
        let tbl = &dbh.tables[index];
        let flags_desc = tbl.flags_desc();
        match db::table_stat_view(&dbh.db, tbl.name.as_deref(), &flags_desc) {
            Ok(rows) => app.stat_cache = Some((index, rows)),
            Err(e) => app.status = Status::Msg(t.stat_fail(&e)),
        }
    }
    let Some((_, rows)) = &app.stat_cache else {
        return;
    };
    let rows = rows.clone();

    egui::ScrollArea::both().show(ui, |ui| {
        ui.horizontal(|ui| {
            if ui.button(t.refresh).clicked() {
                app.stat_cache = None;
            }
        });
        egui::Grid::new("table_stat_grid")
            .num_columns(2)
            .striped(true)
            .spacing([24.0, 6.0])
            .show(ui, |ui| {
                for (k, v) in &rows {
                    ui.label(k);
                    ui.monospace(v);
                    ui.end_row();
                }
            });
    });
}

/// "环境信息"页签：整个库一份缓存，点"刷新"重读。
pub fn show_env_info(ui: &mut egui::Ui, app: &mut MdbxerApp) {
    let t = tr();
    if app.env_cache.is_none() {
        let Some(dbh) = &app.db else { return };
        match db::env_info_view(&dbh.db) {
            Ok(rows) => app.env_cache = Some(rows),
            Err(e) => app.status = Status::Msg(t.env_fail(&e)),
        }
    }
    let Some(rows) = &app.env_cache else { return };
    let rows = rows.clone();

    egui::ScrollArea::both().show(ui, |ui| {
        ui.horizontal(|ui| {
            if ui.button(t.refresh).clicked() {
                app.env_cache = None;
            }
        });
        egui::Grid::new("env_info_grid")
            .num_columns(2)
            .striped(true)
            .spacing([24.0, 6.0])
            .show(ui, |ui| {
                let mut last_group = String::new();
                for (g, k, v) in &rows {
                    if *g != last_group {
                        last_group = g.clone();
                        ui.label(egui::RichText::new(g).strong());
                        ui.label("");
                        ui.end_row();
                    }
                    ui.label(format!("  {k}"));
                    ui.monospace(v);
                    ui.end_row();
                }
            });
    });
}
