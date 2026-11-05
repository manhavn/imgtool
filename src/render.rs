//! 1:1 native-resolution software rasterizer combining the base `RgbaImage`
//! with all vector annotations (`Pen`, `Highlighter`, `Arrow`, `Line`, `Rectangle`,
//! `Ellipse`, `Text`, `NumberStamp`, `Redact/Pixelate`).
//!
//! When saving to disk or copying to the clipboard, annotations are rendered directly
//! at the native pixel resolution of the image — never degraded by viewport zoom.

use crate::annotations::{
    compute_arrow_geometry, Annotation, AnnotationKind, BBox, Pt, RedactStyle,
};
use ab_glyph::{point, Font, FontRef, PxScale, ScaleFont};
use eframe::egui::Color32;
use image::RgbaImage;
use std::sync::LazyLock;

/// Renders the base image and all annotations at 1:1 native image resolution.
pub fn render_final_image(base: &RgbaImage, annotations: &[Annotation]) -> RgbaImage {
    let mut canvas = base.clone();
    for ann in annotations {
        render_annotation(&mut canvas, ann);
    }
    canvas
}

static DEFAULT_FONT: LazyLock<FontRef<'static>> = LazyLock::new(|| {
    FontRef::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT)
        .or_else(|_| FontRef::try_from_slice(epaint_default_fonts::HACK_REGULAR))
        .expect("embedded epaint_default_fonts must be valid TrueType fonts")
});

fn default_font() -> &'static FontRef<'static> {
    &DEFAULT_FONT
}

/// Renders a single annotation onto `canvas`.
pub fn render_annotation(canvas: &mut RgbaImage, ann: &Annotation) {
    match &ann.kind {
        AnnotationKind::Freehand {
            points,
            is_highlighter,
        } => {
            if points.is_empty() {
                return;
            }
            let color = if *is_highlighter {
                Color32::from_rgba_unmultiplied(ann.color.r(), ann.color.g(), ann.color.b(), 96)
            } else {
                ann.color
            };
            let stroke = if *is_highlighter {
                (ann.stroke_width * 2.8).max(12.0)
            } else {
                ann.stroke_width.max(1.5)
            };

            if points.len() == 1 {
                draw_filled_circle(canvas, points[0], stroke * 0.5, color);
            } else if !*is_highlighter && points.len() == 2 {
                draw_capsule_segment(canvas, points[0], points[1], stroke * 0.5, color);
            } else {
                // Render multi-segment strokes via per-segment coverage mask to avoid
                // O(W*H*N) distance checks and eliminate joint overdraw.
                draw_uniform_polyline(canvas, points, stroke, color);
            }
        }
        AnnotationKind::Line { start, end } => {
            draw_capsule_segment(canvas, *start, *end, (ann.stroke_width * 0.5).max(1.0), ann.color);
        }
        AnnotationKind::Arrow { start, end } => {
            let (tip, left, right, shaft_end) =
                compute_arrow_geometry(*start, *end, ann.stroke_width);
            draw_capsule_segment(
                canvas,
                *start,
                shaft_end,
                (ann.stroke_width * 0.5).max(1.0),
                ann.color,
            );
            draw_filled_triangle(canvas, tip, left, right, ann.color);
        }
        AnnotationKind::Rectangle { min, max, filled } => {
            let b = BBox::from_points(*min, *max);
            if *filled {
                let fill = Color32::from_rgba_unmultiplied(
                    ann.color.r(),
                    ann.color.g(),
                    ann.color.b(),
                    56,
                );
                draw_filled_rect(canvas, b, fill);
            }
            draw_stroked_rect(canvas, b, ann.stroke_width.max(1.5), ann.color);
        }
        AnnotationKind::Ellipse { min, max, filled } => {
            let b = BBox::from_points(*min, *max);
            if *filled {
                let fill = Color32::from_rgba_unmultiplied(
                    ann.color.r(),
                    ann.color.g(),
                    ann.color.b(),
                    56,
                );
                draw_ellipse(canvas, b, 0.0, true, fill);
            }
            draw_ellipse(canvas, b, ann.stroke_width.max(1.5), false, ann.color);
        }
        AnnotationKind::Text {
            pos,
            text,
            font_size,
            bg_box,
        } => {
            draw_text_annotation(canvas, *pos, text, *font_size, ann.color, *bg_box);
        }
        AnnotationKind::NumberStamp {
            center,
            number,
            radius,
        } => {
            draw_number_stamp(canvas, *center, *number, *radius, ann.color);
        }
        AnnotationKind::Redact {
            min,
            max,
            block_size,
            style,
        } => {
            let b = BBox::from_points(*min, *max);
            match style {
                RedactStyle::Pixelate => apply_pixelate_region(canvas, b, (*block_size).max(4)),
                RedactStyle::Blackout => draw_filled_rect(canvas, b, ann.color),
            }
        }
    }
}

/// Fast integer / fixed-point RGBA pixel blender operating directly on a 4-byte pixel slice.
#[inline(always)]
fn blend_rgba_alpha(dst: &mut [u8], sr: u32, sg: u32, sb: u32, alpha: u32) {
    if alpha == 0 {
        return;
    }
    if alpha >= 255 {
        dst[0] = sr as u8;
        dst[1] = sg as u8;
        dst[2] = sb as u8;
        dst[3] = 255;
        return;
    }
    let da = dst[3] as u32;
    if da == 255 {
        let inv = 255 - alpha;
        dst[0] = ((sr * alpha + dst[0] as u32 * inv + 127) / 255) as u8;
        dst[1] = ((sg * alpha + dst[1] as u32 * inv + 127) / 255) as u8;
        dst[2] = ((sb * alpha + dst[2] as u32 * inv + 127) / 255) as u8;
        return;
    }
    let src_a = alpha as f32 / 255.0;
    let dst_a = da as f32 / 255.0;
    let out_a = src_a + dst_a * (1.0 - src_a);
    if out_a <= 0.001 {
        return;
    }
    let inv_out = 1.0 / out_a;
    dst[0] = ((sr as f32 * src_a + dst[0] as f32 * dst_a * (1.0 - src_a)) * inv_out + 0.5)
        .clamp(0.0, 255.0) as u8;
    dst[1] = ((sg as f32 * src_a + dst[1] as f32 * dst_a * (1.0 - src_a)) * inv_out + 0.5)
        .clamp(0.0, 255.0) as u8;
    dst[2] = ((sb as f32 * src_a + dst[2] as f32 * dst_a * (1.0 - src_a)) * inv_out + 0.5)
        .clamp(0.0, 255.0) as u8;
    dst[3] = (out_a * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
}

/// Blends an unmultiplied `Color32` with an additional anti-aliasing coverage factor `cov` (0.0..=1.0)
/// onto a pixel in `canvas`.
#[inline(always)]
fn blend_pixel(canvas: &mut RgbaImage, x: i32, y: i32, color: Color32, cov: f32) {
    if x < 0 || y < 0 {
        return;
    }
    let (ux, uy) = (x as u32, y as u32);
    let w = canvas.width();
    let h = canvas.height();
    if ux >= w || uy >= h {
        return;
    }
    let alpha = (color.a() as f32 * cov.clamp(0.0, 1.0) + 0.5) as u32;
    if alpha == 0 {
        return;
    }
    let idx = ((uy * w + ux) * 4) as usize;
    let raw = canvas.as_mut();
    blend_rgba_alpha(
        &mut raw[idx..idx + 4],
        color.r() as u32,
        color.g() as u32,
        color.b() as u32,
        alpha,
    );
}

fn draw_filled_circle(canvas: &mut RgbaImage, center: Pt, radius: f32, color: Color32) {
    let w = canvas.width() as i32;
    let h = canvas.height() as i32;
    if w <= 0 || h <= 0 {
        return;
    }
    let min_x = (center.x - radius - 1.5).floor().max(0.0) as i32;
    let max_x = (center.x + radius + 1.5).ceil().min((w - 1) as f32) as i32;
    let min_y = (center.y - radius - 1.5).floor().max(0.0) as i32;
    let max_y = (center.y + radius + 1.5).ceil().min((h - 1) as f32) as i32;
    if min_x > max_x || min_y > max_y {
        return;
    }

    let r_outer = radius + 0.5;
    let r_inner = (radius - 0.5).max(0.0);
    let r_outer_sq = r_outer * r_outer;
    let r_inner_sq = r_inner * r_inner;
    let sr = color.r() as u32;
    let sg = color.g() as u32;
    let sb = color.b() as u32;
    let sa = color.a() as u32;
    let stride = (w as usize) * 4;
    let raw = canvas.as_mut();

    for y in min_y..=max_y {
        let dy = y as f32 + 0.5 - center.y;
        let dy_sq = dy * dy;
        if dy_sq >= r_outer_sq {
            continue;
        }
        let row_base = (y as usize) * stride;
        for x in min_x..=max_x {
            let dx = x as f32 + 0.5 - center.x;
            let d_sq = dx * dx + dy_sq;
            if d_sq >= r_outer_sq {
                continue;
            }
            let alpha = if d_sq <= r_inner_sq {
                sa
            } else {
                let cov = (0.5 - (d_sq.sqrt() - radius)).clamp(0.0, 1.0);
                (sa as f32 * cov + 0.5) as u32
            };
            let idx = row_base + (x as usize) * 4;
            blend_rgba_alpha(&mut raw[idx..idx + 4], sr, sg, sb, alpha);
        }
    }
}

fn draw_capsule_segment(canvas: &mut RgbaImage, a: Pt, b: Pt, radius: f32, color: Color32) {
    let w = canvas.width() as i32;
    let h = canvas.height() as i32;
    if w <= 0 || h <= 0 {
        return;
    }
    let bbox = BBox::from_points(a, b).expand(radius + 1.5);
    let min_x = bbox.min.x.floor().max(0.0) as i32;
    let max_x = bbox.max.x.ceil().min((w - 1) as f32) as i32;
    let min_y = bbox.min.y.floor().max(0.0) as i32;
    let max_y = bbox.max.y.ceil().min((h - 1) as f32) as i32;
    if min_x > max_x || min_y > max_y {
        return;
    }

    let vx = b.x - a.x;
    let vy = b.y - a.y;
    let len_sq = vx * vx + vy * vy;
    let inv_len_sq = if len_sq > 0.0 { 1.0 / len_sq } else { 0.0 };

    let r_outer = radius + 0.5;
    let r_inner = (radius - 0.5).max(0.0);
    let r_outer_sq = r_outer * r_outer;
    let r_inner_sq = r_inner * r_inner;
    let sr = color.r() as u32;
    let sg = color.g() as u32;
    let sb = color.b() as u32;
    let sa = color.a() as u32;
    let stride = (w as usize) * 4;
    let raw = canvas.as_mut();

    for y in min_y..=max_y {
        let py = y as f32 + 0.5;
        let wy = py - a.y;
        let wy_vy = wy * vy;
        let row_base = (y as usize) * stride;
        for x in min_x..=max_x {
            let px = x as f32 + 0.5;
            let wx = px - a.x;
            let t = ((wx * vx + wy_vy) * inv_len_sq).clamp(0.0, 1.0);
            let dx = wx - t * vx;
            let dy = wy - t * vy;
            let d_sq = dx * dx + dy * dy;
            if d_sq >= r_outer_sq {
                continue;
            }
            let alpha = if d_sq <= r_inner_sq {
                sa
            } else {
                let cov = (0.5 - (d_sq.sqrt() - radius)).clamp(0.0, 1.0);
                (sa as f32 * cov + 0.5) as u32
            };
            let idx = row_base + (x as usize) * 4;
            blend_rgba_alpha(&mut raw[idx..idx + 4], sr, sg, sb, alpha);
        }
    }
}

fn draw_uniform_polyline(canvas: &mut RgbaImage, points: &[Pt], stroke: f32, color: Color32) {
    if points.is_empty() {
        return;
    }
    let radius = stroke * 0.5;
    if points.len() == 1 {
        draw_filled_circle(canvas, points[0], radius, color);
        return;
    }

    let w = canvas.width() as i32;
    let h = canvas.height() as i32;
    if w <= 0 || h <= 0 {
        return;
    }

    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for p in points {
        min_x = min_x.min(p.x);
        min_y = min_y.min(p.y);
        max_x = max_x.max(p.x);
        max_y = max_y.max(p.y);
    }

    let x0 = (min_x - radius - 1.5).floor().max(0.0) as i32;
    let x1 = (max_x + radius + 1.5).ceil().min((w - 1) as f32) as i32;
    let y0 = (min_y - radius - 1.5).floor().max(0.0) as i32;
    let y1 = (max_y + radius + 1.5).ceil().min((h - 1) as f32) as i32;
    if x0 > x1 || y0 > y1 {
        return;
    }

    let mask_w = (x1 - x0 + 1) as usize;
    let mask_h = (y1 - y0 + 1) as usize;
    let mut mask = vec![0u8; mask_w * mask_h];
    let mut row_spans = vec![(usize::MAX, 0usize); mask_h];

    let r_outer = radius + 0.5;
    let r_inner = (radius - 0.5).max(0.0);
    let r_outer_sq = r_outer * r_outer;
    let r_inner_sq = r_inner * r_inner;

    // Rasterize each segment only within its own local bounding box
    for seg in points.windows(2) {
        let a = seg[0];
        let b = seg[1];
        let seg_bbox = BBox::from_points(a, b).expand(r_outer + 0.5);
        let sx0 = (seg_bbox.min.x.floor() as i32).max(x0);
        let sx1 = (seg_bbox.max.x.ceil() as i32).min(x1);
        let sy0 = (seg_bbox.min.y.floor() as i32).max(y0);
        let sy1 = (seg_bbox.max.y.ceil() as i32).min(y1);
        if sx0 > sx1 || sy0 > sy1 {
            continue;
        }

        let vx = b.x - a.x;
        let vy = b.y - a.y;
        let len_sq = vx * vx + vy * vy;
        let inv_len_sq = if len_sq > 0.0 { 1.0 / len_sq } else { 0.0 };

        let local_x0 = (sx0 - x0) as usize;
        let local_x1 = (sx1 - x0) as usize;

        for y in sy0..=sy1 {
            let ry = (y - y0) as usize;
            let span = &mut row_spans[ry];
            if local_x0 < span.0 {
                span.0 = local_x0;
            }
            if local_x1 > span.1 {
                span.1 = local_x1;
            }

            let py = y as f32 + 0.5;
            let wy = py - a.y;
            let wy_vy = wy * vy;
            let mask_row = ry * mask_w;

            for x in sx0..=sx1 {
                let rx = (x - x0) as usize;
                let cell = &mut mask[mask_row + rx];
                if *cell == 255 {
                    continue;
                }
                let px = x as f32 + 0.5;
                let wx = px - a.x;
                let t = ((wx * vx + wy_vy) * inv_len_sq).clamp(0.0, 1.0);
                let dx = wx - t * vx;
                let dy = wy - t * vy;
                let d_sq = dx * dx + dy * dy;
                if d_sq >= r_outer_sq {
                    continue;
                }
                let cov_u8 = if d_sq <= r_inner_sq {
                    255
                } else {
                    ((0.5 - (d_sq.sqrt() - radius)).clamp(0.0, 1.0) * 255.0 + 0.5) as u8
                };
                if cov_u8 > *cell {
                    *cell = cov_u8;
                }
            }
        }
    }

    // Composite only touched spans onto the canvas
    let sr = color.r() as u32;
    let sg = color.g() as u32;
    let sb = color.b() as u32;
    let sa = color.a() as u32;
    let stride = (w as usize) * 4;
    let raw = canvas.as_mut();

    for (ry, &(rx0, rx1)) in row_spans.iter().enumerate() {
        if rx0 > rx1 {
            continue;
        }
        let y = y0 as usize + ry;
        let row_base = y * stride;
        let mask_row = ry * mask_w;
        for rx in rx0..=rx1 {
            let cov = mask[mask_row + rx] as u32;
            if cov == 0 {
                continue;
            }
            let alpha = (sa * cov + 127) / 255;
            let x = x0 as usize + rx;
            let idx = row_base + x * 4;
            blend_rgba_alpha(&mut raw[idx..idx + 4], sr, sg, sb, alpha);
        }
    }
}

fn draw_filled_triangle(canvas: &mut RgbaImage, a: Pt, b: Pt, c: Pt, color: Color32) {
    let w = canvas.width() as i32;
    let h = canvas.height() as i32;
    if w <= 0 || h <= 0 {
        return;
    }
    let min_x = (a.x.min(b.x).min(c.x).floor() as i32 - 1).max(0);
    let max_x = (a.x.max(b.x).max(c.x).ceil() as i32 + 1).min(w - 1);
    let min_y = (a.y.min(b.y).min(c.y).floor() as i32 - 1).max(0);
    let max_y = (a.y.max(b.y).max(c.y).ceil() as i32 + 1).min(h - 1);
    if min_x > max_x || min_y > max_y {
        return;
    }

    let sr = color.r() as u32;
    let sg = color.g() as u32;
    let sb = color.b() as u32;
    let sa = color.a() as u32;
    let stride = (w as usize) * 4;
    let raw = canvas.as_mut();

    for y in min_y..=max_y {
        let row_base = (y as usize) * stride;
        for x in min_x..=max_x {
            let p = Pt::new(x as f32 + 0.5, y as f32 + 0.5);
            let d = signed_triangle_dist(p, a, b, c);
            if d < 0.5 {
                let alpha = if d <= -0.5 {
                    sa
                } else {
                    let cov = (0.5 - d).clamp(0.0, 1.0);
                    (sa as f32 * cov + 0.5) as u32
                };
                let idx = row_base + (x as usize) * 4;
                blend_rgba_alpha(&mut raw[idx..idx + 4], sr, sg, sb, alpha);
            }
        }
    }
}

#[inline]
fn signed_triangle_dist(p: Pt, a: Pt, b: Pt, c: Pt) -> f32 {
    use crate::annotations::distance_to_segment_sq;
    let s1 = (p.x - c.x) * (b.y - c.y) - (b.x - c.x) * (p.y - c.y);
    let s2 = (p.x - a.x) * (c.y - a.y) - (c.x - a.x) * (p.y - a.y);
    let s3 = (p.x - b.x) * (a.y - b.y) - (a.x - b.x) * (p.y - b.y);
    let has_neg = (s1 < 0.0) || (s2 < 0.0) || (s3 < 0.0);
    let has_pos = (s1 > 0.0) || (s2 > 0.0) || (s3 > 0.0);
    let inside = !(has_neg && has_pos);

    let edge_d_sq = distance_to_segment_sq(p, a, b)
        .min(distance_to_segment_sq(p, b, c))
        .min(distance_to_segment_sq(p, c, a));
    if !inside && edge_d_sq >= 0.25 {
        return 1.0;
    }
    if inside && edge_d_sq >= 0.25 {
        return -1.0;
    }
    let edge_d = edge_d_sq.sqrt();
    if inside {
        -edge_d
    } else {
        edge_d
    }
}

fn draw_filled_rect(canvas: &mut RgbaImage, bbox: BBox, color: Color32) {
    let w = canvas.width() as i32;
    let h = canvas.height() as i32;
    let x0 = (bbox.min.x.round() as i32).clamp(0, w) as usize;
    let x1 = (bbox.max.x.round() as i32).clamp(0, w) as usize;
    let y0 = (bbox.min.y.round() as i32).clamp(0, h) as usize;
    let y1 = (bbox.max.y.round() as i32).clamp(0, h) as usize;
    if x0 >= x1 || y0 >= y1 {
        return;
    }

    let sr = color.r() as u32;
    let sg = color.g() as u32;
    let sb = color.b() as u32;
    let sa = color.a() as u32;
    if sa == 0 {
        return;
    }

    let stride = (w as usize) * 4;
    let raw = canvas.as_mut();

    if sa >= 255 {
        let px = [color.r(), color.g(), color.b(), 255];
        for y in y0..y1 {
            let row_slice = &mut raw[y * stride + x0 * 4..y * stride + x1 * 4];
            for chunk in row_slice.chunks_exact_mut(4) {
                chunk.copy_from_slice(&px);
            }
        }
    } else {
        for y in y0..y1 {
            let row_slice = &mut raw[y * stride + x0 * 4..y * stride + x1 * 4];
            for chunk in row_slice.chunks_exact_mut(4) {
                blend_rgba_alpha(chunk, sr, sg, sb, sa);
            }
        }
    }
}

fn draw_stroked_rect(canvas: &mut RgbaImage, bbox: BBox, stroke: f32, color: Color32) {
    let w = canvas.width() as i32;
    let h = canvas.height() as i32;
    if w <= 0 || h <= 0 {
        return;
    }
    let half = stroke * 0.5;
    let outer = bbox.expand(half + 1.0);
    let x0 = outer.min.x.floor().max(0.0) as i32;
    let x1 = outer.max.x.ceil().min((w - 1) as f32) as i32;
    let y0 = outer.min.y.floor().max(0.0) as i32;
    let y1 = outer.max.y.ceil().min((h - 1) as f32) as i32;
    if x0 > x1 || y0 > y1 {
        return;
    }

    let cx = (bbox.min.x + bbox.max.x) * 0.5;
    let cy = (bbox.min.y + bbox.max.y) * 0.5;
    let hx = bbox.width() * 0.5;
    let hy = bbox.height() * 0.5;
    let skip_inner_x0 = (cx - hx + half + 1.0).ceil() as i32;
    let skip_inner_x1 = (cx + hx - half - 1.0).floor() as i32;

    let sr = color.r() as u32;
    let sg = color.g() as u32;
    let sb = color.b() as u32;
    let sa = color.a() as u32;
    let stride = (w as usize) * 4;
    let raw = canvas.as_mut();

    for y in y0..=y1 {
        let py = y as f32 + 0.5;
        let dy = (py - cy).abs() - hy;
        let row_base = (y as usize) * stride;

        let mut x = x0;
        while x <= x1 {
            // Fast skip across the hollow interior of the rectangle
            if dy < -(half + 1.0) && x >= skip_inner_x0 && x <= skip_inner_x1 {
                x = skip_inner_x1 + 1;
                continue;
            }
            let px = x as f32 + 0.5;
            let dx = (px - cx).abs() - hx;
            let sdf = if dx <= 0.0 && dy <= 0.0 {
                dx.max(dy)
            } else {
                (dx.max(0.0) * dx.max(0.0) + dy.max(0.0) * dy.max(0.0)).sqrt()
            };
            let ring_d = sdf.abs() - half;
            if ring_d < 0.5 {
                let alpha = if ring_d <= -0.5 {
                    sa
                } else {
                    let cov = (0.5 - ring_d).clamp(0.0, 1.0);
                    (sa as f32 * cov + 0.5) as u32
                };
                let idx = row_base + (x as usize) * 4;
                blend_rgba_alpha(&mut raw[idx..idx + 4], sr, sg, sb, alpha);
            }
            x += 1;
        }
    }
}

fn draw_ellipse(canvas: &mut RgbaImage, bbox: BBox, stroke: f32, filled: bool, color: Color32) {
    let w = canvas.width() as i32;
    let h = canvas.height() as i32;
    if w <= 0 || h <= 0 {
        return;
    }
    let rx = (bbox.width() * 0.5).max(1.0);
    let ry = (bbox.height() * 0.5).max(1.0);
    let cx = (bbox.min.x + bbox.max.x) * 0.5;
    let cy = (bbox.min.y + bbox.max.y) * 0.5;
    let half = stroke * 0.5;

    let pad = if filled { 2.0 } else { half + 2.0 };
    let outer = bbox.expand(pad);
    let x0 = outer.min.x.floor().max(0.0) as i32;
    let x1 = outer.max.x.ceil().min((w - 1) as f32) as i32;
    let y0 = outer.min.y.floor().max(0.0) as i32;
    let y1 = outer.max.y.ceil().min((h - 1) as f32) as i32;
    if x0 > x1 || y0 > y1 {
        return;
    }

    let inv_rx = 1.0 / rx;
    let inv_ry = 1.0 / ry;
    let inv_rx2 = inv_rx * inv_rx;
    let inv_ry2 = inv_ry * inv_ry;
    let min_r = rx.min(ry);
    let inner_norm_skip = ((min_r - half - 1.5).max(0.0) / min_r).powi(2);
    let fill_inner_norm_sq = ((min_r - 1.5).max(0.0) / min_r).powi(2);

    let sr = color.r() as u32;
    let sg = color.g() as u32;
    let sb = color.b() as u32;
    let sa = color.a() as u32;
    let stride = (w as usize) * 4;
    let raw = canvas.as_mut();

    for y in y0..=y1 {
        let py = y as f32 + 0.5 - cy;
        let ny = py * inv_ry;
        let ny_sq = ny * ny;
        let gy = py * inv_ry2;
        let gy_sq = gy * gy;
        let row_base = (y as usize) * stride;

        for x in x0..=x1 {
            let px = x as f32 + 0.5 - cx;
            let nx = px * inv_rx;
            let norm_sq = nx * nx + ny_sq;

            if filled {
                if norm_sq <= fill_inner_norm_sq {
                    let idx = row_base + (x as usize) * 4;
                    blend_rgba_alpha(&mut raw[idx..idx + 4], sr, sg, sb, sa);
                    continue;
                }
            } else if norm_sq < inner_norm_skip {
                continue;
            }

            let norm = norm_sq.sqrt();
            let gx = px * inv_rx2;
            let grad = (gx * gx + gy_sq).sqrt().max(1e-4);
            let sdf = (norm - 1.0) / grad;

            let d = if filled { sdf } else { sdf.abs() - half };
            if d < 0.5 {
                let alpha = if d <= -0.5 {
                    sa
                } else {
                    let cov = (0.5 - d).clamp(0.0, 1.0);
                    (sa as f32 * cov + 0.5) as u32
                };
                let idx = row_base + (x as usize) * 4;
                blend_rgba_alpha(&mut raw[idx..idx + 4], sr, sg, sb, alpha);
            }
        }
    }
}

fn draw_text_annotation(
    canvas: &mut RgbaImage,
    pos: Pt,
    text: &str,
    font_size: f32,
    color: Color32,
    bg_box: bool,
) {
    if text.is_empty() {
        return;
    }
    let font = default_font();
    let scale = PxScale::from(font_size.max(8.0));
    let scaled_font = font.as_scaled(scale);
    let ascent = scaled_font.ascent();
    let descent = scaled_font.descent();
    let line_gap = scaled_font.line_gap();
    let line_height = (ascent - descent + line_gap).max(font_size * 1.15);

    let pad = if bg_box { (font_size * 0.35).max(6.0) } else { 0.0 };

    // Layout glyphs and compute exact bounds
    let mut glyphs = Vec::new();
    let mut max_line_w = 0.0_f32;
    let lines: Vec<&str> = text.lines().collect();
    let num_lines = lines.len().max(1);

    for (line_idx, line) in lines.iter().enumerate() {
        let mut cursor_x = pos.x + pad;
        let baseline_y = pos.y + pad + ascent + (line_idx as f32) * line_height;
        let mut prev_glyph = None;

        for ch in line.chars() {
            let glyph_id = scaled_font.glyph_id(ch);
            if let Some(prev) = prev_glyph {
                cursor_x += scaled_font.kern(prev, glyph_id);
            }
            let glyph = glyph_id.with_scale_and_position(scale, point(cursor_x, baseline_y));
            cursor_x += scaled_font.h_advance(glyph_id);
            prev_glyph = Some(glyph_id);
            glyphs.push(glyph);
        }
        max_line_w = max_line_w.max(cursor_x - (pos.x + pad));
    }

    if bg_box {
        let total_w = max_line_w + pad * 2.0;
        let total_h = (num_lines as f32) * line_height + pad * 2.0;
        let rect = BBox::from_points(pos, Pt::new(pos.x + total_w, pos.y + total_h));
        let bg_color = if luminance(color) > 0.55 {
            Color32::from_rgba_unmultiplied(15, 23, 42, 215)
        } else {
            Color32::from_rgba_unmultiplied(248, 250, 252, 225)
        };
        draw_filled_rect(canvas, rect, bg_color);
        draw_stroked_rect(canvas, rect, 1.5, color);
    }

    for glyph in glyphs {
        if let Some(outlined) = font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            outlined.draw(|gx, gy, cov| {
                let px = bounds.min.x as i32 + gx as i32;
                let py = bounds.min.y as i32 + gy as i32;
                blend_pixel(canvas, px, py, color, cov);
            });
        }
    }
}

fn draw_number_stamp(
    canvas: &mut RgbaImage,
    center: Pt,
    number: u32,
    radius: f32,
    color: Color32,
) {
    // Draw outer white halo for contrast, then solid colored circle
    let r = radius.max(10.0);
    draw_filled_circle(canvas, center, r + 2.0, Color32::from_rgba_unmultiplied(255, 255, 255, 235));
    draw_filled_circle(canvas, center, r, color);

    let label = number.to_string();
    let font = default_font();
    let font_size = if label.len() >= 2 { r * 1.15 } else { r * 1.35 };
    let scale = PxScale::from(font_size);
    let scaled_font = font.as_scaled(scale);

    // Measure horizontal advance for exact centering
    let mut total_advance = 0.0_f32;
    let mut prev = None;
    for ch in label.chars() {
        let gid = scaled_font.glyph_id(ch);
        if let Some(p) = prev {
            total_advance += scaled_font.kern(p, gid);
        }
        total_advance += scaled_font.h_advance(gid);
        prev = Some(gid);
    }

    let text_color = if luminance(color) > 0.62 {
        Color32::from_rgb(15, 23, 42)
    } else {
        Color32::WHITE
    };

    let mut cursor_x = center.x - total_advance * 0.5;
    let baseline_y = center.y + (scaled_font.ascent() + scaled_font.descent()) * 0.38;
    let mut prev_gid = None;

    for ch in label.chars() {
        let gid = scaled_font.glyph_id(ch);
        if let Some(p) = prev_gid {
            cursor_x += scaled_font.kern(p, gid);
        }
        let glyph = gid.with_scale_and_position(scale, point(cursor_x, baseline_y));
        cursor_x += scaled_font.h_advance(gid);
        prev_gid = Some(gid);

        if let Some(outlined) = font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            outlined.draw(|gx, gy, cov| {
                let px = bounds.min.x as i32 + gx as i32;
                let py = bounds.min.y as i32 + gy as i32;
                blend_pixel(canvas, px, py, text_color, cov);
            });
        }
    }
}

/// Pixelates a rectangular region on `canvas` by replacing each `block_size × block_size` tile
/// with its average RGBA value.
pub fn apply_pixelate_region(canvas: &mut RgbaImage, bbox: BBox, block_size: u32) {
    let step = block_size.max(4) as usize;
    let w = canvas.width() as usize;
    let h = canvas.height() as usize;
    let x0 = (bbox.min.x.floor().max(0.0) as usize).min(w);
    let y0 = (bbox.min.y.floor().max(0.0) as usize).min(h);
    let x1 = (bbox.max.x.ceil().max(0.0) as usize).min(w);
    let y1 = (bbox.max.y.ceil().max(0.0) as usize).min(h);
    if x0 >= x1 || y0 >= y1 {
        return;
    }

    let stride = w * 4;
    let raw = canvas.as_mut();

    let mut by = y0;
    while by < y1 {
        let bh = (by + step).min(y1) - by;
        let mut bx = x0;
        while bx < x1 {
            let bw = (bx + step).min(x1) - bx;
            let mut r_sum = 0u32;
            let mut g_sum = 0u32;
            let mut b_sum = 0u32;
            let mut a_sum = 0u32;
            let count = (bw * bh).max(1) as u32;

            for py in by..(by + bh) {
                let row_slice = &raw[py * stride + bx * 4..py * stride + (bx + bw) * 4];
                for px in row_slice.chunks_exact(4) {
                    r_sum += px[0] as u32;
                    g_sum += px[1] as u32;
                    b_sum += px[2] as u32;
                    a_sum += px[3] as u32;
                }
            }

            let half = count / 2;
            let avg = [
                ((r_sum + half) / count) as u8,
                ((g_sum + half) / count) as u8,
                ((b_sum + half) / count) as u8,
                ((a_sum + half) / count) as u8,
            ];

            for py in by..(by + bh) {
                let row_slice = &mut raw[py * stride + bx * 4..py * stride + (bx + bw) * 4];
                for px in row_slice.chunks_exact_mut(4) {
                    px.copy_from_slice(&avg);
                }
            }
            bx += step;
        }
        by += step;
    }
}

/// Computes a pixelated `egui::ColorImage` for just the sub-rectangle `bbox` of `base`
/// in a single pass without cloning `base` or allocating an intermediate `RgbaImage`.
pub fn render_pixelate_patch_color_image(
    base: &RgbaImage,
    bbox: BBox,
    block_size: u32,
) -> Option<([u32; 4], eframe::egui::ColorImage)> {
    let step = block_size.max(4) as usize;
    let w = base.width() as usize;
    let h = base.height() as usize;
    let x0 = (bbox.min.x.floor().max(0.0) as usize).min(w);
    let y0 = (bbox.min.y.floor().max(0.0) as usize).min(h);
    let x1 = (bbox.max.x.ceil().max(0.0) as usize).min(w);
    let y1 = (bbox.max.y.ceil().max(0.0) as usize).min(h);
    if x0 >= x1 || y0 >= y1 {
        return None;
    }

    let pw = x1 - x0;
    let ph = y1 - y0;
    let mut pixels = vec![Color32::TRANSPARENT; pw * ph];
    let stride = w * 4;
    let raw = base.as_raw();

    let mut by = y0;
    while by < y1 {
        let bh = (by + step).min(y1) - by;
        let local_y = by - y0;
        let mut bx = x0;
        while bx < x1 {
            let bw = (bx + step).min(x1) - bx;
            let local_x = bx - x0;
            let mut r_sum = 0u32;
            let mut g_sum = 0u32;
            let mut b_sum = 0u32;
            let mut a_sum = 0u32;
            let count = (bw * bh).max(1) as u32;

            for py in by..(by + bh) {
                let row_slice = &raw[py * stride + bx * 4..py * stride + (bx + bw) * 4];
                for px in row_slice.chunks_exact(4) {
                    r_sum += px[0] as u32;
                    g_sum += px[1] as u32;
                    b_sum += px[2] as u32;
                    a_sum += px[3] as u32;
                }
            }

            let half = count / 2;
            let avg = Color32::from_rgba_unmultiplied(
                ((r_sum + half) / count) as u8,
                ((g_sum + half) / count) as u8,
                ((b_sum + half) / count) as u8,
                ((a_sum + half) / count) as u8,
            );

            for ly in local_y..(local_y + bh) {
                let dst_row = &mut pixels[ly * pw + local_x..ly * pw + local_x + bw];
                dst_row.fill(avg);
            }
            bx += step;
        }
        by += step;
    }

    Some((
        [x0 as u32, y0 as u32, x1 as u32, y1 as u32],
        eframe::egui::ColorImage {
            size: [pw, ph],
            source_size: eframe::egui::Vec2::new(pw as f32, ph as f32),
            pixels,
        },
    ))
}

fn luminance(c: Color32) -> f32 {
    (0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32) / 255.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::{apply_crop, apply_expand_canvas, apply_rotate_cw, CanvasState, History};
    use image::Rgba;
    #[test]
    fn renders_all_annotation_kinds_at_native_resolution() {
        let base = RgbaImage::from_pixel(240, 180, Rgba([20, 30, 45, 255]));
        let annotations = vec![
            Annotation {
                id: 1,
                color: Color32::from_rgb(244, 63, 94),
                stroke_width: 4.0,
                kind: AnnotationKind::Arrow {
                    start: Pt::new(20.0, 20.0),
                    end: Pt::new(100.0, 70.0),
                },
            },
            Annotation {
                id: 2,
                color: Color32::from_rgb(250, 204, 21),
                stroke_width: 4.0,
                kind: AnnotationKind::Freehand {
                    points: vec![Pt::new(30.0, 120.0), Pt::new(90.0, 120.0)],
                    is_highlighter: true,
                },
            },
            Annotation {
                id: 3,
                color: Color32::from_rgb(56, 189, 248),
                stroke_width: 3.0,
                kind: AnnotationKind::Rectangle {
                    min: Pt::new(110.0, 20.0),
                    max: Pt::new(210.0, 80.0),
                    filled: true,
                },
            },
            Annotation {
                id: 4,
                color: Color32::from_rgb(34, 197, 94),
                stroke_width: 3.0,
                kind: AnnotationKind::NumberStamp {
                    center: Pt::new(160.0, 130.0),
                    number: 1,
                    radius: 16.0,
                },
            },
            Annotation {
                id: 5,
                color: Color32::WHITE,
                stroke_width: 2.0,
                kind: AnnotationKind::Text {
                    pos: Pt::new(20.0, 140.0),
                    text: "Fix button alignment".to_string(),
                    font_size: 18.0,
                    bg_box: true,
                },
            },
        ];

        let out = render_final_image(&base, &annotations);
        assert_eq!(out.dimensions(), (240, 180));
        // Verify number stamp center was painted green
        let stamp_px = out.get_pixel(160, 118).0;
        assert_ne!(stamp_px, [20, 30, 45, 255]);
    }

    #[test]
    fn pixelate_averages_blocks_and_history_restores_crop_and_rotate() {
        let mut base = RgbaImage::from_pixel(100, 80, Rgba([10, 10, 10, 255]));
        for y in 20..40 {
            for x in 20..40 {
                base.put_pixel(x, y, Rgba([250, 0, 0, 255]));
            }
        }
        let mut state = CanvasState::new(base);
        let mut history = History::default();

        history.record(&state);
        assert!(apply_crop(
            &mut state,
            BBox::from_points(Pt::new(10.0, 10.0), Pt::new(70.0, 60.0))
        ));
        assert_eq!(state.dimensions(), (60, 50));

        history.record(&state);
        apply_rotate_cw(&mut state);
        assert_eq!(state.dimensions(), (50, 60));

        history.record(&state);
        apply_expand_canvas(&mut state, 10, Color32::WHITE);
        assert_eq!(state.dimensions(), (70, 80));

        assert!(history.undo(&mut state));
        assert_eq!(state.dimensions(), (50, 60));
        assert!(history.undo(&mut state));
        assert_eq!(state.dimensions(), (60, 50));
        assert!(history.undo(&mut state));
        assert_eq!(state.dimensions(), (100, 80));
        assert!(history.redo(&mut state));
        assert_eq!(state.dimensions(), (60, 50));
    }

    #[test]
    fn pixelate_patch_color_image_matches_in_place_pixelate() {
        let mut img = RgbaImage::from_fn(64, 48, |x, y| {
            Rgba([(x * 3) as u8, (y * 5) as u8, ((x + y) * 2) as u8, 255])
        });
        let bbox = BBox::from_points(Pt::new(10.0, 8.0), Pt::new(42.0, 36.0));
        let ([x0, y0, x1, y1], patch) =
            render_pixelate_patch_color_image(&img, bbox, 8).expect("valid patch");
        apply_pixelate_region(&mut img, bbox, 8);

        assert_eq!((x0, y0, x1, y1), (10, 8, 42, 36));
        let pw = (x1 - x0) as usize;
        for py in y0..y1 {
            for px in x0..x1 {
                let expected = img.get_pixel(px, py).0;
                let c = patch.pixels[(py - y0) as usize * pw + (px - x0) as usize];
                assert_eq!([c.r(), c.g(), c.b(), c.a()], expected);
            }
        }
    }
}
