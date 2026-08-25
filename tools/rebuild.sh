#!/usr/bin/env bash
# Rebuild the real binary and leave it ready to launch.
#
# The kill is not optional: `tauri build` renames `jarvis.exe` to
# `jarvis-desktop.exe` as its very last step (D9), and Windows refuses the
# rename while the previous build is running. The failure arrives two minutes
# in, after a full successful compile, as `os error 5`.
set -e
root="c:/Users/Alan Araujo/Projetos/j.a.r.v.i.s"
powershell -NoProfile -Command "Get-Process jarvis-desktop -ErrorAction SilentlyContinue | Stop-Process -Force" || true
cd "$root/apps/desktop"
pnpm tauri build --no-bundle
