#!/bin/sh
# Build the browser version into dist/web/ (plain static files).
#   tools/web.sh            build
#   tools/web.sh serve      build, then run a server for it on :7777
#                           (friends open http://<your address>:7777/)
# Needs: rustup target add wasm32-unknown-unknown
set -e
cd "$(dirname "$0")/.."
dest=dist/web
# Build beside the live directory and swap it in at the end, so a server
# that is serving $dest never sees half a build.
out=$dest.tmp
target=${CARGO_TARGET_DIR:-target}
cargo build --release --target wasm32-unknown-unknown -p client
rm -rf "$out"
mkdir -p "$out/assets"
# wasm-bindgen writes game.js and game_bg.wasm (the version must match the
# crate's: cargo install wasm-bindgen-cli --version <it> --locked).
wasm-bindgen --target web --no-typescript --out-name game --out-dir "$out" \
    "$target/wasm32-unknown-unknown/release/snowballz.wasm"
cp web/index.html web/snowballz.js "$out/"
# Only what the game reads: no server-side maps' specs, no source art.
cp -r assets/fonts assets/models assets/maps "$out/assets/"
# The page downloads everything listed here before it starts the game.
(cd "$out/assets" && find . -type f ! -name manifest.txt | sed 's|^\./||' | sort |
    while read -r f; do printf '%s\t%s\n' "$f" "$(wc -c < "$f")"; done > manifest.txt)
# Pre-compress what compresses (the server sends .gz to browsers that accept it).
for f in "$out"/game_bg.wasm "$out"/*.js "$out"/assets/*/*.sbm "$out"/assets/*/*.sbmap "$out"/assets/manifest.txt; do
    gzip -9 -k -f "$f"
done
rm -rf "$dest.old"
[ -d "$dest" ] && mv "$dest" "$dest.old"
mv "$out" "$dest"
rm -rf "$dest.old"
out=$dest
du -sh "$out"; ls -lh "$out"/game_bg.wasm*
if [ "$1" = serve ]; then
    cargo build --release -p server
    exec "$target"/release/snowballz-server --web "$out" --no-announce
fi
