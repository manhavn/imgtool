//! Image loading, exporting, Quick Save (`Ctrl+S`), Save As (`Ctrl+Shift+S`),
//! Clipboard Copy (`Ctrl+C` + `wl-copy` Wayland persistence fallback) & Paste (`Ctrl+V`),
//! and sibling image folder navigation (`◀` / `▶`).

use chrono::Local;
use image::{ImageFormat, RgbImage, Rgba, RgbaImage};
use std::borrow::Cow;
#[cfg(target_os = "linux")]
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
#[cfg(target_os = "linux")]
use std::process::{Command, Stdio};

/// Supported image file extensions for opening and folder navigation.
pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "gif", "bmp", "ico", "tiff", "tif",
];

/// Checks if a path has a supported image file extension.
pub fn is_supported_image_file(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|e| e.to_str())
            .map(|ext| {
                SUPPORTED_EXTENSIONS
                    .iter()
                    .any(|&s| s.eq_ignore_ascii_case(ext))
            })
            .unwrap_or(false)
}

/// Loads an image file from disk into an `RgbaImage`.
pub fn load_image_from_path(path: &Path) -> Result<RgbaImage, String> {
    let bytes = std::fs::read(path)
        .map_err(|e| format!("Failed to read '{}': {}", path.display(), e))?;
    let img = image::load_from_memory(&bytes)
        .map_err(|e| format!("Failed to decode '{}': {}", path.display(), e))?;
    Ok(img.into_rgba8())
}

/// Creates a blank dark slate canvas when `imgtool` is launched without an image argument,
/// ready for `Ctrl+V` clipboard paste, drag-and-drop, or immediate sketching.
pub fn create_blank_canvas(width: u32, height: u32) -> RgbaImage {
    RgbaImage::from_pixel(width, height, Rgba([248, 250, 252, 255]))
}

/// Computes the non-destructive Quick Save output path:
/// `<original-stem>_edited_<YYYYMMDD_HHMMSS>.png` in the same folder as `source_path`
/// (or in `~/Pictures` / current directory if pasted from clipboard).
pub fn build_quick_save_path(source_path: Option<&Path>) -> PathBuf {
    let timestamp = Local::now().format("%Y%m%d_%H%M%S");
    let (dir, stem) = match source_path {
        Some(p) => {
            let parent = p
                .parent()
                .filter(|d| !d.as_os_str().is_empty())
                .map(Path::to_path_buf)
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
            let stem = p
                .file_stem()
                .and_then(|s| s.to_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("image");
            (parent, stem.to_string())
        }
        None => {
            let default_dir = default_pictures_dir();
            (default_dir, "clipboard".to_string())
        }
    };

    let filename = format!("{}_edited_{}.png", stem, timestamp);
    let candidate = dir.join(&filename);
    if !candidate.exists() {
        return candidate;
    }

    // Disambiguate if saved multiple times within the same second
    for idx in 2..1000 {
        let alt = dir.join(format!("{}_edited_{}_{}.png", stem, timestamp, idx));
        if !alt.exists() {
            return alt;
        }
    }
    candidate
}

fn default_pictures_dir() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let pics = PathBuf::from(&home).join("Pictures");
        if pics.is_dir() {
            return pics;
        }
        return PathBuf::from(home);
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Saves `image` to `dest_path`, automatically selecting the format from the file extension
/// (`png`, `jpg`/`jpeg`, `webp`, `bmp`, `ico`, `tiff`, `gif`; defaults to PNG).
pub fn save_image_to_path(image: &RgbaImage, dest_path: &Path) -> Result<(), String> {
    if let Some(parent) = dest_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| {
                format!(
                    "Failed to create directory '{}': {}",
                    parent.display(),
                    e
                )
            })?;
        }
    }

    let ext = dest_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_ascii_lowercase();

    match ext.as_str() {
        "jpg" | "jpeg" => {
            // JPEG does not support alpha channel; composite directly into RGB8 onto white
            let rgb = rgba_to_rgb_white_bg(image);
            rgb.save_with_format(dest_path, ImageFormat::Jpeg)
                .map_err(|e| format!("Failed to save JPEG '{}': {}", dest_path.display(), e))
        }
        "webp" => image
            .save_with_format(dest_path, ImageFormat::WebP)
            .map_err(|e| format!("Failed to save WebP '{}': {}", dest_path.display(), e)),
        "bmp" => image
            .save_with_format(dest_path, ImageFormat::Bmp)
            .map_err(|e| format!("Failed to save BMP '{}': {}", dest_path.display(), e)),
        "gif" => image
            .save_with_format(dest_path, ImageFormat::Gif)
            .map_err(|e| format!("Failed to save GIF '{}': {}", dest_path.display(), e)),
        "ico" => image
            .save_with_format(dest_path, ImageFormat::Ico)
            .map_err(|e| format!("Failed to save ICO '{}': {}", dest_path.display(), e)),
        "tif" | "tiff" => image
            .save_with_format(dest_path, ImageFormat::Tiff)
            .map_err(|e| format!("Failed to save TIFF '{}': {}", dest_path.display(), e)),
        _ => image
            .save_with_format(dest_path, ImageFormat::Png)
            .map_err(|e| format!("Failed to save PNG '{}': {}", dest_path.display(), e)),
    }
}

fn rgba_to_rgb_white_bg(image: &RgbaImage) -> RgbImage {
    let (w, h) = image.dimensions();
    let raw = image.as_raw();
    let mut out = vec![255u8; (w as usize) * (h as usize) * 3];
    for (src, dst) in raw.chunks_exact(4).zip(out.chunks_exact_mut(3)) {
        let a = src[3] as u32;
        if a == 255 {
            dst[0] = src[0];
            dst[1] = src[1];
            dst[2] = src[2];
        } else if a > 0 {
            let inv = 255 - a;
            dst[0] = ((src[0] as u32 * a + 255 * inv + 127) / 255) as u8;
            dst[1] = ((src[1] as u32 * a + 255 * inv + 127) / 255) as u8;
            dst[2] = ((src[2] as u32 * a + 255 * inv + 127) / 255) as u8;
        }
    }
    RgbImage::from_raw(w, h, out).expect("buffer size matches w * h * 3")
}

/// Copies the rendered `RgbaImage` at full 1:1 resolution into the system clipboard.
///
/// On Linux Wayland, also attempts `wl-copy --type image/png` if installed so clipboard
/// contents persist even after the user closes `imgtool`.
pub fn copy_image_to_clipboard(image: &RgbaImage) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    if is_wayland_session() {
        if try_wl_copy_png(image).is_ok() {
            // Also populate arboard in case XWayland apps read from X11 selection
            let _ = copy_via_arboard(image);
            return Ok(());
        }
    }

    copy_via_arboard(image)
}

fn copy_via_arboard(image: &RgbaImage) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|e| format!("Clipboard unavailable: {}", e))?;
    let data = arboard::ImageData {
        width: image.width() as usize,
        height: image.height() as usize,
        bytes: Cow::Borrowed(image.as_raw()),
    };
    clipboard
        .set_image(data)
        .map_err(|e| format!("Failed to copy image to clipboard: {}", e))
}

#[cfg(target_os = "linux")]
fn is_wayland_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty())
}

#[cfg(target_os = "linux")]
fn try_wl_copy_png(image: &RgbaImage) -> Result<(), String> {
    let mut png_bytes = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)
        .map_err(|e| e.to_string())?;

    let mut child = Command::new("wl-copy")
        .args(["--type", "image/png"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(&png_bytes).map_err(|e| e.to_string())?;
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("wl-copy exited with non-zero status".to_string())
    }
}

/// Reads an image from the system clipboard (`Ctrl+V`).
pub fn paste_image_from_clipboard() -> Result<RgbaImage, String> {
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        if let Ok(img_data) = clipboard.get_image() {
            let width = img_data.width as u32;
            let height = img_data.height as u32;
            let raw = img_data.bytes.into_owned();
            if let Some(rgba) = RgbaImage::from_raw(width, height, raw) {
                return Ok(rgba);
            }
        }
        // Check if clipboard contains a file path to an image
        if let Ok(text) = clipboard.get_text() {
            let trimmed = text.trim().trim_start_matches("file://");
            let path = Path::new(trimmed);
            if is_supported_image_file(path) {
                return load_image_from_path(path);
            }
        }
    }

    #[cfg(target_os = "linux")]
    if is_wayland_session() {
        if let Ok(img) = try_wl_paste_image() {
            return Ok(img);
        }
    }

    Err("Clipboard does not contain a valid image".to_string())
}

#[cfg(target_os = "linux")]
fn try_wl_paste_image() -> Result<RgbaImage, String> {
    let output = Command::new("wl-paste")
        .args(["--type", "image/png"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() || output.stdout.is_empty() {
        return Err("wl-paste returned no PNG data".to_string());
    }
    let decoded = image::load_from_memory(&output.stdout).map_err(|e| e.to_string())?;
    Ok(decoded.into_rgba8())
}

/// Lists all supported sibling image files in the same directory as `current_file`,
/// sorted alphabetically (case-insensitive) for `◀` / `▶` folder navigation.
pub fn list_sibling_images(current_file: &Path) -> (Vec<PathBuf>, Option<usize>) {
    let Some(parent) = current_file.parent() else {
        return (Vec::new(), None);
    };
    let dir = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };

    let Ok(entries) = std::fs::read_dir(dir) else {
        return (Vec::new(), None);
    };

    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| is_supported_image_file(p))
        .collect();

    files.sort_by(|a, b| {
        let an = a.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let bn = b.file_name().and_then(|s| s.to_str()).unwrap_or("");
        an.bytes()
            .map(|c| c.to_ascii_lowercase())
            .cmp(bn.bytes().map(|c| c.to_ascii_lowercase()))
            .then_with(|| a.cmp(b))
    });

    let index = files.iter().position(|p| p == current_file).or_else(|| {
        let canon_cur = std::fs::canonicalize(current_file).ok()?;
        files
            .iter()
            .position(|p| std::fs::canonicalize(p).ok().as_ref() == Some(&canon_cur))
    });

    (files, index)
}

/// Opens the native system file picker to open an image.
pub fn pick_open_image_file(start_dir: Option<&Path>) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().add_filter(
        "Images (PNG, JPEG, WebP, GIF, BMP, ICO, TIFF)",
        SUPPORTED_EXTENSIONS,
    );
    if let Some(dir) = start_dir {
        let folder = if dir.is_dir() {
            dir
        } else {
            dir.parent().unwrap_or_else(|| Path::new("."))
        };
        if folder.is_dir() {
            dialog = dialog.set_directory(folder);
        }
    }
    dialog.pick_file()
}

/// Opens the native system Save As dialog (`PNG`, `JPEG`, `WebP`).
pub fn pick_save_image_file(source_path: Option<&Path>) -> Option<PathBuf> {
    let default_target = build_quick_save_path(source_path);
    let default_name = default_target
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("edited.png");

    let mut dialog = rfd::FileDialog::new()
        .set_file_name(default_name)
        .add_filter("PNG Image (*.png)", &["png"])
        .add_filter("JPEG Image (*.jpg, *.jpeg)", &["jpg", "jpeg"])
        .add_filter("WebP Image (*.webp)", &["webp"])
        .add_filter("BMP Image (*.bmp)", &["bmp"])
        .add_filter("TIFF Image (*.tiff)", &["tiff", "tif"]);

    if let Some(parent) = default_target.parent() {
        if parent.is_dir() {
            dialog = dialog.set_directory(parent);
        }
    }
    dialog.save_file()
}

/// Reveals a saved file's parent folder in the system file manager.
pub fn reveal_in_folder(file_path: &Path) -> Result<(), String> {
    let folder = file_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    open::that(folder).map_err(|e| format!("Could not open folder '{}': {}", folder.display(), e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_save_never_overwrites_original_and_uses_timestamp_pattern() {
        let orig = Path::new("/tmp/screenshot.jpg");
        let saved = build_quick_save_path(Some(orig));
        assert_ne!(saved, orig);
        let name = saved.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with("screenshot_edited_"));
        assert!(name.ends_with(".png"));
    }

    #[test]
    fn sibling_navigation_finds_sorted_supported_images() {
        let dir = std::env::temp_dir().join(format!("imgtool_nav_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let img = RgbaImage::from_pixel(4, 4, Rgba([255, 0, 0, 255]));
        let p_b = dir.join("b_photo.png");
        let p_a = dir.join("a_photo.jpg");
        let p_txt = dir.join("notes.txt");

        save_image_to_path(&img, &p_b).unwrap();
        save_image_to_path(&img, &p_a).unwrap();
        std::fs::write(&p_txt, b"ignore").unwrap();

        let (siblings, idx) = list_sibling_images(&p_b);
        assert_eq!(siblings.len(), 2);
        assert_eq!(siblings[0].file_name().unwrap(), "a_photo.jpg");
        assert_eq!(siblings[1].file_name().unwrap(), "b_photo.png");
        assert_eq!(idx, Some(1));

        std::fs::remove_dir_all(&dir).ok();
    }
}
