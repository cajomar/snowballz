#!/bin/sh
# Build downloadable packages into dist/:
#   dist/snowballz-windows.zip     (cross-compiled in Docker, see tools/windows)
#   dist/snowballz-linux.tar.gz
set -e
cd "$(dirname "$0")/.."
mkdir -p dist
rm -rf dist/stage

stage() { # stage <dir> <client> <server>
    mkdir -p "$1"
    cp "$2" "$3" "$1/"
    cp -r assets "$1/assets"
    cp tools/dist/PLAY.txt LICENSE "$1/"
}

echo "== Linux"
cargo build --release -p client -p server
stage dist/stage/snowballz target/release/snowballz target/release/snowballz-server
tar -C dist/stage -czf dist/snowballz-linux.tar.gz snowballz
rm -rf dist/stage

echo "== Windows"
docker build -q -t snowballz-win tools/windows >/dev/null
docker run --rm -u "$(id -u):$(id -g)" -e CARGO_HOME=/cargo -e CARGO_TARGET_DIR=/src/target/windows \
    -v "$HOME/.cargo:/cargo" -v "$PWD:/src" -w /src snowballz-win \
    cargo build --release --target x86_64-pc-windows-gnu -p client -p server
win=target/windows/x86_64-pc-windows-gnu/release
stage dist/stage/snowballz "$win/snowballz.exe" "$win/snowballz-server.exe"
sed -i 's/$/\r/' dist/stage/snowballz/PLAY.txt
rm -f dist/snowballz-windows.zip
(cd dist/stage && zip -qr ../snowballz-windows.zip snowballz)
rm -rf dist/stage

ls -lh dist/
