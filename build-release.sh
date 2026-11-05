#!/usr/bin/env bash
set -euo pipefail

# ==============================================================================
# imgtool — One-Command Local Cross-Platform Release Builder -> ./release/
# ==============================================================================
# Compiles all 8 release variants locally on the host PC in a single run:
#   - x86_64-unknown-linux-musl    (Linux musl amd64, dynamic ld-musl for dlopen)
#   - aarch64-unknown-linux-musl   (Linux musl arm64, dynamic ld-musl for dlopen)
#   - x86_64-unknown-linux-gnu     (Linux gnu amd64, pinned to glibc 2.17 floor)
#   - aarch64-unknown-linux-gnu    (Linux gnu arm64, pinned to glibc 2.17 floor)
#   - x86_64-pc-windows-gnu        (Windows amd64)
#   - aarch64-pc-windows-gnullvm   (Windows arm64)
#   - x86_64-apple-darwin          (macOS Intel x86_64)
#   - aarch64-apple-darwin         (macOS Apple Silicon arm64)
#
# Missing tools and SDKs are automatically resolved in strict priority order:
#   1. Auto-install via `mise` (userspace, no sudo)
#   2. Auto-download prebuilt binaries / Rust targets / Alpine musl loader /
#      MacOSX.sdk into $HOME / .build-cache (userspace, no sudo)
#   3. System package manager fallback: if root (`EUID == 0`) or passwordless
#      `sudo -n true` is available, installs automatically; otherwise prints the
#      exact `sudo ...` command to run and exits immediately without hanging.
#
# Usage:
#   ./build-release.sh
#   TARGETS=x86_64-unknown-linux-gnu,x86_64-unknown-linux-musl ./build-release.sh
# ==============================================================================

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"

export PATH="${HOME}/.local/share/mise/shims:${HOME}/.local/bin:${HOME}/.cargo/bin:${PATH}"
if command -v mise >/dev/null 2>&1; then
  eval "$(mise activate bash --shims 2>/dev/null || true)"
fi

PROJECT_NAME="imgtool"
BIN_NAME="imgtool"
RELEASE_DIR="$ROOT/release"
BUILD_CACHE="$ROOT/.build-cache"
GLIBC_FLOOR="${GLIBC_FLOOR:-2.17}"

DEFAULT_TARGETS="x86_64-unknown-linux-musl,aarch64-unknown-linux-musl,x86_64-unknown-linux-gnu,aarch64-unknown-linux-gnu,x86_64-pc-windows-gnu,aarch64-pc-windows-gnullvm,x86_64-apple-darwin,aarch64-apple-darwin"
TARGETS_CSV="${TARGETS:-$DEFAULT_TARGETS}"

MACOS_SDK_VERSION="15.4"
MACOS_SDK_ARCHIVE="MacOSX${MACOS_SDK_VERSION}.sdk.tar.xz"
MACOS_SDK_URL="https://github.com/joseluisq/macosx-sdks/releases/download/${MACOS_SDK_VERSION}/${MACOS_SDK_ARCHIVE}"
MACOS_SDK_SHA256="a0b7b66912ac0da0e45b304a332bacdbe58ca172220820d425edb28213962f81"

have() { command -v "$1" >/dev/null 2>&1; }

# Priority 3 helper: Installs system packages without ever hanging on a sudo password prompt.
install_system_packages_non_interactive() {
  local packages="$1"
  local elevate=()

  if [[ "${EUID:-$(id -u)}" -eq 0 ]]; then
    elevate=()
  elif have sudo && sudo -n true >/dev/null 2>&1; then
    elevate=(sudo -n)
  else
    echo "" >&2
    echo "============================================================================" >&2
    echo "[ERROR] Missing required system package(s): $packages" >&2
    echo "        Automatic installation requires root privileges, and passwordless" >&2
    echo "        sudo is not enabled. Run the following command manually and retry:" >&2
    echo "" >&2
    if have apt-get; then
      echo "    sudo apt-get update && sudo apt-get install -y $packages" >&2
    elif have dnf; then
      echo "    sudo dnf install -y $packages" >&2
    elif have pacman; then
      echo "    sudo pacman -S --noconfirm $packages" >&2
    elif have zypper; then
      echo "    sudo zypper --non-interactive install $packages" >&2
    elif have apk; then
      echo "    sudo apk add $packages" >&2
    else
      echo "    # Install packages using your system package manager: $packages" >&2
    fi
    echo "============================================================================" >&2
    exit 1
  fi

  if have apt-get; then
    "${elevate[@]}" apt-get update -qq && "${elevate[@]}" apt-get install -y $packages
  elif have dnf; then
    "${elevate[@]}" dnf install -y $packages
  elif have pacman; then
    "${elevate[@]}" pacman -S --noconfirm $packages
  elif have zypper; then
    "${elevate[@]}" zypper --non-interactive install $packages
  elif have apk; then
    "${elevate[@]}" apk add $packages
  else
    echo "[ERROR] No supported system package manager found to install: $packages" >&2
    exit 1
  fi
}

compute_sha256_value() {
  local file="$1"
  if have sha256sum; then
    sha256sum "$file" | awk '{print $1}'
  elif have shasum; then
    shasum -a 256 "$file" | awk '{print $1}'
  else
    python3 -c 'import hashlib,sys; print(hashlib.sha256(open(sys.argv[1],"rb").read()).hexdigest())' "$file"
  fi
}

write_sha256_file() {
  local file_path="$1"
  local dir_name base_name
  dir_name="$(dirname "$file_path")"
  base_name="$(basename "$file_path")"
  if have sha256sum; then
    (cd "$dir_name" && sha256sum "$base_name" > "${base_name}.sha256")
  elif have shasum; then
    (cd "$dir_name" && shasum -a 256 "$base_name" > "${base_name}.sha256")
  else
    python3 - "$file_path" <<'PY'
import hashlib, os, sys
p = sys.argv[1]
h = hashlib.sha256(open(p, "rb").read()).hexdigest()
with open(p + ".sha256", "w") as f:
    f.write(f"{h}  {os.path.basename(p)}\n")
PY
  fi
}

echo "============================================================"
echo "     imgtool — Local Cross-Platform Release Builder         "
echo "============================================================"

# 0. Check basic host utilities (curl, tar, xz)
for base_tool in curl tar xz; do
  if ! have "$base_tool"; then
    pkg="$base_tool"
    [[ "$base_tool" == "xz" ]] && pkg="xz-utils"
    install_system_packages_non_interactive "$pkg"
  fi
done

# 1. Priority 1: Ensure `mise` is installed in userspace
if ! have mise; then
  echo "[SETUP] 'mise' not found. Installing via https://mise.run into ~/.local/bin/mise ..."
  curl -fsSL https://mise.run | sh
  export PATH="${HOME}/.local/bin:${HOME}/.local/share/mise/shims:${PATH}"
  eval "$(mise activate bash --shims 2>/dev/null || true)"
  have mise || { echo "[ERROR] Failed to install mise." >&2; exit 1; }
  echo "[SETUP] [+] Installed mise: $(mise --version)"
else
  echo "[SETUP] [+] Found mise: $(mise --version)"
fi

# 2. Priority 1: Ensure `python3` is available (via mise if missing)
if ! have python3; then
  echo "[SETUP] 'python3' not found. Installing python@latest via mise..."
  mise use -g python@latest
  export PATH="${HOME}/.local/share/mise/shims:${PATH}"
  have python3 || install_system_packages_non_interactive "python3"
fi
echo "[SETUP] [+] Found python3: $(python3 --version)"

# 3. Priority 1: Ensure `zig` is available (via mise zig@0.13.0, fallback direct download)
if ! have zig; then
  echo "[SETUP] 'zig' not found. Installing zig@0.13.0 via mise..."
  if mise use -g zig@0.13.0; then
    export PATH="${HOME}/.local/share/mise/shims:${PATH}"
  fi
  if ! have zig; then
    ARCH="$(uname -m)"
    [[ "$ARCH" == "arm64" ]] && ARCH="aarch64"
    HOST_OS_LOWER="$(uname -s | tr '[:upper:]' '[:lower:]')"
    [[ "$HOST_OS_LOWER" == "darwin" ]] && HOST_OS_LOWER="macos"
    echo "[SETUP] Downloading official zig 0.13.0 binary for ${HOST_OS_LOWER}-${ARCH}..."
    mkdir -p "$HOME/.local/bin" "$HOME/.local/opt"
    curl -fsSL "https://ziglang.org/download/0.13.0/zig-${HOST_OS_LOWER}-${ARCH}-0.13.0.tar.xz" \
      | tar -xJ -C "$HOME/.local/opt/"
    ln -sf "$HOME/.local/opt/zig-${HOST_OS_LOWER}-${ARCH}-0.13.0/zig" "$HOME/.local/bin/zig"
    export PATH="${HOME}/.local/bin:${PATH}"
  fi
  have zig || { echo "[ERROR] Failed to install zig." >&2; exit 1; }
fi
echo "[SETUP] [+] Found zig: $(zig version)"

# 4. Priority 1: Ensure Rust (`cargo`, `rustc`, `rustup`) is available
if ! have cargo || ! have rustc; then
  echo "[SETUP] 'cargo'/'rustc' not found. Installing rust@latest via mise..."
  mise use -g rust@latest
  export PATH="${HOME}/.local/share/mise/shims:${HOME}/.cargo/bin:${PATH}"
fi
if ! have rustup; then
  echo "[SETUP] 'rustup' not found. Installing rustup in userspace..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
  export PATH="${HOME}/.cargo/bin:${PATH}"
fi
echo "[SETUP] [+] Found rustc: $(rustc --version)"

# 5. Priority 1/2: Ensure host C compiler (`cc`) is available for build.rs scripts
if ! have cc; then
  echo "[SETUP] Host 'cc' not found. Installing clang via mise / creating zig cc wrapper..."
  if ! mise use -g clang@latest >/dev/null 2>&1; then
    mkdir -p "$HOME/.local/bin"
    printf '#!/bin/sh\nexec zig cc "$@"\n' > "$HOME/.local/bin/cc"
    chmod +x "$HOME/.local/bin/cc"
  fi
  export PATH="${HOME}/.local/share/mise/shims:${HOME}/.local/bin:${PATH}"
fi

# 6. Priority 1/2: Ensure `cargo-zigbuild` is available
if ! have cargo-zigbuild; then
  echo "[SETUP] 'cargo-zigbuild' not found. Installing via mise / official prebuilt binary..."
  if mise use -g "ubi:rust-cross/cargo-zigbuild" >/dev/null 2>&1 || mise use -g "cargo:cargo-zigbuild" >/dev/null 2>&1; then
    export PATH="${HOME}/.local/share/mise/shims:${HOME}/.cargo/bin:${PATH}"
  elif curl --proto '=https' --tlsv1.2 -LsSf https://github.com/rust-cross/cargo-zigbuild/releases/latest/download/cargo-zigbuild-installer.sh | sh; then
    export PATH="${HOME}/.cargo/bin:${PATH}"
  else
    cargo install cargo-zigbuild --locked
  fi
  have cargo-zigbuild || { echo "[ERROR] Failed to install cargo-zigbuild." >&2; exit 1; }
fi
echo "[SETUP] [+] Found cargo-zigbuild: $(cargo-zigbuild --version)"

# 7. Ensure macOS SDK (`SDKROOT`) when building `*-apple-darwin` targets on Linux
ensure_macos_sdk() {
  if [[ "$(uname -s)" == "Darwin" ]]; then
    if [[ -z "${SDKROOT:-}" ]] && have xcrun; then
      export SDKROOT="$(xcrun --sdk macosx --show-sdk-path)"
    fi
    return 0
  fi

  # Check existing SDKROOT or known local SDK paths
  for candidate in \
    "${SDKROOT:-}" \
    "$HOME/.local/macos-sdk/MacOSX${MACOS_SDK_VERSION}.sdk" \
    "$HOME/.local/macos-sdk/MacOSX.sdk" \
    "$HOME/.sdk/MacOSX.sdk"
  do
    if [[ -n "$candidate" && -d "$candidate/System/Library/Frameworks/AppKit.framework" ]]; then
      export SDKROOT="$candidate"
      echo "[SETUP] [+] Using MacOSX SDK at: $SDKROOT"
      return 0
    fi
  done

  local sdk_base="$HOME/.local/macos-sdk"
  local archive_path="$BUILD_CACHE/$MACOS_SDK_ARCHIVE"
  mkdir -p "$sdk_base" "$BUILD_CACHE"

  if [[ ! -f "$archive_path" ]] || [[ "$(compute_sha256_value "$archive_path")" != "$MACOS_SDK_SHA256" ]]; then
    echo "[SETUP] Downloading MacOSX${MACOS_SDK_VERSION}.sdk.tar.xz for Darwin cross-compilation..."
    curl --fail --location --retry 3 --show-error "$MACOS_SDK_URL" -o "$archive_path"
    local actual_sha
    actual_sha="$(compute_sha256_value "$archive_path")"
    if [[ "$actual_sha" != "$MACOS_SDK_SHA256" ]]; then
      rm -f "$archive_path"
      echo "[ERROR] SHA256 mismatch for $MACOS_SDK_ARCHIVE (expected $MACOS_SDK_SHA256, got $actual_sha)" >&2
      exit 1
    fi
  fi

  echo "[SETUP] Extracting MacOSX${MACOS_SDK_VERSION}.sdk into $sdk_base ..."
  rm -rf "$sdk_base/MacOSX${MACOS_SDK_VERSION}.sdk" "$sdk_base/MacOSX.sdk"
  tar -xJf "$archive_path" -C "$sdk_base"
  ln -sfn "$sdk_base/MacOSX${MACOS_SDK_VERSION}.sdk" "$sdk_base/MacOSX.sdk"
  export SDKROOT="$sdk_base/MacOSX.sdk"
  echo "[SETUP] [+] MacOSX SDK ready at: $SDKROOT"
}

IFS=',' read -r -a BUILD_TARGETS <<< "$TARGETS_CSV"

# Clean stale artifacts in ./release/ before building
mkdir -p "$RELEASE_DIR" "$BUILD_CACHE"
if [[ "$TARGETS_CSV" == "$DEFAULT_TARGETS" ]]; then
  rm -rf "${RELEASE_DIR:?}"/*
else
  for target in "${BUILD_TARGETS[@]}"; do
    target="$(echo "$target" | xargs)"
    [[ -n "$target" ]] || continue
    rm -f "$RELEASE_DIR/${PROJECT_NAME}-${target}.tar.gz" \
          "$RELEASE_DIR/${PROJECT_NAME}-${target}.tar.gz.sha256" \
          "$RELEASE_DIR/${PROJECT_NAME}-${target}.zip" \
          "$RELEASE_DIR/${PROJECT_NAME}-${target}.zip.sha256" \
          "$RELEASE_DIR/SHA256SUMS.txt"
  done
fi

# Pre-check if any apple-darwin or musl targets are requested
for target in "${BUILD_TARGETS[@]}"; do
  target="$(echo "$target" | xargs)"
  case "$target" in
    *apple-darwin*)
      ensure_macos_sdk
      break
      ;;
  esac
done

BASE_RUSTFLAGS="${RUSTFLAGS:-}"

echo ""
echo "Targets to build : ${BUILD_TARGETS[*]}"
echo "Output directory : $RELEASE_DIR/"
echo "------------------------------------------------------------"

built_archives=()

for target in "${BUILD_TARGETS[@]}"; do
  target="$(echo "$target" | xargs)"
  [[ -n "$target" ]] || continue

  echo ""
  echo "==> [BUILD] $PROJECT_NAME for target: $target"

  # Ensure rustup target is installed
  rustup target add "$target" >/dev/null 2>&1 || true

  case "$target" in
    x86_64-unknown-linux-musl)
      musl_lib_dir="$(python3 "$ROOT/scripts/musl-runtime.py" x86_64 "$BUILD_CACHE/musl")"
      rm -f "$ROOT/target/$target/release/${BIN_NAME}"
      ZIG_BIN="$(command -v zig)" \
      ZIG_MODE="cc" \
      ZIG_TARGET="x86_64-linux-musl" \
      ZIG_DYNAMIC_MUSL="1" \
      ZIG_MUSL_LIB_DIR="$musl_lib_dir" \
      ZIG_MUSL_LOADER="/lib/ld-musl-x86_64.so.1" \
      CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER="$ROOT/scripts/zig-linker.py" \
      RUSTFLAGS="$BASE_RUSTFLAGS -C target-feature=-crt-static -A linker_messages -A warnings" \
      cargo build --release --target "$target"
      ;;
    aarch64-unknown-linux-musl)
      musl_lib_dir="$(python3 "$ROOT/scripts/musl-runtime.py" aarch64 "$BUILD_CACHE/musl")"
      rm -f "$ROOT/target/$target/release/${BIN_NAME}"
      ZIG_BIN="$(command -v zig)" \
      ZIG_MODE="cc" \
      ZIG_TARGET="aarch64-linux-musl" \
      ZIG_DYNAMIC_MUSL="1" \
      ZIG_MUSL_LIB_DIR="$musl_lib_dir" \
      ZIG_MUSL_LOADER="/lib/ld-musl-aarch64.so.1" \
      CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER="$ROOT/scripts/zig-linker.py" \
      RUSTFLAGS="$BASE_RUSTFLAGS -C target-feature=-crt-static -A linker_messages -A warnings" \
      cargo build --release --target "$target"
      ;;
    *-unknown-linux-gnu)
      export RUSTFLAGS="$BASE_RUSTFLAGS -A linker_messages -A warnings"
      cargo zigbuild --release --target "${target}.${GLIBC_FLOOR}"
      ;;
    *windows*)
      export RUSTFLAGS="$BASE_RUSTFLAGS -A linker_messages -A warnings"
      cargo zigbuild --release --target "$target"
      ;;
    *apple-darwin*)
      export RUSTFLAGS="$BASE_RUSTFLAGS -A linker_messages -A warnings"
      cargo zigbuild --release --target "$target"
      ;;
    *)
      export RUSTFLAGS="$BASE_RUSTFLAGS -A linker_messages -A warnings"
      cargo zigbuild --release --target "$target"
      ;;
  esac

  ext=""
  [[ "$target" == *windows* ]] && ext=".exe"
  src_bin="$ROOT/target/$target/release/${BIN_NAME}${ext}"
  [[ -f "$src_bin" ]] || { echo "[ERROR] Built binary not found at: $src_bin" >&2; exit 1; }

  stage_dir="$(mktemp -d "${TMPDIR:-/tmp}/imgtool-stage.XXXXXX")"
  cp -f "$src_bin" "$stage_dir/${BIN_NAME}${ext}"
  chmod +x "$stage_dir/${BIN_NAME}${ext}"

  if [[ "$target" == *windows* ]]; then
    archive="$RELEASE_DIR/${PROJECT_NAME}-${target}.zip"
    rm -f "${archive}.tmp"
    python3 - "${archive}.tmp" "$stage_dir/${BIN_NAME}${ext}" <<'PY'
import os, sys, zipfile
with zipfile.ZipFile(sys.argv[1], "w", zipfile.ZIP_DEFLATED) as zf:
    for path in sys.argv[2:]:
        zf.write(path, os.path.basename(path))
PY
    mv -f "${archive}.tmp" "$archive"
  else
    archive="$RELEASE_DIR/${PROJECT_NAME}-${target}.tar.gz"
    rm -f "${archive}.tmp"
    tar -C "$stage_dir" -czf "${archive}.tmp" "${BIN_NAME}${ext}"
    mv -f "${archive}.tmp" "$archive"
  fi

  rm -rf "$stage_dir"
  write_sha256_file "$archive"
  built_archives+=("$(basename "$archive")")
  echo "    [+] Packaged: $archive"
done

# Generate unified SHA256SUMS.txt in ./release/
(
  cd "$RELEASE_DIR"
  mapfile -t archives < <(find . -maxdepth 1 -type f \( -name '*.tar.gz' -o -name '*.zip' \) -exec basename {} \; | sort)
  if [[ "${#archives[@]}" -gt 0 ]]; then
    if have sha256sum; then
      sha256sum -- "${archives[@]}" > SHA256SUMS.txt
    elif have shasum; then
      shasum -a 256 -- "${archives[@]}" > SHA256SUMS.txt
    fi
  fi
)

VERSION="$(sed -nE 's/^version *= *"([^"]+)".*/\1/p' "$ROOT/Cargo.toml" | head -n1)"
TAG="v${VERSION:-0.1.0}"

echo ""
echo "============================================================"
echo "     ALL RELEASE ARTIFACTS BUILT SUCCESSFULLY IN ./release/ "
echo "============================================================"
ls -lh "$RELEASE_DIR/"
echo "------------------------------------------------------------"
echo "To publish ${TAG} to GitHub Releases (manhavn/imgtool):"
echo ""
echo "  gh release create ${TAG} ./release/*.tar.gz ./release/*.zip ./release/*.sha256 ./release/SHA256SUMS.txt \\"
echo "    --repo manhavn/imgtool --title \"imgtool ${TAG}\" --generate-notes"
echo ""
echo "Or upload to an existing release:"
echo "  gh release upload ${TAG} ./release/*.tar.gz ./release/*.zip ./release/*.sha256 ./release/SHA256SUMS.txt \\"
echo "    --repo manhavn/imgtool --clobber"
echo ""
echo "Install globally via mise:"
echo "  mise use -g github:manhavn/imgtool"
echo "============================================================"
