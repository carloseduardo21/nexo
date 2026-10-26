#!/usr/bin/env bash
# Gera os pacotes Linux (.deb, .rpm, .AppImage) do commit atual, num container.
# Uso: docker/linux-build/build.sh [saída]   (padrão: release/linux)
set -euo pipefail
root=$(git rev-parse --show-toplevel)
out=$(realpath -m "${1:-$root/release/linux}")
mkdir -p "$out"

docker build -t nexo-linux-build "$root/docker/linux-build"
# Fonte via `git archive`: não leva node_modules/target do Windows para o container.
# Os volumes guardam o cache do cargo e o target entre builds.
git -C "$root" archive HEAD | docker run --rm -i \
    -v nexo-cargo-registry:/root/.cargo/registry \
    -v nexo-linux-target:/src/src-tauri/target \
    -v "$out":/out \
    nexo-linux-build bash -c '
        set -e
        tar x
        npm ci
        npx tauri build
        cd src-tauri/target/release/bundle
        cp deb/*.deb rpm/*.rpm appimage/*.AppImage /out/
        chown -R '"$(id -u):$(id -g)"' /out'
ls -lh "$out"
