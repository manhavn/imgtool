#!/usr/bin/env python3
"""
Wrapper around `zig cc` / `zig c++` for cross-linking Rust binaries:
1. Filters out `-Wl,-O1` / `--O1` which Zig's bundled LLD marks as deprecated.
2. When `ZIG_DYNAMIC_MUSL=1` is set (for `*-unknown-linux-musl` GUI builds),
   removes `-static` / `-Wl,-Bstatic` and links dynamically against Alpine's
   `ld-musl-<arch>.so.1` so `dlopen()` works at runtime for Wayland/X11/EGL/GLX.
"""

import os
import sys


def main() -> int:
    zig = os.environ.get("ZIG_BIN", "zig")
    mode = os.environ.get("ZIG_MODE", "cc")
    target = os.environ.get("ZIG_TARGET", "")
    dynamic_musl = os.environ.get("ZIG_DYNAMIC_MUSL", "") == "1"
    musl_lib_dir = os.environ.get("ZIG_MUSL_LIB_DIR", "")
    musl_loader = os.environ.get("ZIG_MUSL_LOADER", "")
    filtered_args = []
    for arg in sys.argv[1:]:
        if arg in ("-Wl,-O1", "-Wl,--O1", "-Wl,--fix-cortex-a53-843419"):
            continue
        if dynamic_musl and arg == "-lgcc_s":
            filtered_args.append("-Wl,-Bstatic")
            filtered_args.append("-lunwind")
            filtered_args.append("-Wl,-Bdynamic")
            continue
        if dynamic_musl and arg in ("-static", "-static-pie", "-Wl,-Bstatic"):
            continue
        if dynamic_musl and (
            arg.endswith("rcrt1.o") or arg.endswith("crt1.o") or arg.endswith("Scrt1.o")
        ):
            # Let zig cc supply the dynamic PIE startup object for musl
            continue
        if arg.startswith("-Wl,"):
            parts = []
            for p in arg[4:].split(","):
                if p in ("-O1", "--O1", "--fix-cortex-a53-843419"):
                    continue
                if dynamic_musl and p in ("-Bstatic", "-static"):
                    continue
                parts.append(p)
            if parts:
                filtered_args.append("-Wl," + ",".join(parts))
            continue
        filtered_args.append(arg)
    cmd = [zig, mode]
    if target:
        cmd.extend(["-target", target])
    if dynamic_musl:
        cmd.append("-pie")
        if musl_lib_dir:
            cmd.append(f"-L{musl_lib_dir}")
        if musl_loader:
            cmd.append(f"-Wl,--dynamic-linker={musl_loader}")
    cmd.extend(filtered_args)
    if dynamic_musl and musl_lib_dir:
        cmd.append(f"{musl_lib_dir}/libc.so")

    os.execvp(zig, cmd)


if __name__ == "__main__":
    raise SystemExit(main())
