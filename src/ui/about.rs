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
const URL_LICENSE: &str = "https://www.apache.org/licenses/LICENSE-2.0";

/// "关于"页签内容（居中、限宽 640，便于阅读长段落）。
pub fn show(ui: &mut egui::Ui, build_date: &str) {
    let t = tr();
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(14.0);
        ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
            ui.set_max_width(640.0);

            ui.vertical_centered(|ui| {
                ui.heading(egui::RichText::new("MDBXer").size(28.0).strong());
                ui.label(egui::RichText::new(format!("v{VERSION}")).weak());
                ui.add_space(4.0);
                ui.label(t.about_tagline);
            });
            ui.add_space(12.0);

            // ── 应用 ──
            section(ui, t.about_group_app);
            grid(ui, "about_grid_app", |ui| {
                row(ui, t.about_version, VERSION);
                row(ui, t.about_build, build_date);
                row(ui, t.about_stack, "Rust 2024 · egui/eframe 0.36 · libmdbx-rs");
                row_link(ui, t.about_license, LICENSE, URL_LICENSE, t.about_open_link_tip);
                row(ui, t.about_copyright, COPYRIGHT);
            });

            // ── 运行环境 ──
            section(ui, t.about_group_runtime);
            let os_arch = format!("{} · {}", std::env::consts::OS, std::env::consts::ARCH);
            let config_path = crate::config::config_path()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "—".to_string());
            let history_path = crate::history::storage_path()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "—".to_string());
            grid(ui, "about_grid_runtime", |ui| {
                row(ui, t.about_os_arch, &os_arch);
                long_row(ui, t.about_config, &config_path);
                long_row(ui, t.about_history, &history_path);
            });

            // ── 链接 ──
            section(ui, t.about_group_links);
            ui.horizontal(|ui| {
                ui.hyperlink_to("Gitee", URL_GITEE)
                    .on_hover_text(t.about_open_link_tip);
                ui.separator();
                ui.hyperlink_to("GitHub", URL_GITHUB)
                    .on_hover_text(t.about_open_link_tip);
                ui.separator();
                ui.hyperlink_to("libmdbx", URL_LIBMDBX)
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
    });
}

/// 分组标题 + 分隔线。
fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(4.0);
    ui.label(egui::RichText::new(title).strong());
    ui.separator();
    ui.add_space(2.0);
}

/// 两列键值表。`id_salt` 必须在同一帧内唯一（同页有多个 Grid）。
/// 固定首列最小宽度，使多个分组的键列/值列竖向对齐。
fn grid(ui: &mut egui::Ui, id_salt: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::Grid::new(id_salt)
        .num_columns(2)
        .spacing([20.0, 5.0])
        .min_col_width(90.0)
        .striped(false)
        .show(ui, body);
}

/// 普通键值行（值等宽字体）。
fn row(ui: &mut egui::Ui, k: &str, v: &str) {
    ui.label(k);
    ui.monospace(v);
    ui.end_row();
}

/// 长值行（路径）：不用等宽，允许自动换行以免超出 640 宽。
fn long_row(ui: &mut egui::Ui, k: &str, v: &str) {
    ui.label(k);
    ui.label(egui::RichText::new(v).weak());
    ui.end_row();
}

/// 值是可点击链接的行。
fn row_link(ui: &mut egui::Ui, k: &str, text: &str, url: &str, tip: &str) {
    ui.label(k);
    ui.hyperlink_to(text, url).on_hover_text(tip);
    ui.end_row();
}
