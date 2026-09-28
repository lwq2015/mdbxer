//! 右侧详情：Key / Value 卡片（格式下拉、复制、文本、hex dump、多值翻页）。

use super::MdbxerApp;
use crate::fmt::{self, DecodeMode};

pub fn show(ui: &mut egui::Ui, app: &mut MdbxerApp) {
    egui::Panel::right("detail_panel")
        .default_size(360.0)
        .size_range(240.0..=720.0)
        .show(ui, |ui| {
            let Some(row_idx) = app.selected_row else {
                ui.vertical_centered(|ui| {
                    ui.add_space(80.0);
                    ui.weak("在中间表格选择一行以查看详情");
                });
                return;
            };
            let Some(row) = app.rows.get(row_idx) else { return };
            let key = row.key.clone();
            let Some(value) = app.current_value() else { return };
            let key_no = app.base_index.map(|b| b + row_idx + 1);
            let dup_sort = app.cur_table().map(|t| t.dup_sort).unwrap_or(false);
            let (dup_index, dup_total) = (app.dup_index, app.dup_total);

            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("十六进制视图：");
                    ui.checkbox(&mut app.show_addr, "地址");
                    ui.checkbox(&mut app.show_hex, "HEX");
                    ui.checkbox(&mut app.show_ascii, "ASCII");
                    ui.separator();
                    ui.label("宽度");
                    let mut w = app.hex_width;
                    let ir = egui::ComboBox::from_id_salt("hex_width")
                        .selected_text(w.to_string())
                        .show_ui(ui, |ui| {
                            for v in fmt::HEX_WIDTHS {
                                ui.selectable_value(&mut w, v, v.to_string());
                            }
                        });
                    super::wheel_cycle(ui.ctx(), &ir.response, &fmt::HEX_WIDTHS, &mut w);
                    app.hex_width = w;
                });
                ui.separator();

                let key_title = match key_no {
                    Some(n) => format!("Key #{n}"),
                    None => "Key".to_string(),
                };
                kv_card(ui, app, &key_title, &key, true);
                ui.add_space(8.0);

                let val_title = if dup_sort {
                    format!("Value（第 {}/{} 个值）", dup_index + 1, dup_total)
                } else {
                    "Value".to_string()
                };
                kv_card(ui, app, &val_title, &value, false);
            });
        });
}

/// 一个 Key 或 Value 卡片。`is_key` 决定使用 key_mode 还是 val_mode。
fn kv_card(ui: &mut egui::Ui, app: &mut MdbxerApp, title: &str, bytes: &[u8], is_key: bool) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());

        ui.horizontal(|ui| {
            ui.strong(title);
            // 多值翻页
            if !is_key && app.dup_total > 1 {
                if ui
                    .add_enabled(app.dup_index > 0, egui::Button::new("◀"))
                    .clicked()
                {
                    app.dup_step(-1);
                }
                if ui
                    .add_enabled(
                        app.dup_index + 1 < app.dup_total,
                        egui::Button::new("▶"),
                    )
                    .clicked()
                {
                    app.dup_step(1);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("复制").clicked() {
                    ui.ctx()
                        .copy_text(text_of(bytes, mode_of(app, is_key), app.endian));
                }
                // 格式下拉
                let mut mode = mode_of(app, is_key);
                let ir = egui::ComboBox::from_id_salt(("detail_mode", is_key))
                    .selected_text(mode.label())
                    .height(430.0)
                    .show_ui(ui, |ui| {
                        for m in DecodeMode::ALL {
                            ui.selectable_value(&mut mode, m, m.label());
                        }
                    });
                super::wheel_cycle(ui.ctx(), &ir.response, &DecodeMode::ALL, &mut mode);
                set_mode(app, is_key, mode);
            });
        });

        // 自动模式时显示猜测的类型
        if mode_of(app, is_key) == DecodeMode::Auto {
            ui.weak(format!(
                "猜测：{}，{} 字节",
                fmt::guess(bytes, app.endian).0,
                bytes.len()
            ));
        } else {
            ui.weak(format!("{} 字节", bytes.len()));
        }

        // 文本视图（可折叠；长文本默认折叠，方便直接看 hex）
        let text = text_of(bytes, mode_of(app, is_key), app.endian);
        let default_open = text.chars().count() <= 512;
        egui::CollapsingHeader::new("文本")
            .id_salt(("detail_text", is_key, default_open))
            .default_open(default_open)
            .show(ui, |ui| {
                let mut text = text;
                ui.add(
                    egui::TextEdit::multiline(&mut text)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .desired_rows(2),
                );
            });

        // 十六进制视图
        egui::CollapsingHeader::new("十六进制")
            .id_salt(("detail_hex", is_key))
            .default_open(true)
            .show(ui, |ui| {
                let mut dump = fmt::hex_dump(
                    bytes,
                    app.hex_width,
                    app.show_addr,
                    app.show_hex,
                    app.show_ascii,
                );
                ui.add(
                    egui::TextEdit::multiline(&mut dump)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .desired_rows(4),
                );
            });
    });
}

fn mode_of(app: &MdbxerApp, is_key: bool) -> DecodeMode {
    if is_key { app.key_mode } else { app.val_mode }
}

fn set_mode(app: &mut MdbxerApp, is_key: bool, mode: DecodeMode) {
    if is_key {
        app.key_mode = mode;
    } else {
        app.val_mode = mode;
    }
}

fn text_of(bytes: &[u8], mode: DecodeMode, endian: fmt::Endian) -> String {
    fmt::decode(bytes, mode, endian, usize::MAX)
}
