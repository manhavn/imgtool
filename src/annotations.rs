//! Annotation object models, tool definitions, hit-testing, and geometry helpers.
//!
//! Coordinates are stored in 1:1 native image pixels (`Pt`), ensuring annotations
//! never lose quality or drift when the viewport is zoomed or panned.

use eframe::egui::Color32;

/// A 2D point in native image pixel coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
}

impl Pt {
    #[inline]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    #[inline]
    pub fn distance_sq(self, other: Self) -> f32 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        dx * dx + dy * dy
    }

    #[inline]
    pub fn distance(self, other: Self) -> f32 {
        self.distance_sq(other).sqrt()
    }

    #[inline]
    pub const fn translated(self, dx: f32, dy: f32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
        }
    }
}

/// Axis-aligned bounding box in native image pixel coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BBox {
    pub min: Pt,
    pub max: Pt,
}

impl BBox {
    pub fn from_points(a: Pt, b: Pt) -> Self {
        Self {
            min: Pt::new(a.x.min(b.x), a.y.min(b.y)),
            max: Pt::new(a.x.max(b.x), a.y.max(b.y)),
        }
    }

    pub fn width(self) -> f32 {
        (self.max.x - self.min.x).max(0.0)
    }

    pub fn height(self) -> f32 {
        (self.max.y - self.min.y).max(0.0)
    }

    pub fn center(self) -> Pt {
        Pt::new(
            (self.min.x + self.max.x) * 0.5,
            (self.min.y + self.max.y) * 0.5,
        )
    }

    pub fn expand(self, margin: f32) -> Self {
        Self {
            min: Pt::new(self.min.x - margin, self.min.y - margin),
            max: Pt::new(self.max.x + margin, self.max.y + margin),
        }
    }

    #[inline]
    pub fn contains(self, p: Pt) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }

    #[inline]
    pub fn intersects(self, other: Self) -> bool {
        self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.y <= other.max.y
            && self.max.y >= other.min.y
    }
}

/// Interactive tool modes in the annotation toolbar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Pen,
    Highlighter,
    Arrow,
    Line,
    Rectangle,
    Ellipse,
    Text,
    NumberStamp,
    Redact,
    Crop,
}

impl Tool {
    pub const ALL: [Tool; 11] = [
        Tool::Select,
        Tool::Pen,
        Tool::Highlighter,
        Tool::Arrow,
        Tool::Line,
        Tool::Rectangle,
        Tool::Ellipse,
        Tool::Text,
        Tool::NumberStamp,
        Tool::Redact,
        Tool::Crop,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Pen => "Pen",
            Tool::Highlighter => "Marker",
            Tool::Arrow => "Arrow",
            Tool::Line => "Line",
            Tool::Rectangle => "Rect",
            Tool::Ellipse => "Ellipse",
            Tool::Text => "Text",
            Tool::NumberStamp => "Number",
            Tool::Redact => "Blur",
            Tool::Crop => "Crop",
        }
    }

    pub fn shortcut_label(self) -> &'static str {
        match self {
            Tool::Select => "V / Esc",
            Tool::Pen => "P",
            Tool::Highlighter => "H",
            Tool::Arrow => "A",
            Tool::Line => "L",
            Tool::Rectangle => "R",
            Tool::Ellipse => "E",
            Tool::Text => "T",
            Tool::NumberStamp => "N",
            Tool::Redact => "B / X",
            Tool::Crop => "C",
        }
    }

    pub fn tooltip(self) -> &'static str {
        match self {
            Tool::Select => "Select, move, resize, recolor, or delete annotations (V / Esc)",
            Tool::Pen => "Freehand pen with stroke smoothing (P)",
            Tool::Highlighter => "Semi-transparent marker stroke for highlighting text (H)",
            Tool::Arrow => "Directional arrow in ksnip style (A)",
            Tool::Line => "Straight line segment; hold Shift to snap angles (L)",
            Tool::Rectangle => "Outlined or semi-transparent filled rectangle (R)",
            Tool::Ellipse => "Outlined or filled ellipse; hold Shift for a circle (E)",
            Tool::Text => "Click to type UTF-8 text on the canvas (T)",
            Tool::NumberStamp => "Click to place auto-incrementing step badges 1, 2, 3... (N)",
            Tool::Redact => "Pixelate or black-out sensitive regions (B / X)",
            Tool::Crop => "Drag a crop region; press Enter to apply or Esc to cancel (C)",
        }
    }
}

/// Redaction style for `Tool::Redact`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RedactStyle {
    /// Pixelates the underlying image region using block averaging.
    Pixelate,
    /// Fills the region with a solid opaque color.
    Blackout,
}

/// Resize / control handle on a selected annotation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    Start,
    End,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// A single non-destructive annotation layer item.
#[derive(Clone, Debug, PartialEq)]
pub struct Annotation {
    pub id: u64,
    pub color: Color32,
    pub stroke_width: f32,
    pub kind: AnnotationKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AnnotationKind {
    /// Freehand pen or semi-transparent highlighter stroke.
    Freehand {
        points: Vec<Pt>,
        is_highlighter: bool,
    },
    /// Directional arrow from `start` to `end` (arrowhead at `end`).
    Arrow {
        start: Pt,
        end: Pt,
    },
    /// Straight line segment from `start` to `end`.
    Line {
        start: Pt,
        end: Pt,
    },
    /// Rectangle from `min` to `max`.
    Rectangle {
        min: Pt,
        max: Pt,
        filled: bool,
    },
    /// Ellipse bounded by `min` to `max`.
    Ellipse {
        min: Pt,
        max: Pt,
        filled: bool,
    },
    /// UTF-8 text annotation anchored at top-left `pos`.
    Text {
        pos: Pt,
        text: String,
        font_size: f32,
        bg_box: bool,
    },
    /// Auto-incrementing numbered badge centered at `center`.
    NumberStamp {
        center: Pt,
        number: u32,
        radius: f32,
    },
    /// Redact / pixelate / blackout region from `min` to `max`.
    Redact {
        min: Pt,
        max: Pt,
        block_size: u32,
        style: RedactStyle,
    },
}

impl Annotation {
    /// Computes the bounding box in native image pixels.
    pub fn bounding_box(&self) -> BBox {
        let pad = (self.stroke_width * 0.6).max(2.0);
        match &self.kind {
            AnnotationKind::Freehand { points, .. } => {
                if points.is_empty() {
                    return BBox::from_points(Pt::new(0.0, 0.0), Pt::new(0.0, 0.0));
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
                BBox::from_points(Pt::new(min_x, min_y), Pt::new(max_x, max_y)).expand(pad)
            }
            AnnotationKind::Arrow { start, end } | AnnotationKind::Line { start, end } => {
                BBox::from_points(*start, *end).expand(pad * 2.5)
            }
            AnnotationKind::Rectangle { min, max, .. }
            | AnnotationKind::Ellipse { min, max, .. }
            | AnnotationKind::Redact { min, max, .. } => BBox::from_points(*min, *max).expand(pad),
            AnnotationKind::Text {
                pos,
                text,
                font_size,
                bg_box,
            } => {
                let (w, h) = estimate_text_dimensions(text, *font_size, *bg_box);
                BBox::from_points(*pos, Pt::new(pos.x + w, pos.y + h))
            }
            AnnotationKind::NumberStamp { center, radius, .. } => BBox::from_points(
                Pt::new(center.x - radius, center.y - radius),
                Pt::new(center.x + radius, center.y + radius),
            )
            .expand(2.0),
        }
    }

    /// Translates the entire annotation by `(dx, dy)` in image pixels.
    pub fn translate(&mut self, dx: f32, dy: f32) {
        match &mut self.kind {
            AnnotationKind::Freehand { points, .. } => {
                for p in points {
                    *p = p.translated(dx, dy);
                }
            }
            AnnotationKind::Arrow { start, end } | AnnotationKind::Line { start, end } => {
                *start = start.translated(dx, dy);
                *end = end.translated(dx, dy);
            }
            AnnotationKind::Rectangle { min, max, .. }
            | AnnotationKind::Ellipse { min, max, .. }
            | AnnotationKind::Redact { min, max, .. } => {
                *min = min.translated(dx, dy);
                *max = max.translated(dx, dy);
            }
            AnnotationKind::Text { pos, .. } => {
                *pos = pos.translated(dx, dy);
            }
            AnnotationKind::NumberStamp { center, .. } => {
                *center = center.translated(dx, dy);
            }
        }
    }

    /// Returns interactive resize handles for a selected annotation.
    pub fn handles(&self) -> Vec<(Handle, Pt)> {
        match &self.kind {
            AnnotationKind::Arrow { start, end } | AnnotationKind::Line { start, end } => {
                vec![(Handle::Start, *start), (Handle::End, *end)]
            }
            AnnotationKind::Rectangle { min, max, .. }
            | AnnotationKind::Ellipse { min, max, .. }
            | AnnotationKind::Redact { min, max, .. } => {
                let b = BBox::from_points(*min, *max);
                vec![
                    (Handle::TopLeft, b.min),
                    (Handle::TopRight, Pt::new(b.max.x, b.min.y)),
                    (Handle::BottomLeft, Pt::new(b.min.x, b.max.y)),
                    (Handle::BottomRight, b.max),
                ]
            }
            AnnotationKind::NumberStamp { center, radius, .. } => {
                vec![(Handle::End, Pt::new(center.x + *radius, center.y))]
            }
            AnnotationKind::Freehand { .. } | AnnotationKind::Text { .. } => Vec::new(),
        }
    }

    /// Moves a specific resize handle to `new_pt`.
    pub fn move_handle(&mut self, handle: Handle, new_pt: Pt) {
        match &mut self.kind {
            AnnotationKind::Arrow { start, end } | AnnotationKind::Line { start, end } => {
                match handle {
                    Handle::Start => *start = new_pt,
                    Handle::End => *end = new_pt,
                    _ => {}
                }
            }
            AnnotationKind::Rectangle { min, max, .. }
            | AnnotationKind::Ellipse { min, max, .. }
            | AnnotationKind::Redact { min, max, .. } => {
                let mut x0 = min.x;
                let mut y0 = min.y;
                let mut x1 = max.x;
                let mut y1 = max.y;
                match handle {
                    Handle::TopLeft => {
                        x0 = new_pt.x;
                        y0 = new_pt.y;
                    }
                    Handle::TopRight => {
                        x1 = new_pt.x;
                        y0 = new_pt.y;
                    }
                    Handle::BottomLeft => {
                        x0 = new_pt.x;
                        y1 = new_pt.y;
                    }
                    Handle::BottomRight => {
                        x1 = new_pt.x;
                        y1 = new_pt.y;
                    }
                    _ => {}
                }
                *min = Pt::new(x0.min(x1), y0.min(y1));
                *max = Pt::new(x0.max(x1), y0.max(y1));
            }
            AnnotationKind::NumberStamp { center, radius, .. } => {
                if handle == Handle::End {
                    *radius = center.distance(new_pt).clamp(10.0, 96.0);
                }
            }
            _ => {}
        }
    }

    /// Hit-tests `pt` in native image coordinates with `tolerance` (in image pixels).
    pub fn hit_test(&self, pt: Pt, tolerance: f32) -> bool {
        let hit_radius = (self.stroke_width * 0.6 + tolerance).max(6.0);
        if !self.bounding_box().expand(hit_radius + 4.0).contains(pt) {
            return false;
        }
        let hit_r_sq = hit_radius * hit_radius;
        match &self.kind {
            AnnotationKind::Freehand { points, .. } => {
                if points.len() == 1 {
                    return points[0].distance_sq(pt) <= hit_r_sq;
                }
                points
                    .windows(2)
                    .any(|w| distance_to_segment_sq(pt, w[0], w[1]) <= hit_r_sq)
            }
            AnnotationKind::Arrow { start, end } | AnnotationKind::Line { start, end } => {
                let r = hit_radius * 1.4;
                distance_to_segment_sq(pt, *start, *end) <= r * r
            }
            AnnotationKind::Rectangle { min, max, filled } => {
                let b = BBox::from_points(*min, *max);
                if *filled {
                    return b.expand(hit_radius).contains(pt);
                }
                let outer = b.expand(hit_radius);
                let inner = b.expand(-hit_radius);
                outer.contains(pt) && !inner.contains(pt)
            }
            AnnotationKind::Ellipse { min, max, filled } => {
                let b = BBox::from_points(*min, *max);
                let cx = (b.min.x + b.max.x) * 0.5;
                let cy = (b.min.y + b.max.y) * 0.5;
                let rx = (b.width() * 0.5).max(1.0);
                let ry = (b.height() * 0.5).max(1.0);
                let norm = ((pt.x - cx) / rx).powi(2) + ((pt.y - cy) / ry).powi(2);
                if *filled {
                    norm <= 1.15
                } else {
                    let dr = hit_radius / rx.min(ry).max(1.0);
                    let d = norm.sqrt();
                    (d - 1.0).abs() <= dr.max(0.12)
                }
            }
            AnnotationKind::Text { .. } | AnnotationKind::Redact { .. } => {
                self.bounding_box().expand(tolerance).contains(pt)
            }
            AnnotationKind::NumberStamp { center, radius, .. } => {
                let r = *radius + tolerance;
                center.distance_sq(pt) <= r * r
            }
        }
    }
}

/// Estimates text block dimensions `(width, height)` in native pixels.
pub fn estimate_text_dimensions(text: &str, font_size: f32, bg_box: bool) -> (f32, f32) {
    let lines: Vec<&str> = if text.is_empty() {
        vec![" "]
    } else {
        text.lines().collect()
    };
    let line_count = lines.len().max(1) as f32;
    let max_chars = lines
        .iter()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(1)
        .max(1) as f32;
    let pad = if bg_box { font_size * 0.35 } else { 4.0 };
    let width = max_chars * (font_size * 0.58) + pad * 2.0;
    let height = line_count * (font_size * 1.25) + pad * 2.0;
    (width.max(font_size), height.max(font_size * 1.2))
}

/// Snaps `end` relative to `start` to 0°, 45°, 90°, 135°, 180° increments when Shift is held.
pub fn snap_angle(start: Pt, end: Pt) -> Pt {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1.0 {
        return end;
    }
    let angle = dy.atan2(dx);
    let step = std::f32::consts::FRAC_PI_4; // 45 degrees
    let snapped = (angle / step).round() * step;
    Pt::new(start.x + len * snapped.cos(), start.y + len * snapped.sin())
}

/// Enforces a 1:1 square aspect ratio from `start` to `end` when Shift is held.
pub fn snap_square(start: Pt, end: Pt) -> Pt {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let size = dx.abs().max(dy.abs());
    Pt::new(
        start.x + size * dx.signum(),
        start.y + size * dy.signum(),
    )
}

/// Smooths a raw freehand polyline using Chaikin corner-cutting + radial distance decimation.
pub fn smooth_freehand_points(raw: &[Pt]) -> Vec<Pt> {
    if raw.len() <= 2 {
        return raw.to_vec();
    }

    // Filter out micro-jitter points closer than 1.5 px (distance_sq >= 2.25)
    let mut filtered = Vec::with_capacity(raw.len());
    filtered.push(raw[0]);
    for &pt in &raw[1..] {
        if filtered.last().unwrap().distance_sq(pt) >= 2.25 {
            filtered.push(pt);
        }
    }
    if let Some(&last) = raw.last() {
        if filtered.last().copied() != Some(last) {
            filtered.push(last);
        }
    }
    if filtered.len() <= 2 {
        return filtered;
    }

    // Two iterations of Chaikin's corner-cutting subdivision, skipping nearly-collinear vertices
    // to avoid 4x vertex explosion on straight/gentle segments.
    let mut current = filtered;
    for _ in 0..2 {
        let mut next = Vec::with_capacity(current.len() * 2);
        next.push(current[0]);
        for pair in current.windows(2) {
            let p0 = pair[0];
            let p1 = pair[1];
            if p0.distance_sq(p1) < 4.0 {
                next.push(p1);
            } else {
                next.push(Pt::new(0.75 * p0.x + 0.25 * p1.x, 0.75 * p0.y + 0.25 * p1.y));
                next.push(Pt::new(0.25 * p0.x + 0.75 * p1.x, 0.25 * p0.y + 0.75 * p1.y));
            }
        }
        next.push(*current.last().unwrap());
        current = next;
    }

    // Prune nearly-collinear intermediate vertices (deviation < 0.25 px) to keep GPU vertex count compact
    if current.len() > 3 {
        let mut simplified = Vec::with_capacity(current.len());
        simplified.push(current[0]);
        for i in 1..(current.len() - 1) {
            let prev = *simplified.last().unwrap();
            let curr = current[i];
            let next = current[i + 1];
            if distance_to_segment_sq(curr, prev, next) >= 0.0625 {
                simplified.push(curr);
            }
        }
        simplified.push(*current.last().unwrap());
        return simplified;
    }

    current
}

/// Computes the 3 vertices `(tip, left_wing, right_wing)` and shaft endpoint for an arrow.
pub fn compute_arrow_geometry(start: Pt, end: Pt, stroke_width: f32) -> (Pt, Pt, Pt, Pt) {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-3 {
        return (end, end, end, start);
    }
    let ux = dx / len;
    let uy = dy / len;
    let px = -uy;
    let py = ux;

    let head_len = (stroke_width * 4.2).max(14.0).min(len * 0.85);
    let head_width = (stroke_width * 2.4).max(8.5);

    let base_x = end.x - ux * head_len;
    let base_y = end.y - uy * head_len;

    let left = Pt::new(base_x + px * head_width, base_y + py * head_width);
    let right = Pt::new(base_x - px * head_width, base_y - py * head_width);
    let shaft_end = Pt::new(end.x - ux * (head_len * 0.7), end.y - uy * (head_len * 0.7));

    (end, left, right, shaft_end)
}

#[inline]
pub fn distance_to_segment_sq(p: Pt, a: Pt, b: Pt) -> f32 {
    let vx = b.x - a.x;
    let vy = b.y - a.y;
    let wx = p.x - a.x;
    let wy = p.y - a.y;
    let len_sq = vx * vx + vy * vy;
    let t = if len_sq > 0.0 {
        ((wx * vx + wy * wy) / len_sq).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let dx = p.x - (a.x + t * vx);
    let dy = p.y - (a.y + t * vy);
    dx * dx + dy * dy
}

#[inline]
pub fn distance_to_segment(p: Pt, a: Pt, b: Pt) -> f32 {
    distance_to_segment_sq(p, a, b).sqrt()
}

/// Standard quick-select annotation color palette (`ksnip` defaults).
pub const PALETTE_COLORS: [(Color32, &str); 9] = [
    (Color32::from_rgb(244, 63, 94), "Crimson Red"),
    (Color32::from_rgb(249, 115, 22), "Vibrant Orange"),
    (Color32::from_rgb(250, 204, 21), "Golden Yellow"),
    (Color32::from_rgb(34, 197, 94), "Emerald Green"),
    (Color32::from_rgb(56, 189, 248), "Sky Blue"),
    (Color32::from_rgb(168, 85, 247), "Royal Purple"),
    (Color32::from_rgb(236, 72, 153), "Hot Pink"),
    (Color32::from_rgb(255, 255, 255), "Pure White"),
    (Color32::from_rgb(15, 23, 42), "Deep Slate / Black"),
];
