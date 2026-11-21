# Cross-compiles the Windows build: docker build -t snowballz-win tools/windows
FROM rust:1-bookworm
RUN apt-get update && apt-get install -y --no-install-recommends gcc-mingw-w64-x86-64 zip && rm -rf /var/lib/apt/lists/*
RUN rustup target add x86_64-pc-windows-gnu
ENV CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc
