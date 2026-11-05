//! Undo / Redo stack for both non-destructive vector annotations and canvas transforms
//! (Crop, Rotate 90° CW/CCW, Flip Horizontal/Vertical, and Expand Canvas / Padding).

use crate::annotations::{Annotation, AnnotationKind, BBox, Pt};
use eframe::egui::Color32;
use image::{imageops, Rgba, RgbaImage};
use std::collections::VecDeque;
use std::sync::Arc;

/// Complete document state snapshot stored in the Undo / Redo history.
#[derive(Clone)]
pub struct CanvasState {
    pub base_image: Arc<RgbaImage>,
    pub annotations: Vec<Annotation>,
    pub next_number: u32,
    pub next_id: u64,
}

impl CanvasState {
    pub fn new(image: RgbaImage) -> Self {
        Self {
            base_image: Arc::new(image),
            annotations: Vec::new(),
            next_number: 1,
            next_id: 1,
        }
    }

    pub fn dimensions(&self) -> (u32, u32) {
        self.base_image.dimensions()
    }
}

/// Manages undo and redo snapshots.
///
/// Because `base_image` is wrapped in `Arc<RgbaImage>`, pushing a vector annotation change
/// only clones the lightweight `Annotation` vector without copying image pixels.
/// Canvas transforms (`Crop`, `Rotate`, `Flip`, `Expand`) allocate a new `Arc<RgbaImage>`
/// and transform all existing annotations so both image and vector layers undo cleanly.
pub struct History {
    undo_stack: VecDeque<CanvasState>,
    redo_stack: Vec<CanvasState>,
    max_entries: usize,
}

impl Default for History {
    fn default() -> Self {
        Self::new(64)
    }
}

impl History {
    pub fn new(max_entries: usize) -> Self {
        Self {
            undo_stack: VecDeque::with_capacity(max_entries),
            redo_stack: Vec::with_capacity(max_entries),
            max_entries,
        }
    }

    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Saves `current` onto the undo stack before mutating state, clearing the redo stack.
    pub fn record(&mut self, current: &CanvasState) {
        if self.undo_stack.len() >= self.max_entries {
            self.undo_stack.pop_front();
        }
        self.undo_stack.push_back(current.clone());
        self.redo_stack.clear();
    }

    /// Reverts to the previous state, pushing `current` onto the redo stack.
    pub fn undo(&mut self, current: &mut CanvasState) -> bool {
        if let Some(prev) = self.undo_stack.pop_back() {
            let old = std::mem::replace(current, prev);
            self.redo_stack.push(old);
            true
        } else {
            false
        }
    }

    /// Re-applies an undone state.
    pub fn redo(&mut self, current: &mut CanvasState) -> bool {
        if let Some(next) = self.redo_stack.pop() {
            let old = std::mem::replace(current, next);
            self.undo_stack.push_back(old);
            true
        } else {
            false
        }
    }
}

/// Crops the base image to `crop_box` and shifts all annotations into the new coordinate space.
pub fn apply_crop(state: &mut CanvasState, crop_box: BBox) -> bool {
    let (img_w, img_h) = state.dimensions();
    let x0 = (crop_box.min.x.round() as i64).clamp(0, img_w as i64) as u32;
    let y0 = (crop_box.min.y.round() as i64).clamp(0, img_h as i64) as u32;
    let x1 = (crop_box.max.x.round() as i64).clamp(0, img_w as i64) as u32;
    let y1 = (crop_box.max.y.round() as i64).clamp(0, img_h as i64) as u32;

    let w = x1.saturating_sub(x0);
    let h = y1.saturating_sub(y0);
    if w < 4 || h < 4 {
        return false;
    }

    let cropped = imageops::crop_imm(state.base_image.as_ref(), x0, y0, w, h).to_image();
    state.base_image = Arc::new(cropped);

    let dx = -(x0 as f32);
    let dy = -(y0 as f32);
    for ann in &mut state.annotations {
        ann.translate(dx, dy);
    }
    true
}

/// Rotates the base image and all annotations 90° clockwise.
pub fn apply_rotate_cw(state: &mut CanvasState) {
    let (_old_w, old_h) = state.dimensions();
    let rotated = imageops::rotate90(state.base_image.as_ref());
    state.base_image = Arc::new(rotated);

    let h = old_h as f32;
    let map_pt = |p: Pt| Pt::new(h - p.y, p.x);

    for ann in &mut state.annotations {
        transform_annotation_points(ann, map_pt);
    }
}

/// Rotates the base image and all annotations 90° counter-clockwise.
pub fn apply_rotate_ccw(state: &mut CanvasState) {
    let (old_w, _old_h) = state.dimensions();
    let rotated = imageops::rotate270(state.base_image.as_ref());
    state.base_image = Arc::new(rotated);

    let w = old_w as f32;
    let map_pt = |p: Pt| Pt::new(p.y, w - p.x);

    for ann in &mut state.annotations {
        transform_annotation_points(ann, map_pt);
    }
}

/// Flips the base image and all annotations horizontally.
pub fn apply_flip_horizontal(state: &mut CanvasState) {
    let (w, _) = state.dimensions();
    let flipped = imageops::flip_horizontal(state.base_image.as_ref());
    state.base_image = Arc::new(flipped);

    let wf = w as f32;
    let map_pt = |p: Pt| Pt::new(wf - p.x, p.y);

    for ann in &mut state.annotations {
        transform_annotation_points(ann, map_pt);
    }
}

/// Flips the base image and all annotations vertically.
pub fn apply_flip_vertical(state: &mut CanvasState) {
    let (_, h) = state.dimensions();
    let flipped = imageops::flip_vertical(state.base_image.as_ref());
    state.base_image = Arc::new(flipped);

    let hf = h as f32;
    let map_pt = |p: Pt| Pt::new(p.x, hf - p.y);

    for ann in &mut state.annotations {
        transform_annotation_points(ann, map_pt);
    }
}

/// Expands the canvas around all four sides by `margin` pixels filled with `fill_color`,
/// shifting all annotations by `(margin, margin)` so callout notes can be drawn outside.
pub fn apply_expand_canvas(state: &mut CanvasState, margin: u32, fill_color: Color32) {
    if margin == 0 {
        return;
    }
    let (old_w, old_h) = state.dimensions();
    let new_w = old_w + margin * 2;
    let new_h = old_h + margin * 2;

    let bg = Rgba([
        fill_color.r(),
        fill_color.g(),
        fill_color.b(),
        fill_color.a(),
    ]);
    let mut expanded = RgbaImage::from_pixel(new_w, new_h, bg);
    imageops::replace(&mut expanded, state.base_image.as_ref(), margin as i64, margin as i64);
    state.base_image = Arc::new(expanded);

    let m = margin as f32;
    for ann in &mut state.annotations {
        ann.translate(m, m);
    }
}

fn transform_annotation_points(ann: &mut Annotation, map_pt: impl Fn(Pt) -> Pt) {
    match &mut ann.kind {
        AnnotationKind::Freehand { points, .. } => {
            for p in points {
                *p = map_pt(*p);
            }
        }
        AnnotationKind::Arrow { start, end } | AnnotationKind::Line { start, end } => {
            *start = map_pt(*start);
            *end = map_pt(*end);
        }
        AnnotationKind::Rectangle { min, max, .. }
        | AnnotationKind::Ellipse { min, max, .. }
        | AnnotationKind::Redact { min, max, .. } => {
            let a = map_pt(*min);
            let b = map_pt(*max);
            let bbox = BBox::from_points(a, b);
            *min = bbox.min;
            *max = bbox.max;
        }
        AnnotationKind::Text { pos, .. } => {
            *pos = map_pt(*pos);
        }
        AnnotationKind::NumberStamp { center, .. } => {
            *center = map_pt(*center);
        }
    }
}
