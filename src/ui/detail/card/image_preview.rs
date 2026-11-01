// Copyright 2026 lwq_yu
// SPDX-License-Identifier: Apache-2.0

//! 图片预览：字节魔数命中 PNG/JPEG/GIF/WebP/ICO 时，卡片标题行出现预览按钮，
//! 点击后弹窗显示解码后的图像。大图先缩略到 [`MAX_SIDE`]，纹理按内容指纹缓存，
//! 同一侧只保留一张，避免显存随浏览记录累积。

use super::super::state::Side;
use crate::i18n::tr;

/// 解码后最长边上限（预览不需要原始分辨率，避免大图占显存）。
const MAX_SIDE: u32 = 1024;
/// 弹窗内图像的显示尺寸上限（等比缩放到该框内）。
const FIT: f32 = 520.0;

/// 魔数是否命中可解码的图片格式。
pub(super) fn is_image(bytes: &[u8]) -> bool {
    matches!(
        image::guess_format(bytes),
        Ok(image::ImageFormat::Png
            | image::ImageFormat::Jpeg
            | image::ImageFormat::Gif
            | image::ImageFormat::WebP
            | image::ImageFormat::Ico)
    )
}

/// 标题行的预览按钮与预览弹窗；非图片字节不显示按钮。
pub(super) fn button(ui: &mut egui::Ui, side: Side, bytes: &[u8]) {
    if !is_image(bytes) {
        return;
    }
    let t = tr();
    let open_id = egui::Id::new(("mdbxer_image_open", side.is_key()));
    if ui.button("🖼").on_hover_text(t.image_preview_tip).clicked() {
        ui.ctx().data_mut(|d| d.insert_temp(open_id, true));
    }

    let mut open = ui
        .ctx()
        .data(|d| d.get_temp::<bool>(open_id))
        .unwrap_or(false);
    if !open {
        return;
    }
    let tex_id = egui::Id::new(("mdbxer_image_texture", side.is_key()));
    egui::Window::new(t.image_preview)
        .id(egui::Id::new(("mdbxer_image_window", side.is_key())))
        .open(&mut open)
        .default_width(FIT + 24.0)
        .resizable(true)
        .show(ui.ctx(), |ui| match texture(ui.ctx(), tex_id, bytes) {
            Ok(tex) => {
                let sz = tex.size_vec2();
                // 小图放大不超过 4 倍，大图等比缩进 FIT 框
                let scale = (FIT / sz.x).min(FIT / sz.y).min(4.0);
                ui.add(egui::Image::new(egui::load::SizedTexture::new(
                    tex.id(),
                    sz * scale,
                )));
            }
            Err(e) => {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
        });
    if !open {
        ui.ctx().data_mut(|d| {
            d.insert_temp(open_id, false);
            d.insert_temp(tex_id, None::<(u64, egui::TextureHandle)>);
        });
    }
}

/// 字节内容指纹（与文本指纹同算法，仅入参不同）。
fn bytes_fp(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

/// 取（或按内容指纹缓存）解码后的纹理；同一侧只保留一张，避免显存累积。
fn texture(ctx: &egui::Context, id: egui::Id, bytes: &[u8]) -> Result<egui::TextureHandle, String> {
    let fp = bytes_fp(bytes);
    if let Some((stored_fp, tex)) = ctx
        .data(|d| d.get_temp::<Option<(u64, egui::TextureHandle)>>(id))
        .flatten()
        && stored_fp == fp
    {
        return Ok(tex);
    }

    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let img = if img.width() > MAX_SIDE || img.height() > MAX_SIDE {
        img.thumbnail(MAX_SIDE, MAX_SIDE)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
    let tex = ctx.load_texture("mdbxer_image_preview", color, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, Some((fp, tex.clone()))));
    Ok(tex)
}
