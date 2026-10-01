//! MDBXer — libmdbx 数据库查看工具（只读）。
//!
//! 分层：main → ui → db / fmt / history，单向依赖。

// release 版不弹出控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

mod db;
mod fmt;
mod history;
mod ui;

/// 程序入口：设置窗口选项、加载 CJK 字体、启动 eframe 事件循环。
/// 支持命令行传入数据库路径直接打开（便于拖文件到 exe）。
fn main() -> eframe::Result<()> {
    let title = app_title();
    let app_title = title.clone();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 760.0])
            .with_min_inner_size([900.0, 560.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        &title,
        options,
        Box::new(move |cc| {
            load_cjk_fonts(&cc.egui_ctx);
            let mut app = ui::MdbxerApp::new(app_title);
            // 支持命令行传入路径直接打开（也便于拖文件到 exe）
            if let Some(p) = std::env::args().nth(1) {
                app.open_db(&p);
            }
            Ok(Box::new(app))
        }),
    )
}

/// 窗口标题："MDBXer v<版本> · <构建日期>"。
/// 版本取 Cargo.toml；日期取 exe 自身的修改时间（即本次构建/发布时间），
/// 无需 build.rs，跨平台。取不到时省略日期。
fn app_title() -> String {
    let date = std::env::current_exe()
        .and_then(|p| std::fs::metadata(p))
        .and_then(|m| m.modified())
        .ok()
        .map(|t| {
            let dt: chrono::DateTime<chrono::Local> = t.into();
            dt.format("%Y-%m-%d").to_string()
        });
    match date {
        Some(d) => format!("MDBXer v{} · {d}", env!("CARGO_PKG_VERSION")),
        None => format!("MDBXer v{}", env!("CARGO_PKG_VERSION")),
    }
}

/// 运行时从系统目录查找并加载一个 CJK 字体，作为两族字体的 fallback。
/// 候选路径全部按平台规则/环境变量动态生成，不写死盘符或用户目录。
fn load_cjk_fonts(ctx: &egui::Context) {
    let Some(path) = cjk_font_candidates().into_iter().find(|p| p.is_file()) else {
        eprintln!("未找到系统 CJK 字体，中文可能显示为方框（可安装 微软雅黑/苹方/Noto CJK）");
        return;
    };
    let Ok(data) = std::fs::read(&path) else {
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts
        .font_data
        .insert("cjk".to_string(), std::sync::Arc::new(
            egui::FontData::from_owned(data),
        ));
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("cjk".to_string());
    }
    ctx.set_fonts(fonts);
}

/// Windows：系统字体目录取 %WINDIR%\Fonts，并补查用户字体目录。
#[cfg(target_os = "windows")]
fn cjk_font_candidates() -> Vec<PathBuf> {
    const NAMES: [&str; 5] = [
        "msyh.ttc",   // 微软雅黑
        "msjh.ttc",   // 微软正黑体
        "simhei.ttf", // 黑体
        "simsun.ttc", // 宋体
        "Deng.tt",    // 等线
    ];
    let mut dirs = Vec::new();
    if let Some(windir) = std::env::var_os("WINDIR") {
        dirs.push(PathBuf::from(windir).join("Fonts"));
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        dirs.push(
            PathBuf::from(local)
                .join("Microsoft")
                .join("Windows")
                .join("Fonts"),
        );
    }
    let mut paths = Vec::new();
    for dir in &dirs {
        for name in NAMES {
            paths.push(dir.join(name));
        }
    }
    paths
}

/// macOS：系统自带中文字体（/System/Library/Fonts 为平台固定位置）。
#[cfg(target_os = "macos")]
fn cjk_font_candidates() -> Vec<PathBuf> {
    const PATHS: [&str; 5] = [
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
        "/System/Library/Fonts/Supplemental/Songti.ttc",
        "/Library/Fonts/Arial Unicode.ttf",
    ];
    PATHS.iter().map(PathBuf::from).collect()
}

/// Linux/BSD：遍历 FHS 与 XDG 字体目录下的常见 CJK 字体，
/// 最后用 fontconfig（fc-match）查询系统实际配置作为兜底。
#[cfg(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd"
))]
fn cjk_font_candidates() -> Vec<PathBuf> {
    const RELATIVE: [&str; 10] = [
        "opentype/noto/NotoSansCJK-Regular.ttc",
        "truetype/noto/NotoSansCJK-Regular.ttc",
        "opentype/noto/NotoSansCJKsc-Regular.otf",
        "truetype/noto/NotoSansCJKsc-Regular.otf",
        "opentype/google-noto-cjk/NotoSansCJK-Regular.ttc",
        "truetype/wqy/wqy-microhei.ttc",
        "truetype/wqy/wqy-zenhei.ttc",
        "truetype/droid/DroidSansFallbackFull.ttf",
        "truetype/droid/DroidSansFallback.ttf",
        "truetype/arphic/uming.ttc",
    ];
    let mut dirs: Vec<PathBuf> = vec![
        PathBuf::from("/usr/share/fonts"),
        PathBuf::from("/usr/local/share/fonts"),
    ];
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        dirs.push(PathBuf::from(xdg).join("fonts"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        dirs.push(home.join(".local").join("share").join("fonts"));
        dirs.push(home.join(".fonts"));
    }
    let mut paths = Vec::new();
    for dir in &dirs {
        for rel in RELATIVE {
            paths.push(dir.join(rel));
        }
    }
    if let Some(p) = fontconfig_cjk() {
        paths.push(p);
    }
    paths
}

/// 调用 `fc-match` 让 fontconfig 给出当前系统匹配中文的字体文件。
#[cfg(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd"
))]
fn fontconfig_cjk() -> Option<PathBuf> {
    let out = std::process::Command::new("fc-match")
        .args(["-f", "%{file}", ":lang=zh"])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then(|| PathBuf::from(s))
}

/// 其他平台：不内置路径（egui 默认字体无 CJK 字形，缺失时仅提示）。
#[cfg(not(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "linux",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd"
)))]
fn cjk_font_candidates() -> Vec<PathBuf> {
    Vec::new()
}
