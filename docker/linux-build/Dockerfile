# Ambiente de build Linux do Nexo. Ubuntu 22.04 (glibc 2.35): o binário roda nas distros
# atuais e nas de 2022 em diante.
FROM ubuntu:22.04

ENV DEBIAN_FRONTEND=noninteractive \
    # linuxdeploy (usado no AppImage) é ele mesmo um AppImage: sem FUSE no container.
    APPIMAGE_EXTRACT_AND_RUN=1 \
    PATH=/root/.cargo/bin:$PATH

RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential curl wget file ca-certificates git pkg-config xdg-utils \
        libwebkit2gtk-4.1-dev libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev \
        libdbus-1-dev patchelf rpm \
    && curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*

RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable

WORKDIR /src
