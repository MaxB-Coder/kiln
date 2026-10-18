# Linux build and test image for Kiln: GPUI's system libraries plus Mesa's software Vulkan
# (lavapipe), so the headless screenshots render without a GPU or a display.
FROM rust:1.98.1-bookworm
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      clang pkg-config zsh git \
      libfontconfig-dev libfreetype-dev libwayland-dev libx11-xcb-dev libxcb1-dev \
      libxkbcommon-dev libxkbcommon-x11-dev libvulkan-dev libvulkan1 mesa-vulkan-drivers \
 && rm -rf /var/lib/apt/lists/*
RUN rustup component add rustfmt clippy
WORKDIR /work
