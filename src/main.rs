//! MDBXer — libmdbx 数据库查看工具（只读）。
//!
//! 分层：main → ui → db / fmt / history，单向依赖。

mod db;
mod fmt;
mod history;
mod ui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 760.0])
            .with_min_inner_size([900.0, 560.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "MDBXer",
        options,
        Box::new(|cc| {
            load_cjk_fonts(&cc.egui_ctx);
            let mut app = ui::MdbxerApp::new();
            // 支持命令行传入路径直接打开（也便于拖文件到 exe）
            if let Some(p) = std::env::args().nth(1) {
                app.path_input = p;
                app.open_db();
            }
            Ok(Box::new(app))
        }),
    )
}

/// 运行时从系统目录加载 CJK 字体，作为两族字体的 fallback。
fn load_cjk_fonts(ctx: &egui::Context) {
    const CANDIDATES: [&str; 5] = [
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\msjh.ttc",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\simsun.ttc",
        r"C:\Windows\Fonts\Deng.ttf",
    ];
    for path in CANDIDATES {
        let Ok(data) = std::fs::read(path) else { continue };
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "cjk".to_string(),
            std::sync::Arc::new(egui::FontData::from_owned(data)),
        );
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .push("cjk".to_string());
        }
        ctx.set_fonts(fonts);
        return;
    }
}
