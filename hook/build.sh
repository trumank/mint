#!/bin/bash
CXXSTDLIB=msvcprt RUSTY_V8_ARCHIVE=https://github.com/denoland/rusty_v8/releases/download/v0.91.0/rusty_v8_release_x86_64-pc-windows-msvc.lib.gz cargo xwin build --release --target x86_64-pc-windows-msvc
