//! Interactive image viewport canvas:
//! - Cursor-centered Zoom (`Scroll` / `Ctrl+Scroll`), Pan (Middle-click drag or `Space + Left-Click drag`),
//!   `Fit to Window` (`F`), and `100%` (`1`).
//! - Screen <-> native image pixel coordinate mapping.
//! - Live vector preview of annotations + interactive selection, move, and resize handles.
//! - Interactive Crop rectangle with dimmed outside overlay and live `W × H px` badge.

use crate::annotations::{
    compute_arrow_geometry, smooth_freehand_points, snap_angle, snap_square, Annotation,
    AnnotationKind, BBox, Handle, Pt, RedactStyle, Tool,
};
use crate::history::CanvasState;
use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, FontId, Key, Painter, PointerButton, Pos2,
    Rect, Response, Sense, Shape, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui, Vec2,
};

/// Active drag interaction state on the canvas.
#[derive(Clone, Debug)]
enum DragMode {
    /// Panning the viewport with middle mouse button or Space + Left-Click.
    Panning,
    /// Moving an existing selected annotation (`id`), with undo already recorded (`recorded`).
    MovingAnnotation {
        id: u64,
        last_img_pt: Pt,
        recorded: bool,
    },
    /// Dragging a resize handle on a selected annotation.
    ResizingHandle {
        id: u64,
        handle: Handle,
        recorded: bool,
    },
    /// Drawing a new annotation (Freehand, Arrow, Line, Rect, Ellipse, Redact).
    DrawingNew {
        start: Pt,
        current: Pt,
        points: Vec<Pt>,
    },
    /// Dragging a crop rectangle.
    DraggingCrop {
        start: Pt,
    },
}

/// Active inline text entry box on the canvas (`Tool::Text`).
#[derive(Clone, Debug)]
pub struct ActiveTextInput {
    pub pos: Pt,
    pub text: String,
    pub color: Color32,
    pub font_size: f32,
    pub bg_box: bool,
    pub just_opened: bool,
}

struct PixelatePatchCache {
    ann_id: u64,
    key: u64,
    bounds_px: [u32; 4],
    texture: TextureHandle,
}

/// Viewport state for zoom, pan, GPU texture cache, and interactive tool handling.
pub struct ViewportCanvas {
    pub zoom: f32,
    pub pan: Vec2,
    pub needs_fit: bool,
    pub selected_id: Option<u64>,
    pub crop_rect: Option<BBox>,
    pub active_text: Option<ActiveTextInput>,
    drag_mode: Option<DragMode>,
    texture: Option<TextureHandle>,
    cached_image_ptr: usize,
    cached_nearest_filter: bool,
    pixelate_patches: Vec<PixelatePatchCache>,
}

impl Default for ViewportCanvas {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan: Vec2::ZERO,
            needs_fit: true,
            selected_id: None,
            crop_rect: None,
            active_text: None,
            drag_mode: None,
            texture: None,
            cached_image_ptr: 0,
            cached_nearest_filter: false,
            pixelate_patches: Vec::new(),
        }
    }
}

/// Tool settings passed from the main toolbar into the canvas each frame.
#[derive(Clone, Copy, Debug)]
pub struct ToolConfig {
    pub tool: Tool,
    pub color: Color32,
    pub stroke_width: f32,
    pub font_size: f32,
    pub fill_shapes: bool,
    pub text_bg_box: bool,
    pub redact_style: RedactStyle,
    pub redact_block_size: u32,
    pub stamp_radius: f32,
}

/// Events emitted by the canvas back to the main application.
pub enum CanvasEvent {
    RecordUndo(CanvasState),
    CommitCrop(BBox),
    SwitchTool(Tool),
    StatusMessage(String),
}

impl ViewportCanvas {
    pub fn reset_view(&mut self) {
        self.needs_fit = true;
        self.selected_id = None;
        self.crop_rect = None;
        self.active_text = None;
        self.drag_mode = None;
    }

    pub fn invalidate_texture(&mut self) {
        // Base texture tracks `Arc::as_ptr(&state.base_image)` automatically;
        // only clear unused pixelate patch caches when invalidated.
    }

    /// Fits the image inside `viewport_rect` with a small margin.
    pub fn fit_to_rect(&mut self, viewport_rect: Rect, img_w: u32, img_h: u32) {
        if img_w == 0 || img_h == 0 || viewport_rect.width() <= 10.0 || viewport_rect.height() <= 10.0
        {
            return;
        }
        let avail_w = (viewport_rect.width() - 32.0).max(32.0);
        let avail_h = (viewport_rect.height() - 32.0).max(32.0);
        let scale_x = avail_w / img_w as f32;
        let scale_y = avail_h / img_h as f32;
        self.zoom = scale_x.min(scale_y).min(1.0).clamp(0.05, 32.0);
        self.pan = Vec2::ZERO;
        self.needs_fit = false;
    }

    /// Sets zoom to 100% (1:1 native pixels) centered in the viewport.
    pub fn set_zoom_100(&mut self) {
        self.zoom = 1.0;
        self.pan = Vec2::ZERO;
        self.needs_fit = false;
    }

    /// Zooms by `factor`, keeping `anchor_screen_pos` fixed on the same image pixel.
    pub fn zoom_around(&mut self, factor: f32, anchor_screen_pos: Pos2, viewport_center: Pos2) {
        let old_zoom = self.zoom;
        let new_zoom = (old_zoom * factor).clamp(0.05, 32.0);
        if (new_zoom - old_zoom).abs() < 1e-5 {
            return;
        }
        let rel = anchor_screen_pos - (viewport_center + self.pan);
        self.pan += rel * (1.0 - new_zoom / old_zoom);
        self.zoom = new_zoom;
        self.needs_fit = false;
    }

    /// Computes the screen-space `Rect` where the image is drawn inside `viewport_rect`.
    pub fn image_screen_rect(&self, viewport_rect: Rect, img_w: u32, img_h: u32) -> Rect {
        let size = Vec2::new(img_w as f32 * self.zoom, img_h as f32 * self.zoom);
        let center = viewport_rect.center() + self.pan;
        Rect::from_center_size(center, size)
    }

    /// Converts a screen position (`Pos2`) to native image pixel coordinates (`Pt`).
    pub fn screen_to_image(&self, pos: Pos2, img_rect: Rect) -> Pt {
        Pt::new(
            (pos.x - img_rect.min.x) / self.zoom,
            (pos.y - img_rect.min.y) / self.zoom,
        )
    }

    /// Converts native image pixel coordinates (`Pt`) to a screen position (`Pos2`).
    pub fn image_to_screen(&self, pt: Pt, img_rect: Rect) -> Pos2 {
        Pos2::new(
            img_rect.min.x + pt.x * self.zoom,
            img_rect.min.y + pt.y * self.zoom,
        )
    }

    /// Commits any active inline text editor into `state.annotations`.
    pub fn commit_active_text(&mut self, state: &mut CanvasState, events: &mut Vec<CanvasEvent>) {
        if let Some(active) = self.active_text.take() {
            let trimmed = active.text.trim_end();
            if !trimmed.is_empty() {
                events.push(CanvasEvent::RecordUndo(state.clone()));
                let id = state.next_id;
                state.next_id += 1;
                state.annotations.push(Annotation {
                    id,
                    color: active.color,
                    stroke_width: 2.0,
                    kind: AnnotationKind::Text {
                        pos: active.pos,
                        text: trimmed.to_string(),
                        font_size: active.font_size,
                        bg_box: active.bg_box,
                    },
                });
                self.selected_id = Some(id);
            }
        }
    }

    /// Ensures the base image is uploaded to the GPU texture only when `state.base_image` changes.
    /// Never clones `state.base_image` and never re-uploads the full image on annotation edits.
    fn ensure_texture(&mut self, ctx: &egui::Context, state: &CanvasState) {
        let ptr = std::sync::Arc::as_ptr(&state.base_image) as usize;
        let use_nearest = self.zoom >= 2.0;
        let opts = if use_nearest {
            TextureOptions::NEAREST
        } else {
            TextureOptions::LINEAR
        };

        if self.texture.is_none() || self.cached_image_ptr != ptr {
            let (w, h) = state.base_image.dimensions();
            let color_image = egui::ColorImage::from_rgba_unmultiplied(
                [w as usize, h as usize],
                state.base_image.as_raw(),
            );
            if let Some(tex) = &mut self.texture {
                tex.set(color_image, opts);
            } else {
                self.texture = Some(ctx.load_texture("imgtool-canvas", color_image, opts));
            }
            self.cached_image_ptr = ptr;
            self.cached_nearest_filter = use_nearest;
            self.pixelate_patches.clear();
        }

        // Prune pixelate patch caches for deleted annotations
        self.pixelate_patches.retain(|p| {
            p.ann_id == 0
                || state.annotations.iter().any(|a| {
                    a.id == p.ann_id
                        && matches!(
                            a.kind,
                            AnnotationKind::Redact {
                                style: RedactStyle::Pixelate,
                                ..
                            }
                        )
                })
        });
    }

    /// Renders the interactive canvas and returns any emitted `CanvasEvent`s.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        state: &mut CanvasState,
        cfg: ToolConfig,
    ) -> Vec<CanvasEvent> {
        let mut events = Vec::new();
        let viewport_rect = ui.available_rect_before_wrap();
        let (img_w, img_h) = state.dimensions();

        if self.needs_fit {
            self.fit_to_rect(viewport_rect, img_w, img_h);
        }

        self.ensure_texture(ui.ctx(), state);

        let response = ui.allocate_rect(viewport_rect, Sense::click_and_drag());
        let painter = ui.painter_at(viewport_rect);

        // Dark workspace backdrop
        painter.rect_filled(viewport_rect, CornerRadius::ZERO, Color32::from_rgb(11, 17, 30));

        // Handle scroll wheel zoom centered at pointer
        if response.hovered() {
            let scroll_delta = ui.input(|i| i.smooth_scroll_delta.y);
            let zoom_delta = ui.input(|i| i.zoom_delta());
            if (zoom_delta - 1.0).abs() > 1e-3 {
                if let Some(hover_pos) = ui.input(|i| i.pointer.hover_pos()) {
                    self.zoom_around(zoom_delta, hover_pos, viewport_rect.center());
                }
            } else if scroll_delta.abs() > 0.1 {
                let factor = (1.0 + scroll_delta * 0.0018).clamp(0.8, 1.25);
                if let Some(hover_pos) = ui.input(|i| i.pointer.hover_pos()) {
                    self.zoom_around(factor, hover_pos, viewport_rect.center());
                }
            }
        }

        let img_rect = self.image_screen_rect(viewport_rect, img_w, img_h);

        // Subtle drop shadow + checkerboard under transparent images
        let shadow_rect = img_rect.translate(Vec2::new(4.0, 6.0));
        painter.rect_filled(
            shadow_rect,
            CornerRadius::same(2),
            Color32::from_rgba_unmultiplied(0, 0, 0, 110),
        );
        draw_checkerboard(&painter, img_rect.intersect(viewport_rect));

        // Draw base image texture
        if let Some(tex) = &self.texture {
            painter.image(
                tex.id(),
                img_rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }

        // Border around image canvas
        painter.rect_stroke(
            img_rect,
            CornerRadius::ZERO,
            Stroke::new(1.0, Color32::from_rgb(51, 65, 85)),
            StrokeKind::Outside,
        );

        // Draw all committed vector annotations (with viewport frustum culling)
        for ann in &state.annotations {
            self.draw_annotation_preview(ui.ctx(), &painter, viewport_rect, img_rect, state, ann);
        }

        // Handle pointer interaction
        self.handle_pointer_interaction(
            ui,
            &response,
            &painter,
            viewport_rect,
            img_rect,
            state,
            cfg,
            &mut events,
        );

        // Draw selection bounding box & handles if an annotation is selected
        if let Some(sel_id) = self.selected_id {
            if let Some(ann) = state.annotations.iter().find(|a| a.id == sel_id) {
                self.draw_selection_gizmo(&painter, img_rect, ann);
            } else {
                self.selected_id = None;
            }
        }

        // Draw interactive crop overlay if active
        if cfg.tool == Tool::Crop {
            if let Some(crop) = self.crop_rect {
                self.draw_crop_overlay(&painter, img_rect, crop, img_w, img_h);
            }
        }

        // Render inline text input box if active
        self.show_inline_text_editor(ui, img_rect, state, &mut events);

        events
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_pointer_interaction(
        &mut self,
        ui: &mut Ui,
        response: &Response,
        painter: &Painter,
        _viewport_rect: Rect,
        img_rect: Rect,
        state: &mut CanvasState,
        cfg: ToolConfig,
        events: &mut Vec<CanvasEvent>,
    ) {
        let space_down = ui.input(|i| i.key_down(Key::Space)) && self.active_text.is_none();
        let shift_down = ui.input(|i| i.modifiers.shift);
        let pointer_pos = response.interact_pointer_pos();

        // Set appropriate cursor icon
        if space_down || matches!(self.drag_mode, Some(DragMode::Panning)) {
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        } else if response.hovered() {
            let icon = match cfg.tool {
                Tool::Select => CursorIcon::Default,
                Tool::Text => CursorIcon::Text,
                _ => CursorIcon::Crosshair,
            };
            ui.ctx().set_cursor_icon(icon);
        }

        // Middle-mouse drag or Space + Left-drag pans the canvas
        if response.dragged_by(PointerButton::Middle)
            || (space_down && response.dragged_by(PointerButton::Primary))
        {
            self.drag_mode = Some(DragMode::Panning);
            self.pan += response.drag_delta();
            return;
        }

        if response.drag_stopped() && matches!(self.drag_mode, Some(DragMode::Panning)) {
            self.drag_mode = None;
            return;
        }

        let Some(pos) = pointer_pos else {
            return;
        };
        let img_pt = self.screen_to_image(pos, img_rect);
        let (img_w, img_h) = state.dimensions();
        let clamped_img_pt = Pt::new(
            img_pt.x.clamp(0.0, img_w as f32),
            img_pt.y.clamp(0.0, img_h as f32),
        );

        // Single-click tools: NumberStamp & Text
        if response.clicked_by(PointerButton::Primary) && !space_down {
            match cfg.tool {
                Tool::NumberStamp => {
                    self.commit_active_text(state, events);
                    events.push(CanvasEvent::RecordUndo(state.clone()));
                    let id = state.next_id;
                    state.next_id += 1;
                    let num = state.next_number;
                    state.next_number += 1;
                    state.annotations.push(Annotation {
                        id,
                        color: cfg.color,
                        stroke_width: cfg.stroke_width,
                        kind: AnnotationKind::NumberStamp {
                            center: img_pt,
                            number: num,
                            radius: cfg.stamp_radius,
                        },
                    });
                    self.selected_id = Some(id);
                    events.push(CanvasEvent::StatusMessage(format!(
                        "Placed step badge #{}",
                        num
                    )));
                    return;
                }
                Tool::Text => {
                    if self.active_text.is_some() {
                        self.commit_active_text(state, events);
                    } else {
                        self.active_text = Some(ActiveTextInput {
                            pos: img_pt,
                            text: String::new(),
                            color: cfg.color,
                            font_size: cfg.font_size,
                            bg_box: cfg.text_bg_box,
                            just_opened: true,
                        });
                    }
                    return;
                }
                Tool::Select => {
                    self.commit_active_text(state, events);
                    let tol = (8.0 / self.zoom).max(4.0);
                    let hit = state
                        .annotations
                        .iter()
                        .rev()
                        .find(|a| a.hit_test(img_pt, tol))
                        .map(|a| a.id);
                    self.selected_id = hit;
                    return;
                }
                _ => {}
            }
        }

        // Drag start
        if response.drag_started_by(PointerButton::Primary) && !space_down {
            self.commit_active_text(state, events);

            match cfg.tool {
                Tool::Select => {
                    let handle_tol = (10.0 / self.zoom).max(5.0);
                    // 1. Check if user grabbed a handle of the currently selected annotation
                    if let Some(sel_id) = self.selected_id {
                        if let Some(ann) = state.annotations.iter().find(|a| a.id == sel_id) {
                            if let Some((handle, _)) = ann
                                .handles()
                                .into_iter()
                                .find(|(_, hp)| hp.distance(img_pt) <= handle_tol)
                            {
                                self.drag_mode = Some(DragMode::ResizingHandle {
                                    id: sel_id,
                                    handle,
                                    recorded: false,
                                });
                                return;
                            }
                        }
                    }

                    // 2. Otherwise hit-test topmost annotation to move it
                    let tol = (8.0 / self.zoom).max(4.0);
                    if let Some(hit_id) = state
                        .annotations
                        .iter()
                        .rev()
                        .find(|a| a.hit_test(img_pt, tol))
                        .map(|a| a.id)
                    {
                        self.selected_id = Some(hit_id);
                        self.drag_mode = Some(DragMode::MovingAnnotation {
                            id: hit_id,
                            last_img_pt: img_pt,
                            recorded: false,
                        });
                    } else {
                        self.selected_id = None;
                    }
                }
                Tool::Crop => {
                    self.crop_rect = Some(BBox::from_points(clamped_img_pt, clamped_img_pt));
                    self.drag_mode = Some(DragMode::DraggingCrop {
                        start: clamped_img_pt,
                    });
                }
                Tool::Pen
                | Tool::Highlighter
                | Tool::Arrow
                | Tool::Line
                | Tool::Rectangle
                | Tool::Ellipse
                | Tool::Redact => {
                    self.drag_mode = Some(DragMode::DrawingNew {
                        start: img_pt,
                        current: img_pt,
                        points: vec![img_pt],
                    });
                }
                Tool::Text | Tool::NumberStamp => {}
            }
        }

        // Active drag update
        if response.dragged_by(PointerButton::Primary) {
            match &mut self.drag_mode {
                Some(DragMode::MovingAnnotation {
                    id,
                    last_img_pt,
                    recorded,
                }) => {
                    let dx = img_pt.x - last_img_pt.x;
                    let dy = img_pt.y - last_img_pt.y;
                    if dx.abs() > 0.1 || dy.abs() > 0.1 {
                        if !*recorded {
                            events.push(CanvasEvent::RecordUndo(state.clone()));
                            *recorded = true;
                        }
                        if let Some(ann) = state.annotations.iter_mut().find(|a| a.id == *id) {
                            ann.translate(dx, dy);
                        }
                        *last_img_pt = img_pt;
                    }
                }
                Some(DragMode::ResizingHandle {
                    id,
                    handle,
                    recorded,
                }) => {
                    if !*recorded {
                        events.push(CanvasEvent::RecordUndo(state.clone()));
                        *recorded = true;
                    }
                    if let Some(ann) = state.annotations.iter_mut().find(|a| a.id == *id) {
                        ann.move_handle(*handle, img_pt);
                    }
                }
                Some(DragMode::DraggingCrop { start }) => {
                    let end = if shift_down {
                        snap_square(*start, clamped_img_pt)
                    } else {
                        clamped_img_pt
                    };
                    self.crop_rect = Some(BBox::from_points(*start, end));
                }
                Some(DragMode::DrawingNew {
                    start,
                    current,
                    points,
                }) => {
                    let end = match cfg.tool {
                        Tool::Line | Tool::Arrow if shift_down => snap_angle(*start, img_pt),
                        Tool::Rectangle | Tool::Ellipse if shift_down => {
                            snap_square(*start, img_pt)
                        }
                        _ => img_pt,
                    };
                    *current = end;
                    if matches!(cfg.tool, Tool::Pen | Tool::Highlighter) {
                        if points.last().map_or(true, |p| p.distance_sq(img_pt) >= 1.0) {
                            points.push(img_pt);
                        }
                    }

                    if let Some(preview) =
                        build_draft_annotation(0, cfg, *start, *current, points)
                    {
                        self.draw_annotation_preview(
                            ui.ctx(),
                            painter,
                            _viewport_rect,
                            img_rect,
                            state,
                            &preview,
                        );
                    }
                }
                _ => {}
            }
        }

        // Drag release
        if response.drag_stopped() {
            if let Some(mode) = self.drag_mode.take() {
                if let DragMode::DrawingNew {
                    start,
                    current,
                    points,
                } = mode
                {
                    let min_dist = if matches!(cfg.tool, Tool::Pen | Tool::Highlighter) {
                        0.0
                    } else {
                        2.0
                    };
                    if start.distance(current) >= min_dist {
                        let id = state.next_id;
                        if let Some(ann) = build_draft_annotation(id, cfg, start, current, &points) {
                            events.push(CanvasEvent::RecordUndo(state.clone()));
                            state.next_id += 1;
                            state.annotations.push(ann);
                            self.selected_id = Some(id);
                        }
                    }
                }
            }
        }
    }

    fn show_inline_text_editor(
        &mut self,
        ui: &mut Ui,
        img_rect: Rect,
        state: &mut CanvasState,
        events: &mut Vec<CanvasEvent>,
    ) {
        let Some(mut active) = self.active_text.clone() else {
            return;
        };

        let screen_pos = self.image_to_screen(active.pos, img_rect);
        let scaled_font = (active.font_size * self.zoom).clamp(11.0, 96.0);
        let editor_rect = Rect::from_min_size(screen_pos, Vec2::new(280.0, scaled_font * 2.4 + 28.0));

        let mut commit = false;
        let mut cancel = false;

        egui::Area::new(egui::Id::new("imgtool-inline-text-editor"))
            .fixed_pos(editor_rect.min)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(15, 23, 42, 235))
                    .stroke(Stroke::new(1.5, active.color))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(egui::Margin::same(6))
                    .show(ui, |ui| {
                        let te_id = egui::Id::new("imgtool-inline-text-input");
                        let resp = ui.add(
                            egui::TextEdit::multiline(&mut active.text)
                                .id(te_id)
                                .font(FontId::proportional(scaled_font.min(28.0)))
                                .text_color(active.color)
                                .desired_width(240.0)
                                .desired_rows(2)
                                .hint_text("Type annotation text... (Ctrl+Enter or click Done)"),
                        );
                        if active.just_opened {
                            resp.request_focus();
                            active.just_opened = false;
                        }
                        ui.horizontal(|ui| {
                            if ui
                                .button(
                                    egui::RichText::new("Done (Ctrl+Enter)")
                                        .size(11.5)
                                        .color(Color32::from_rgb(56, 189, 248)),
                                )
                                .clicked()
                            {
                                commit = true;
                            }
                            if ui
                                .button(
                                    egui::RichText::new("Cancel (Esc)")
                                        .size(11.5)
                                        .color(Color32::from_rgb(148, 163, 184)),
                                )
                                .clicked()
                            {
                                cancel = true;
                            }
                        });
                    });
            });

        if ui.input(|i| i.key_pressed(Key::Escape)) {
            cancel = true;
        } else if ui.input(|i| i.key_pressed(Key::Enter) && (i.modifiers.ctrl || i.modifiers.command))
        {
            commit = true;
        }

        if cancel {
            self.active_text = None;
        } else if commit {
            self.active_text = Some(active);
            self.commit_active_text(state, events);
            events.push(CanvasEvent::SwitchTool(Tool::Select));
        } else {
            self.active_text = Some(active);
        }
    }

    fn draw_annotation_preview(
        &mut self,
        ctx: &egui::Context,
        painter: &Painter,
        viewport_rect: Rect,
        img_rect: Rect,
        state: &CanvasState,
        ann: &Annotation,
    ) {
        // Fast viewport frustum culling: skip off-screen annotations completely
        let ann_bbox = ann.bounding_box();
        let screen_bbox = Rect::from_min_max(
            self.image_to_screen(ann_bbox.min, img_rect),
            self.image_to_screen(ann_bbox.max, img_rect),
        );
        if !screen_bbox.expand(8.0).intersects(viewport_rect) {
            return;
        }

        let stroke_px = (ann.stroke_width * self.zoom).max(1.2);
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
                let width = if *is_highlighter {
                    (ann.stroke_width * 2.8 * self.zoom).max(8.0)
                } else {
                    stroke_px
                };
                let mut screen_pts: Vec<Pos2> = Vec::with_capacity(points.len());
                for &p in points {
                    let sp = self.image_to_screen(p, img_rect);
                    if screen_pts
                        .last()
                        .is_none_or(|prev| prev.distance_sq(sp) >= 0.5)
                    {
                        screen_pts.push(sp);
                    }
                }
                if screen_pts.len() == 1 {
                    painter.circle_filled(screen_pts[0], width * 0.5, color);
                } else {
                    painter.add(Shape::line(screen_pts, Stroke::new(width, color)));
                }
            }
            AnnotationKind::Line { start, end } => {
                let p0 = self.image_to_screen(*start, img_rect);
                let p1 = self.image_to_screen(*end, img_rect);
                painter.line_segment([p0, p1], Stroke::new(stroke_px, ann.color));
            }
            AnnotationKind::Arrow { start, end } => {
                let (tip, left, right, shaft_end) =
                    compute_arrow_geometry(*start, *end, ann.stroke_width);
                let s_start = self.image_to_screen(*start, img_rect);
                let s_shaft = self.image_to_screen(shaft_end, img_rect);
                let s_tip = self.image_to_screen(tip, img_rect);
                let s_left = self.image_to_screen(left, img_rect);
                let s_right = self.image_to_screen(right, img_rect);

                painter.line_segment([s_start, s_shaft], Stroke::new(stroke_px, ann.color));
                painter.add(Shape::convex_polygon(
                    vec![s_tip, s_left, s_right],
                    ann.color,
                    Stroke::NONE,
                ));
            }
            AnnotationKind::Rectangle { min, max, filled } => {
                let b = BBox::from_points(*min, *max);
                let rect = Rect::from_min_max(
                    self.image_to_screen(b.min, img_rect),
                    self.image_to_screen(b.max, img_rect),
                );
                if *filled {
                    let fill = Color32::from_rgba_unmultiplied(
                        ann.color.r(),
                        ann.color.g(),
                        ann.color.b(),
                        56,
                    );
                    painter.rect_filled(rect, CornerRadius::ZERO, fill);
                }
                painter.rect_stroke(
                    rect,
                    CornerRadius::ZERO,
                    Stroke::new(stroke_px, ann.color),
                    StrokeKind::Middle,
                );
            }
            AnnotationKind::Ellipse { min, max, filled } => {
                let b = BBox::from_points(*min, *max);
                let rect = Rect::from_min_max(
                    self.image_to_screen(b.min, img_rect),
                    self.image_to_screen(b.max, img_rect),
                );
                let radius = rect.size() * 0.5;
                let fill = if *filled {
                    Color32::from_rgba_unmultiplied(
                        ann.color.r(),
                        ann.color.g(),
                        ann.color.b(),
                        56,
                    )
                } else {
                    Color32::TRANSPARENT
                };
                painter.add(egui::epaint::EllipseShape {
                    center: rect.center(),
                    radius,
                    fill,
                    stroke: Stroke::new(stroke_px, ann.color),
                    angle: 0.0,
                });
            }
            AnnotationKind::Text {
                pos,
                text,
                font_size,
                bg_box,
            } => {
                if text.is_empty() {
                    return;
                }
                let scaled_size = (*font_size * self.zoom).max(4.0);
                let pad = if *bg_box {
                    (*font_size * 0.35 * self.zoom).max(3.0)
                } else {
                    0.0
                };
                let origin = self.image_to_screen(*pos, img_rect);
                let galley = painter.layout_no_wrap(
                    text.clone(),
                    FontId::proportional(scaled_size),
                    ann.color,
                );
                if *bg_box {
                    let box_rect = Rect::from_min_size(
                        origin,
                        galley.size() + Vec2::splat(pad * 2.0),
                    );
                    let lum = (0.299 * ann.color.r() as f32
                        + 0.587 * ann.color.g() as f32
                        + 0.114 * ann.color.b() as f32)
                        / 255.0;
                    let bg = if lum > 0.55 {
                        Color32::from_rgba_unmultiplied(15, 23, 42, 215)
                    } else {
                        Color32::from_rgba_unmultiplied(248, 250, 252, 225)
                    };
                    painter.rect_filled(box_rect, CornerRadius::same(3), bg);
                    painter.rect_stroke(
                        box_rect,
                        CornerRadius::same(3),
                        Stroke::new((1.5 * self.zoom).max(1.0), ann.color),
                        StrokeKind::Middle,
                    );
                }
                painter.galley(origin + Vec2::splat(pad), galley, ann.color);
            }
            AnnotationKind::NumberStamp {
                center,
                number,
                radius,
            } => {
                let c = self.image_to_screen(*center, img_rect);
                let r = (*radius * self.zoom).max(6.0);
                painter.circle_filled(
                    c,
                    r + (2.0 * self.zoom).max(1.5),
                    Color32::from_rgba_unmultiplied(255, 255, 255, 235),
                );
                painter.circle_filled(c, r, ann.color);

                let lum = (0.299 * ann.color.r() as f32
                    + 0.587 * ann.color.g() as f32
                    + 0.114 * ann.color.b() as f32)
                    / 255.0;
                let text_col = if lum > 0.62 {
                    Color32::from_rgb(15, 23, 42)
                } else {
                    Color32::WHITE
                };
                painter.text(
                    c,
                    Align2::CENTER_CENTER,
                    number.to_string(),
                    FontId::proportional(r * 1.25),
                    text_col,
                );
            }
            AnnotationKind::Redact {
                min,
                max,
                block_size,
                style,
            } => {
                let b = BBox::from_points(*min, *max);
                let rect = Rect::from_min_max(
                    self.image_to_screen(b.min, img_rect),
                    self.image_to_screen(b.max, img_rect),
                );
                match style {
                    RedactStyle::Blackout => {
                        painter.rect_filled(rect, CornerRadius::ZERO, ann.color);
                    }
                    RedactStyle::Pixelate => {
                        let ptr = std::sync::Arc::as_ptr(&state.base_image) as usize;
                        let key = compute_single_redact_key(ptr, *min, *max, *block_size);
                        let opts = if self.zoom >= 2.0 {
                            TextureOptions::NEAREST
                        } else {
                            TextureOptions::LINEAR
                        };

                        let patch_idx = self
                            .pixelate_patches
                            .iter()
                            .position(|p| p.ann_id == ann.id);

                        let needs_update = match patch_idx {
                            Some(idx) => self.pixelate_patches[idx].key != key,
                            None => true,
                        };

                        if needs_update {
                            if let Some((bounds_px, patch_img)) =
                                crate::render::render_pixelate_patch_color_image(
                                    &state.base_image,
                                    b,
                                    *block_size,
                                )
                            {
                                if let Some(idx) = patch_idx {
                                    let entry = &mut self.pixelate_patches[idx];
                                    entry.key = key;
                                    entry.bounds_px = bounds_px;
                                    entry.texture.set(patch_img, opts);
                                } else {
                                    let tex = ctx.load_texture(
                                        format!("imgtool-redact-{}", ann.id),
                                        patch_img,
                                        opts,
                                    );
                                    self.pixelate_patches.push(PixelatePatchCache {
                                        ann_id: ann.id,
                                        key,
                                        bounds_px,
                                        texture: tex,
                                    });
                                }
                            }
                        }

                        if let Some(entry) =
                            self.pixelate_patches.iter().find(|p| p.ann_id == ann.id)
                        {
                            let [x0, y0, x1, y1] = entry.bounds_px;
                            let patch_rect = Rect::from_min_max(
                                self.image_to_screen(Pt::new(x0 as f32, y0 as f32), img_rect),
                                self.image_to_screen(Pt::new(x1 as f32, y1 as f32), img_rect),
                            );
                            painter.image(
                                entry.texture.id(),
                                patch_rect,
                                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                                Color32::WHITE,
                            );
                        }

                        // While dragging a new pixelate box, also draw an accent preview border
                        if ann.id == 0 {
                            painter.rect_stroke(
                                rect,
                                CornerRadius::ZERO,
                                Stroke::new(1.5, Color32::from_rgb(56, 189, 248)),
                                StrokeKind::Inside,
                            );
                        }
                    }
                }
            }
        }
    }

    fn draw_selection_gizmo(&self, painter: &Painter, img_rect: Rect, ann: &Annotation) {
        let bbox = ann.bounding_box();
        let rect = Rect::from_min_max(
            self.image_to_screen(bbox.min, img_rect),
            self.image_to_screen(bbox.max, img_rect),
        );
        let accent = Color32::from_rgb(56, 189, 248);
        painter.rect_stroke(
            rect,
            CornerRadius::same(2),
            Stroke::new(1.2, accent),
            StrokeKind::Outside,
        );

        for (_, hp) in ann.handles() {
            let sp = self.image_to_screen(hp, img_rect);
            painter.circle_filled(sp, 5.5, Color32::WHITE);
            painter.circle_stroke(sp, 5.5, Stroke::new(2.0, accent));
        }
    }

    fn draw_crop_overlay(
        &self,
        painter: &Painter,
        img_rect: Rect,
        crop: BBox,
        img_w: u32,
        img_h: u32,
    ) {
        let crop_screen = Rect::from_min_max(
            self.image_to_screen(crop.min, img_rect),
            self.image_to_screen(crop.max, img_rect),
        )
        .intersect(img_rect);

        let dim = Color32::from_rgba_unmultiplied(2, 6, 23, 165);
        // Top, Bottom, Left, Right dim bands around crop_screen
        painter.rect_filled(
            Rect::from_min_max(img_rect.min, Pos2::new(img_rect.max.x, crop_screen.min.y)),
            CornerRadius::ZERO,
            dim,
        );
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(img_rect.min.x, crop_screen.max.y), img_rect.max),
            CornerRadius::ZERO,
            dim,
        );
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(img_rect.min.x, crop_screen.min.y),
                Pos2::new(crop_screen.min.x, crop_screen.max.y),
            ),
            CornerRadius::ZERO,
            dim,
        );
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(crop_screen.max.x, crop_screen.min.y),
                Pos2::new(img_rect.max.x, crop_screen.max.y),
            ),
            CornerRadius::ZERO,
            dim,
        );

        let accent = Color32::from_rgb(56, 189, 248);
        painter.rect_stroke(
            crop_screen,
            CornerRadius::ZERO,
            Stroke::new(2.0, accent),
            StrokeKind::Outside,
        );

        let w_px = (crop.width().round() as u32).min(img_w);
        let h_px = (crop.height().round() as u32).min(img_h);
        let label = format!("{} × {} px  (Press Enter to Crop, Esc to Cancel)", w_px, h_px);
        let badge_pos = Pos2::new(crop_screen.min.x, (crop_screen.min.y - 24.0).max(img_rect.min.y + 4.0));
        let galley = painter.layout_no_wrap(label, FontId::proportional(12.0), Color32::WHITE);
        let badge_rect = Rect::from_min_size(badge_pos, galley.size() + Vec2::new(12.0, 6.0));
        painter.rect_filled(
            badge_rect,
            CornerRadius::same(4),
            Color32::from_rgba_unmultiplied(15, 23, 42, 230),
        );
        painter.galley(badge_pos + Vec2::new(6.0, 3.0), galley, Color32::WHITE);
    }
}

fn build_draft_annotation(
    id: u64,
    cfg: ToolConfig,
    start: Pt,
    end: Pt,
    raw_points: &[Pt],
) -> Option<Annotation> {
    let kind = match cfg.tool {
        Tool::Pen => AnnotationKind::Freehand {
            points: smooth_freehand_points(raw_points),
            is_highlighter: false,
        },
        Tool::Highlighter => AnnotationKind::Freehand {
            points: smooth_freehand_points(raw_points),
            is_highlighter: true,
        },
        Tool::Arrow => AnnotationKind::Arrow { start, end },
        Tool::Line => AnnotationKind::Line { start, end },
        Tool::Rectangle => {
            let b = BBox::from_points(start, end);
            AnnotationKind::Rectangle {
                min: b.min,
                max: b.max,
                filled: cfg.fill_shapes,
            }
        }
        Tool::Ellipse => {
            let b = BBox::from_points(start, end);
            AnnotationKind::Ellipse {
                min: b.min,
                max: b.max,
                filled: cfg.fill_shapes,
            }
        }
        Tool::Redact => {
            let b = BBox::from_points(start, end);
            AnnotationKind::Redact {
                min: b.min,
                max: b.max,
                block_size: cfg.redact_block_size,
                style: cfg.redact_style,
            }
        }
        _ => return None,
    };

    Some(Annotation {
        id,
        color: cfg.color,
        stroke_width: cfg.stroke_width,
        kind,
    })
}

#[inline]
fn compute_single_redact_key(ptr: usize, min: Pt, max: Pt, block_size: u32) -> u64 {
    let mut h = 1469598103934665603u64 ^ (ptr as u64);
    for v in [
        min.x.to_bits(),
        min.y.to_bits(),
        max.x.to_bits(),
        max.y.to_bits(),
        block_size,
    ] {
        h ^= v as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h
}

fn draw_checkerboard(painter: &Painter, rect: Rect) {
    if !rect.is_positive() {
        return;
    }
    painter.rect_filled(rect, CornerRadius::ZERO, Color32::from_rgb(30, 41, 59));
}
