//! Entry point for `imgtool` — Fast Rust `eframe` Image Viewer & Quick Annotation Tool.

pub mod annotations;
pub mod app;
pub mod canvas;
pub mod desktop;
pub mod history;
pub mod icon;
pub mod io;
pub mod render;

use eframe::egui;
use std::path::PathBuf;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn print_help() {
    println!(
        "imgtool {version} — Fast Image Viewer & Quick Annotation Tool (ksnip style)

USAGE:
    imgtool [OPTIONS] [IMAGE_FILE]

ARGUMENTS:
    [IMAGE_FILE]            Optional path to an image file to open immediately
                            (PNG, JPEG, WebP, GIF, BMP, ICO, TIFF)

OPTIONS:
    --install-desktop       Register imgtool as the Default Image Viewer on the current OS
                            (automatically uses the mise shim path so upgrades never break)
    --uninstall-desktop     Remove imgtool desktop file associations
    --print-stable-path     Print the resolved upgrade-safe executable path (mise shim)
    -h, --help              Print this help message and exit
    -V, --version           Print version information and exit

KEYBOARD SHORTCUTS:
    V / Esc                 Select, move, resize, or delete annotations
    P                       Pen (smooth freehand drawing)
    H                       Highlighter (semi-transparent marker)
    A                       Arrow tool
    L                       Line tool (hold Shift to snap angles)
    R                       Rectangle tool
    E                       Ellipse / Circle tool (hold Shift for circle)
    T                       Text annotation tool
    N                       Auto-incrementing Number Stamp (1, 2, 3...)
    B / X                   Blur / Pixelate / Blackout sensitive areas
    C                       Crop image (Enter to apply, Esc to cancel)
    Ctrl+S                  Quick Save as a new file (<stem>_edited_<timestamp>.png)
    Ctrl+Shift+S            Save As... (PNG, JPEG, WebP, BMP, TIFF)
    Ctrl+C                  Copy annotated image at 1:1 native resolution to Clipboard
    Ctrl+V                  Paste image from Clipboard
    Ctrl+Z / Ctrl+Y         Undo / Redo (annotations & canvas transforms)
    Left / Right Arrows     Navigate previous / next image in the same folder
    Scroll / Space+Drag     Cursor-centered Zoom / Pan (F = Fit, 1 = 100%)",
        version = VERSION
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut image_path: Option<PathBuf> = None;

    for arg in &args {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                return;
            }
            "-V" | "--version" => {
                println!("imgtool {}", VERSION);
                return;
            }
            "--install-desktop" => match desktop::install_desktop() {
                Ok(msg) => {
                    println!("{}", msg);
                    return;
                }
                Err(err) => {
                    eprintln!("Error: {}", err);
                    std::process::exit(1);
                }
            },
            "--uninstall-desktop" => match desktop::uninstall_desktop() {
                Ok(msg) => {
                    println!("{}", msg);
                    return;
                }
                Err(err) => {
                    eprintln!("Error: {}", err);
                    std::process::exit(1);
                }
            },
            "--print-stable-path" => match desktop::stable_path() {
                Ok(p) => {
                    println!("{}", p.display());
                    return;
                }
                Err(err) => {
                    eprintln!("Error: {}", err);
                    std::process::exit(1);
                }
            },
            s if s.starts_with('-') => {
                eprintln!("Unknown option: {}\nRun 'imgtool --help' for usage.", s);
                std::process::exit(2);
            }
            path_str => {
                if image_path.is_none() {
                    let cleaned = path_str.strip_prefix("file://").unwrap_or(path_str);
                    image_path = Some(PathBuf::from(cleaned));
                }
            }
        }
    }

    let viewport = egui::ViewportBuilder::default()
        .with_title("ImgTool — Image Viewer & Quick Annotation Tool")
        .with_inner_size([1240.0, 820.0])
        .with_min_inner_size([640.0, 460.0])
        .with_app_id("imgtool")
        .with_drag_and_drop(true)
        .with_icon(icon::window_icon());

    let native_options = eframe::NativeOptions {
        viewport,
        centered: true,
        ..Default::default()
    };

    if let Err(err) = eframe::run_native(
        "ImgTool",
        native_options,
        Box::new(move |cc| Ok(Box::new(app::ImgToolApp::new(cc, image_path)))),
    ) {
        eprintln!("Failed to launch ImgTool GUI: {}", err);
        std::process::exit(1);
    }
}
