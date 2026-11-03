#!/usr/bin/env python3
"""Draw the Valence app icon (formaldehyde, H2C=O, in the app's colours).

    python3 packaging/icon.py <out.png> <size> [--mac]

--mac draws the tile on Apple's icon grid (824 px body on a 1024 canvas).
Needs pycairo. The outputs are committed (see packaging/icons.sh), so
building never needs this.
"""
import colorsys
import math
import sys

import cairo


def hsl(h, s, l):
    r, g, b = colorsys.hls_to_rgb((h % 360) / 360, l, s)
    return (r, g, b)


# Same formula as src/palette.rs: hue by periodic-table column.
def column(group):
    return hsl((group - 1) * 285 / 17, 0.68, 0.58)


H, C, O = column(1), column(14), column(16)


def draw(path, size, mac):
    s = cairo.ImageSurface(cairo.FORMAT_ARGB32, size, size)
    cr = cairo.Context(s)
    cr.scale(size / 1024, size / 1024)
    if mac:
        x0, w, r = 100, 824, 185
    else:
        x0, w, r = 0, 1024, 224
    # Tile: rounded square, dark blue-grey gradient.
    cr.new_sub_path()
    for cx, cy, a in ((x0 + w - r, x0 + r, -90), (x0 + w - r, x0 + w - r, 0),
                      (x0 + r, x0 + w - r, 90), (x0 + r, x0 + r, 180)):
        cr.arc(cx, cy, r, math.radians(a), math.radians(a + 90))
    cr.close_path()
    g = cairo.LinearGradient(0, x0, 0, x0 + w)
    g.add_color_stop_rgb(0, 0.13, 0.15, 0.21)
    g.add_color_stop_rgb(1, 0.05, 0.06, 0.09)
    cr.set_source(g)
    cr.fill_preserve()
    cr.clip()

    # Map a 0..1 molecule space onto the tile.
    def P(x, y):
        return x0 + x * w, x0 + y * w

    c = P(0.47, 0.56)
    o = P(0.75, 0.30)
    h1 = P(0.20, 0.36)
    h2 = P(0.40, 0.85)
    k = w / 1024

    def tube(a, b, width, offset=0.0):
        dx, dy = b[0] - a[0], b[1] - a[1]
        L = math.hypot(dx, dy)
        nx, ny = -dy / L * offset, dx / L * offset
        cr.set_line_cap(cairo.LINE_CAP_ROUND)
        cr.move_to(a[0] + nx, a[1] + ny)
        cr.line_to(b[0] + nx, b[1] + ny)
        cr.set_source_rgba(0, 0, 0, 0.35)
        cr.set_line_width(width + 10 * k)
        cr.stroke_preserve()
        cr.set_source_rgb(0.95, 0.96, 0.98)
        cr.set_line_width(width)
        cr.stroke()

    tube(c, h1, 44 * k)
    tube(c, h2, 44 * k)
    tube(c, o, 36 * k, 34 * k)   # C=O double bond
    tube(c, o, 36 * k, -34 * k)

    def ball(p, rad, col):
        # Soft glow, then a shaded sphere with a highlight.
        glow = cairo.RadialGradient(p[0], p[1], rad * 0.8, p[0], p[1], rad * 1.6)
        glow.add_color_stop_rgba(0, *col, 0.35)
        glow.add_color_stop_rgba(1, *col, 0.0)
        cr.set_source(glow)
        cr.arc(p[0], p[1], rad * 1.6, 0, 2 * math.pi)
        cr.fill()
        hx, hy = p[0] - rad * 0.35, p[1] - rad * 0.4
        sh = cairo.RadialGradient(hx, hy, rad * 0.05, p[0], p[1], rad)
        light = tuple(min(1, v * 0.5 + 0.55) for v in col)
        dark = tuple(v * 0.45 for v in col)
        sh.add_color_stop_rgb(0, *light)
        sh.add_color_stop_rgb(0.55, *col)
        sh.add_color_stop_rgb(1, *dark)
        cr.set_source(sh)
        cr.arc(p[0], p[1], rad, 0, 2 * math.pi)
        cr.fill()

    ball(h1, 80 * k, H)
    ball(h2, 80 * k, H)
    ball(o, 116 * k, O)
    ball(c, 126 * k, C)
    s.write_to_png(path)


if __name__ == "__main__":
    draw(sys.argv[1], int(sys.argv[2]), "--mac" in sys.argv[3:])
