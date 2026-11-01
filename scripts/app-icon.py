"""Rasterizes the wowlua mark tile (wowlua-mark-tile.svg) into Studio's app icons.

The tile is straight lines only: a bg-100 ground, the brackets (a 3.5 wide
stroke with square caps) and the letters (two polygons). Polygons are filled
with 4x4 supersampling; the PNG is written with zlib alone.

Decision S020. Studio's icons are made with:

    python3 scripts/app-icon.py square 256 crates/wowlua-studio/assets/icon.png
    python3 scripts/app-icon.py macos crates/wowlua-studio/assets/icon-macos.png

usage: app-icon.py square <size> <out.png>  the tile as designed
       app-icon.py macos <out.png>          1024 px, Apple's icon grid: a rounded
                                            square of 824 px with the mark inside
"""
import math
import struct
import sys
import zlib

GROUND = (0x14, 0x12, 0x0E)
INK = (0xEC, 0xE6, 0xD6)
AMBER = (0xFF, 0xB5, 0x47)
SS = 4


def letters():
    """The 'wl' polygons of the tile, in its 48-unit box."""
    d = ("M0.36 -11.16H3.33L3.73 -6.48L3.87 -2.72H4.23L4.9 -11.16H8.08L8.69 -2.72H9.05L9.18 -6.48L9.56 -11.16H12.24L11 0.0H7.11L6.52 -8.41H6.16L5.51 0.0H1.6Z"
         "M20.12 -15.26V-2.84H23.36V0.0H13.64V-2.84H16.88V-12.42H13.64V-15.26Z")
    polys, cur, x, y = [], [], 0.0, 0.0
    i = 0
    toks = []
    num = ''
    for ch in d:
        if ch.isalpha():
            if num:
                toks.append(float(num)); num = ''
            toks.append(ch)
        elif ch in ' ,':
            if num:
                toks.append(float(num)); num = ''
        elif ch == '-' and num:
            toks.append(float(num)); num = '-'
        else:
            num += ch
    if num:
        toks.append(float(num))
    cmd = None
    while i < len(toks):
        t = toks[i]
        if isinstance(t, str):
            cmd = t; i += 1
            if cmd == 'Z':
                polys.append(cur); cur = []
            continue
        if cmd in ('M', 'L'):
            x, y = toks[i], toks[i + 1]; i += 2
        elif cmd == 'H':
            x = toks[i]; i += 1
        elif cmd == 'V':
            y = toks[i]; i += 1
        cur.append((x + 12.14, y + 31.63))
    return polys


def rect(x0, y0, x1, y1):
    return [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]


def brackets():
    """The stroke 'M9.5 5H4V43H9.5 M38.5 5H44V43H38.5', 3.5 wide, square caps."""
    h = 1.75
    return [
        rect(4 - h, 5 - h, 4 + h, 43 + h), rect(4 - h, 5 - h, 9.5 + h, 5 + h), rect(4 - h, 43 - h, 9.5 + h, 43 + h),
        rect(44 - h, 5 - h, 44 + h, 43 + h), rect(38.5 - h, 5 - h, 44 + h, 5 + h), rect(38.5 - h, 43 - h, 44 + h, 43 + h),
    ]


def rounded(x0, y0, size, r, seg=48):
    pts = []
    for cx, cy, a0 in ((x0 + size - r, y0 + r, -90), (x0 + size - r, y0 + size - r, 0),
                       (x0 + r, y0 + size - r, 90), (x0 + r, y0 + r, 180)):
        for k in range(seg + 1):
            a = math.radians(a0 + 90 * k / seg)
            pts.append((cx + r * math.cos(a), cy + r * math.sin(a)))
    return pts


def coverage(polys, size):
    """Per-pixel coverage (0..1) of the union of `polys`, in pixel units."""
    cov = [[0.0] * size for _ in range(size)]
    edges = []
    for poly in polys:
        for (ax, ay), (bx, by) in zip(poly, poly[1:] + poly[:1]):
            if ay != by:
                edges.append((ax, ay, bx, by))
    step = 1.0 / SS
    for py in range(size):
        row = cov[py]
        for s in range(SS):
            y = py + (s + 0.5) * step
            xs = []
            for ax, ay, bx, by in edges:
                if (ay <= y < by) or (by <= y < ay):
                    xs.append((ax + (y - ay) * (bx - ax) / (by - ay), 1 if by > ay else -1))
            if not xs:
                continue
            xs.sort()
            wind = 0
            for k, (x, dirn) in enumerate(xs):
                before = wind
                wind += dirn
                if before == 0 and wind != 0:
                    start = x
                elif before != 0 and wind == 0:
                    span(row, start, x, size)
    return cov


def span(row, x0, x1, size):
    """Adds a horizontal span of one sub-row to `row`."""
    w = 1.0 / SS
    x0 = max(0.0, x0); x1 = min(float(size), x1)
    if x1 <= x0:
        return
    i0, i1 = int(x0), int(x1)
    if i0 == i1:
        row[i0] += (x1 - x0) * w
        return
    row[i0] += (i0 + 1 - x0) * w
    for i in range(i0 + 1, min(i1, size)):
        row[i] += w
    if i1 < size:
        row[i1] += (x1 - i1) * w


def compose(layers, size):
    """Layers of (coverage, colour) over transparency, as straight RGBA."""
    out = bytearray(size * size * 4)
    for y in range(size):
        for x in range(size):
            r = g = b = a = 0.0
            for cov, (cr, cg, cb) in layers:
                c = min(1.0, cov[y][x])
                if c <= 0:
                    continue
                na = c + a * (1 - c)
                r = (cr * c + r * a * (1 - c)) / na
                g = (cg * c + g * a * (1 - c)) / na
                b = (cb * c + b * a * (1 - c)) / na
                a = na
            o = (y * size + x) * 4
            out[o:o + 4] = bytes((round(r), round(g), round(b), round(a * 255)))
    return out


def write_png(path, rgba, size):
    raw = b''.join(b'\x00' + bytes(rgba[y * size * 4:(y + 1) * size * 4]) for y in range(size))
    def chunk(kind, data):
        c = struct.pack('>I', len(data)) + kind + data
        return c + struct.pack('>I', zlib.crc32(kind + data) & 0xFFFFFFFF)
    png = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', size, size, 8, 6, 0, 0, 0))
    png += chunk(b'IDAT', zlib.compress(raw, 9)) + chunk(b'IEND', b'')
    open(path, 'wb').write(png)


def place(polys, scale, ox, oy):
    return [[(ox + x * scale, oy + y * scale) for x, y in p] for p in polys]


def main():
    if sys.argv[1] == 'square':
        size = int(sys.argv[2]); out = sys.argv[3]
        s = size / 48
        ground = [rect(0, 0, size, size)]
        ink = place(brackets(), s, 0, 0)
        amber = place(letters(), s, 0, 0)
    else:
        size = 1024; out = sys.argv[2]
        body, off = 824.0, 100.0
        ground = [rounded(off, off, body, body * 0.225)]
        # The mark at 84 % of the body, centred, clear of the corners.
        s = body * 0.84 / 48
        o = off + (body - 48 * s) / 2
        ink = place(brackets(), s, o, o)
        amber = place(letters(), s, o, o)
    layers = [(coverage(ground, size), GROUND), (coverage(ink, size), INK), (coverage(amber, size), AMBER)]
    write_png(out, compose(layers, size), size)
    print('wrote', out)


main()
