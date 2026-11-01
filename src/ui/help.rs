// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! "帮助"页签：功能速览。按使用场景分组列出功能名与一句话说明。
//! 文案全部在 i18n 表里（随界面语言切换）；无数据库打开时也可查看。

use crate::i18n::tr;

/// 正文列最大宽度（窄窗口时随可用宽度收窄）。
const COLUMN_WIDTH: f32 = 820.0;

/// 帮助页内容：与"关于"页同一套版式（两侧对称留白、正文列居中、列内左对齐）。
pub fn show(ui: &mut egui::Ui) {
    let t = tr();
    egui::ScrollArea::vertical().show(ui, |ui| {
        let avail = ui.available_width();
        let w = avail.min(COLUMN_WIDTH);
        let side = (avail - w).max(0.0) / 2.0;
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.add_space(side);
            ui.vertical(|ui| {
                ui.set_min_width(w);
                ui.set_max_width(w);

                ui.heading(t.help_title);
                ui.label(egui::RichText::new(t.help_intro).weak());
                ui.add_space(2.0);

                for group in t.help_groups {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(group.title).strong().size(15.0));
                    ui.separator();
                    for (name, desc) in group.items {
                        item(ui, name, desc);
                    }
                }
                ui.add_space(16.0);
            });
            ui.add_space(side);
        });
    });
}

/// 单条功能：缩进的圆点 + 功能名（加粗）+ 说明（弱化，过长时随列宽换行）。
fn item(ui: &mut egui::Ui, name: &str, desc: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.add_space(12.0);
        ui.label(egui::RichText::new("•").weak());
        ui.label(egui::RichText::new(name).strong());
        ui.label(egui::RichText::new(desc).weak());
    });
}
