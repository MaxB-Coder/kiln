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
# PowerShell 7 for the shell-integration tests (official release, by architecture).
ARG PWSH_VERSION=7.6.7
RUN arch=$(dpkg --print-architecture | sed 's/amd64/x64/') \
 && curl -fsSL -o /tmp/pwsh.tgz "https://github.com/PowerShell/PowerShell/releases/download/v${PWSH_VERSION}/powershell-${PWSH_VERSION}-linux-${arch}.tar.gz" \
 && mkdir -p /opt/pwsh && tar -xzf /tmp/pwsh.tgz -C /opt/pwsh && chmod +x /opt/pwsh/pwsh \
 && ln -s /opt/pwsh/pwsh /usr/local/bin/pwsh && rm /tmp/pwsh.tgz
WORKDIR /work
