//! 右侧详情：Key / Value 卡片（格式下拉、复制、文本、hex dump、多值翻页）。

use super::MdbxerApp;
use crate::fmt::{self, DecodeMode};

pub fn show(ui: &mut egui::Ui, app: &mut MdbxerApp) {
    let max_w = detail_max_width(ui, app);
    egui::Panel::right("detail_panel")
        .default_size(360.0)
        .size_range(240.0..=max_w)
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
    let dup_sort = app.cur_table().map(|t| t.dup_sort).unwrap_or(false);
    let save_name = if is_key {
        "key.bin".to_string()
    } else if dup_sort {
        format!("value_{:06}.bin", app.dup_index + 1)
    } else {
        "value.bin".to_string()
    };
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());

        ui.horizontal(|ui| {
            ui.strong(title);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("复制").clicked() {
                    ui.ctx()
                        .copy_text(text_of(bytes, mode_of(app, is_key), app.endian));
                }
                if ui
                    .button("另存…")
                    .on_hover_text("把完整原始字节保存为文件（不做任何截断）")
                    .clicked()
                {
                    app.save_bytes(&save_name, bytes);
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

        // 大字段分段：固定放在标题行正下方，避免随字节数/多值导航行数上下位移。
        // 每段 fmt::PAGE_BYTES 字节，超出时显示导航条。
        let total = bytes.len();
        let off = app.seg_off(is_key);
        let off = if off >= total {
            total.saturating_sub(fmt::PAGE_BYTES.min(total))
        } else {
            off
        };
        if total > fmt::PAGE_BYTES {
            ui.horizontal(|ui| {
                // 按钮/输入框固定在最左：段号与偏移文本长度会变，放前面会挤动按钮
                if ui
                    .add_enabled(off > 0, egui::Button::new("◀ 段"))
                    .on_hover_text("上一段（64 KiB）")
                    .clicked()
                {
                    app.seg_step(is_key, total, -1);
                }
                if ui
                    .add_enabled(
                        off + fmt::PAGE_BYTES < total,
                        egui::Button::new("段 ▶"),
                    )
                    .on_hover_text("下一段（64 KiB）")
                    .clicked()
                {
                    app.seg_step(is_key, total, 1);
                }
                let input = if is_key {
                    &mut app.key_seg_input
                } else {
                    &mut app.val_seg_input
                };
                let resp = ui.add(
                    egui::TextEdit::singleline(input)
                        .desired_width(84.0)
                        .hint_text("偏移/0x.."),
                );
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    app.seg_jump(is_key, total);
                }
                let seg_no = off / fmt::PAGE_BYTES + 1;
                let seg_cnt = (total + fmt::PAGE_BYTES - 1) / fmt::PAGE_BYTES;
                ui.weak(format!(
                    "第 {seg_no}/{seg_cnt} 段 · 偏移 {off} / {total}（0x{off:X}）"
                ));
            });
        }

        // 多值导航：仅 Value 卡片、多值表显示
        let is_dup = !is_key && app.cur_table().map(|t| t.dup_sort).unwrap_or(false);
        if is_dup {
            let (idx, total) = (app.dup_index, app.dup_total);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(idx > 0, egui::Button::new("⏮"))
                    .on_hover_text("第一个值")
                    .clicked()
                {
                    app.dup_goto(0);
                }
                if ui
                    .add_enabled(idx > 0, egui::Button::new("⏪"))
                    .on_hover_text("向前翻 100 个值")
                    .clicked()
                {
                    app.dup_page_step(-1);
                }
                if ui
                    .add_enabled(idx > 0, egui::Button::new("◀"))
                    .on_hover_text("上一个值")
                    .clicked()
                {
                    app.dup_step(-1);
                }
                if ui
                    .add_enabled(idx + 1 < total, egui::Button::new("▶"))
                    .on_hover_text("下一个值")
                    .clicked()
                {
                    app.dup_step(1);
                }
                if ui
                    .add_enabled(idx + 1 < total, egui::Button::new("⏩"))
                    .on_hover_text("向后翻 100 个值")
                    .clicked()
                {
                    app.dup_page_step(1);
                }
                if ui
                    .add_enabled(idx + 1 < total, egui::Button::new("⏭"))
                    .on_hover_text("最后一个值")
                    .clicked()
                {
                    app.dup_goto(total.saturating_sub(1));
                }
                ui.label("跳至");
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut app.dup_jump_input)
                        .desired_width(48.0)
                        .hint_text("#"),
                );
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    app.dup_jump();
                }
            });
            ui.horizontal(|ui| {
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut app.dup_search_input)
                        .desired_width(178.0)
                        .hint_text("搜索值：文本或 hex(...)"),
                );
                let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui
                    .button("↑")
                    .on_hover_text("向前查找（值子串；到头回绕）")
                    .clicked()
                {
                    app.dup_search(false);
                }
                if ui
                    .button("↓")
                    .on_hover_text("向后查找（回车等效；到头回绕）")
                    .clicked() || enter
                {
                    app.dup_search(true);
                }
            });
            ui.add_space(2.0);
        }

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

        // 分段窗口（导航条固定在卡片标题行正下方）
        let end = (off + fmt::PAGE_BYTES).min(total);
        let window = &bytes[off..end];

        // 文本视图（可折叠；长文本默认折叠，方便直接看 hex）。
        // 高度按内容自适应：行少就收缩，超过 16 行封顶并出滚动条。
        let text = fmt::decode(
            window,
            mode_of(app, is_key),
            app.endian,
            window.len() * 4 + 16,
        );
        let text_rows = text.lines().count().clamp(1, 16);
        let default_open = total <= 512;
        egui::CollapsingHeader::new("文本")
            .id_salt(("detail_text", is_key, default_open))
            .default_open(default_open)
            .show(ui, |ui| {
                let mut text = text;
                ui.add(
                    egui::TextEdit::multiline(&mut text)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .desired_rows(text_rows),
                );
            });

        // 十六进制视图：高度随段内实际行数自适应（1 行数据就 1 行高），
        // 超过 20 行封顶并出滚动条，避免整段（宽 8 时最多 8192 行）撑爆。
        let dump = fmt::hex_dump(
            window,
            app.hex_width,
            app.show_addr,
            app.show_hex,
            app.show_ascii,
            off,
        );
        let hex_rows = dump.lines().count().clamp(1, 20);
        egui::CollapsingHeader::new("十六进制")
            .id_salt(("detail_hex", is_key))
            .default_open(true)
            .show(ui, |ui| {
                let mut dump = dump;
                ui.add(
                    egui::TextEdit::multiline(&mut dump)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .desired_rows(hex_rows),
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

/// 右栏宽度上限：保证当前 hex 配置下最长一行（32 字节时最宽）
/// 在面板内不折行。按等宽字体实测字宽计算，再扣除各级边距；
/// 同时不超过窗口宽度减去给左栏+表格保留的 300 点。
fn detail_max_width(ui: &egui::Ui, app: &MdbxerApp) -> f32 {
    let font_id = egui::TextStyle::Monospace.resolve(ui.style());
    let char_w = ui.fonts_mut(|f| f.glyph_width(&font_id, '0'));

    let n = app.hex_width as f32;
    let mut line_chars = 0.0_f32;
    if app.show_addr {
        // 8 位十六进制地址 + 列后 2 空格
        line_chars += 10.0;
    }
    if app.show_hex {
        // 每字节 "XX "，宽行中间额外 1 个分隔空格
        line_chars += n * 3.0 + if app.hex_width >= 8 { 1.0 } else { 0.0 };
    }
    if app.show_ascii {
        line_chars += n;
    }
    // 面板边框/分组 frame/折叠缩进/文本框内边距与滚动条余量
    const CHROME: f32 = 100.0;
    let needed = line_chars * char_w + CHROME;

    let screen = ui.ctx().viewport_rect().width();
    needed
        .max(720.0)
        .min((screen - 300.0).max(720.0))
}
