// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! "关于"页签：应用信息、运行环境、仓库链接、致谢与免责声明。
//! 无数据库打开时也可查看。

use crate::i18n::tr;

/// 版本号（编译期取自 Cargo.toml）。
const VERSION: &str = env!("CARGO_PKG_VERSION");
const LICENSE: &str = "Apache-2.0";
const COPYRIGHT: &str = "Copyright 2026 lwq_yu";

const URL_GITEE: &str = "https://gitee.com/lwq_yu/mdbxer";
const URL_GITHUB: &str = "https://github.com/lwq2015/mdbxer";
const URL_LIBMDBX: &str = "https://libmdbx.dqdkfa.ru/";
const URL_LIBMDBX_RS: &str = "https://github.com/vorot93/libmdbx-rs";
const URL_LICENSE: &str = "https://www.apache.org/licenses/LICENSE-2.0";

/// 正文列最大宽度（超出则随窗口收窄；长段落按此宽度自动换行）。
const COLUMN_WIDTH: f32 = 640.0;

/// "关于"页签内容：两侧对称留白、正文列水平居中，列内一律左对齐
/// （不能用 `with_layout(Align::Center)+set_max_width`：那样 Grid/段落
/// 会被当定宽子项整体居中而偏出面板被裁切，且 label 换行宽度计算异常）。
pub fn show(ui: &mut egui::Ui, build_date: &str) {
    let t = tr();
    egui::ScrollArea::vertical().show(ui, |ui| {
        let avail = ui.available_width();
        let w = avail.min(COLUMN_WIDTH);
        let side = (avail - w).max(0.0) / 2.0;
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            ui.add_space(side);
            ui.vertical(|ui| {
                ui.set_min_width(w);
                ui.set_max_width(w);

                // 标题区居中
                ui.vertical_centered(|ui| {
                    ui.heading(egui::RichText::new("MDBXer").size(28.0).strong());
                    ui.label(egui::RichText::new(format!("v{VERSION}")).weak());
                    ui.add_space(4.0);
                    ui.label(t.about_tagline);
                });
                ui.add_space(12.0);

                // ── 应用 ──
                section(ui, t.about_group_app);
                kv_row(ui, t.about_version, |ui| {
                    ui.monospace(VERSION);
                });
                kv_row(ui, t.about_build, |ui| {
                    ui.monospace(build_date);
                });
                kv_row(ui, t.about_stack, |ui| {
                    ui.monospace("Rust 2024 · egui/eframe 0.36 · libmdbx-rs");
                });
                kv_row(ui, t.about_license, |ui| {
                    ui.hyperlink_to(LICENSE, URL_LICENSE)
                        .on_hover_text(t.about_open_link_tip);
                });
                kv_row(ui, t.about_copyright, |ui| {
                    ui.monospace(COPYRIGHT);
                });

                // ── 运行环境 ──
                section(ui, t.about_group_runtime);
                let config_path = crate::config::config_path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "—".to_string());
                let history_path = crate::history::storage_path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "—".to_string());
                kv_row(ui, t.about_os_arch, |ui| {
                    ui.monospace(format!(
                        "{} · {}",
                        std::env::consts::OS,
                        std::env::consts::ARCH
                    ));
                });
                kv_row(ui, t.about_config, |ui| {
                    ui.label(egui::RichText::new(&config_path).weak());
                });
                kv_row(ui, t.about_history, |ui| {
                    ui.label(egui::RichText::new(&history_path).weak());
                });

                // ── 链接 ──
                section(ui, t.about_group_links);
                ui.horizontal(|ui| {
                    ui.hyperlink_to("Gitee", URL_GITEE)
                        .on_hover_text(t.about_open_link_tip);
                    dot(ui);
                    ui.hyperlink_to("GitHub", URL_GITHUB)
                        .on_hover_text(t.about_open_link_tip);
                    dot(ui);
                    ui.hyperlink_to("libmdbx", URL_LIBMDBX)
                        .on_hover_text(t.about_open_link_tip);
                    dot(ui);
                    ui.hyperlink_to("libmdbx-rs", URL_LIBMDBX_RS)
                        .on_hover_text(t.about_open_link_tip);
                });
                ui.add_space(8.0);

                // ── 致谢 ──
                section(ui, t.about_group_thanks);
                ui.label(t.about_thanks);
                ui.add_space(10.0);

                // ── 免责声明 ──
                section(ui, t.about_group_disclaimer);
                ui.label(egui::RichText::new(t.about_disclaimer).weak());
                ui.add_space(16.0);
            });
            ui.add_space(side);
        });
    });
}

/// 链接之间的居中点间隔。
fn dot(ui: &mut egui::Ui) {
    ui.label(egui::RichText::new("·").weak());
}

/// 分组标题（居中）+ 分隔线。
fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(4.0);
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new(title).strong());
    });
    ui.separator();
    ui.add_space(2.0);
}

/// 键列固定宽度：所有键值行的值都从此 x 位置开始，竖向对齐。
const KEY_COL: f32 = 120.0;

/// 键值行：键占固定 120 宽，值紧贴其后占剩余宽度并可自动换行；
/// 值多行时键顶对齐。不用 Grid——Grid 列按内容撑宽，长路径/窄窗口会溢出。
fn kv_row(ui: &mut egui::Ui, key: &str, draw_value: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let line_h = ui.text_style_height(&egui::TextStyle::Body);
        ui.add_sized([KEY_COL, line_h], egui::Label::new(key));
        draw_value(ui);
    });
}
