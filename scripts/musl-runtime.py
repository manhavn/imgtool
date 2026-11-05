#!/usr/bin/env python3
"""
Downloads and SHA256-verifies Alpine Linux `musl-1.2.5-r4.apk` for `x86_64` and `aarch64`
into `.build-cache/musl/<arch>/` and extracts `lib/ld-musl-<arch>.so.1` + `libc.so`
so `cargo-zigbuild` can link dynamic `musl` GUI binaries (`-C target-feature=-crt-static`)
that support `dlopen()` for Wayland, X11, EGL, and OpenGL drivers.
"""

import hashlib
import os
import sys
import tarfile
import urllib.request
from pathlib import Path

ALPINE_MUSL_PACKAGES = {
    "x86_64": {
        "url": "https://dl-cdn.alpinelinux.org/alpine/v3.20/main/x86_64/musl-1.2.5-r4.apk",
        "sha256": "b255cdcf4f64eb301f4dbd40d1fbdc0fcb021613b3f0d124a41eeff62e2bc7d8",
        "loader": "ld-musl-x86_64.so.1",
    },
    "aarch64": {
        "url": "https://dl-cdn.alpinelinux.org/alpine/v3.20/main/aarch64/musl-1.2.5-r4.apk",
        "sha256": "71d53cd568d014bf71fa3f76819ddcd68d4f2ec5d9e9203e8ad4dd44667b9e9f",
        "loader": "ld-musl-aarch64.so.1",
    },
}


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        while chunk := f.read(65536):
            h.update(chunk)
    return h.hexdigest()


def ensure_arch(cache_root: Path, arch: str) -> Path:
    if arch not in ALPINE_MUSL_PACKAGES:
        raise ValueError(f"Unsupported musl architecture: {arch}")

    meta = ALPINE_MUSL_PACKAGES[arch]
    arch_dir = cache_root / arch
    lib_dir = arch_dir / "lib"
    loader_path = lib_dir / meta["loader"]
    libc_so = lib_dir / "libc.so"

    if loader_path.is_file() and libc_so.is_file():
        return lib_dir

    arch_dir.mkdir(parents=True, exist_ok=True)
    apk_path = arch_dir / f"musl-1.2.5-r4-{arch}.apk"

    if not apk_path.is_file() or sha256_file(apk_path) != meta["sha256"]:
        print(f"[MUSL] Downloading Alpine musl-1.2.5 loader for {arch}...", file=sys.stderr)
        req = urllib.request.Request(meta["url"], headers={"User-Agent": "imgtool-builder/1.0"})
        with urllib.request.urlopen(req, timeout=60) as resp, apk_path.open("wb") as out:
            out.write(resp.read())

        actual = sha256_file(apk_path)
        if actual != meta["sha256"]:
            apk_path.unlink(missing_ok=True)
            raise RuntimeError(
                f"SHA256 mismatch for {meta['url']}: expected {meta['sha256']}, got {actual}"
            )

    lib_dir.mkdir(parents=True, exist_ok=True)
    with tarfile.open(apk_path, mode="r:gz") as tf:
        member = tf.getmember(f"lib/{meta['loader']}")
        src = tf.extractfile(member)
        if src is None:
            raise RuntimeError(f"Could not extract lib/{meta['loader']} from {apk_path}")
        loader_path.write_bytes(src.read())
        loader_path.chmod(0o755)

    # Create libc.so and libc.musl-<arch>.so.1 copies/symlinks for the dynamic linker
    for alias in ("libc.so", f"libc.musl-{arch}.so.1"):
        alias_path = lib_dir / alias
        alias_path.unlink(missing_ok=True)
        alias_path.write_bytes(loader_path.read_bytes())
        alias_path.chmod(0o755)

    return lib_dir


def main() -> int:
    if len(sys.argv) < 2:
        print("Usage: musl-runtime.py <x86_64|aarch64|all> [cache_dir]", file=sys.stderr)
        return 1

    target_arg = sys.argv[1]
    cache_dir = Path(sys.argv[2]) if len(sys.argv) >= 3 else Path(".build-cache/musl")

    if target_arg == "all":
        for arch in ("x86_64", "aarch64"):
            path = ensure_arch(cache_dir, arch)
            print(f"{arch}:{path.resolve()}")
    else:
        path = ensure_arch(cache_dir, target_arg)
        print(str(path.resolve()))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
