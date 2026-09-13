"""Export every Windows icon from ONE correct master (the edited emblem).

Do not redraw the cave in code — previous ellipse/pieslice overlays were the
broken blobs in icons/. Explorer and the tray all come from this same PNG.
"""
from __future__ import annotations

import struct
from collections import deque
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[1]
MASTER = ROOT / "tmp" / "nidong-emblem-master.jpg"
OUT = ROOT / "src-tauri" / "icons"
ASSETS = ROOT / "assets"


def flood_knockout(im: Image.Image, thresh: int = 48) -> Image.Image:
    im = im.convert("RGBA")
    w, h = im.size
    px = im.load()
    q: deque[tuple[int, int]] = deque(
        [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)]
    )
    seen = bytearray(w * h)
    while q:
        x, y = q.popleft()
        if x < 0 or y < 0 or x >= w or y >= h:
            continue
        i = y * w + x
        if seen[i]:
            continue
        r, g, b, a = px[x, y]
        if a > 0 and r + g + b > thresh:
            continue
        seen[i] = 1
        px[x, y] = (0, 0, 0, 0)
        q.append((x + 1, y))
        q.append((x - 1, y))
        q.append((x, y + 1))
        q.append((x, y - 1))
    return im


def to_square(im: Image.Image) -> Image.Image:
    im = im.convert("RGBA")
    w, h = im.size
    s = max(w, h)
    canvas = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    canvas.paste(im, ((s - w) // 2, (s - h) // 2), im)
    return canvas


def inner_disk(im: Image.Image) -> Image.Image:
    w, h = im.size
    cx, cy = w / 2, h / 2
    r = min(w, h) * 0.335
    box = (int(cx - r), int(cy - r), int(cx + r + 1), int(cy + r + 1))
    crop = im.crop(box)
    m = Image.new("L", crop.size, 0)
    ImageDraw.Draw(m).ellipse((1, 1, crop.size[0] - 2, crop.size[1] - 2), fill=255)
    out = Image.new("RGBA", crop.size, (0, 0, 0, 0))
    out.paste(crop, (0, 0), m)
    return out


def fit(im: Image.Image, size: int) -> Image.Image:
    return im.resize((size, size), Image.Resampling.LANCZOS)


def ico_bmp(im: Image.Image) -> bytes:
    """32-bit BMP XOR + 1-bit AND mask. NSIS / RC.EXE stretch PNG-in-ICO frames."""
    im = im.convert("RGBA")
    w, h = im.size
    if w != h:
        raise ValueError(f"non-square {im.size}")
    px = im.load()
    xor = bytearray()
    and_rows: list[bytes] = []
    for y in range(h - 1, -1, -1):
        bits: list[int] = []
        for x in range(w):
            r, g, b, a = px[x, y]
            xor.extend((b, g, r, a))
            bits.append(1 if a == 0 else 0)
        while len(bits) % 32:
            bits.append(0)
        row = bytearray()
        for i in range(0, len(bits), 8):
            byte = 0
            for j in range(8):
                byte = (byte << 1) | bits[i + j]
            row.append(byte)
        and_rows.append(bytes(row))
    and_mask = b"".join(and_rows)
    header = struct.pack(
        "<IIIHHIIIIII",
        40,
        w,
        h * 2,
        1,
        32,
        0,
        len(xor) + len(and_mask),
        0,
        0,
        0,
        0,
    )
    return header + bytes(xor) + and_mask


def write_ico(path: Path, images: list[Image.Image]) -> None:
    # BMP-in-ICO for every size (incl. 256). PNG-compressed ICO looks stretched
    # in the NSIS installer and in Explorer after RC.EXE embeds the resource.
    frames: list[tuple[int, bytes]] = []
    for im in images:
        if im.size[0] != im.size[1]:
            raise ValueError(f"non-square {im.size}")
        data = ico_bmp(im)
        frames.append((im.size[0], data))
    count = len(frames)
    header = struct.pack("<HHH", 0, 1, count)
    offset = 6 + 16 * count
    directory = b""
    payload = b""
    for size, data in frames:
        directory += struct.pack(
            "<BBBBHHII",
            0 if size >= 256 else size,
            0 if size >= 256 else size,
            0,
            0,
            1,
            32,
            len(data),
            offset,
        )
        offset += len(data)
        payload += data
    path.write_bytes(header + directory + payload)


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    ASSETS.mkdir(parents=True, exist_ok=True)
    if not MASTER.exists():
        raise SystemExit(f"missing master {MASTER}")
    seal = to_square(flood_knockout(Image.open(MASTER)))
    disk = inner_disk(seal)
    seal.save(ASSETS / "nidong-app-icon.png")
    disk.save(ASSETS / "nidong-tray-icon.png")

    frames = {n: fit(seal, n) for n in (16, 24, 32, 48, 64, 128, 256)}
    for n, im in frames.items():
        im.save(OUT / f"{n}x{n}.png")
    frames[32].save(OUT / "henry.32.png")
    fit(seal, 512).save(OUT / "icon.png")

    tray32 = fit(disk, 32)
    tray32.save(OUT / "tray-32.png")
    (OUT / "tray-32.rgba").write_bytes(tray32.tobytes())
    # Keep 32x32.rgba the same square seal as 32x32.png (not the cropped tray).
    (OUT / "32x32.rgba").write_bytes(frames[32].tobytes())

    # 32px first: Tauri / Explorer pick the first ICO layer in some shells.
    write_ico(
        OUT / "icon.ico",
        [frames[n] for n in (32, 16, 24, 48, 64, 256)],
    )
    print("wrote", OUT, "master", seal.size)


if __name__ == "__main__":
    main()
