FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive
ENV PATH=/root/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential binutils nasm lld xorriso mtools gdisk \
    qemu-system-x86 python3 curl git ca-certificates pkg-config \
    && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal \
    && rustup toolchain install nightly --component rust-src --component rustfmt --component llvm-tools \
    && rustup target add --toolchain nightly x86_64-unknown-none

RUN mkdir -p /opt/octagram-cargo-registry /opt/octagram-cargo-git /usr/local/bin \
    && printf '%s\\n' '#!/bin/sh' 'mkdir -p /tmp/cargo' \
       'ln -sfn /opt/octagram-cargo-registry /tmp/cargo/registry' \
       'ln -sfn /opt/octagram-cargo-git /tmp/cargo/git' \
       'exec "$@"' > /usr/local/bin/octagram-entrypoint \
    && chmod 755 /usr/local/bin/octagram-entrypoint

WORKDIR /workspace
ENTRYPOINT ["/usr/local/bin/octagram-entrypoint"]
