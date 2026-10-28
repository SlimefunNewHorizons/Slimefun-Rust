#!/usr/bin/env bash
# Benchmark reproducible Java vs Rust. Requiere JDK 21+ y `cargo build --release -p slimefun-ffi`.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release --offline -p slimefun-ffi >/dev/null
OUT=$(mktemp -d)
javac -d "$OUT" bench/java/io/github/thebusybiscuit/slimefun4/core/services/nativeengine/RustNativeEngine.java bench/java/Bench.java
java -Xms256m -Xmx256m -XX:+UseG1GC -cp "$OUT" Bench "$PWD/target/release/libslimefun_ffi.so"
