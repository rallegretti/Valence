# Build environment for the Linux release: Ubuntu 22.04 (glibc 2.35), so the
# binary runs on Ubuntu 22.04+, Debian 12+, Fedora 36+ and similar. Built and
# used by packaging/linux/package.sh; nothing here touches the network at
# build time except apt and rustup.
FROM ubuntu:22.04
ARG RUST_VERSION=1.99.0
ENV DEBIAN_FRONTEND=noninteractive \
    RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/opt/cargo \
    PATH=/opt/cargo/bin:$PATH
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential pkg-config curl ca-certificates \
        libx11-dev libxkbcommon-dev libwayland-dev \
    && rm -rf /var/lib/apt/lists/*
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --no-modify-path \
        --profile minimal --default-toolchain "$RUST_VERSION" \
    && chmod -R a+rX /opt/rustup /opt/cargo
