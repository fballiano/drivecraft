# The environment of the Linux tray test (scripts/tray-test.sh).
FROM ubuntu:24.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl ca-certificates build-essential pkg-config \
    libxkbcommon-dev libxkbcommon-x11-0 libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
    sway grim dbus libglib2.0-bin \
    mesa-vulkan-drivers libvulkan1 libgl1-mesa-dri libegl1 \
    xvfb openbox wmctrl xdotool x11-utils imagemagick python3 python3-gi gir1.2-glib-2.0 \
 && rm -rf /var/lib/apt/lists/*
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
ENV PATH=/root/.cargo/bin:$PATH
