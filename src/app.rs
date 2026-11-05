//! Main `eframe::App` implementation for `imgtool`:
//! - Top Action Bar: Open, Quick Save (`Ctrl+S`), Save As (`Ctrl+Shift+S`), Copy (`Ctrl+C`),
//!   Paste (`Ctrl+V`), Undo/Redo, Folder Prev/Next (`◀`/`▶`), Rotate/Flip/Expand Canvas,
//!   and `Set as Default Image Viewer`.
//! - Left Tool Dock & Property Bar: All `ksnip` tools, quick color palette,
//!   stroke width / font size sliders, shape fill toggle, and redaction style.
//! - Bottom Status Bar: Image dimensions, zoom controls (`Fit`, `100%`, `-`, `+`), sibling file counter,
//!   and toast notification with quick `Reveal in Folder` button.

use crate::annotations::{AnnotationKind, RedactStyle, Tool, PALETTE_COLORS};
use crate::canvas::{CanvasEvent, ToolConfig, ViewportCanvas};
use crate::desktop;
use crate::history::{
    apply_crop, apply_expand_canvas, apply_flip_horizontal, apply_flip_vertical, apply_rotate_ccw,
    apply_rotate_cw, CanvasState, History,
};
use crate::io;
use crate::render::render_final_image;
use eframe::egui::{
    self, Color32, CornerRadius, Key, Margin, RichText, Stroke, Vec2,
};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

struct Toast {
    message: String,
    is_error: bool,
    saved_path: Option<PathBuf>,
    created_at: Instant,
}

pub struct ImgToolApp {
    pub state: CanvasState,
    pub history: History,
    pub canvas: ViewportCanvas,

    pub current_path: Option<PathBuf>,
    pub sibling_files: Vec<PathBuf>,
    pub sibling_index: Option<usize>,

    pub active_tool: Tool,
    pub active_color: Color32,
    pub stroke_width: f32,
    pub font_size: f32,
    pub fill_shapes: bool,
    pub text_bg_box: bool,
    pub redact_style: RedactStyle,
    pub redact_block_size: u32,
    pub stamp_radius: f32,
    pub expand_margin: u32,

    pub desktop_installed: bool,
    toast: Option<Toast>,
    ime_preedit: Option<(String, Option<Range<usize>>)>,
}

impl ImgToolApp {
    pub fn new(cc: &eframe::CreationContext<'_>, initial_path: Option<PathBuf>) -> Self {
        apply_dark_theme(&cc.egui_ctx);

        #[cfg(target_os = "macos")]
        crate::icon::apply_macos_native_icon(None);

        // Self-healing check on startup so any versioned mise desktop path is updated to the shim
        desktop::refresh_exec_path();
        let mut app = Self {
            state: CanvasState::new(io::create_blank_canvas(1280, 800)),
            history: History::default(),
            canvas: ViewportCanvas::default(),
            current_path: None,
            sibling_files: Vec::new(),
            sibling_index: None,
            active_tool: Tool::Arrow,
            active_color: PALETTE_COLORS[0].0,
            stroke_width: 4.0,
            font_size: 22.0,
            fill_shapes: false,
            text_bg_box: true,
            redact_style: RedactStyle::Pixelate,
            redact_block_size: 14,
            stamp_radius: 16.0,
            expand_margin: 64,
            desktop_installed: desktop::is_desktop_installed(),
            toast: None,
            ime_preedit: None,
        };

        if let Some(path) = initial_path {
            app.open_path(&path, &cc.egui_ctx);
        } else {
            app.set_toast(
                "Ready — Open an image (Ctrl+O), Paste from Clipboard (Ctrl+V), or Drag & Drop a file",
                false,
                None,
            );
        }

        app
    }

    pub fn open_path(&mut self, path: &Path, ctx: &egui::Context) {
        self.load_image_file(path, true, ctx);
    }

    fn load_image_file(&mut self, path: &Path, rescan_siblings: bool, ctx: &egui::Context) {
        match io::load_image_from_path(path) {
            Ok(rgba) => {
                let (w, h) = rgba.dimensions();
                self.state = CanvasState::new(rgba);
                self.history.clear();
                self.canvas.reset_view();
                self.canvas.invalidate_texture();
                self.current_path = Some(path.to_path_buf());
                if rescan_siblings {
                    let (siblings, idx) = io::list_sibling_images(path);
                    self.sibling_files = siblings;
                    self.sibling_index = idx;
                }

                let name = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("image");
                ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
                    "{} ({}×{}) — ImgTool",
                    name, w, h
                )));
                self.set_toast(
                    format!("Opened {} ({} × {} px)", name, w, h),
                    false,
                    None,
                );
            }
            Err(err) => {
                self.set_toast(err, true, None);
            }
        }
    }

    fn navigate_sibling(&mut self, delta: isize, ctx: &egui::Context) {
        if self.sibling_files.len() <= 1 {
            return;
        }
        let cur = self.sibling_index.unwrap_or(0) as isize;
        let len = self.sibling_files.len() as isize;
        let next = (cur + delta).rem_euclid(len) as usize;
        if let Some(target) = self.sibling_files.get(next).cloned() {
            self.sibling_index = Some(next);
            self.load_image_file(&target, false, ctx);
        }
    }

    fn set_toast(&mut self, message: impl Into<String>, is_error: bool, saved_path: Option<PathBuf>) {
        self.toast = Some(Toast {
            message: message.into(),
            is_error,
            saved_path,
            created_at: Instant::now(),
        });
    }

    fn flush_active_text(&mut self) {
        let mut events = Vec::new();
        self.canvas.commit_active_text(&mut self.state, &mut events);
        for ev in events {
            if let CanvasEvent::RecordUndo(snap) = ev {
                self.history.record(&snap);
            }
        }
    }

    fn quick_save(&mut self) {
        self.flush_active_text();
        let rendered = render_final_image(&self.state.base_image, &self.state.annotations);
        let out_path = io::build_quick_save_path(self.current_path.as_deref());
        match io::save_image_to_path(&rendered, &out_path) {
            Ok(()) => {
                let file_name = out_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("edited.png");
                self.set_toast(
                    format!("Saved new file: {}", file_name),
                    false,
                    Some(out_path),
                );
            }
            Err(e) => self.set_toast(e, true, None),
        }
    }

    fn save_as(&mut self) {
        self.flush_active_text();
        let Some(dest_path) = io::pick_save_image_file(self.current_path.as_deref()) else {
            return;
        };
        let rendered = render_final_image(&self.state.base_image, &self.state.annotations);
        match io::save_image_to_path(&rendered, &dest_path) {
            Ok(()) => {
                self.set_toast(
                    format!("Saved as '{}'", dest_path.display()),
                    false,
                    Some(dest_path),
                );
            }
            Err(e) => self.set_toast(e, true, None),
        }
    }

    fn copy_to_clipboard(&mut self) {
        self.flush_active_text();
        let rendered = render_final_image(&self.state.base_image, &self.state.annotations);
        match io::copy_image_to_clipboard(&rendered) {
            Ok(()) => {
                let (w, h) = rendered.dimensions();
                self.set_toast(
                    format!("Copied {} × {} px image to Clipboard (ready to paste with Ctrl+V)", w, h),
                    false,
                    None,
                );
            }
            Err(e) => self.set_toast(e, true, None),
        }
    }

    fn paste_from_clipboard(&mut self, ctx: &egui::Context) {
        match io::paste_image_from_clipboard() {
            Ok(rgba) => {
                let (w, h) = rgba.dimensions();
                self.history.record(&self.state);
                self.state = CanvasState::new(rgba);
                self.canvas.reset_view();
                self.canvas.invalidate_texture();
                self.current_path = None;
                self.sibling_files.clear();
                self.sibling_index = None;
                ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
                    "Clipboard Image ({}×{}) — ImgTool",
                    w, h
                )));
                self.set_toast(
                    format!("Pasted {} × {} px image from Clipboard", w, h),
                    false,
                    None,
                );
            }
            Err(e) => self.set_toast(e, true, None),
        }
    }

    fn delete_selected_annotation(&mut self) {
        if let Some(sel_id) = self.canvas.selected_id.take() {
            if self.state.annotations.iter().any(|a| a.id == sel_id) {
                self.history.record(&self.state);
                self.state.annotations.retain(|a| a.id != sel_id);
                self.canvas.invalidate_texture();
                self.set_toast("Deleted selected annotation", false, None);
            }
        }
    }

    fn handle_keyboard_shortcuts(&mut self, ctx: &egui::Context) {
        // Check for Drag & Drop files first
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if let Some(first_file) = dropped.first() {
            self.open_path(first_file, ctx);
            return;
        }

        // Do not intercept single-letter tool shortcuts while typing inside a text field
        let text_editing = ctx.egui_wants_keyboard_input() || self.canvas.active_text.is_some();

        let (ctrl, shift) = ctx.input(|i| (i.modifiers.ctrl || i.modifiers.command, i.modifiers.shift));

        // Global Ctrl/Cmd shortcuts
        if ctrl && ctx.input(|i| i.key_pressed(Key::O)) {
            if let Some(path) = io::pick_open_image_file(self.current_path.as_deref()) {
                self.open_path(&path, ctx);
            }
            return;
        }
        if ctrl && shift && ctx.input(|i| i.key_pressed(Key::S)) {
            self.save_as();
            return;
        }
        if ctrl && !shift && ctx.input(|i| i.key_pressed(Key::S)) {
            self.quick_save();
            return;
        }
        if ctrl && !text_editing && ctx.input(|i| i.key_pressed(Key::C)) {
            self.copy_to_clipboard();
            return;
        }
        if ctrl && !text_editing && ctx.input(|i| i.key_pressed(Key::V)) {
            self.paste_from_clipboard(ctx);
            return;
        }
        if ctrl && shift && ctx.input(|i| i.key_pressed(Key::Z)) {
            if self.history.redo(&mut self.state) {
                self.canvas.invalidate_texture();
            }
            return;
        }
        if ctrl && !shift && ctx.input(|i| i.key_pressed(Key::Z)) {
            if self.history.undo(&mut self.state) {
                self.canvas.invalidate_texture();
            }
            return;
        }
        if ctrl && ctx.input(|i| i.key_pressed(Key::Y)) {
            if self.history.redo(&mut self.state) {
                self.canvas.invalidate_texture();
            }
            return;
        }

        if text_editing {
            return;
        }

        // Crop Enter / Escape handling
        if self.active_tool == Tool::Crop {
            if ctx.input(|i| i.key_pressed(Key::Enter)) {
                if let Some(crop_box) = self.canvas.crop_rect.take() {
                    self.history.record(&self.state);
                    if apply_crop(&mut self.state, crop_box) {
                        self.canvas.reset_view();
                        self.canvas.invalidate_texture();
                        let (w, h) = self.state.dimensions();
                        self.set_toast(format!("Cropped to {} × {} px", w, h), false, None);
                    }
                    self.active_tool = Tool::Select;
                }
                return;
            }
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                self.canvas.crop_rect = None;
                self.active_tool = Tool::Select;
                return;
            }
        }

        // Single-key tool & navigation shortcuts
        ctx.input(|i| {
            if i.key_pressed(Key::Escape) || i.key_pressed(Key::V) {
                self.active_tool = Tool::Select;
            } else if i.key_pressed(Key::P) {
                self.active_tool = Tool::Pen;
            } else if i.key_pressed(Key::H) {
                self.active_tool = Tool::Highlighter;
            } else if i.key_pressed(Key::A) {
                self.active_tool = Tool::Arrow;
            } else if i.key_pressed(Key::L) {
                self.active_tool = Tool::Line;
            } else if i.key_pressed(Key::R) {
                self.active_tool = Tool::Rectangle;
            } else if i.key_pressed(Key::E) {
                self.active_tool = Tool::Ellipse;
            } else if i.key_pressed(Key::T) {
                self.active_tool = Tool::Text;
            } else if i.key_pressed(Key::N) {
                self.active_tool = Tool::NumberStamp;
            } else if i.key_pressed(Key::B) || i.key_pressed(Key::X) {
                self.active_tool = Tool::Redact;
            } else if i.key_pressed(Key::C) {
                self.active_tool = Tool::Crop;
            } else if i.key_pressed(Key::F) {
                self.canvas.needs_fit = true;
            } else if i.key_pressed(Key::Num1) {
                self.canvas.set_zoom_100();
            }
        });

        if ctx.input(|i| i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace)) {
            self.delete_selected_annotation();
        }
        if ctx.input(|i| i.key_pressed(Key::ArrowLeft)) {
            self.navigate_sibling(-1, ctx);
        }
        if ctx.input(|i| i.key_pressed(Key::ArrowRight)) {
            self.navigate_sibling(1, ctx);
        }
    }

    fn draw_top_bar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);

            // File operations
            if toolbar_button(ui, "📂 Open", false)
                .on_hover_text("Open an image file (Ctrl+O)")
                .clicked()
            {
                if let Some(path) = io::pick_open_image_file(self.current_path.as_deref()) {
                    self.open_path(&path, &ctx);
                }
            }

            if primary_action_button(ui, "💾 Quick Save")
                .on_hover_text(
                    "Save immediately as a new file <name>_edited_<timestamp>.png in the same folder (Ctrl+S)",
                )
                .clicked()
            {
                self.quick_save();
            }

            if toolbar_button(ui, "Save As...", false)
                .on_hover_text("Save As PNG, JPEG, WebP, BMP, or TIFF (Ctrl+Shift+S)")
                .clicked()
            {
                self.save_as();
            }

            if primary_action_button(ui, "📋 Copy")
                .on_hover_text("Copy annotated image at 1:1 native resolution to Clipboard (Ctrl+C)")
                .clicked()
            {
                self.copy_to_clipboard();
            }

            if toolbar_button(ui, "📥 Paste", false)
                .on_hover_text("Paste image from Clipboard (Ctrl+V)")
                .clicked()
            {
                self.paste_from_clipboard(&ctx);
            }

            ui.separator();

            // Folder navigation
            let has_siblings = self.sibling_files.len() > 1;
            ui.add_enabled_ui(has_siblings, |ui| {
                if toolbar_button(ui, "◀ Prev", false)
                    .on_hover_text("Previous image in folder (Left Arrow)")
                    .clicked()
                {
                    self.navigate_sibling(-1, &ctx);
                }
                if toolbar_button(ui, "Next ▶", false)
                    .on_hover_text("Next image in folder (Right Arrow)")
                    .clicked()
                {
                    self.navigate_sibling(1, &ctx);
                }
            });

            ui.separator();

            // Undo / Redo / Clear
            ui.add_enabled_ui(self.history.can_undo(), |ui| {
                if toolbar_button(ui, "↩ Undo", false)
                    .on_hover_text("Undo last action (Ctrl+Z)")
                    .clicked()
                {
                    if self.history.undo(&mut self.state) {
                        self.canvas.invalidate_texture();
                    }
                }
            });
            ui.add_enabled_ui(self.history.can_redo(), |ui| {
                if toolbar_button(ui, "↪ Redo", false)
                    .on_hover_text("Redo action (Ctrl+Y / Ctrl+Shift+Z)")
                    .clicked()
                {
                    if self.history.redo(&mut self.state) {
                        self.canvas.invalidate_texture();
                    }
                }
            });

            if !self.state.annotations.is_empty()
                && toolbar_button(ui, "🗑 Clear", false)
                    .on_hover_text("Clear all annotations (Undoable)")
                    .clicked()
            {
                self.history.record(&self.state);
                self.state.annotations.clear();
                self.state.next_number = 1;
                self.canvas.selected_id = None;
                self.canvas.invalidate_texture();
            }

            ui.separator();

            // Canvas Transforms: Rotate / Flip / Expand Margin
            if toolbar_button(ui, "⟲ 90°", false)
                .on_hover_text("Rotate 90° Counter-Clockwise")
                .clicked()
            {
                self.history.record(&self.state);
                apply_rotate_ccw(&mut self.state);
                self.canvas.needs_fit = true;
                self.canvas.invalidate_texture();
            }
            if toolbar_button(ui, "⟳ 90°", false)
                .on_hover_text("Rotate 90° Clockwise")
                .clicked()
            {
                self.history.record(&self.state);
                apply_rotate_cw(&mut self.state);
                self.canvas.needs_fit = true;
                self.canvas.invalidate_texture();
            }
            if toolbar_button(ui, "⇄ Flip H", false)
                .on_hover_text("Flip Horizontally")
                .clicked()
            {
                self.history.record(&self.state);
                apply_flip_horizontal(&mut self.state);
                self.canvas.invalidate_texture();
            }
            if toolbar_button(ui, "⇅ Flip V", false)
                .on_hover_text("Flip Vertically")
                .clicked()
            {
                self.history.record(&self.state);
                apply_flip_vertical(&mut self.state);
                self.canvas.invalidate_texture();
            }
            if toolbar_button(ui, "⊞ Expand Canvas", false)
                .on_hover_text("Add white margin around the image for external callout notes")
                .clicked()
            {
                self.history.record(&self.state);
                apply_expand_canvas(&mut self.state, self.expand_margin, Color32::WHITE);
                self.canvas.needs_fit = true;
                self.canvas.invalidate_texture();
                self.set_toast(
                    format!("Added {} px padding around canvas", self.expand_margin),
                    false,
                    None,
                );
            }

            ui.separator();

            // Default Image Viewer Registration button
            let default_label = if self.desktop_installed {
                "✓ Default Viewer"
            } else {
                "★ Set Default Viewer"
            };
            if toolbar_button(ui, default_label, self.desktop_installed)
                .on_hover_text(
                    "Register ImgTool as the Default Image Viewer (uses mise shim path so upgrades never break)",
                )
                .clicked()
            {
                match desktop::install_desktop() {
                    Ok(msg) => {
                        self.desktop_installed = true;
                        self.set_toast(msg, false, None);
                    }
                    Err(err) => self.set_toast(err, true, None),
                }
            }
        });
    }

    fn draw_property_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);

            // Tool selector row
            for tool in Tool::ALL {
                let selected = self.active_tool == tool;
                let text = format!("{} ({})", tool.label(), tool.shortcut_label());
                if toolbar_button(ui, &text, selected)
                    .on_hover_text(tool.tooltip())
                    .clicked()
                {
                    self.active_tool = tool;
                }
            }

            ui.separator();

            // Color palette swatches
            ui.label(RichText::new("Color:").size(12.0).color(Color32::from_rgb(148, 163, 184)));
            for &(col, name) in &PALETTE_COLORS {
                let is_active = self.active_color == col;
                let (rect, resp) = ui.allocate_exact_size(Vec2::splat(20.0), egui::Sense::click());
                if ui.is_rect_visible(rect) {
                    ui.painter().rect_filled(rect, CornerRadius::same(4), col);
                    let border_col = if is_active {
                        Color32::from_rgb(56, 189, 248)
                    } else {
                        Color32::from_rgb(71, 85, 105)
                    };
                    let stroke_w = if is_active { 2.5 } else { 1.0 };
                    ui.painter().rect_stroke(
                        rect,
                        CornerRadius::same(4),
                        Stroke::new(stroke_w, border_col),
                        egui::StrokeKind::Outside,
                    );
                }
                if resp.on_hover_text(name).clicked() {
                    self.active_color = col;
                    // Also recolor currently selected annotation if one is selected
                    if let Some(sel_id) = self.canvas.selected_id {
                        if let Some(ann) =
                            self.state.annotations.iter_mut().find(|a| a.id == sel_id)
                        {
                            ann.color = col;
                        }
                    }
                }
            }

            ui.separator();

            // Context-sensitive tool properties
            match self.active_tool {
                Tool::Text => {
                    ui.label(RichText::new("Font Size:").size(12.0));
                    ui.add(egui::Slider::new(&mut self.font_size, 10.0..=72.0).suffix(" px"));
                    ui.checkbox(&mut self.text_bg_box, "Callout Box");
                }
                Tool::NumberStamp => {
                    ui.label(RichText::new("Stamp Size:").size(12.0));
                    ui.add(egui::Slider::new(&mut self.stamp_radius, 10.0..=48.0).suffix(" px"));
                    ui.label(
                        RichText::new(format!("Next: #{}", self.state.next_number))
                            .size(12.0)
                            .color(Color32::from_rgb(56, 189, 248)),
                    );
                    if ui.small_button("Reset to #1").clicked() {
                        self.state.next_number = 1;
                    }
                }
                Tool::Redact => {
                    let mut changed = false;
                    if ui
                        .selectable_value(
                            &mut self.redact_style,
                            RedactStyle::Pixelate,
                            "Pixelate",
                        )
                        .changed()
                    {
                        changed = true;
                    }
                    if ui
                        .selectable_value(
                            &mut self.redact_style,
                            RedactStyle::Blackout,
                            "Solid Blackout",
                        )
                        .changed()
                    {
                        changed = true;
                    }
                    if self.redact_style == RedactStyle::Pixelate {
                        ui.label(RichText::new("Block:").size(12.0));
                        if ui
                            .add(egui::Slider::new(&mut self.redact_block_size, 6..=40).suffix(" px"))
                            .changed()
                        {
                            changed = true;
                        }
                    }
                    if changed {
                        if let Some(sel_id) = self.canvas.selected_id {
                            if let Some(ann) =
                                self.state.annotations.iter_mut().find(|a| a.id == sel_id)
                            {
                                if let AnnotationKind::Redact {
                                    block_size, style, ..
                                } = &mut ann.kind
                                {
                                    *style = self.redact_style;
                                    *block_size = self.redact_block_size;
                                }
                            }
                        }
                    }
                }
                Tool::Crop => {
                    if let Some(crop) = self.canvas.crop_rect {
                        let w = crop.width().round() as u32;
                        let h = crop.height().round() as u32;
                        if w >= 4 && h >= 4 {
                            if primary_action_button(ui, &format!("✓ Apply Crop ({}×{})", w, h))
                                .clicked()
                            {
                                self.history.record(&self.state);
                                if apply_crop(&mut self.state, crop) {
                                    self.canvas.reset_view();
                                    self.canvas.invalidate_texture();
                                    self.active_tool = Tool::Select;
                                }
                            }
                            if toolbar_button(ui, "✕ Cancel", false).clicked() {
                                self.canvas.crop_rect = None;
                                self.active_tool = Tool::Select;
                            }
                        }
                    } else {
                        ui.label(
                            RichText::new("Drag on the image to select crop region (Enter = Apply, Esc = Cancel)")
                                .size(12.0)
                                .color(Color32::from_rgb(56, 189, 248)),
                        );
                    }
                }
                Tool::Rectangle | Tool::Ellipse => {
                    ui.label(RichText::new("Stroke:").size(12.0));
                    if ui
                        .add(egui::Slider::new(&mut self.stroke_width, 1.0..=24.0).suffix(" px"))
                        .changed()
                    {
                        self.apply_stroke_to_selection();
                    }
                    ui.checkbox(&mut self.fill_shapes, "Semi-Transparent Fill");
                }
                _ => {
                    ui.label(RichText::new("Stroke:").size(12.0));
                    if ui
                        .add(egui::Slider::new(&mut self.stroke_width, 1.0..=24.0).suffix(" px"))
                        .changed()
                    {
                        self.apply_stroke_to_selection();
                    }
                    if self.canvas.selected_id.is_some()
                        && toolbar_button(ui, "🗑 Delete Selected (Del)", false).clicked()
                    {
                        self.delete_selected_annotation();
                    }
                }
            }
        });
    }

    fn apply_stroke_to_selection(&mut self) {
        if let Some(sel_id) = self.canvas.selected_id {
            if let Some(ann) = self.state.annotations.iter_mut().find(|a| a.id == sel_id) {
                ann.stroke_width = self.stroke_width;
                if let AnnotationKind::Text { font_size, .. } = &mut ann.kind {
                    *font_size = self.font_size;
                }
            }
        }
    }

    fn draw_status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let (w, h) = self.state.dimensions();
            let file_label = self
                .current_path
                .as_ref()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .unwrap_or("Unsaved Canvas");

            ui.label(
                RichText::new(format!("{}  •  {} × {} px", file_label, w, h))
                    .size(12.0)
                    .color(Color32::from_rgb(203, 213, 225)),
            );

            if let Some(idx) = self.sibling_index {
                ui.separator();
                ui.label(
                    RichText::new(format!("Folder: {} / {}", idx + 1, self.sibling_files.len()))
                        .size(12.0)
                        .color(Color32::from_rgb(148, 163, 184)),
                );
            }

            ui.separator();
            ui.label(
                RichText::new(format!("Annotations: {}", self.state.annotations.len()))
                    .size(12.0)
                    .color(Color32::from_rgb(148, 163, 184)),
            );

            // Toast notification in center
            let mut clear_toast = false;
            let mut reveal_target = None;
            if let Some(toast) = &self.toast {
                if toast.created_at.elapsed() < Duration::from_secs(12) {
                    ui.separator();
                    let col = if toast.is_error {
                        Color32::from_rgb(248, 113, 113)
                    } else {
                        Color32::from_rgb(56, 189, 248)
                    };
                    ui.label(RichText::new(&toast.message).size(12.0).strong().color(col));
                    if let Some(saved_path) = toast.saved_path.clone() {
                        if ui.small_button("📂 Reveal in Folder").clicked() {
                            reveal_target = Some(saved_path);
                        }
                    }
                } else {
                    clear_toast = true;
                }
            }
            if clear_toast {
                self.toast = None;
            }
            if let Some(saved_path) = reveal_target {
                if let Err(e) = io::reveal_in_folder(&saved_path) {
                    self.set_toast(e, true, None);
                }
            }

            // Right-aligned zoom controls
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("100% (1)")
                    .on_hover_text("Zoom to 1:1 native pixels (Shortcut: 1)")
                    .clicked()
                {
                    self.canvas.set_zoom_100();
                }
                if ui
                    .small_button("Fit (F)")
                    .on_hover_text("Fit image to window (Shortcut: F)")
                    .clicked()
                {
                    self.canvas.needs_fit = true;
                }
                if ui.small_button("＋").clicked() {
                    self.canvas.zoom = (self.canvas.zoom * 1.25).clamp(0.05, 32.0);
                    self.canvas.needs_fit = false;
                }
                ui.label(
                    RichText::new(format!("{:.0}%", self.canvas.zoom * 100.0))
                        .size(12.0)
                        .strong()
                        .color(Color32::from_rgb(248, 250, 252)),
                );
                if ui.small_button("－").clicked() {
                    self.canvas.zoom = (self.canvas.zoom / 1.25).clamp(0.05, 32.0);
                    self.canvas.needs_fit = false;
                }
            });
        });
    }
}

impl eframe::App for ImgToolApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_keyboard_shortcuts(&ctx);

        egui::Panel::top("imgtool-top-bar")
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(15, 23, 42))
                    .inner_margin(Margin::symmetric(10, 6))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(30, 41, 59))),
            )
            .show(ui, |ui| {
                self.draw_top_bar(ui);
                ui.add_space(4.0);
                self.draw_property_bar(ui);
            });

        egui::Panel::bottom("imgtool-status-bar")
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(15, 23, 42))
                    .inner_margin(Margin::symmetric(10, 5))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(30, 41, 59))),
            )
            .show(ui, |ui| {
                self.draw_status_bar(ui);
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(Color32::from_rgb(11, 17, 30)))
            .show(ui, |ui| {
                let cfg = ToolConfig {
                    tool: self.active_tool,
                    color: self.active_color,
                    stroke_width: self.stroke_width,
                    font_size: self.font_size,
                    fill_shapes: self.fill_shapes,
                    text_bg_box: self.text_bg_box,
                    redact_style: self.redact_style,
                    redact_block_size: self.redact_block_size,
                    stamp_radius: self.stamp_radius,
                };

                let events = self.canvas.show(ui, &mut self.state, cfg);
                for ev in events {
                    match ev {
                        CanvasEvent::RecordUndo(snapshot) => {
                            self.history.record(&snapshot);
                            self.canvas.invalidate_texture();
                        }
                        CanvasEvent::CommitCrop(bbox) => {
                            self.history.record(&self.state);
                            if apply_crop(&mut self.state, bbox) {
                                self.canvas.reset_view();
                                self.canvas.invalidate_texture();
                            }
                            self.active_tool = Tool::Select;
                        }
                        CanvasEvent::SwitchTool(t) => {
                            self.active_tool = t;
                        }
                        CanvasEvent::StatusMessage(msg) => {
                            self.set_toast(msg, false, None);
                        }
                    }
                }
            });
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        drop_repeated_ime_preedit(&mut raw_input.events, &mut self.ime_preedit);
    }
}

fn drop_repeated_ime_preedit(
    events: &mut Vec<egui::Event>,
    last: &mut Option<(String, Option<Range<usize>>)>,
) {
    events.retain(|event| match event {
        egui::Event::Ime(egui::ImeEvent::Preedit {
            text,
            active_range_chars,
        }) => {
            let repeat = last
                .as_ref()
                .is_some_and(|prev| prev.0 == *text && prev.1 == *active_range_chars);
            if !repeat {
                *last = Some((text.clone(), active_range_chars.clone()));
            }
            !repeat
        }
        egui::Event::Ime(_) => {
            *last = None;
            true
        }
        _ => true,
    });
}

fn toolbar_button(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
    let fill = if selected {
        Color32::from_rgb(14, 165, 233)
    } else {
        Color32::from_rgb(30, 41, 59)
    };
    let text_col = if selected {
        Color32::from_rgb(2, 6, 23)
    } else {
        Color32::from_rgb(241, 245, 249)
    };
    let border = if selected {
        Color32::from_rgb(56, 189, 248)
    } else {
        Color32::from_rgb(51, 65, 85)
    };

    ui.add(
        egui::Button::new(RichText::new(label).size(12.5).strong().color(text_col))
            .fill(fill)
            .stroke(Stroke::new(1.0, border))
            .corner_radius(CornerRadius::same(6))
            .min_size(Vec2::new(0.0, 26.0)),
    )
}

fn primary_action_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(
            RichText::new(label)
                .size(12.5)
                .strong()
                .color(Color32::from_rgb(2, 6, 23)),
        )
        .fill(Color32::from_rgb(56, 189, 248))
        .stroke(Stroke::new(1.0, Color32::from_rgb(125, 211, 252)))
        .corner_radius(CornerRadius::same(6))
        .min_size(Vec2::new(0.0, 26.0)),
    )
}

fn apply_dark_theme(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Dark);
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = Color32::from_rgb(15, 23, 42);
    visuals.window_fill = Color32::from_rgb(24, 34, 52);
    visuals.extreme_bg_color = Color32::from_rgb(11, 17, 30);
    visuals.selection.bg_fill = Color32::from_rgb(56, 189, 248);
    visuals.selection.stroke = Stroke::new(1.0, Color32::from_rgb(2, 6, 23));
    ctx.set_visuals_of(egui::Theme::Dark, visuals);
}
