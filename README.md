# `imgtool` — Fast Image Viewer & Quick Annotation Tool

`imgtool` is a fast, lightweight image viewer and quick annotation/redaction tool written in **Rust (`eframe` / `egui`)**, inspired by **KDE `ksnip`**.

It focuses 100% on opening existing images instantaneously (from your system file manager, CLI, drag-and-drop, or pasted from the Clipboard), drawing crisp vector annotations or redacting sensitive areas, and **saving as a new file** or **copying directly to the clipboard** at 1:1 native pixel resolution.

---

## Quick Install

Install `imgtool` globally on Linux (`gnu` & `musl`, `amd64` & `arm64`), Windows (`amd64` & `arm64`), or macOS (`x86_64` & `arm64`) using [`mise`](https://mise.jdx.dev):

```bash
mise use -g github:manhavn/imgtool
```

---

## Set as Default Image Viewer & `mise` Shims Rule

You can register `imgtool` as your default system image viewer either by clicking the **`★ Set Default Viewer`** button in the top toolbar or by running:

```bash
imgtool --install-desktop
```

To remove the file associations:

```bash
imgtool --uninstall-desktop
```

### Important Note on `mise` Shims (Upgrade-Safe Paths)

When `imgtool` is installed via `mise use -g github:manhavn/imgtool`, the binary is unpacked inside a versioned directory (`~/.local/share/mise/installs/github-manhavn-imgtool/<version>/imgtool`). When you upgrade to a newer version and prune old releases (`mise upgrade` / `mise prune`), old `<version>` directories are deleted.

To ensure desktop entries, file manager **"Open With"** associations, and custom keyboard shortcuts **never break after an upgrade**:

1. `imgtool --install-desktop` (and the in-app **Set Default Viewer** button) automatically resolves and registers the **`mise` shim path** via `stable_path()`, and self-heals stale paths on startup (`refresh_exec_path()`).
2. If you create any custom desktop launcher, custom file-manager action, or window-manager keybinding manually, **always point it to the `mise` shim path** instead of the versioned `installs/` folder:
   - **Linux & macOS**: `~/.local/share/mise/shims/imgtool`
   - **Windows**: `%LOCALAPPDATA%\mise\shims\imgtool.exe` (or `%USERPROFILE%\AppData\Local\mise\shims\imgtool.exe`)

You can verify the resolved shim path at any time with:

```bash
imgtool --print-stable-path
```

---

## Key Features

- **Instant Image Viewing & Folder Navigation**:
  - Supports `PNG`, `JPEG`, `WebP`, `GIF`, `BMP`, `ICO`, and `TIFF`.
  - Smooth cursor-centered Zoom (`Scroll` / `Ctrl+Scroll`), Pan (`Middle-Click drag` or `Space + Left-Click drag`), `Fit to Window` (`F`), and `100%` (`1`).
  - Cycle through sibling images in the same folder using `◀` / `▶` (`Left` / `Right` arrow keys).
- **Non-Destructive Vector Annotation Layer**:
  - Select, move, resize, recolor, change stroke width, or delete any drawn object before exporting.
  - Full `Undo` (`Ctrl+Z`) and `Redo` (`Ctrl+Y` / `Ctrl+Shift+Z`) across both vector annotations and canvas transforms (`Crop`, `Rotate`, `Flip`, `Expand Canvas`).
- **1:1 Native-Resolution Export**:
  - Annotations are rasterized directly onto the native pixel dimensions of the image — never blurred or downscaled by viewport zoom.
  - **Quick Save (`Ctrl+S`)**: Saves immediately as a new file `<original-stem>_edited_<YYYYMMDD_HHMMSS>.png` in the same folder without overwriting your original image, complete with a **Reveal in Folder** button.
  - **Save As (`Ctrl+Shift+S`)**: Export to `PNG`, `JPEG`, `WebP`, `BMP`, or `TIFF`.
  - **Copy to Clipboard (`Ctrl+C`)**: Copies the full-resolution annotated image directly to the system clipboard (with `wl-copy` persistence support on Linux Wayland).

---

## Keyboard Shortcuts & Tools

| Shortcut | Tool / Action | Description |
|---|---|---|
| `V` / `Esc` | **Select / Move** | Click any annotation to move, drag resize handles, recolor, or press `Del`/`Backspace` to delete |
| `P` | **Pen** | Smooth freehand drawing |
| `H` | **Highlighter** | Semi-transparent marker stroke for highlighting text or UI areas |
| `A` | **Arrow** | Directional arrow in `ksnip` style |
| `L` | **Line** | Straight line segment (hold `Shift` to snap to 45° angles) |
| `R` | **Rectangle** | Outlined or semi-transparent filled rectangle (hold `Shift` for square) |
| `E` | **Ellipse / Circle** | Outlined or semi-transparent filled ellipse (hold `Shift` for circle) |
| `T` | **Text** | Click on the canvas to type UTF-8 text with optional callout box (`Ctrl+Enter` to finish) |
| `N` | **Number Stamp** | Single-click to place auto-incrementing step badges (`1`, `2`, `3`...) |
| `B` / `X` | **Blur / Redact** | Drag over sensitive info to pixelate or black out |
| `C` | **Crop** | Drag a crop region; press `Enter` to apply or `Esc` to cancel |
| `Ctrl+S` | **Quick Save** | Save as a new file `<stem>_edited_<YYYYMMDD_HHMMSS>.png` in the same folder |
| `Ctrl+Shift+S` | **Save As...** | Open file dialog to choose folder and format (`PNG`, `JPEG`, `WebP`, `BMP`, `TIFF`) |
| `Ctrl+C` | **Copy Image** | Copy rendered image to Clipboard at 1:1 native resolution |
| `Ctrl+V` | **Paste Image** | Paste image from Clipboard onto the canvas |
| `Ctrl+O` | **Open Image** | Open image file dialog |
| `Ctrl+Z` / `Ctrl+Y` | **Undo / Redo** | Undo or redo annotations and canvas transforms |
| `◀` / `▶` | **Prev / Next Image** | Navigate through sibling images in the current directory |
| `F` / `1` | **Fit / 100%** | Fit image to window (`F`) or view at 1:1 100% pixels (`1`) |

---

## CLI Options

```text
USAGE:
    imgtool [OPTIONS] [IMAGE_FILE]

ARGUMENTS:
    [IMAGE_FILE]            Optional path to an image file to open immediately

OPTIONS:
    --install-desktop       Register imgtool as the Default Image Viewer on the current OS
    --uninstall-desktop     Remove imgtool desktop file associations
    --print-stable-path     Print the resolved upgrade-safe executable path (mise shim)
    -h, --help              Print help message and exit
    -V, --version           Print version information and exit
```

---

## Local Cross-Platform Release Build (`./build-release.sh`)

All release archives are cross-compiled locally on your PC in a single command (no GitHub Actions required):

```bash
./build-release.sh
```

To build a specific subset of targets:

```bash
TARGETS=x86_64-unknown-linux-gnu,x86_64-unknown-linux-musl ./build-release.sh
```

### Output Artifacts in `./release/`

- `imgtool-x86_64-unknown-linux-musl.tar.gz` (+ `.sha256`)
- `imgtool-aarch64-unknown-linux-musl.tar.gz` (+ `.sha256`)
- `imgtool-x86_64-unknown-linux-gnu.tar.gz` (+ `.sha256`)
- `imgtool-aarch64-unknown-linux-gnu.tar.gz` (+ `.sha256`)
- `imgtool-x86_64-pc-windows-gnu.zip` (+ `.sha256`)
- `imgtool-aarch64-pc-windows-gnullvm.zip` (+ `.sha256`)
- `imgtool-x86_64-apple-darwin.tar.gz` (+ `.sha256`)
- `imgtool-aarch64-apple-darwin.tar.gz` (+ `.sha256`)
- `SHA256SUMS.txt`
