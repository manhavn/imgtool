//! Embedded application icon and procedural 128x128 RGBA / PNG generator for window and desktop integration.

use eframe::egui;
use image::{ImageFormat, RgbaImage};
use std::io::Cursor;

/// Raw SVG source embedded from `assets/icon.svg` for scalable Linux desktop icon installation.
pub const ICON_SVG: &str = include_str!("../assets/icon.svg");

/// Generates a crisp RGBA application icon of `size × size` matching `assets/icon.svg`.
pub fn generate_icon_rgba(size: u32) -> RgbaImage {
    let mut img = RgbaImage::new(size, size);
    let raw = img.as_mut();
    let s = size as f32;
    let inv_s = 128.0 / s;

    for y in 0..size {
        let fy = (y as f32 + 0.5) * inv_s;
        let dy = (fy - 64.0).abs() - 32.0;
        let dy_pos_sq = dy.max(0.0) * dy.max(0.0);
        let row_offset = (y * size * 4) as usize;

        for x in 0..size {
            let fx = (x as f32 + 0.5) * inv_s;

            // Rounded rectangle SDF: center (64, 64), half-size 58, radius 26
            let dx = (fx - 64.0).abs() - 32.0;
            let dist = (dx.max(0.0) * dx.max(0.0) + dy_pos_sq).sqrt() + dx.max(dy).min(0.0) - 26.0;

            if dist > 1.0 {
                continue;
            }
            let edge_alpha = (1.0 - dist).clamp(0.0, 1.0);

            // Background slate gradient (#0f172a -> #1e293b)
            let t = ((fx + fy) / 256.0).clamp(0.0, 1.0);
            let mut r = lerp(15.0, 30.0, t);
            let mut g = lerp(23.0, 41.0, t);
            let mut b = lerp(42.0, 59.0, t);

            // Outer border (#38bdf8)
            if dist > -3.5 {
                let bt = ((dist + 3.5) / 2.5).clamp(0.0, 1.0);
                r = lerp(r, 56.0, bt);
                g = lerp(g, 189.0, bt);
                b = lerp(b, 248.0, bt);
            }

            // Sun circle at (46, 48), r = 9.5 (#fbbf24)
            let sun_dx = fx - 46.0;
            let sun_dy = fy - 48.0;
            let sun_d_sq = sun_dx * sun_dx + sun_dy * sun_dy;
            if sun_d_sq < 110.25 {
                let sun_d = sun_d_sq.sqrt() - 9.5;
                let st = (1.0 - sun_d).clamp(0.0, 1.0);
                r = lerp(r, 251.0, st);
                g = lerp(g, 191.0, st);
                b = lerp(b, 36.0, st);
            }

            // Mountains polygon in bottom half (#38bdf8 -> #0284c7)
            if fy <= 92.0 && fx >= 28.0 && fx <= 100.0 {
                let m1 = 92.0 - (30.0 / 24.0) * (fx - 28.0).abs(); // Peak at (52, 62)
                let m2 = 92.0 - (26.0 / 16.0) * (fx - 84.0).abs(); // Peak at (84, 66)
                let mountain_top = m1.min(m2);
                let md = fy - mountain_top;
                if md > -1.0 {
                    let mt = (md + 1.0).clamp(0.0, 1.0) * 0.92;
                    let grad = ((fy - 62.0) / 30.0).clamp(0.0, 1.0);
                    let mr = lerp(56.0, 2.0, grad);
                    let mg = lerp(189.0, 132.0, grad);
                    let mb = lerp(248.0, 199.0, grad);
                    r = lerp(r, mr, mt);
                    g = lerp(g, mg, mt);
                    b = lerp(b, mb, mt);
                }
            }

            // Corner crop brackets (#38bdf8) — fast bounding-box pre-check
            if (22.0..=106.0).contains(&fx)
                && (22.0..=106.0).contains(&fy)
                && !((45.5..=82.5).contains(&fx) && (45.5..=82.5).contains(&fy))
            {
                let bracket_d = min_bracket_dist(fx, fy);
                if bracket_d < 2.8 {
                    let ct = (2.8 - bracket_d).clamp(0.0, 1.0);
                    r = lerp(r, 56.0, ct);
                    g = lerp(g, 189.0, ct);
                    b = lerp(b, 248.0, ct);
                }
            }

            // Annotation diagonal arrow (#f43f5e) from (38, 82) to (84, 40)
            if (33.0..=93.0).contains(&fx) && (31.0..=87.0).contains(&fy) {
                let seg_d = dist_to_segment(fx, fy, 38.0, 82.0, 82.0, 42.0) - 3.6;
                let tri_d = dist_to_triangle(fx, fy, (66.0, 35.0), (91.0, 33.0), (88.0, 58.0));
                let arrow_d = seg_d.min(tri_d);
                if arrow_d < 1.0 {
                    let at = (1.0 - arrow_d).clamp(0.0, 1.0);
                    r = lerp(r, 244.0, at);
                    g = lerp(g, 63.0, at);
                    b = lerp(b, 94.0, at);
                }
            }

            let idx = row_offset + (x as usize) * 4;
            raw[idx] = (r + 0.5).clamp(0.0, 255.0) as u8;
            raw[idx + 1] = (g + 0.5).clamp(0.0, 255.0) as u8;
            raw[idx + 2] = (b + 0.5).clamp(0.0, 255.0) as u8;
            raw[idx + 3] = (edge_alpha * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
        }
    }

    img
}

/// Encodes an `RgbaImage` icon at `size × size` as a PNG byte vector.
pub fn generate_icon_png_sized(size: u32) -> Vec<u8> {
    let img = generate_icon_rgba(size);
    let mut buf = Vec::new();
    let _ = img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png);
    buf
}

/// Encodes the 128x128 icon as a PNG byte vector for desktop icon installation.
pub fn generate_icon_png() -> Vec<u8> {
    generate_icon_png_sized(128)
}

/// Generates a complete Apple `.icns` icon bundle containing multi-resolution PNG chunks
/// (`16x16`, `32x32`, `64x64`, `128x128`, `256x256`, and `512x512`) for macOS `ImgTool.app`.
pub fn generate_icon_icns() -> Vec<u8> {
    let base_256 = generate_icon_rgba(256);
    let entries: [(&[u8; 4], u32); 6] = [
        (b"icp4", 16),
        (b"icp5", 32),
        (b"icp6", 64),
        (b"ic07", 128),
        (b"ic08", 256),
        (b"ic09", 512),
    ];

    let mut chunks: Vec<([u8; 4], Vec<u8>)> = Vec::with_capacity(entries.len());
    for (ostype, sz) in entries {
        let scaled = if sz == 256 {
            base_256.clone()
        } else {
            image::imageops::resize(&base_256, sz, sz, image::imageops::FilterType::Triangle)
        };
        let mut png = Vec::new();
        let _ = scaled.write_to(&mut Cursor::new(&mut png), ImageFormat::Png);
        chunks.push((*ostype, png));
    }

    let total_len: u32 = 8 + chunks.iter().map(|(_, p)| 8 + p.len() as u32).sum::<u32>();
    let mut icns = Vec::with_capacity(total_len as usize);
    icns.extend_from_slice(b"icns");
    icns.extend_from_slice(&total_len.to_be_bytes());
    for (tag, png) in chunks {
        let chunk_len = (8 + png.len()) as u32;
        icns.extend_from_slice(&tag);
        icns.extend_from_slice(&chunk_len.to_be_bytes());
        icns.extend_from_slice(&png);
    }
    icns
}
/// Returns `egui::IconData` for the native window titlebar / taskbar.
pub fn window_icon() -> egui::IconData {
    let img = generate_icon_rgba(128);
    let (width, height) = img.dimensions();
    egui::IconData {
        rgba: img.into_raw(),
        width,
        height,
    }
}

/// On macOS, builds an `NSImage` backed by a `'static` pixel buffer (avoiding `eframe`'s
/// dangling temporary buffer bug) and optionally sets the custom Finder icon on `target_path`
/// (`ImgTool.app` or binary) via `NSWorkspace::setIcon:forFile:options:` as well as the running
/// `NSApplication` Dock icon.
#[cfg(target_os = "macos")]
pub fn apply_macos_native_icon(target_path: Option<&std::path::Path>) {
    use objc2::AnyThread as _;
    use objc2_app_kit::{
        NSApplication, NSBitmapImageRep, NSDeviceRGBColorSpace, NSImage,
        NSWorkspace, NSWorkspaceIconCreationOptions,
    };
    use objc2_foundation::{NSSize, NSString};
    use std::sync::LazyLock;

    static STATIC_ICON_256: LazyLock<Vec<u8>> =
        LazyLock::new(|| generate_icon_rgba(256).into_raw());

    unsafe extern "C" {
        static NSApp: Option<&'static NSApplication>;
    }

    let raw_ptr = STATIC_ICON_256.as_ptr().cast_mut();
    let mut planes = [raw_ptr];

    unsafe {
        let Some(rep) = NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            planes.as_mut_ptr(),
            256,
            256,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            256 * 4,
            32,
        ) else {
            return;
        };

        let ns_img = NSImage::initWithSize(NSImage::alloc(), NSSize::new(256.0, 256.0));
        ns_img.addRepresentation(&rep);

        if let Some(app) = NSApp {
            app.setApplicationIconImage(Some(&ns_img));
        }

        if let Some(path) = target_path {
            let path_str = NSString::from_str(&path.to_string_lossy());
            let workspace = NSWorkspace::sharedWorkspace();
            let _ = workspace.setIcon_forFile_options(
                Some(&ns_img),
                &path_str,
                NSWorkspaceIconCreationOptions::empty(),
            );
        }
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[inline]
fn dist_to_segment_sq(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let vx = bx - ax;
    let vy = by - ay;
    let wx = px - ax;
    let wy = py - ay;
    let len_sq = vx * vx + vy * vy;
    let t = if len_sq > 0.0 {
        ((wx * vx + wy * vy) / len_sq).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let dx = px - (ax + t * vx);
    let dy = py - (ay + t * vy);
    dx * dx + dy * dy
}

#[inline]
fn dist_to_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    dist_to_segment_sq(px, py, ax, ay, bx, by).sqrt()
}

fn min_bracket_dist(fx: f32, fy: f32) -> f32 {
    const SEGS: [(f32, f32, f32, f32); 8] = [
        (26.0, 42.0, 26.0, 26.0),
        (26.0, 26.0, 42.0, 26.0),
        (86.0, 26.0, 102.0, 26.0),
        (102.0, 26.0, 102.0, 42.0),
        (26.0, 86.0, 26.0, 102.0),
        (26.0, 102.0, 42.0, 102.0),
        (102.0, 86.0, 102.0, 102.0),
        (102.0, 102.0, 86.0, 102.0),
    ];
    SEGS.iter()
        .map(|&(ax, ay, bx, by)| dist_to_segment_sq(fx, fy, ax, ay, bx, by))
        .fold(f32::INFINITY, f32::min)
        .sqrt()
}

fn dist_to_triangle(px: f32, py: f32, a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> f32 {
    let d1 = sign((px, py), a, b);
    let d2 = sign((px, py), b, c);
    let d3 = sign((px, py), c, a);
    let has_neg = (d1 < 0.0) || (d2 < 0.0) || (d3 < 0.0);
    let has_pos = (d1 > 0.0) || (d2 > 0.0) || (d3 > 0.0);
    let inside = !(has_neg && has_pos);

    let edge_d = dist_to_segment_sq(px, py, a.0, a.1, b.0, b.1)
        .min(dist_to_segment_sq(px, py, b.0, b.1, c.0, c.1))
        .min(dist_to_segment_sq(px, py, c.0, c.1, a.0, a.1))
        .sqrt();
    if inside {
        -edge_d
    } else {
        edge_d
    }
}

fn sign(p1: (f32, f32), p2: (f32, f32), p3: (f32, f32)) -> f32 {
    (p1.0 - p3.0) * (p2.1 - p3.1) - (p2.0 - p3.0) * (p1.1 - p3.1)
}
