//! Default Image Viewer registration and `mise`-compatible `stable_path` resolution.
//!
//! Critical Upgrade-Safety Rule:
//! When installed via `mise use -g github:manhavn/imgtool`, the actual binary resides in
//! a versioned directory (`~/.local/share/mise/installs/github-manhavn-imgtool/<version>/imgtool`).
//! Whenever the user upgrades and prunes old versions, the `<version>` directory is removed.
//! Therefore, desktop entries, Windows registry associations, and macOS `.app` launchers
//! MUST resolve to the `mise` shim (`~/.local/share/mise/shims/imgtool` or
//! `%LOCALAPPDATA%\mise\shims\imgtool.exe`) so upgrades never break file associations.

use std::path::{Path, PathBuf};

/// Common image MIME types registered on Linux (`xdg-mime` + `mimeapps.list`).
pub const IMAGE_MIME_TYPES: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/webp",
    "image/gif",
    "image/bmp",
    "image/tiff",
    "image/x-icon",
    "image/vnd.microsoft.icon",
];

/// Common image file extensions registered on Windows and macOS.
pub const IMAGE_EXTENSIONS: &[&str] = &[
    ".png", ".jpg", ".jpeg", ".webp", ".bmp", ".gif", ".ico", ".tiff", ".tif",
];

pub const DESKTOP_FILE_NAME: &str = "imgtool.desktop";

/// Resolves the upgrade-safe executable path (`stable_path`) for desktop/file-opener registration.
///
/// Priority:
/// 1. If `current_exe` is inside a `mise` install tree (or a `mise` shim for `imgtool` exists),
///    returns `~/.local/share/mise/shims/imgtool` (Linux/macOS) or
///    `%LOCALAPPDATA%\mise\shims\imgtool.exe` (Windows).
/// 2. Fallback: `~/.local/share/mise/installs/github-manhavn-imgtool/latest/imgtool` if the shim
///    does not exist yet.
/// 3. Otherwise: canonicalized `current_exe`.
pub fn stable_path() -> std::io::Result<PathBuf> {
    let current = std::env::current_exe()?;
    for data_dir in candidate_mise_data_dirs() {
        if let Some(resolved) = resolve_mise_stable_path(&current, &data_dir) {
            return Ok(resolved);
        }
    }
    Ok(std::fs::canonicalize(&current).unwrap_or(current))
}

/// Pure path resolver against a specific `mise` data directory (used by `stable_path` and unit tests).
pub fn resolve_mise_stable_path(exe: &Path, data_dir: &Path) -> Option<PathBuf> {
    let bin_name = exe.file_name()?;
    let canon_exe = std::fs::canonicalize(exe).unwrap_or_else(|_| exe.to_path_buf());
    let canon_data = std::fs::canonicalize(data_dir).unwrap_or_else(|_| data_dir.to_path_buf());

    let installs_dir = canon_data.join("installs");
    let is_inside_mise_installs = canon_exe
        .strip_prefix(&installs_dir)
        .map(|rel| rel.components().count() >= 3)
        .unwrap_or(false)
        || exe
            .to_string_lossy()
            .contains("installs/github-manhavn-imgtool")
        || exe
            .to_string_lossy()
            .contains(r"installs\github-manhavn-imgtool");

    let shim = data_dir.join("shims").join(bin_name);
    if is_inside_mise_installs {
        if shim.is_file() {
            return Some(shim);
        }
        // Fallback to `latest` symlink if `shims` binary is absent
        let latest_direct = data_dir
            .join("installs")
            .join("github-manhavn-imgtool")
            .join("latest")
            .join(bin_name);
        if latest_direct.is_file() {
            return Some(latest_direct);
        }
        let latest_bin = data_dir
            .join("installs")
            .join("github-manhavn-imgtool")
            .join("latest")
            .join("bin")
            .join(bin_name);
        if latest_bin.is_file() {
            return Some(latest_bin);
        }
        // Even if the shim file hasn't been materialized on disk yet, return the shim path
        // when the parent `shims` directory exists so versioned paths are never written.
        if data_dir.join("shims").is_dir() {
            return Some(shim);
        }
    }

    // Also check if the running binary is already the shim itself
    if canon_exe == std::fs::canonicalize(&shim).unwrap_or_else(|_| shim.clone()) && shim.is_file()
    {
        return Some(shim);
    }

    None
}

fn candidate_mise_data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(env_dir) = std::env::var_os("MISE_DATA_DIR").filter(|v| !v.is_empty()) {
        dirs.push(PathBuf::from(env_dir));
    }
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        dirs.push(PathBuf::from(xdg).join("mise"));
    }
    if let Some(home) = std::env::var_os("HOME").filter(|v| !v.is_empty()) {
        let home_path = PathBuf::from(&home);
        // Primary Linux & macOS default (`~/.local/share/mise`)
        dirs.push(home_path.join(".local").join("share").join("mise"));
        #[cfg(target_os = "macos")]
        dirs.push(
            home_path
                .join("Library")
                .join("Application Support")
                .join("mise"),
        );
    }
    #[cfg(windows)]
    {
        if let Some(local_appdata) = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()) {
            dirs.push(PathBuf::from(local_appdata).join("mise"));
        }
        if let Some(userprofile) = std::env::var_os("USERPROFILE").filter(|v| !v.is_empty()) {
            dirs.push(
                PathBuf::from(userprofile)
                    .join("AppData")
                    .join("Local")
                    .join("mise"),
            );
        }
    }
    dirs
}

/// Checks whether `imgtool` is currently registered as a desktop image viewer.
pub fn is_desktop_installed() -> bool {
    #[cfg(target_os = "linux")]
    {
        linux::is_installed()
    }
    #[cfg(windows)]
    {
        windows::is_installed()
    }
    #[cfg(target_os = "macos")]
    {
        macos::is_installed()
    }
    #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
    {
        false
    }
}

/// Installs `imgtool` as the default system image viewer/opener using `stable_path()`.
pub fn install_desktop() -> Result<String, String> {
    let exe = stable_path().map_err(|e| format!("Failed to resolve executable path: {}", e))?;
    #[cfg(target_os = "linux")]
    {
        linux::install(&exe)
    }
    #[cfg(windows)]
    {
        windows::install(&exe)
    }
    #[cfg(target_os = "macos")]
    {
        macos::install(&exe)
    }
    #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
    {
        let _ = exe;
        Err("Desktop integration is not supported on this platform".to_string())
    }
}

/// Uninstalls `imgtool` desktop registration.
pub fn uninstall_desktop() -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        linux::uninstall()
    }
    #[cfg(windows)]
    {
        windows::uninstall()
    }
    #[cfg(target_os = "macos")]
    {
        macos::uninstall()
    }
    #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
    {
        Err("Desktop integration is not supported on this platform".to_string())
    }
}

/// Self-healing startup check (`refresh_exec_path`):
/// Inspects existing desktop registrations on every startup and rewrites any versioned
/// `/installs/.../<version>/` or stale executable path to the current `stable_path()` shim.
pub fn refresh_exec_path() {
    let Ok(exe) = stable_path() else {
        return;
    };
    #[cfg(target_os = "linux")]
    {
        let _ = linux::refresh(&exe);
    }
    #[cfg(windows)]
    {
        let _ = windows::refresh(&exe);
    }
    #[cfg(target_os = "macos")]
    {
        let _ = macos::refresh(&exe);
    }
}

/// Quotes an executable path for `.desktop` `Exec=` or Windows registry commands if it contains spaces.
pub fn quote_exec_path(path: &Path) -> String {
    let s = path.display().to_string();
    if s.contains(' ') {
        format!("\"{}\"", s)
    } else {
        s
    }
}

/// Generates the contents of `~/.local/share/applications/imgtool.desktop`.
pub fn generate_linux_desktop_entry(exe_path: &Path) -> String {
    let quoted = quote_exec_path(exe_path);
    let raw = exe_path.display();
    let mimes = IMAGE_MIME_TYPES.join(";") + ";";
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Version=1.0\n\
         Name=ImgTool\n\
         GenericName=Image Viewer & Annotation Tool\n\
         Comment=Fast image viewer and quick annotation tool inspired by ksnip\n\
         Exec={quoted} %F\n\
         TryExec={raw}\n\
         Icon=imgtool\n\
         Terminal=false\n\
         Categories=Graphics;2DGraphics;RasterGraphics;Viewer;Utility;\n\
         MimeType={mimes}\n\
         StartupNotify=true\n\
         StartupWMClass=imgtool\n"
    )
}

/// Rewrites `Exec=` and `TryExec=` lines in an existing `.desktop` file if they point to
/// a different or versioned executable path. Returns `Some(updated_content)` when changed.
pub fn rewrite_linux_desktop_entry(existing: &str, new_exe: &Path) -> Option<String> {
    let expected_exec = format!("Exec={} %F", quote_exec_path(new_exe));
    let expected_try = format!("TryExec={}", new_exe.display());

    let mut changed = false;
    let mut out = String::with_capacity(existing.len() + 64);

    for line in existing.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Exec=") {
            if trimmed != expected_exec {
                changed = true;
            }
            out.push_str(&expected_exec);
            out.push('\n');
        } else if trimmed.starts_with("TryExec=") {
            if trimmed != expected_try {
                changed = true;
            }
            out.push_str(&expected_try);
            out.push('\n');
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }

    changed.then_some(out)
}

/// Updates `mimeapps.list` content so `imgtool.desktop` is set under `[Default Applications]`
/// and `[Added Associations]` for all supported image MIME types.
pub fn update_mimeapps_content(existing: &str) -> String {
    let mut lines: Vec<String> = existing.lines().map(|s| s.to_string()).collect();

    ensure_section_associations(&mut lines, "[Default Applications]");
    ensure_section_associations(&mut lines, "[Added Associations]");

    let mut result = lines.join("\n");
    if !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

fn ensure_section_associations(lines: &mut Vec<String>, section_header: &str) {
    let section_idx = lines
        .iter()
        .position(|l| l.trim().eq_ignore_ascii_case(section_header));

    let start_idx = match section_idx {
        Some(idx) => idx + 1,
        None => {
            if !lines.is_empty() && !lines.last().unwrap().trim().is_empty() {
                lines.push(String::new());
            }
            lines.push(section_header.to_string());
            lines.len()
        }
    };

    let end_idx = lines[start_idx..]
        .iter()
        .position(|l| l.trim().starts_with('['))
        .map(|rel| start_idx + rel)
        .unwrap_or(lines.len());

    for &mime in IMAGE_MIME_TYPES {
        let prefix = format!("{}=", mime);
        let entry = format!("{}={}", mime, DESKTOP_FILE_NAME);
        let mut found = false;
        for line in &mut lines[start_idx..end_idx] {
            if line.trim().starts_with(&prefix) {
                *line = entry.clone();
                found = true;
                break;
            }
        }
        if !found {
            lines.insert(end_idx, entry);
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use crate::icon::{generate_icon_png, ICON_SVG};
    use std::process::Command;

    fn data_dir() -> Option<PathBuf> {
        std::env::var_os("XDG_DATA_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .filter(|v| !v.is_empty())
                    .map(|h| PathBuf::from(h).join(".local").join("share"))
            })
    }

    fn config_dir() -> Option<PathBuf> {
        std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .filter(|v| !v.is_empty())
                    .map(|h| PathBuf::from(h).join(".config"))
            })
    }

    fn desktop_file_path() -> Option<PathBuf> {
        data_dir().map(|d| d.join("applications").join(DESKTOP_FILE_NAME))
    }

    pub fn is_installed() -> bool {
        desktop_file_path().is_some_and(|p| p.is_file())
    }

    pub fn install(exe_path: &Path) -> Result<String, String> {
        let data = data_dir().ok_or("Could not resolve $XDG_DATA_HOME or $HOME")?;
        let cfg = config_dir().ok_or("Could not resolve $XDG_CONFIG_HOME or $HOME")?;

        // 1. Install scalable SVG and 128x128 PNG icons
        let svg_dir = data.join("icons/hicolor/scalable/apps");
        std::fs::create_dir_all(&svg_dir)
            .map_err(|e| format!("Failed to create '{}': {}", svg_dir.display(), e))?;
        std::fs::write(svg_dir.join("imgtool.svg"), ICON_SVG.as_bytes())
            .map_err(|e| format!("Failed to write imgtool.svg: {}", e))?;

        let png_dir = data.join("icons/hicolor/128x128/apps");
        std::fs::create_dir_all(&png_dir)
            .map_err(|e| format!("Failed to create '{}': {}", png_dir.display(), e))?;
        std::fs::write(png_dir.join("imgtool.png"), generate_icon_png())
            .map_err(|e| format!("Failed to write imgtool.png: {}", e))?;

        // 2. Write ~/.local/share/applications/imgtool.desktop
        let apps_dir = data.join("applications");
        std::fs::create_dir_all(&apps_dir)
            .map_err(|e| format!("Failed to create '{}': {}", apps_dir.display(), e))?;
        let desktop_path = apps_dir.join(DESKTOP_FILE_NAME);
        let desktop_content = generate_linux_desktop_entry(exe_path);
        std::fs::write(&desktop_path, desktop_content)
            .map_err(|e| format!("Failed to write '{}': {}", desktop_path.display(), e))?;

        // 3. Update ~/.config/mimeapps.list
        std::fs::create_dir_all(&cfg).ok();
        let mimeapps_path = cfg.join("mimeapps.list");
        let existing_mimeapps = std::fs::read_to_string(&mimeapps_path).unwrap_or_default();
        let updated_mimeapps = update_mimeapps_content(&existing_mimeapps);
        std::fs::write(&mimeapps_path, updated_mimeapps)
            .map_err(|e| format!("Failed to update '{}': {}", mimeapps_path.display(), e))?;

        // 4. Invoke xdg-mime and update-desktop-database if present on host
        for &mime in IMAGE_MIME_TYPES {
            let _ = Command::new("xdg-mime")
                .args(["default", DESKTOP_FILE_NAME, mime])
                .status();
        }
        let _ = Command::new("update-desktop-database")
            .arg(&apps_dir)
            .status();

        Ok(format!(
            "Registered as Default Image Viewer ({} -> {})",
            desktop_path.display(),
            exe_path.display()
        ))
    }

    pub fn uninstall() -> Result<String, String> {
        let Some(desktop_path) = desktop_file_path() else {
            return Err("Could not resolve $XDG_DATA_HOME or $HOME".to_string());
        };
        if desktop_path.exists() {
            std::fs::remove_file(&desktop_path)
                .map_err(|e| format!("Failed to remove '{}': {}", desktop_path.display(), e))?;
        }
        if let Some(cfg) = config_dir() {
            let mimeapps_path = cfg.join("mimeapps.list");
            if let Ok(content) = std::fs::read_to_string(&mimeapps_path) {
                let filtered: Vec<&str> = content
                    .lines()
                    .filter(|l| !l.contains(DESKTOP_FILE_NAME))
                    .collect();
                let _ = std::fs::write(&mimeapps_path, filtered.join("\n") + "\n");
            }
        }
        Ok(format!("Removed {}", desktop_path.display()))
    }

    pub fn refresh(exe_path: &Path) -> std::io::Result<()> {
        let Some(desktop_path) = desktop_file_path() else {
            return Ok(());
        };
        if !desktop_path.is_file() {
            return Ok(());
        }
        let existing = std::fs::read_to_string(&desktop_path)?;
        if let Some(updated) = rewrite_linux_desktop_entry(&existing, exe_path) {
            std::fs::write(&desktop_path, updated)?;
        }
        Ok(())
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use winreg::enums::*;
    use winreg::RegKey;

    const APP_KEY_PATH: &str = r"Software\Classes\Applications\imgtool.exe";
    const PROGID_KEY_PATH: &str = r"Software\Classes\ImgTool.Image";

    pub fn is_installed() -> bool {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        hkcu.open_subkey(format!(r"{}\shell\open\command", APP_KEY_PATH))
            .is_ok()
    }

    pub fn install(exe_path: &Path) -> Result<String, String> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let cmd_str = format!("\"{}\" \"%1\"", exe_path.display());

        // 1. Register under HKCU\Software\Classes\Applications\imgtool.exe\shell\open\command
        let (app_cmd_key, _) = hkcu
            .create_subkey(format!(r"{}\shell\open\command", APP_KEY_PATH))
            .map_err(|e| format!("Registry error: {}", e))?;
        app_cmd_key
            .set_value("", &cmd_str)
            .map_err(|e| format!("Registry error: {}", e))?;

        let (app_key, _) = hkcu
            .create_subkey(APP_KEY_PATH)
            .map_err(|e| format!("Registry error: {}", e))?;
        let _ = app_key.set_value("FriendlyAppName", &"ImgTool Image Viewer & Annotation Tool");

        // 2. Register ProgID HKCU\Software\Classes\ImgTool.Image\shell\open\command
        let (prog_cmd_key, _) = hkcu
            .create_subkey(format!(r"{}\shell\open\command", PROGID_KEY_PATH))
            .map_err(|e| format!("Registry error: {}", e))?;
        prog_cmd_key
            .set_value("", &cmd_str)
            .map_err(|e| format!("Registry error: {}", e))?;

        // 3. Add OpenWithProgids for each image extension
        for &ext in IMAGE_EXTENSIONS {
            if let Ok((ext_key, _)) =
                hkcu.create_subkey(format!(r"Software\Classes\{}\OpenWithProgids", ext))
            {
                let _ = ext_key.set_value("ImgTool.Image", &"");
            }
        }

        Ok(format!(
            "Registered ImgTool in Windows Explorer Open With ({})",
            exe_path.display()
        ))
    }

    pub fn uninstall() -> Result<String, String> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let _ = hkcu.delete_subkey_all(APP_KEY_PATH);
        let _ = hkcu.delete_subkey_all(PROGID_KEY_PATH);
        Ok("Removed ImgTool Windows registry file associations".to_string())
    }

    pub fn refresh(exe_path: &Path) -> std::io::Result<()> {
        if !is_installed() {
            return Ok(());
        }
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let expected = format!("\"{}\" \"%1\"", exe_path.display());
        if let Ok((cmd_key, _)) =
            hkcu.create_subkey(format!(r"{}\shell\open\command", APP_KEY_PATH))
        {
            let cur: String = cmd_key.get_value("").unwrap_or_default();
            if cur != expected {
                cmd_key.set_value("", &expected)?;
            }
        }
        if let Ok((prog_key, _)) =
            hkcu.create_subkey(format!(r"{}\shell\open\command", PROGID_KEY_PATH))
        {
            let cur: String = prog_key.get_value("").unwrap_or_default();
            if cur != expected {
                prog_key.set_value("", &expected)?;
            }
        }
        Ok(())
    }
}

/// Generates the `Contents/Info.plist` for `ImgTool.app` on macOS, including `CFBundleIconFile`
/// and `CFBundleTypeIconFile` pointing to `Contents/Resources/AppIcon.icns`.
pub fn generate_macos_info_plist() -> &'static str {
    concat!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>ImgTool</string>
    <key>CFBundleDisplayName</key>
    <string>ImgTool</string>
    <key>CFBundleIdentifier</key>
    <string>io.github.manhavn.imgtool</string>
    <key>CFBundleVersion</key>
    <string>"#,
        env!("CARGO_PKG_VERSION"),
        r#"</string>
    <key>CFBundleShortVersionString</key>
    <string>"#,
        env!("CARGO_PKG_VERSION"),
        r#"</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleExecutable</key>
    <string>ImgTool</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon.icns</string>
    <key>CFBundleIconName</key>
    <string>AppIcon</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeName</key>
            <string>Image Document</string>
            <key>CFBundleTypeRole</key>
            <string>Editor</string>
            <key>CFBundleTypeIconFile</key>
            <string>AppIcon.icns</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>public.image</string>
                <string>public.png</string>
                <string>public.jpeg</string>
                <string>org.webmproject.webp</string>
                <string>com.compuserve.gif</string>
                <string>com.microsoft.bmp</string>
                <string>public.tiff</string>
                <string>com.microsoft.ico</string>
            </array>
        </dict>
    </array>
</dict>
</plist>
"#
    )
}

/// Writes a complete macOS `.app` bundle structure (`Contents/Info.plist`,
/// `Contents/Resources/AppIcon.icns`, `Contents/Resources/AppIcon.png`, and `Contents/MacOS/ImgTool`).
pub fn write_macos_app_bundle(bundle: &Path, exe_path: &Path) -> Result<(), String> {
    if bundle.exists() {
        let _ = std::fs::remove_dir_all(bundle);
    }
    let macos_dir = bundle.join("Contents/MacOS");
    let resources_dir = bundle.join("Contents/Resources");
    std::fs::create_dir_all(&macos_dir)
        .map_err(|e| format!("Failed to create '{}': {}", macos_dir.display(), e))?;
    std::fs::create_dir_all(&resources_dir)
        .map_err(|e| format!("Failed to create '{}': {}", resources_dir.display(), e))?;
    let plist_path = bundle.join("Contents/Info.plist");
    std::fs::write(&plist_path, generate_macos_info_plist())
        .map_err(|e| format!("Failed to write '{}': {}", plist_path.display(), e))?;

    let icns_path = resources_dir.join("AppIcon.icns");
    std::fs::write(&icns_path, crate::icon::generate_icon_icns())
        .map_err(|e| format!("Failed to write '{}': {}", icns_path.display(), e))?;

    let png_path = resources_dir.join("AppIcon.png");
    std::fs::write(&png_path, crate::icon::generate_icon_png())
        .map_err(|e| format!("Failed to write '{}': {}", png_path.display(), e))?;

    let script = format!("#!/bin/sh\nexec \"{}\" \"$@\"\n", exe_path.display());
    let bin_path = macos_dir.join("ImgTool");
    std::fs::write(&bin_path, script)
        .map_err(|e| format!("Failed to write '{}': {}", bin_path.display(), e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&bin_path)
            .map_err(|e| e.to_string())?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&bin_path, perms).map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use std::process::Command;

    fn user_app_bundle_dir() -> Option<PathBuf> {
        std::env::var_os("HOME")
            .filter(|v| !v.is_empty())
            .map(|h| PathBuf::from(h).join("Applications").join("ImgTool.app"))
    }

    fn system_app_bundle_dir() -> PathBuf {
        PathBuf::from("/Applications/ImgTool.app")
    }

    fn candidate_bundles() -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Some(u) = user_app_bundle_dir() {
            out.push(u);
        }
        out.push(system_app_bundle_dir());
        out
    }

    pub fn is_installed() -> bool {
        candidate_bundles()
            .into_iter()
            .any(|d| d.join("Contents/MacOS/ImgTool").is_file())
    }

    fn launcher_script(exe_path: &Path) -> String {
        format!("#!/bin/sh\nexec \"{}\" \"$@\"\n", exe_path.display())
    }

    fn notify_launch_services(bundle: &Path) {
        crate::icon::apply_macos_native_icon(Some(bundle));
        let lsregister = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";
        if Path::new(lsregister).is_file() {
            let _ = Command::new(lsregister)
                .args(["-f", "-R", "-trusted"])
                .arg(bundle)
                .status();
        }
        let _ = Command::new("/usr/bin/touch").arg(bundle).status();
    }

    pub fn install(exe_path: &Path) -> Result<String, String> {
        let user_bundle = user_app_bundle_dir().ok_or("Could not resolve $HOME")?;
        write_macos_app_bundle(&user_bundle, exe_path)?;
        notify_launch_services(&user_bundle);

        let sys_bundle = system_app_bundle_dir();
        let installed_path = if Path::new("/Applications").is_dir()
            && write_macos_app_bundle(&sys_bundle, exe_path).is_ok()
        {
            notify_launch_services(&sys_bundle);
            sys_bundle
        } else {
            user_bundle
        };

        Ok(format!(
            "Created macOS app bundle with icon at {} -> {}",
            installed_path.display(),
            exe_path.display()
        ))
    }

    pub fn uninstall() -> Result<String, String> {
        let mut removed = Vec::new();
        for bundle in candidate_bundles() {
            if bundle.exists() {
                std::fs::remove_dir_all(&bundle)
                    .map_err(|e| format!("Failed to remove '{}': {}", bundle.display(), e))?;
                removed.push(bundle.display().to_string());
            }
        }
        if removed.is_empty() {
            Ok("No ImgTool.app bundle found to remove".to_string())
        } else {
            Ok(format!("Removed {}", removed.join(", ")))
        }
    }

    pub fn refresh(exe_path: &Path) -> std::io::Result<()> {
        let expected_script = launcher_script(exe_path);
        let expected_plist = generate_macos_info_plist();

        for bundle in candidate_bundles() {
            let bin_path = bundle.join("Contents/MacOS/ImgTool");
            if !bin_path.is_file() {
                continue;
            }
            let mut updated = false;
            if std::fs::read_to_string(&bin_path).ok().as_deref() != Some(&expected_script) {
                std::fs::write(&bin_path, &expected_script)?;
                updated = true;
            }

            let plist_path = bundle.join("Contents/Info.plist");
            if std::fs::read_to_string(&plist_path).ok().as_deref() != Some(expected_plist) {
                std::fs::write(&plist_path, expected_plist)?;
                updated = true;
            }

            let icns_path = bundle.join("Contents/Resources/AppIcon.icns");
            if !icns_path.is_file() {
                if let Some(parent) = icns_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&icns_path, crate::icon::generate_icon_icns())?;
                let _ = std::fs::write(
                    bundle.join("Contents/Resources/AppIcon.png"),
                    crate::icon::generate_icon_png(),
                );
                updated = true;
            }

            if updated {
                notify_launch_services(&bundle);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mise_versioned_install_resolves_to_shim_path() {
        let root = std::env::temp_dir().join(format!("imgtool_mise_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);

        let version_dir = root
            .join("installs")
            .join("github-manhavn-imgtool")
            .join("0.1.0");
        let shims_dir = root.join("shims");
        std::fs::create_dir_all(&version_dir).unwrap();
        std::fs::create_dir_all(&shims_dir).unwrap();

        let versioned_exe = version_dir.join("imgtool");
        let shim_exe = shims_dir.join("imgtool");
        std::fs::write(&versioned_exe, b"bin").unwrap();
        std::fs::write(&shim_exe, b"shim").unwrap();

        let resolved = resolve_mise_stable_path(&versioned_exe, &root);
        assert_eq!(resolved, Some(shim_exe.clone()));

        // Verify .desktop entry uses the shim path and never the versioned directory
        let desktop = generate_linux_desktop_entry(&shim_exe);
        assert!(desktop.contains(&format!("Exec={} %F", shim_exe.display())));
        assert!(desktop.contains(&format!("TryExec={}", shim_exe.display())));
        assert!(!desktop.contains("/0.1.0/"));

        // Verify self-healing rewrite replaces a stale versioned path with the shim
        let stale_desktop = generate_linux_desktop_entry(&versioned_exe);
        let healed = rewrite_linux_desktop_entry(&stale_desktop, &shim_exe)
            .expect("should rewrite stale versioned path");
        assert!(healed.contains(&format!("Exec={} %F", shim_exe.display())));
        assert!(!healed.contains("/0.1.0/"));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn mimeapps_list_updates_default_and_added_associations() {
        let initial = "[Default Applications]\ntext/plain=org.gnome.TextEditor.desktop\n";
        let updated = update_mimeapps_content(initial);
        assert!(updated.contains("[Default Applications]"));
        assert!(updated.contains("[Added Associations]"));
        assert!(updated.contains("image/png=imgtool.desktop"));
        assert!(updated.contains("image/jpeg=imgtool.desktop"));
        assert!(updated.contains("image/webp=imgtool.desktop"));
        assert!(updated.contains("text/plain=org.gnome.TextEditor.desktop"));
    }

    #[test]
    fn macos_app_bundle_contains_valid_icns_icon_and_plist_keys() {
        let temp = std::env::temp_dir().join(format!("imgtool_macos_bundle_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp);
        let bundle = temp.join("ImgTool.app");
        let shim = Path::new("/Users/test/.local/share/mise/shims/imgtool");

        write_macos_app_bundle(&bundle, shim).unwrap();

        let plist = std::fs::read_to_string(bundle.join("Contents/Info.plist")).unwrap();
        assert!(plist.contains("<key>CFBundleIconFile</key>"));
        assert!(plist.contains("<string>AppIcon.icns</string>"));
        assert!(plist.contains("<key>CFBundleTypeIconFile</key>"));

        let icns = std::fs::read(bundle.join("Contents/Resources/AppIcon.icns")).unwrap();
        assert!(icns.len() > 1024);
        assert_eq!(&icns[0..4], b"icns");
        let recorded_len = u32::from_be_bytes([icns[4], icns[5], icns[6], icns[7]]) as usize;
        assert_eq!(recorded_len, icns.len());
        // Verify PNG magic signature inside first chunk (offset 16)
        assert_eq!(&icns[16..24], b"\x89PNG\r\n\x1a\n");

        std::fs::remove_dir_all(&temp).ok();
    }
}
