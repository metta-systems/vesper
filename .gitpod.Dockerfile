FROM gitpod/workspace-full

USER gitpod

RUN sudo apt-get update \
    && sudo apt-get install -yq \
        pkg-config \
        libpython3.6 \
        rust-lldb \
        qemu-system-aarch64 \
        gdb-multiarch \
    && sudo apt-get clean \
    && sudo rm -rf /tmp/*

RUN .cargo/bin/rustup toolchain install nightly \
    && .cargo/bin/rustup default nightly \
    && .cargo/bin/rustup toolchain uninstall 1.48.0 \
    && .cargo/bin/rustup component add clippy llvm-tools-preview rls rust-analysis rust-src rustfmt \
    && .cargo/bin/rustup target add aarch64-unknown-none-softfloat

RUN bash -lc "cargo install cargo-asm cargo-binutils cargo-bloat cargo-cache cargo-expand cargo-fmt cargo-geiger cargo-graph cargo-make just"

RUN bash -lc "cargo cache -i && cargo cache -e && cargo cache -i"

ENV RUST_LLDB=/usr/bin/lldb-9
ENV GDB=/usr/bin/gdb-multiarch
ENV QEMU=/usr/bin/qemu-system-aarch64
