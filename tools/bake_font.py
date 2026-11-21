#!/usr/bin/env python3
"""Bake a TTF into a signed-distance-field glyph atlas.

    tools/bake_font.py assets/fonts/Mali-Regular.ttf assets/fonts/ui

Writes <out>.png (distance in the alpha channel, 0.5 = glyph edge) and
<out>.txt with metrics, read by crates/client/src/font.rs:

    size <px> line <px> ascent <px> spread <px> atlas <w> <h>
    <codepoint> <x> <y> <w> <h> <xoff> <yoff> <advance>
"""

import sys

import numpy as np
from PIL import Image, ImageDraw, ImageFont

SIZE = 40  # em size of the SDF glyphs in the atlas
UPSCALE = 8  # supersampling for the distance computation
SPREAD = 5  # distance (in atlas pixels) mapped to the 0..1 range
CHARS = [chr(c) for c in range(32, 127)] + ["→", "•", "✓", "×", "…", "°", "▲", "▼", "—", "·", "é"]


def glyph_sdf(font, ch):
    hi = ImageFont.truetype(font.path, SIZE * UPSCALE)
    bbox = hi.getbbox(ch)
    adv = hi.getlength(ch) / UPSCALE
    if bbox[2] <= bbox[0] or bbox[3] <= bbox[1]:
        return None, adv, 0, 0
    pad = SPREAD * UPSCALE
    w = bbox[2] - bbox[0] + 2 * pad
    h = bbox[3] - bbox[1] + 2 * pad
    img = Image.new("L", (w, h), 0)
    ImageDraw.Draw(img).text((pad - bbox[0], pad - bbox[1]), ch, font=hi, fill=255)
    a = np.asarray(img) > 127
    # Edge pixels: inside pixels with an outside neighbour (and vice versa).
    edge = np.zeros_like(a)
    edge[1:, :] |= a[1:, :] != a[:-1, :]
    edge[:-1, :] |= a[1:, :] != a[:-1, :]
    edge[:, 1:] |= a[:, 1:] != a[:, :-1]
    edge[:, :-1] |= a[:, 1:] != a[:, :-1]
    ey, ex = np.nonzero(edge)
    ow, oh = w // UPSCALE, h // UPSCALE
    # Sample at output pixel centers.
    ys = (np.arange(oh) + 0.5) * UPSCALE
    xs = (np.arange(ow) + 0.5) * UPSCALE
    gx, gy = np.meshgrid(xs, ys)
    pts = np.stack([gx.ravel(), gy.ravel()], axis=1)
    edges = np.stack([ex, ey], axis=1).astype(np.float32)
    best = np.full(len(pts), 1e9, dtype=np.float32)
    for i in range(0, len(edges), 4096):
        e = edges[i : i + 4096]
        d = ((pts[:, None, :] - e[None, :, :]) ** 2).sum(axis=2)
        best = np.minimum(best, d.min(axis=1))
    dist = np.sqrt(best) / UPSCALE
    inside = a[np.clip(gy.ravel().astype(int), 0, h - 1), np.clip(gx.ravel().astype(int), 0, w - 1)]
    sd = np.where(inside, dist, -dist)
    v = np.clip(0.5 + sd / (2 * SPREAD), 0, 1).reshape(oh, ow)
    xoff = bbox[0] / UPSCALE - SPREAD
    yoff = bbox[1] / UPSCALE - SPREAD
    return (v * 255).astype(np.uint8), adv, xoff, yoff


def main(ttf, out):
    font = ImageFont.truetype(ttf, SIZE)
    ascent, descent = font.getmetrics()
    glyphs = []
    for ch in CHARS:
        try:
            sdf, adv, xoff, yoff = glyph_sdf(font, ch)
        except Exception as e:  # missing glyph
            print("skip", repr(ch), e)
            continue
        glyphs.append((ch, sdf, adv, xoff, yoff))
    W = 1024
    x = y = row = 0
    placed = []
    for ch, sdf, adv, xoff, yoff in glyphs:
        if sdf is None:
            placed.append((ch, 0, 0, 0, 0, 0, 0, adv))
            continue
        h, w = sdf.shape
        if x + w > W:
            x = 0
            y += row + 1
            row = 0
        placed.append((ch, x, y, w, h, xoff, yoff, adv, sdf))
        x += w + 1
        row = max(row, h)
    H = 1
    while H < y + row + 1:
        H *= 2
    atlas = np.zeros((H, W), dtype=np.uint8)
    for p in placed:
        if len(p) == 9:
            ch, gx, gy, w, h, *_rest, sdf = p
            atlas[gy : gy + h, gx : gx + w] = sdf
    rgba = np.zeros((H, W, 4), dtype=np.uint8)
    rgba[..., :3] = 255
    rgba[..., 3] = atlas
    Image.fromarray(rgba, "RGBA").save(out + ".png", optimize=True)
    with open(out + ".txt", "w") as f:
        f.write(f"size {SIZE} line {ascent + descent} ascent {ascent} spread {SPREAD} atlas {W} {H}\n")
        for p in placed:
            ch, gx, gy, w, h, xoff, yoff, adv = p[:8]
            f.write(f"{ord(ch)} {gx} {gy} {w} {h} {xoff:.2f} {yoff:.2f} {adv:.2f}\n")
    print(f"wrote {out}.png ({W}x{H}), {len(placed)} glyphs")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
