#!/usr/bin/env python3
"""Generate the illustrations for "The Lighthouse Protocol" (DARM example book).

Deterministic and stdlib-only: running it again rewrites byte-identical SVG
files into ../the-lighthouse-protocol/art/.

    python3 tools/make_lighthouse_art.py

Every plate is plain static SVG: no scripts, no event handlers, no
foreignObject, no external references, no raster images. Lettering is limited
to a few chart labels set in the generic `serif` family.

Style: nocturne line-and-wash. Fine bone-white line work over navy washes;
amber is reserved for light.
"""
import math
import os
import random

NAVY = "#0e1a26"
NAVY2 = "#17293a"
FOG = "#8fa3ad"
AMBER = "#f2b45a"
BONE = "#e9e4d6"
INK = "#16202a"

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.normpath(os.path.join(HERE, "..", "the-lighthouse-protocol", "art"))


# --------------------------------------------------------------------------
# small SVG toolkit
# --------------------------------------------------------------------------

def n(v):
    """Format a coordinate with at most one decimal."""
    s = "%.1f" % v
    if s.endswith(".0"):
        s = s[:-2]
    if s == "-0":
        s = "0"
    return s


def pts_path(pts, close=False):
    d = "M" + " L".join("%s %s" % (n(x), n(y)) for x, y in pts)
    return d + (" Z" if close else "")


def smooth_path(pts, close=False):
    """Catmull-Rom spline through pts as cubic Beziers."""
    m = len(pts)
    d = "M%s %s" % (n(pts[0][0]), n(pts[0][1]))
    last = m if close else m - 1
    for i in range(last):
        if close:
            p0, p1, p2, p3 = pts[i - 1], pts[i], pts[(i + 1) % m], pts[(i + 2) % m]
        else:
            p0 = pts[max(i - 1, 0)]
            p1, p2 = pts[i], pts[i + 1]
            p3 = pts[min(i + 2, m - 1)]
        c1 = (p1[0] + (p2[0] - p0[0]) / 6.0, p1[1] + (p2[1] - p0[1]) / 6.0)
        c2 = (p2[0] - (p3[0] - p1[0]) / 6.0, p2[1] - (p3[1] - p1[1]) / 6.0)
        d += " C%s %s %s %s %s %s" % (n(c1[0]), n(c1[1]), n(c2[0]), n(c2[1]), n(p2[0]), n(p2[1]))
    return d + (" Z" if close else "")


def sample_closed(pts, per=14):
    """Dense polyline along the closed Catmull-Rom spline through pts."""
    out = []
    m = len(pts)
    for i in range(m):
        p0, p1, p2, p3 = pts[i - 1], pts[i], pts[(i + 1) % m], pts[(i + 2) % m]
        c1 = (p1[0] + (p2[0] - p0[0]) / 6.0, p1[1] + (p2[1] - p0[1]) / 6.0)
        c2 = (p2[0] - (p3[0] - p1[0]) / 6.0, p2[1] - (p3[1] - p1[1]) / 6.0)
        for k in range(per):
            t = k / float(per)
            a = (1 - t) ** 3
            b = 3 * (1 - t) ** 2 * t
            c = 3 * (1 - t) * t * t
            e = t ** 3
            out.append((a * p1[0] + b * c1[0] + c * c2[0] + e * p2[0],
                        a * p1[1] + b * c1[1] + c * c2[1] + e * p2[1]))
    return out


def inside(poly, x, y):
    hit = False
    j = len(poly) - 1
    for i in range(len(poly)):
        xi, yi = poly[i]
        xj, yj = poly[j]
        if (yi > y) != (yj > y) and x < (xj - xi) * (y - yi) / (yj - yi) + xi:
            hit = not hit
        j = i
    return hit


class Plate:
    def __init__(self, w, h, title, desc):
        self.w, self.h = w, h
        self.title, self.desc = title, desc
        self.defs = []
        self.body = []
        self._ids = set()

    def add(self, s):
        self.body.append(s)

    def lin(self, gid, x1, y1, x2, y2, stops, user=True):
        if gid in self._ids:
            return gid
        self._ids.add(gid)
        if user:
            head = '<linearGradient id="%s" gradientUnits="userSpaceOnUse" x1="%s" y1="%s" x2="%s" y2="%s">' % (
                gid, n(x1), n(y1), n(x2), n(y2))
        else:
            head = '<linearGradient id="%s" x1="%s" y1="%s" x2="%s" y2="%s">' % (gid, x1, y1, x2, y2)
        self.defs.append(head + _stops(stops) + "</linearGradient>")
        return gid

    def rad(self, gid, stops, cx=None, cy=None, r=None):
        if gid in self._ids:
            return gid
        self._ids.add(gid)
        if cx is None:
            head = '<radialGradient id="%s">' % gid
        else:
            head = '<radialGradient id="%s" gradientUnits="userSpaceOnUse" cx="%s" cy="%s" r="%s">' % (
                gid, n(cx), n(cy), n(r))
        self.defs.append(head + _stops(stops) + "</radialGradient>")
        return gid

    def blur(self, fid, dev):
        if fid in self._ids:
            return fid
        self._ids.add(fid)
        self.defs.append(
            '<filter id="%s" x="-20%%" y="-20%%" width="140%%" height="140%%">'
            '<feGaussianBlur stdDeviation="%s"/></filter>' % (fid, n(dev)))
        return fid

    def svg(self):
        return (
            '<?xml version="1.0" encoding="UTF-8"?>\n'
            '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 %d %d" width="%d" height="%d">\n'
            "<title>%s</title>\n<desc>%s</desc>\n<defs>\n%s\n</defs>\n%s\n</svg>\n"
            % (self.w, self.h, self.w, self.h, self.title, self.desc,
               "\n".join(self.defs), "\n".join(self.body)))

    def save(self, name):
        path = os.path.join(OUT, name)
        with open(path, "w", encoding="utf-8", newline="\n") as fh:
            fh.write(self.svg())
        return path


def _stops(stops):
    out = ""
    for off, col, op in stops:
        out += '<stop offset="%s" stop-color="%s"' % (off, col)
        if op != 1:
            out += ' stop-opacity="%s"' % op
        out += "/>"
    return out


# --------------------------------------------------------------------------
# shared drawing helpers
# --------------------------------------------------------------------------

def wave_d(x0, x1, y, amp, wl, rng):
    """A gently irregular wavy stroke from x0 to x1."""
    d = "M%s %s" % (n(x0), n(y))
    x = x0
    sign = -1
    while x < x1 - 1:
        step = min(wl * rng.uniform(0.8, 1.25), x1 - x)
        a = amp * rng.uniform(0.6, 1.2) * sign
        d += " q%s %s %s 0" % (n(step / 2.0), n(a), n(step))
        x += step
        sign = -sign
    return d


def fog_bank(p, rng, y0, y1, count, rx_rng, ry_rng, op_rng, color=FOG, gid="fogg", xr=None):
    p.rad(gid, [(0, color, 1), (0.55, color, 0.55), (1, color, 0)])
    x_lo, x_hi = xr if xr else (-100, p.w + 100)
    for _ in range(count):
        cx = rng.uniform(x_lo, x_hi)
        cy = rng.uniform(y0, y1)
        p.add('<ellipse cx="%s" cy="%s" rx="%s" ry="%s" fill="url(#%s)" opacity="%s"/>' % (
            n(cx), n(cy), n(rng.uniform(*rx_rng)), n(rng.uniform(*ry_rng)), gid,
            "%.2f" % rng.uniform(*op_rng)))


def fog_lines(p, rng, y0, y1, count, color=BONE, op=0.16, xr=None):
    x_lo, x_hi = xr if xr else (0, p.w)
    for _ in range(count):
        y = rng.uniform(y0, y1)
        length = rng.uniform(140, 420)
        x = rng.uniform(x_lo - 60, x_hi - length + 60)
        p.add('<path d="%s" fill="none" stroke="%s" stroke-width="1.1" stroke-linecap="round" opacity="%s"/>' % (
            wave_d(x, x + length, y, 3.5, 70, rng), color, "%.2f" % (op * rng.uniform(0.6, 1.2))))


def stars(p, rng, count, y_max, x_min=0, x_max=None, op_scale=1.0):
    x_max = x_max or p.w
    for i in range(count):
        x = rng.uniform(x_min + 20, x_max - 20)
        y = rng.uniform(24, y_max)
        r = rng.choice([0.9, 1.1, 1.3, 1.7])
        fade = 1.0 - 0.6 * (y / float(y_max))
        o = rng.uniform(0.35, 0.85) * fade * op_scale
        p.add('<circle cx="%s" cy="%s" r="%s" fill="%s" opacity="%s"/>' % (n(x), n(y), r, BONE, "%.2f" % o))
        if i % 9 == 0:
            s = 5.5
            p.add('<path d="M%s %s h%s M%s %s v%s" stroke="%s" stroke-width="0.7" opacity="%s"/>' % (
                n(x - s), n(y), n(2 * s), n(x), n(y - s), n(2 * s), BONE, "%.2f" % (o * 0.7)))


def swell(p, rng, y0, y1, rows, color=BONE, op0=0.30, op1=0.0, x0=0, x1=None, skip=None):
    """Rows of short swell strokes, sparser and longer toward the viewer."""
    x1 = x1 or p.w
    for r in range(rows):
        t = (r + rng.uniform(0.1, 0.9)) / rows
        y = y0 + (y1 - y0) * (t ** 1.5)
        op = op0 + (op1 - op0) * t
        if op <= 0.02:
            continue
        count = int(7 - 4 * t) + 1
        for _ in range(count):
            length = rng.uniform(40, 110) * (0.7 + 1.6 * t)
            x = rng.uniform(x0 - 20, x1 - length + 20)
            if skip and skip(x, x + length, y):
                continue
            p.add('<path d="%s" fill="none" stroke="%s" stroke-width="%s" stroke-linecap="round" opacity="%s"/>' % (
                wave_d(x, x + length, y, 2.2 + 3 * t, 34 + 40 * t, rng), color, n(1.0 + 0.5 * t),
                "%.2f" % (op * rng.uniform(0.6, 1.0))))


def reflection(p, rng, cx, y0, y1, half, color, op0, rows=16, grow=1.0):
    """Broken horizontal dashes: a light (or a silhouette) reflected in water."""
    for r in range(rows):
        t = (r + 0.5) / rows
        y = y0 + (y1 - y0) * t + rng.uniform(-2, 2)
        w = half * (0.55 + grow * t) * rng.uniform(0.5, 1.0)
        x = cx + rng.uniform(-half * 0.35, half * 0.35) * (0.4 + t)
        p.add('<path d="M%s %s h%s" stroke="%s" stroke-width="%s" stroke-linecap="round" opacity="%s"/>' % (
            n(x - w), n(y), n(2 * w), color, n(2.6 - 1.2 * t), "%.2f" % (op0 * (1 - t) ** 1.3)))


def vignette(p, strength=0.55):
    p.rad("vig", [(0, "#05090e", 0), (0.62, "#05090e", 0), (1, "#05090e", strength)],
          p.w / 2.0, p.h / 2.0, math.hypot(p.w, p.h) / 2.0)
    p.add('<rect width="%d" height="%d" fill="url(#vig)"/>' % (p.w, p.h))


def beam(p, gid, ox, oy, a1, a2, length, op=0.5, soft=True):
    """A lamp beam: a wedge from (ox, oy) between two angles (degrees, screen space)."""
    r1, r2 = math.radians(a1), math.radians(a2)
    am = (r1 + r2) / 2.0
    ex, ey = ox + math.cos(am) * length, oy + math.sin(am) * length
    p.lin(gid, ox, oy, ex, ey, [(0, "#ffd998", op), (0.18, AMBER, op * 0.62),
                               (0.55, AMBER, op * 0.2), (1, AMBER, 0)])
    far = length * 1.1
    pts = [(ox, oy), (ox + math.cos(r1) * far, oy + math.sin(r1) * far),
           (ox + math.cos(r2) * far, oy + math.sin(r2) * far)]
    filt = ' filter="url(#%s)"' % p.blur("soft", 7) if soft else ""
    p.add('<path d="%s" fill="url(#%s)"%s/>' % (pts_path(pts, True), gid, filt))
    # a brighter narrow core
    rc1 = am - (r2 - r1) * 0.16
    rc2 = am + (r2 - r1) * 0.16
    core = [(ox, oy), (ox + math.cos(rc1) * far, oy + math.sin(rc1) * far),
            (ox + math.cos(rc2) * far, oy + math.sin(rc2) * far)]
    p.add('<path d="%s" fill="url(#%s)" opacity="0.7"%s/>' % (pts_path(core, True), gid, filt))


# --------------------------------------------------------------------------
# the lighthouse of Skerry Vane (identical construction in every plate)
# --------------------------------------------------------------------------

def lh_geom(cx, base, H):
    """Key measurements. `base` is the foot of the tower, H the full height."""
    g = {"cx": cx, "base": base, "H": H}
    g["tower_h"] = 0.715 * H
    g["bw"] = 0.112 * H          # half width at the foot
    g["tw"] = 0.068 * H          # half width under the gallery
    g["ty"] = base - g["tower_h"]
    g["corbel_h"] = 0.028 * H
    g["gw"] = 0.118 * H          # gallery half width
    g["deck_y"] = g["ty"] - g["corbel_h"]
    g["deck_h"] = 0.012 * H
    g["rail_h"] = 0.05 * H
    g["lw"] = 0.054 * H          # lantern half width
    g["mur_h"] = 0.025 * H
    g["glass_h"] = 0.105 * H
    g["glass_bot"] = g["deck_y"] - g["deck_h"] - g["mur_h"]
    g["glass_top"] = g["glass_bot"] - g["glass_h"]
    g["lamp"] = (cx, g["glass_bot"] - g["glass_h"] * 0.5)
    g["dome_w"] = 0.064 * H
    g["dome_h"] = 0.062 * H
    g["stem_h"] = 0.02 * H
    g["ball_r"] = 0.014 * H
    g["top"] = g["glass_top"] - 0.008 * H - g["dome_h"] - g["stem_h"] - 2 * g["ball_r"]
    return g


def _tower_hw(g, y):
    t = (y - g["ty"]) / g["tower_h"]
    t = max(0.0, min(1.0, t))
    return g["tw"] + (g["bw"] - g["tw"]) * (t ** 2.3)


def lighthouse(p, g, mode="night", uid="lh", glow=True):
    cx, base, H = g["cx"], g["base"], g["H"]
    night = mode == "night"
    sw = max(0.9, min(2.4, H / 230.0))
    line = BONE if night else "#1b2630"
    lo = "0.9" if night else "1"
    lx, ly = g["lamp"]

    if night and glow:
        p.rad(uid + "halo", [(0, "#ffe2a6", 0.75), (0.22, AMBER, 0.38), (0.6, AMBER, 0.1), (1, AMBER, 0)],
              lx, ly, 0.42 * H)
        p.add('<circle cx="%s" cy="%s" r="%s" fill="url(#%shalo)"/>' % (n(lx), n(ly), n(0.42 * H), uid))

    # tower body
    steps = 14
    left, right = [], []
    for i in range(steps + 1):
        y = g["ty"] + g["tower_h"] * i / steps
        hw = _tower_hw(g, y)
        left.append((cx - hw, y))
        right.append((cx + hw, y))
    body = left + right[::-1]
    if night:
        p.lin(uid + "tw", cx - g["bw"], 0, cx + g["bw"], 0,
              [(0, "#38536a", 1), (0.42, "#263d52", 1), (1, "#101d2a", 1)])
        fill = "url(#%stw)" % uid
    else:
        fill = "#1b2630"
    p.add('<path d="%s" fill="%s" stroke="%s" stroke-width="%s" stroke-linejoin="round" stroke-opacity="%s"/>' % (
        pts_path(body, True), fill, line, n(sw), lo))

    if night:
        # masonry courses, bowed to suggest the round of the tower
        courses = 16 if H >= 300 else 9
        d = ""
        for k in range(1, courses):
            y = g["ty"] + g["tower_h"] * k / courses
            hw = _tower_hw(g, y) - sw
            d += "M%s %s q%s %s %s 0 " % (n(cx - hw), n(y), n(hw), n(hw * 0.16), n(2 * hw))
        p.add('<path d="%s" fill="none" stroke="%s" stroke-width="%s" opacity="0.2"/>' % (d.strip(), BONE, n(sw * 0.55)))
        # highlight on the weather side
        hl = [(cx - _tower_hw(g, g["ty"] + g["tower_h"] * i / 6.0) + sw * 2.2, g["ty"] + g["tower_h"] * i / 6.0)
              for i in range(7)]
        p.add('<path d="%s" fill="none" stroke="%s" stroke-width="%s" opacity="0.28"/>' % (pts_path(hl), BONE, n(sw * 0.7)))

    # windows and door
    ww, wh = 0.017 * H, 0.04 * H
    for k, (t, off, lit) in enumerate([(0.2, 0.15, False), (0.46, -0.2, True), (0.7, 0.18, False)]):
        y = g["ty"] + g["tower_h"] * t
        x = cx + off * _tower_hw(g, y) * 2
        d = "M%s %s v%s a%s %s 0 0 1 %s 0 v%s Z" % (
            n(x - ww / 2), n(y + wh / 2), n(-(wh - ww / 2)), n(ww / 2), n(ww / 2), n(ww), n(wh - ww / 2))
        if night:
            wf = AMBER if lit else "#0a121b"
            p.add('<path d="%s" fill="%s" stroke="%s" stroke-width="%s" opacity="%s"/>' % (
                d, wf, BONE, n(sw * 0.5), "0.95" if lit else "0.8"))
        else:
            p.add('<path d="%s" fill="#c3ccca" opacity="0.5"/>' % d)
    dw, dh = 0.036 * H, 0.075 * H
    d = "M%s %s v%s a%s %s 0 0 1 %s 0 v%s Z" % (
        n(cx - dw / 2), n(base), n(-(dh - dw / 2)), n(dw / 2), n(dw / 2), n(dw), n(dh - dw / 2))
    if night:
        p.add('<path d="%s" fill="#0a121b" stroke="%s" stroke-width="%s" opacity="0.85"/>' % (d, BONE, n(sw * 0.6)))

    # corbel and gallery deck
    cor = [(cx - g["tw"], g["ty"]), (cx - g["gw"] * 0.96, g["deck_y"]), (cx + g["gw"] * 0.96, g["deck_y"]),
           (cx + g["tw"], g["ty"])]
    cfill = "#22384b" if night else "#1b2630"
    p.add('<path d="%s" fill="%s" stroke="%s" stroke-width="%s" stroke-linejoin="round" stroke-opacity="%s"/>' % (
        pts_path(cor, True), cfill, line, n(sw), lo))
    p.add('<rect x="%s" y="%s" width="%s" height="%s" fill="%s" stroke="%s" stroke-width="%s" stroke-opacity="%s"/>' % (
        n(cx - g["gw"]), n(g["deck_y"] - g["deck_h"]), n(2 * g["gw"]), n(g["deck_h"]), cfill, line, n(sw), lo))

    # lantern room: murette, glazing, astragals
    mur_top = g["glass_bot"]
    p.add('<rect x="%s" y="%s" width="%s" height="%s" fill="%s" stroke="%s" stroke-width="%s" stroke-opacity="%s"/>' % (
        n(cx - g["lw"] * 1.06), n(mur_top), n(2.12 * g["lw"]), n(g["mur_h"]), cfill, line, n(sw), lo))
    gx, gy, gw2, gh = cx - g["lw"], g["glass_top"], 2 * g["lw"], g["glass_h"]
    if night:
        p.rad(uid + "gl", [(0, "#fff3d0", 1), (0.45, "#ffd58a", 1), (1, AMBER, 1)], lx, ly, g["lw"] * 1.25)
        gfill = "url(#%sgl)" % uid
        gop = "1"
    else:
        gfill = "#cfd6d2"
        gop = "0.6"
    p.add('<rect x="%s" y="%s" width="%s" height="%s" fill="%s" opacity="%s"/>' % (
        n(gx), n(gy), n(gw2), n(gh), gfill, gop))
    bar = INK if night else "#1b2630"
    cols = 4
    d = ""
    for c in range(cols):
        x0 = gx + gw2 * c / cols
        x1 = gx + gw2 * (c + 1) / cols
        if H >= 260:
            d += "M%s %s L%s %s M%s %s L%s %s " % (n(x0), n(gy), n(x1), n(gy + gh), n(x1), n(gy), n(x0), n(gy + gh))
        if c:
            d += "M%s %s v%s " % (n(x0), n(gy), n(gh))
    p.add('<path d="%s" fill="none" stroke="%s" stroke-width="%s" opacity="%s"/>' % (
        d.strip(), bar, n(sw * 0.7), "0.75" if night else "1"))
    p.add('<rect x="%s" y="%s" width="%s" height="%s" fill="none" stroke="%s" stroke-width="%s" stroke-opacity="%s"/>' % (
        n(gx), n(gy), n(gw2), n(gh), line, n(sw), lo))

    # gallery railing (in front of the lantern)
    ry0 = g["deck_y"] - g["deck_h"]
    ry1 = ry0 - g["rail_h"]
    posts = 9 if H >= 260 else 7
    d = "M%s %s h%s M%s %s h%s " % (n(cx - g["gw"]), n(ry1), n(2 * g["gw"]),
                                   n(cx - g["gw"]), n(ry0 - g["rail_h"] * 0.5), n(2 * g["gw"]))
    for k in range(posts):
        x = cx - g["gw"] + 2 * g["gw"] * k / (posts - 1)
        d += "M%s %s v%s " % (n(x), n(ry0), n(-g["rail_h"]))
    p.add('<path d="%s" fill="none" stroke="%s" stroke-width="%s" stroke-linecap="round" opacity="%s"/>' % (
        d.strip(), line, n(sw * 0.75), "0.95" if night else "1"))

    # cupola: eave, dome, stem and vent ball
    ey = g["glass_top"]
    eave_h = 0.008 * H
    p.add('<rect x="%s" y="%s" width="%s" height="%s" fill="%s" stroke="%s" stroke-width="%s" stroke-opacity="%s"/>' % (
        n(cx - g["dome_w"] * 1.08), n(ey - eave_h), n(2.16 * g["dome_w"]), n(eave_h), cfill, line, n(sw), lo))
    dy = ey - eave_h
    dome = "M%s %s C%s %s %s %s %s %s C%s %s %s %s %s %s Z" % (
        n(cx - g["dome_w"]), n(dy),
        n(cx - g["dome_w"]), n(dy - g["dome_h"] * 0.62), n(cx - g["dome_w"] * 0.5), n(dy - g["dome_h"]),
        n(cx), n(dy - g["dome_h"]),
        n(cx + g["dome_w"] * 0.5), n(dy - g["dome_h"]), n(cx + g["dome_w"]), n(dy - g["dome_h"] * 0.62),
        n(cx + g["dome_w"]), n(dy))
    if night:
        p.lin(uid + "dm", cx - g["dome_w"], 0, cx + g["dome_w"], 0, [(0, "#3a556b", 1), (1, "#111f2c", 1)])
        dfill = "url(#%sdm)" % uid
    else:
        dfill = "#1b2630"
    p.add('<path d="%s" fill="%s" stroke="%s" stroke-width="%s" stroke-opacity="%s"/>' % (dome, dfill, line, n(sw), lo))
    if night and H >= 260:
        d = ""
        for f in (-0.5, 0.0, 0.5):
            d += "M%s %s Q%s %s %s %s " % (n(cx + f * g["dome_w"] * 1.9), n(dy), n(cx + f * g["dome_w"] * 1.5),
                                          n(dy - g["dome_h"] * 0.8), n(cx), n(dy - g["dome_h"]))
        p.add('<path d="%s" fill="none" stroke="%s" stroke-width="%s" opacity="0.3"/>' % (d.strip(), BONE, n(sw * 0.5)))
    sy = dy - g["dome_h"]
    p.add('<rect x="%s" y="%s" width="%s" height="%s" fill="%s" stroke="%s" stroke-width="%s" stroke-opacity="%s"/>' % (
        n(cx - g["ball_r"] * 0.5), n(sy - g["stem_h"]), n(g["ball_r"]), n(g["stem_h"]), cfill, line, n(sw * 0.8), lo))
    p.add('<circle cx="%s" cy="%s" r="%s" fill="%s" stroke="%s" stroke-width="%s" stroke-opacity="%s"/>' % (
        n(cx), n(sy - g["stem_h"] - g["ball_r"]), n(g["ball_r"]), cfill, line, n(sw * 0.8), lo))


def rock(p, rng, cx, top, width, height, mode="night", uid="rk", foam=True):
    """A low skerry under the tower. `top` is the platform height (tower foot)."""
    night = mode == "night"
    half = width / 2.0
    pts = []
    m = 22
    for i in range(m + 1):
        u = -1 + 2.0 * i / m
        a = abs(u)
        if a < 0.3:
            f = 1.0
        else:
            f = max(0.0, 1 - ((a - 0.3) / 0.7) ** 1.7) ** 0.8
        y = top + height * (1 - f) + (rng.uniform(-0.05, 0.05) * height if 0.3 <= a < 0.98 else 0)
        pts.append((cx + u * half + (rng.uniform(-5, 5) if 0.3 <= a < 0.98 else 0), y))
    water = top + height
    shape = pts + [(cx + half, water + 3), (cx - half, water + 3)]
    if night:
        p.lin(uid + "f", 0, top, 0, water, [(0, "#1e3345", 1), (1, "#0a131c", 1)])
        fill = "url(#%sf)" % uid
        line, op = BONE, "0.85"
    else:
        fill = "#1b2630"
        line, op = "#1b2630", "1"
    p.add('<path d="%s" fill="%s"/>' % (pts_path(shape, True), fill))
    p.add('<path d="%s" fill="none" stroke="%s" stroke-width="1.4" stroke-linejoin="round" opacity="%s"/>' % (
        pts_path(pts), line, op))
    if night:
        d = ""
        for _ in range(int(width / 16)):
            x = cx + rng.uniform(-half * 0.9, half * 0.9)
            a = abs((x - cx) / half)
            f = 1.0 if a < 0.3 else max(0.0, 1 - ((a - 0.3) / 0.7) ** 1.7) ** 0.8
            ytop = top + height * (1 - f)
            y = rng.uniform(ytop + 5, water - 4) if water - 4 > ytop + 5 else ytop + 4
            ln = rng.uniform(7, 18)
            d += "M%s %s l%s %s " % (n(x), n(y), n(ln * 0.55), n(ln * 0.8))
        p.add('<path d="%s" fill="none" stroke="%s" stroke-width="0.9" stroke-linecap="round" opacity="0.26"/>' % (
            d.strip(), BONE))
    if foam:
        col = BONE if night else "#f1ebdd"
        for k in range(3):
            w = half * (1.04 + 0.1 * k)
            p.add('<path d="%s" fill="none" stroke="%s" stroke-width="%s" stroke-linecap="round" opacity="%s"/>' % (
                wave_d(cx - w, cx + w, water + 2 + 7 * k, 2.6, 30, rng), col, n(1.5 - 0.3 * k),
                "%.2f" % ((0.6 if night else 0.7) - 0.17 * k)))
    return water


# --------------------------------------------------------------------------
# 1. cover.svg
# --------------------------------------------------------------------------

def cover():
    rng = random.Random(1101)
    W, H = 1200, 1600
    p = Plate(W, H, "The Lighthouse Protocol - cover art",
              "A rock lighthouse at night, two amber beams over a fog-banked sea.")
    horizon = 1118
    p.lin("sky", 0, 0, 0, horizon, [(0, "#0a141e", 1), (0.45, NAVY, 1), (0.82, NAVY2, 1), (1, "#22394d", 1)])
    p.add('<rect width="%d" height="%d" fill="url(#sky)"/>' % (W, horizon + 2))
    stars(p, rng, 46, 640, op_scale=0.8)

    g = lh_geom(646, 1248, 576)
    lx, ly = g["lamp"]
    beam(p, "bmL", lx, ly, 193, 206, 640, op=0.52)
    beam(p, "bmR", lx, ly, -25, -11, 580, op=0.48)

    # fog banked along the horizon, behind the tower
    fog_bank(p, rng, 1010, 1120, 16, (220, 520), (26, 60), (0.16, 0.34))
    fog_lines(p, rng, 990, 1110, 16, op=0.17)

    # sea
    p.lin("sea", 0, horizon, 0, H, [(0, "#1d3447", 1), (0.3, NAVY2, 1), (0.7, NAVY, 1), (1, "#0a141e", 1)])
    p.add('<rect y="%d" width="%d" height="%d" fill="url(#sea)"/>' % (horizon, W, H - horizon))
    p.add('<path d="M0 %d h%d" stroke="%s" stroke-width="1" opacity="0.28"/>' % (horizon, W, BONE))
    fog_bank(p, rng, horizon - 6, horizon + 26, 7, (240, 520), (14, 26), (0.1, 0.2))
    swell(p, rng, horizon + 8, 1372, 13, op0=0.3, op1=0.03)

    water = rock(p, rng, 646, 1248, 540, 74, uid="rk")
    reflection(p, rng, 646, water + 12, 1372, 70, AMBER, 0.5, rows=11, grow=0.7)
    lighthouse(p, g, "night", "lh")

    # a low wisp of fog drifting in front of the rock, for depth
    fog_bank(p, rng, 1205, 1262, 5, (200, 380), (16, 30), (0.1, 0.18), gid="fogg")
    vignette(p, 0.6)
    return p.save("cover.svg")


# --------------------------------------------------------------------------
# 2. title-plate.svg
# --------------------------------------------------------------------------

def title_plate():
    rng = random.Random(2202)
    W, H = 1200, 1600
    p = Plate(W, H, "The Lighthouse Protocol - title plate",
              "A distant lighthouse low on the right; one long beam reaches left across bands of fog.")
    horizon = 1124
    p.lin("sky", 0, 0, 0, horizon, [(0, "#0a141e", 1), (0.5, NAVY, 1), (0.85, NAVY2, 1), (1, "#203649", 1)])
    p.add('<rect width="%d" height="%d" fill="url(#sky)"/>' % (W, horizon + 2))
    stars(p, rng, 30, 600, op_scale=0.55)

    g = lh_geom(840, 1184, 352)
    lx, ly = g["lamp"]
    beam(p, "bm", lx, ly, 172.5, 185.5, 900, op=0.55)

    # fog in long bands
    for y, rx, ry, op in [(936, 520, 22, 0.24), (996, 700, 34, 0.28), (1058, 480, 26, 0.3), (1104, 760, 30, 0.34)]:
        fog_bank(p, rng, y - 22, y + 22, 4, (rx * 0.35, rx), (ry * 0.6, ry * 1.3), (op * 0.5, op))
    fog_lines(p, rng, 920, 1112, 22, op=0.18)

    p.lin("sea", 0, horizon, 0, H, [(0, "#1d3447", 1), (0.25, NAVY2, 1), (0.6, NAVY, 1), (1, "#0a141e", 1)])
    p.add('<rect y="%d" width="%d" height="%d" fill="url(#sea)"/>' % (horizon, W, H - horizon))
    p.add('<path d="M0 %d h%d" stroke="%s" stroke-width="1" opacity="0.26"/>' % (horizon, W, BONE))
    fog_bank(p, rng, horizon - 4, horizon + 20, 5, (220, 480), (12, 22), (0.1, 0.2))
    swell(p, rng, horizon + 6, 1300, 15, op0=0.34, op1=0.0)

    water = rock(p, rng, 840, 1184, 380, 50, uid="rk")
    reflection(p, rng, 840, water + 10, 1300, 44, AMBER, 0.5, rows=9, grow=0.6)
    lighthouse(p, g, "night", "lh")
    fog_bank(p, rng, 1150, 1190, 3, (160, 300), (12, 22), (0.1, 0.16), xr=(500, 1200))
    vignette(p, 0.55)
    return p.save("title-plate.svg")


# --------------------------------------------------------------------------
# 3. chart.svg
# --------------------------------------------------------------------------

ISLAND = [(300, 444), (378, 432), (448, 470), (498, 540), (514, 622), (492, 704), (504, 792), (524, 880),
          (510, 968), (474, 1040), (442, 1092), (398, 1150), (336, 1202), (268, 1186), (214, 1128),
          (184, 1040), (158, 962), (178, 906), (150, 846), (160, 768), (198, 700), (212, 618), (228, 530), (254, 472)]
CHART = {
    "tower": (360, 576), "bell": (240, 832), "landing": (432, 1088), "calf": (192, 320),
    "compass": (864, 832), "compass_r": 96,
    "tower_leader_end": (600, 576), "landing_leader_end": (600, 1088),
    "bearing_from": (120, 372),
}


def chart():
    rng = random.Random(3303)
    W, H = 1200, 1600
    p = Plate(W, H, "Chart of Skerry Vane", "A keeper's chart of the rock: lamp tower, bell house, landing and the reef called The Calf.")
    ink = INK
    p.add('<rect width="%d" height="%d" fill="%s"/>' % (W, H, BONE))
    # paper tone
    p.rad("fox", [(0, "#cdbf9f", 0.55), (1, "#cdbf9f", 0)])
    for cx, cy, r, o in [(40, 60, 420, 0.5), (1170, 1560, 480, 0.55), (1150, 80, 300, 0.35), (60, 1540, 320, 0.4)]:
        p.add('<circle cx="%d" cy="%d" r="%d" fill="url(#fox)" opacity="%s"/>' % (cx, cy, r, o))

    shore = sample_closed(ISLAND, 12)
    shore_d = pts_path(shore, True)

    # sea wash, graded away from the shore
    for wdt, op in [(118, 0.13), (76, 0.15), (40, 0.2)]:
        p.add('<path d="%s" fill="none" stroke="%s" stroke-width="%d" stroke-linejoin="round" opacity="%s"/>' % (
            shore_d, FOG, wdt, op))

    # normals (outward) along the shore
    m = len(shore)
    area = sum(shore[i][0] * shore[(i + 1) % m][1] - shore[(i + 1) % m][0] * shore[i][1] for i in range(m))
    sgn = 1 if area > 0 else -1

    def normal(i):
        ax, ay = shore[i - 1]
        bx, by = shore[(i + 1) % m]
        dx, dy = bx - ax, by - ay
        ln = math.hypot(dx, dy) or 1.0
        return (sgn * dy / ln, -sgn * dx / ln)

    # depth contours, dashed
    for off, dash, op in [(34, "9 6", 0.5), (62, "3 7", 0.42)]:
        ring = []
        for i in range(0, m, 3):
            nx, ny = normal(i)
            ring.append((shore[i][0] + nx * off, shore[i][1] + ny * off))
        p.add('<path d="%s" fill="none" stroke="%s" stroke-width="0.9" stroke-dasharray="%s" opacity="%s"/>' % (
            smooth_path(ring, True), ink, dash, op))

    # the rock itself
    p.add('<path d="%s" fill="#efe9da" stroke="%s" stroke-width="2.2" stroke-linejoin="round"/>' % (shore_d, ink))
    # hachures along the shoreline
    d = ""
    acc = 0.0
    k = 0
    for i in range(m):
        ax, ay = shore[i]
        bx, by = shore[(i + 1) % m]
        acc += math.hypot(bx - ax, by - ay)
        if acc >= 8.5:
            acc = 0.0
            nx, ny = normal(i)
            ln = (17 if k % 2 == 0 else 10) + rng.uniform(-2, 2)
            d += "M%s %s l%s %s " % (n(ax - nx * 3), n(ay - ny * 3), n(-nx * ln), n(-ny * ln))
            k += 1
    p.add('<path d="%s" fill="none" stroke="%s" stroke-width="1" stroke-linecap="round" opacity="0.75"/>' % (d.strip(), ink))
    # cliff-top line
    ring = []
    for i in range(0, m, 3):
        nx, ny = normal(i)
        ring.append((shore[i][0] - nx * 26, shore[i][1] - ny * 26))
    p.add('<path d="%s" fill="none" stroke="%s" stroke-width="0.8" opacity="0.5"/>' % (smooth_path(ring, True), ink))

    # form lines round the two high points
    tx, ty = CHART["tower"]
    for (cx, cy, radii, sx, sy) in [(tx - 4, ty + 10, (44, 72, 102), 1.0, 1.15), (300, 960, (40, 70), 1.0, 1.45)]:
        for r in radii:
            ring = []
            for a in range(0, 360, 30):
                rr = r * rng.uniform(0.9, 1.1)
                ring.append((cx + math.cos(math.radians(a)) * rr * sx, cy + math.sin(math.radians(a)) * rr * sy))
            p.add('<path d="%s" fill="none" stroke="%s" stroke-width="0.8" opacity="0.38"/>' % (smooth_path(ring, True), ink))

    # footpath: tower - bell house - landing
    bx, by = CHART["bell"]
    lx, ly = CHART["landing"]
    path = [(tx - 6, ty + 20), (318, 660), (268, 740), (bx + 10, by - 16)]
    p.add('<path d="%s" fill="none" stroke="%s" stroke-width="1.3" stroke-dasharray="2 6" stroke-linecap="round" opacity="0.8"/>' % (
        smooth_path(path), ink))
    path = [(bx + 14, by + 16), (300, 900), (372, 990), (lx - 14, ly - 12)]
    p.add('<path d="%s" fill="none" stroke="%s" stroke-width="1.3" stroke-dasharray="2 6" stroke-linecap="round" opacity="0.8"/>' % (
        smooth_path(path), ink))

    # soundings: dots and little ticks in the sea
    reserved = [(600, 512, 1104, 640), (600, 1024, 1104, 1152)]
    cfx, cfy = CHART["calf"]
    count = 0
    tries = 0
    while count < 150 and tries < 6000:
        tries += 1
        x = rng.uniform(78, 590)
        y = rng.uniform(262, 1262)
        if inside(shore, x, y):
            continue
        dmin = min(math.hypot(x - sx_, y - sy_) for sx_, sy_ in shore[::4])
        if dmin < 16 or dmin > 108:
            continue
        if math.hypot(x - cfx, y - cfy) < 62:
            continue
        if 230 < x < 360 and 286 < y < 340:      # The Calf label
            continue
        if any(a - 8 <= x <= c + 8 and b - 8 <= y <= e + 8 for a, b, c, e in reserved):
            continue
        if abs(y - 576) < 12 and x > 500 or abs(y - 1088) < 12 and x > 440:
            continue
        count += 1
        if count % 3 == 0:
            p.add('<path d="M%s %s h5 M%s %s v5" stroke="%s" stroke-width="0.9" opacity="0.75"/>' % (
                n(x - 2.5), n(y), n(x), n(y - 2.5), ink))
        else:
            p.add('<circle cx="%s" cy="%s" r="%s" fill="%s" opacity="0.75"/>' % (n(x), n(y), rng.choice([1.3, 1.6, 1.9]), ink))

    # a few stacks and drying rocks off the west and south shores
    for (ox, oy, r) in [(118, 902, 7), (104, 880, 4.5), (132, 1010, 5.5), (236, 1232, 6.5), (258, 1244, 4), (470, 1180, 5)]:
        blob = [(ox + math.cos(math.radians(a)) * r * rng.uniform(0.75, 1.2),
                 oy + math.sin(math.radians(a)) * r * rng.uniform(0.75, 1.2)) for a in range(0, 360, 60)]
        p.add('<path d="%s" fill="#efe9da" stroke="%s" stroke-width="1.3"/>' % (smooth_path(blob, True), ink))

    # The Calf: a reef, breaking water
    for (ox, oy, r) in [(-10, -4, 9), (9, 5, 7), (2, -14, 5)]:
        blob = [(cfx + ox + math.cos(math.radians(a)) * r * rng.uniform(0.75, 1.2),
                 cfy + oy + math.sin(math.radians(a)) * r * rng.uniform(0.75, 1.2)) for a in range(0, 360, 60)]
        p.add('<path d="%s" fill="%s" opacity="0.85"/>' % (smooth_path(blob, True), ink))
    for a in range(0, 360, 40):
        r = 27 + (5 if (a // 40) % 2 else 0)
        x = cfx + math.cos(math.radians(a)) * r
        y = cfy + math.sin(math.radians(a)) * r
        p.add('<path d="M%s %s q4 -5 8 0 q4 5 8 0" fill="none" stroke="%s" stroke-width="1" stroke-linecap="round" opacity="0.8"/>' % (
            n(x - 8), n(y), ink))
    for (ox, oy) in [(-30, 20), (26, -26), (30, 22)]:
        p.add('<path d="M%s %s h8 M%s %s v8" stroke="%s" stroke-width="1.1"/>' % (
            n(cfx + ox - 4), n(cfy + oy), n(cfx + ox), n(cfy + oy - 4), ink))
    p.add('<circle cx="%d" cy="%d" r="48" fill="none" stroke="%s" stroke-width="1" stroke-dasharray="1.5 5" stroke-linecap="round"/>' % (
        cfx, cfy, ink))

    # bearing line from the north-west, in amber, with a lamp-flash glyph at its seaward end
    fx, fy = CHART["bearing_from"]
    p.add('<path d="M%s %s L%s %s" stroke="#c9862a" stroke-width="2" stroke-dasharray="12 8" stroke-linecap="round"/>' % (
        n(fx + 16), n(fy + 13.6), n(tx - 15), n(ty - 12.8)))
    d = ""
    for a in range(0, 360, 45):
        ca, sa = math.cos(math.radians(a)), math.sin(math.radians(a))
        r0, r1 = 8.5, (15 if a % 90 == 0 else 12)
        d += "M%s %s L%s %s " % (n(fx + ca * r0), n(fy + sa * r0), n(fx + ca * r1), n(fy + sa * r1))
    p.add('<path d="%s" stroke="#c9862a" stroke-width="1.6" stroke-linecap="round"/>' % d.strip())
    p.add('<circle cx="%d" cy="%d" r="5" fill="%s" stroke="#c9862a" stroke-width="1.4"/>' % (fx, fy, AMBER))

    # leaders to the two button frames
    for (sx_, sy_), (ex, ey) in [((tx + 16, ty), CHART["tower_leader_end"]), ((lx + 16, ly), CHART["landing_leader_end"])]:
        p.add('<path d="M%s %s L%s %s" stroke="%s" stroke-width="1"/>' % (n(sx_), n(sy_), n(ex - 4), n(ey), ink))
        p.add('<circle cx="%s" cy="%s" r="4" fill="%s"/>' % (n(ex), n(ey), ink))

    # lamp tower symbol
    p.add('<circle cx="%d" cy="%d" r="19" fill="%s" opacity="0.5"/>' % (tx, ty, AMBER))
    d = ""
    for a in range(0, 360, 30):
        ca, sa = math.cos(math.radians(a)), math.sin(math.radians(a))
        d += "M%s %s L%s %s " % (n(tx + ca * 13), n(ty + sa * 13), n(tx + ca * 18), n(ty + sa * 18))
    p.add('<path d="%s" stroke="%s" stroke-width="1.1"/>' % (d.strip(), ink))
    p.add('<circle cx="%d" cy="%d" r="10" fill="%s" stroke="%s" stroke-width="1.8"/>' % (tx, ty, BONE, ink))
    p.add('<circle cx="%d" cy="%d" r="3.6" fill="%s"/>' % (tx, ty, ink))

    # bell house symbol: a small roofed square with a bell
    p.add('<path d="M%d %d h22 v18 h-22 Z" fill="%s" stroke="%s" stroke-width="1.6" stroke-linejoin="round"/>' % (
        bx - 11, by - 7, BONE, ink))
    p.add('<path d="M%d %d l15 -11 l15 11" fill="none" stroke="%s" stroke-width="1.6" stroke-linejoin="round" stroke-linecap="round"/>' % (
        bx - 15, by - 7, ink))
    p.add('<path d="M%s %s c0 -7 7 -7 7 0 l2 4 h-11 Z" fill="%s"/>' % (n(bx - 3.5), n(by + 3), ink))

    # landing: a slip running out to the south-east, with steps
    p.add('<path d="M%d %d l56 44 M%d %d l56 44" stroke="%s" stroke-width="1.6" stroke-linecap="round"/>' % (
        lx - 2, ly + 8, lx - 10, ly + 18, ink))
    d = ""
    for k in range(1, 7):
        d += "M%s %s l-8 10 " % (n(lx - 2 + 8 * k), n(ly + 8 + 6.3 * k))
    p.add('<path d="%s" stroke="%s" stroke-width="1"/>' % (d.strip(), ink))
    p.add('<circle cx="%d" cy="%d" r="6" fill="%s" stroke="%s" stroke-width="1.8"/>' % (lx, ly, BONE, ink))
    p.add('<circle cx="%d" cy="%d" r="2.2" fill="%s"/>' % (lx, ly, ink))

    # labels
    def label(x, y, s, size=25, anchor="start"):
        p.add('<text x="%d" y="%d" font-family="serif" font-style="italic" font-size="%d" fill="%s" text-anchor="%s">%s</text>' % (
            x, y, size, ink, anchor, s))
    label(tx + 26, ty - 20, "Lamp tower")
    label(bx + 26, by + 9, "Bell house")
    label(lx + 22, ly - 18, "Landing")
    label(cfx + 58, cfy + 2, "The Calf")

    # compass rose
    cx, cy = CHART["compass"]
    R = CHART["compass_r"]
    p.add('<circle cx="%d" cy="%d" r="%s" fill="none" stroke="%s" stroke-width="1"/>' % (cx, cy, n(R * 0.7), ink))
    p.add('<circle cx="%d" cy="%d" r="%s" fill="none" stroke="%s" stroke-width="0.7"/>' % (cx, cy, n(R * 0.6), ink))
    d = ""
    for a in range(0, 360, 10):
        ca, sa = math.cos(math.radians(a)), math.sin(math.radians(a))
        r0 = R * 0.6
        r1 = R * (0.7 if a % 30 else 0.75)
        d += "M%s %s L%s %s " % (n(cx + ca * r0), n(cy + sa * r0), n(cx + ca * r1), n(cy + sa * r1))
    p.add('<path d="%s" stroke="%s" stroke-width="0.8"/>' % (d.strip(), ink))

    def point(angle, length, half):
        a = math.radians(angle - 90)
        tipx, tipy = cx + math.cos(a) * length, cy + math.sin(a) * length
        la, ra = a - math.pi / 2, a + math.pi / 2
        l = (cx + math.cos(la) * half, cy + math.sin(la) * half)
        r = (cx + math.cos(ra) * half, cy + math.sin(ra) * half)
        p.add('<path d="%s" fill="%s" stroke="%s" stroke-width="0.9" stroke-linejoin="round"/>' % (
            pts_path([(cx, cy), l, (tipx, tipy)], True), ink, ink))
        p.add('<path d="%s" fill="%s" stroke="%s" stroke-width="0.9" stroke-linejoin="round"/>' % (
            pts_path([(cx, cy), r, (tipx, tipy)], True), BONE, ink))
    for a in (45, 135, 225, 315):
        point(a, R * 0.56, R * 0.085)
    for a in (90, 180, 270):
        point(a, R * 0.92, R * 0.11)
    point(0, R, R * 0.11)
    p.add('<circle cx="%d" cy="%d" r="3.4" fill="%s" stroke="%s" stroke-width="1"/>' % (cx, cy, BONE, ink))
    p.add('<text x="%d" y="%d" font-family="serif" font-size="30" fill="%s" text-anchor="middle">N</text>' % (
        cx, cy - R - 10, ink))

    # scale bar (no lettering)
    sx0, sy0, seg = 664, 1214, 78
    for k in range(4):
        p.add('<rect x="%d" y="%d" width="%d" height="7" fill="%s" stroke="%s" stroke-width="1"/>' % (
            sx0 + seg * k, sy0, seg, ink if k % 2 == 0 else BONE, ink))
    d = ""
    for k in range(5):
        d += "M%d %d v-9 " % (sx0 + seg * k, sy0)
    p.add('<path d="%s" stroke="%s" stroke-width="1"/>' % (d.strip(), ink))

    # double keyline border with graduations
    p.add('<rect x="28" y="28" width="%d" height="%d" fill="none" stroke="%s" stroke-width="2.6"/>' % (W - 56, H - 56, ink))
    p.add('<rect x="38" y="38" width="%d" height="%d" fill="none" stroke="%s" stroke-width="0.9"/>' % (W - 76, H - 76, ink))
    d = ""
    for x in range(78, W - 40, 40):
        ln = 9 if (x - 78) % 200 == 0 else 5
        d += "M%d 38 v%d M%d %d v-%d " % (x, ln, x, H - 38, ln)
    for y in range(78, H - 40, 40):
        ln = 9 if (y - 78) % 200 == 0 else 5
        d += "M38 %d h%d M%d %d h-%d " % (y, ln, W - 38, y, ln)
    p.add('<path d="%s" stroke="%s" stroke-width="0.8" opacity="0.8"/>' % (d.strip(), ink))
    return p.save("chart.svg")


# --------------------------------------------------------------------------
# 4. lens.svg
# --------------------------------------------------------------------------

def lens():
    rng = random.Random(4404)
    W, H = 1200, 1500
    p = Plate(W, H, "The lens", "Inside the lantern room: a Fresnel lens glowing amber, the oil reservoir and its hand pump on the pedestal.")
    cx, cy = 600, 640
    gz0, gz1 = 118, 1012          # glazing zone
    p.add('<rect width="%d" height="%d" fill="%s"/>' % (W, H, NAVY))
    p.lin("night", 0, gz0, 0, gz1, [(0, "#0c1722", 1), (0.6, "#15273a", 1), (1, "#1c3246", 1)])
    p.add('<rect y="%d" width="%d" height="%d" fill="url(#night)"/>' % (gz0, W, gz1 - gz0))
    fog_bank(p, rng, 150, 400, 6, (200, 460), (40, 80), (0.08, 0.18))
    fog_bank(p, rng, 860, 1000, 8, (220, 480), (36, 70), (0.14, 0.28))
    fog_lines(p, rng, 160, 980, 14, op=0.12)

    # beams leaving through the glass
    for gid, x2, sgn in (("bmL", 0, -1), ("bmR", W, 1)):
        p.lin(gid, cx, 0, x2, 0, [(0, "#ffd998", 0.55), (0.5, AMBER, 0.3), (1, AMBER, 0.1)])
        p.add('<path d="%s" fill="url(#%s)" filter="url(#%s)"/>' % (
            pts_path([(cx + sgn * 150, cy - 108), (x2 + sgn * 40, cy - 190), (x2 + sgn * 40, cy + 190), (cx + sgn * 150, cy + 108)], True),
            gid, p.blur("soft", 9)))

    # the diagonal astragals of the lantern glazing
    d = ""
    slope = 1.32
    span = (gz1 - gz0) / slope
    for k in range(-6, 9):
        x0 = k * 172.0 + 40
        d += "M%s %s L%s %s M%s %s L%s %s " % (n(x0), n(gz1), n(x0 + span), n(gz0), n(x0 + span), n(gz1), n(x0), n(gz0))
    p.add('<clipPath id="glz"><rect x="0" y="%d" width="%d" height="%d"/></clipPath>' % (gz0, W, gz1 - gz0))
    p.add('<g clip-path="url(#glz)"><path d="%s" fill="none" stroke="%s" stroke-width="7" opacity="0.5"/>'
          '<path d="%s" fill="none" stroke="%s" stroke-width="2" opacity="0.5"/></g>' % (d.strip(), "#0a121b", d.strip(), BONE))

    # roof above the glazing: rafters converging on the cowl
    p.add('<rect width="%d" height="%d" fill="#0a131c"/>' % (W, gz0))
    d = ""
    for k in range(-7, 8):
        d += "M%s %s L%s 0 " % (n(cx + k * 96), n(gz0), n(cx + k * 52))
    p.add('<path d="%s" fill="none" stroke="%s" stroke-width="1.2" opacity="0.28"/>' % (d.strip(), BONE))
    p.add('<path d="M0 %d h%d M0 %d h%d" stroke="%s" stroke-width="2.2" opacity="0.7"/>' % (gz0, W, gz0 - 14, W, BONE))

    # murette below the glazing and the floor
    floor = 1338
    p.lin("wall", 0, gz1, 0, floor, [(0, "#1b3042", 1), (1, "#0f1c29", 1)])
    p.add('<rect y="%d" width="%d" height="%d" fill="url(#wall)"/>' % (gz1, W, floor - gz1))
    p.add('<path d="M0 %d h%d M0 %d h%d" stroke="%s" stroke-width="2.2" opacity="0.7"/>' % (gz1, W, gz1 + 16, W, BONE))
    d = ""
    for x in range(60, W, 180):
        d += "M%d %d v%d " % (x, gz1 + 16, floor - gz1 - 16)
    p.add('<path d="%s" stroke="%s" stroke-width="1.1" opacity="0.25"/>' % (d.strip(), BONE))
    # rivets along the sill
    d = ""
    for x in range(24, W, 36):
        d += '<circle cx="%d" cy="%d" r="1.8"/>' % (x, gz1 + 8)
    p.add('<g fill="%s" opacity="0.5">%s</g>' % (BONE, d))
    p.lin("floor", 0, floor, 0, H, [(0, "#16283a", 1), (1, "#0a121b", 1)])
    p.add('<rect y="%d" width="%d" height="%d" fill="url(#floor)"/>' % (floor, W, H - floor))
    p.add('<path d="M0 %d h%d" stroke="%s" stroke-width="1.6" opacity="0.6"/>' % (floor, W, BONE))
    d = ""
    for k in range(-14, 15):
        d += "M%s %d L%s %d " % (n(cx + k * 70), floor, n(cx + k * 150), H)
    for y in (1372, 1414, 1464):
        d += "M0 %d h%d " % (y, W)
    p.add('<path d="%s" fill="none" stroke="%s" stroke-width="1" opacity="0.2"/>' % (d.strip(), BONE))

    # glow of the lamp on the room
    p.rad("glow", [(0, "#ffe2a6", 0.62), (0.3, AMBER, 0.3), (0.7, AMBER, 0.08), (1, AMBER, 0)], cx, cy, 620)
    p.add('<circle cx="%d" cy="%d" r="620" fill="url(#glow)"/>' % (cx, cy))

    # ---- pedestal ----
    ped_top, ped_bot = 982, 1300
    p.lin("ped", cx - 170, 0, cx + 170, 0, [(0, "#35506a", 1), (0.45, "#22384c", 1), (1, "#0f1c28", 1)])
    p.add('<path d="%s" fill="url(#ped)" stroke="%s" stroke-width="2" stroke-linejoin="round"/>' % (
        pts_path([(cx - 112, ped_top), (cx + 112, ped_top), (cx + 138, ped_bot), (cx - 138, ped_bot)], True), BONE))
    p.add('<rect x="%d" y="%d" width="%d" height="38" fill="url(#ped)" stroke="%s" stroke-width="2"/>' % (
        cx - 184, ped_bot, 368, BONE))
    # access door with hinges
    p.add('<rect x="%d" y="1060" width="110" height="170" rx="6" fill="none" stroke="%s" stroke-width="1.5" opacity="0.8"/>' % (cx - 55, BONE))
    p.add('<circle cx="%d" cy="1146" r="5" fill="none" stroke="%s" stroke-width="1.5" opacity="0.8"/>' % (cx + 38, BONE))
    p.add('<path d="M%d 1082 h14 M%d 1208 h14" stroke="%s" stroke-width="3" stroke-linecap="round" opacity="0.7"/>' % (cx - 62, cx - 62, BONE))
    d = ""
    for k in range(9):
        d += '<circle cx="%s" cy="%d" r="2.2"/>' % (n(cx - 160 + 40 * k), ped_bot + 19)
    p.add('<g fill="%s" opacity="0.6">%s</g>' % (BONE, d))
    # the table carrying the lens
    p.add('<path d="%s" fill="url(#ped)" stroke="%s" stroke-width="2" stroke-linejoin="round"/>' % (
        pts_path([(cx - 232, 944), (cx + 232, 944), (cx + 232, 962), (cx + 128, ped_top), (cx - 128, ped_top), (cx - 232, 962)], True), BONE))
    d = ""
    for k in range(13):
        d += '<circle cx="%s" cy="953" r="2.4"/>' % n(cx - 216 + 36 * k)
    p.add('<g fill="%s" opacity="0.7">%s</g>' % (BONE, d))

    # ---- the lens ----
    def hw(y):
        if y < 520:
            s = (520 - y) / 272.0
            return 250 * math.sqrt(max(0.0, 1 - (0.9 * s) ** 2))
        if y > 770:
            s = (y - 770) / 172.0
            return 250 * math.sqrt(max(0.0, 1 - (0.6 * s) ** 2))
        return 250.0

    def sag(y):
        return (y - cy) * 0.17 * (hw(y) / 250.0)

    top_y, bot_y = 248, 942
    outline = []
    ys = [top_y + (bot_y - top_y) * i / 40.0 for i in range(41)]
    for y in ys:
        outline.append((cx - hw(y), y))
    for y in reversed(ys):
        outline.append((cx + hw(y), y))
    p.rad("glass", [(0, "#fff4d6", 0.96), (0.16, "#ffd98f", 0.82), (0.42, AMBER, 0.5), (0.75, "#b9a27a", 0.26), (1, FOG, 0.2)],
          cx, cy, 400)
    p.add('<path d="%s" fill="#0d1924" opacity="0.55"/>' % pts_path(outline, True))
    p.add('<path d="%s" fill="url(#glass)"/>' % pts_path(outline, True))

    def ring(y0, y1, inner=True):
        w0, w1 = hw(y0), hw(y1)
        d0 = "M%s %s Q%s %s %s %s L%s %s Q%s %s %s %s Z" % (
            n(cx - w1), n(y1), n(cx), n(y1 + 2 * sag(y1)), n(cx + w1), n(y1),
            n(cx + w0), n(y0), n(cx), n(y0 + 2 * sag(y0)), n(cx - w0), n(y0))
        p.add('<path d="%s" fill="%s" fill-opacity="0.04" stroke="%s" stroke-width="1.4" stroke-linejoin="round" opacity="0.72"/>' % (
            d0, BONE, BONE))
        if inner:
            ym = y0 + (y1 - y0) * 0.42
            wm = hw(ym) - 6
            p.add('<path d="M%s %s Q%s %s %s %s" fill="none" stroke="%s" stroke-width="0.9" opacity="0.45"/>' % (
                n(cx - wm), n(ym), n(cx), n(ym + 2 * sag(ym)), n(cx + wm), n(ym), BONE))

    # upper prism rings
    k = 9
    span_u = (508 - 258) / float(k)
    for i in range(k):
        y0 = 258 + span_u * i
        ring(y0 + 3, y0 + span_u - 3)
    # lower prism rings
    k = 6
    span_l = (934 - 782) / float(k)
    for i in range(k):
        y0 = 782 + span_l * i
        ring(y0 + 3, y0 + span_l - 3)

    # central drum: bullseye panels
    for r in (22, 40, 58, 76, 94, 110):
        p.add('<circle cx="%d" cy="%d" r="%d" fill="none" stroke="%s" stroke-width="%s" opacity="0.9"/>' % (
            cx, cy, r, BONE, "1.8" if r in (22, 110) else "1.3"))
        for sgn in (-1, 1):
            p.add('<ellipse cx="%d" cy="%d" rx="%s" ry="%d" fill="none" stroke="%s" stroke-width="1.2" opacity="0.7"/>' % (
                cx + sgn * 190, cy, n(r * 0.46), r, BONE))
    # the flame seen through the bullseye
    p.rad("flame", [(0, "#ffffff", 1), (0.5, "#fff0c4", 0.95), (1, "#ffd27a", 0)], cx, cy, 36)
    p.add('<circle cx="%d" cy="%d" r="36" fill="url(#flame)"/>' % (cx, cy))
    p.add('<path d="M%d %d c-9 -10 -8 -22 0 -36 c8 14 9 26 0 36 Z" fill="#ffffff" opacity="0.95"/>' % (cx, cy + 14))

    # frame: horizontal bands and meridian ribs
    for y in (520, 770):
        s2 = 2 * sag(y)
        p.add('<path d="M%d %d Q%d %s %d %d L%d %d Q%d %s %d %d Z" fill="#1d3246" stroke="%s" stroke-width="1.8" stroke-linejoin="round"/>' % (
            cx - 254, y - 9, cx, n(y - 9 + s2), cx + 254, y - 9, cx + 254, y + 9, cx, n(y + 9 + s2), cx - 254, y + 9, BONE))
        d = ""
        for k in range(-6, 7):
            u = k / 6.5
            d += '<circle cx="%s" cy="%s" r="2.3"/>' % (n(cx + u * 250), n(y + s2 * 0.5 * (1 - u * u)))
        p.add('<g fill="%s" opacity="0.8">%s</g>' % (BONE, d))
    for u in (-1.0, -0.5, 0.5, 1.0):
        rib = [(cx + u * hw(y), y + (sag(y) * (1 - u * u))) for y in ys]
        p.add('<path d="%s" fill="none" stroke="#0d1924" stroke-width="7" stroke-linejoin="round" opacity="0.85"/>' % pts_path(rib))
        p.add('<path d="%s" fill="none" stroke="%s" stroke-width="2.4" stroke-linejoin="round"/>' % (pts_path(rib), BONE))
    # crown piece and flue
    p.add('<path d="M%d 250 q0 -34 %d -44 h%d q%d 10 %d 44 Z" fill="#1d3246" stroke="%s" stroke-width="2" stroke-linejoin="round"/>' % (
        cx - 110, 60, 100, 60, 60, BONE))
    p.add('<rect x="%d" y="%d" width="44" height="%d" fill="#16283a" stroke="%s" stroke-width="1.8"/>' % (cx - 22, gz0 - 14, 206 - gz0 + 14, BONE))
    p.add('<path d="M%d 176 h44 M%d 150 h44" stroke="%s" stroke-width="1" opacity="0.5"/>' % (cx - 22, cx - 22, BONE))

    # ---- oil reservoir with gauge (right) ----
    rx0, rx1, ry0, ry1 = 836, 968, 1056, 1220
    rcx = (rx0 + rx1) / 2.0
    # shelf bracket from the pedestal
    p.add('<path d="M%d 1232 H%d v14 H%d Z M%d 1246 L%d 1296" fill="#1d3246" stroke="%s" stroke-width="1.8" stroke-linejoin="round"/>' % (
        cx + 128, rx1 + 22, cx + 131, rx1 - 10, cx + 138, BONE))
    p.lin("tank", rx0, 0, rx1, 0, [(0, "#3a566e", 1), (0.5, "#253c50", 1), (1, "#101d2a", 1)])
    p.add('<path d="M%d %d v%d a%s 15 0 0 0 %d 0 v%d Z" fill="url(#tank)" stroke="%s" stroke-width="2"/>' % (
        rx0, ry0, ry1 - ry0, n((rx1 - rx0) / 2.0), rx1 - rx0, -(ry1 - ry0), BONE))
    p.add('<ellipse cx="%s" cy="%d" rx="%s" ry="15" fill="#2c465c" stroke="%s" stroke-width="2"/>' % (n(rcx), ry0, n((rx1 - rx0) / 2.0), BONE))
    for y in (ry0 + 30, ry1 - 26):
        p.add('<path d="M%d %d a%s 15 0 0 0 %d 0" fill="none" stroke="%s" stroke-width="1.2" opacity="0.6"/>' % (
            rx0, y, n((rx1 - rx0) / 2.0), rx1 - rx0, BONE))
    # filler cap
    p.add('<rect x="%s" y="%d" width="30" height="16" rx="3" fill="#22384c" stroke="%s" stroke-width="1.6"/>' % (n(rcx + 14), ry0 - 24, BONE))
    # gauge
    gx, gy = rcx - 8, 1142
    p.add('<circle cx="%s" cy="%d" r="34" fill="#0d1924" stroke="%s" stroke-width="2"/>' % (n(gx), gy, BONE))
    p.add('<circle cx="%s" cy="%d" r="27" fill="%s" opacity="0.14"/>' % (n(gx), gy, AMBER))
    d = ""
    for a in range(200, 341, 20):
        ca, sa = math.cos(math.radians(a)), math.sin(math.radians(a))
        d += "M%s %s L%s %s " % (n(gx + ca * 21), n(gy + sa * 21), n(gx + ca * 27), n(gy + sa * 27))
    p.add('<path d="%s" stroke="%s" stroke-width="1.3"/>' % (d.strip(), BONE))
    p.add('<path d="M%s %d L%s %s" stroke="%s" stroke-width="2.2" stroke-linecap="round"/>' % (
        n(gx), gy, n(gx + math.cos(math.radians(222)) * 23), n(gy + math.sin(math.radians(222)) * 23), AMBER))
    p.add('<circle cx="%s" cy="%d" r="3.2" fill="%s"/>' % (n(gx), gy, BONE))
    # feed pipe up to the burner
    p.add('<path d="M%s %d V1010 Q%s 990 %d 990 H%d" fill="none" stroke="#0d1924" stroke-width="9" stroke-linecap="round"/>' % (
        n(rcx - 30), ry0 - 10, n(rcx - 30), int(rcx - 52), cx + 150))
    p.add('<path d="M%s %d V1010 Q%s 990 %d 990 H%d" fill="none" stroke="%s" stroke-width="3.4" stroke-linecap="round"/>' % (
        n(rcx - 30), ry0 - 10, n(rcx - 30), int(rcx - 52), cx + 150, BONE))
    p.add('<rect x="%s" y="1022" width="22" height="12" fill="#22384c" stroke="%s" stroke-width="1.4"/>' % (n(rcx - 41), BONE))

    # ---- hand pump (left) ----
    px = 318
    p.add('<path d="M%d 1232 H%d v14 H%d Z M%d 1246 L%d 1296" fill="#1d3246" stroke="%s" stroke-width="1.8" stroke-linejoin="round"/>' % (
        cx - 128, px - 70, cx - 131, px - 40, cx - 138, BONE))
    p.lin("pump", px - 30, 0, px + 30, 0, [(0, "#3a566e", 1), (1, "#132231", 1)])
    p.add('<rect x="%d" y="1092" width="60" height="140" rx="5" fill="url(#pump)" stroke="%s" stroke-width="2"/>' % (px - 30, BONE))
    p.add('<path d="M%d 1118 h60 M%d 1206 h60" stroke="%s" stroke-width="1.2" opacity="0.6"/>' % (px - 30, px - 30, BONE))
    p.add('<rect x="%d" y="1074" width="76" height="18" rx="3" fill="#22384c" stroke="%s" stroke-width="1.8"/>' % (px - 38, BONE))
    # piston rod, fulcrum post and lever
    p.add('<path d="M%d 1074 V1024" stroke="%s" stroke-width="5" stroke-linecap="round"/>' % (px - 14, BONE))
    p.add('<path d="M%d 1074 V1000" stroke="%s" stroke-width="6" stroke-linecap="round" opacity="0.9"/>' % (px + 28, BONE))
    p.add('<path d="M%d 1000 L%d 1058" stroke="#0d1924" stroke-width="13" stroke-linecap="round"/>' % (px + 38, px - 132))
    p.add('<path d="M%d 1000 L%d 1058" stroke="%s" stroke-width="7" stroke-linecap="round"/>' % (px + 38, px - 132, BONE))
    p.add('<circle cx="%d" cy="1003" r="7" fill="#0d1924" stroke="%s" stroke-width="2"/>' % (px + 28, BONE))
    p.add('<circle cx="%d" cy="1021" r="5.5" fill="#0d1924" stroke="%s" stroke-width="2"/>' % (px - 14, BONE))
    p.add('<path d="M%d 1049 L%d 1063" stroke="%s" stroke-width="19" stroke-linecap="round"/>' % (px - 108, px - 156, BONE))
    p.add('<path d="M%d 1049 L%d 1063" stroke="#22384c" stroke-width="14" stroke-linecap="round"/>' % (px - 108, px - 156))
    # delivery pipe into the pedestal
    p.add('<path d="M%d 1180 H%d" stroke="#0d1924" stroke-width="9"/>' % (px + 30, cx - 126))
    p.add('<path d="M%d 1180 H%d" stroke="%s" stroke-width="3.4"/>' % (px + 30, cx - 126, BONE))

    vignette(p, 0.6)
    return p.save("lens.svg")


# --------------------------------------------------------------------------
# 5. bell.svg
# --------------------------------------------------------------------------

def gear_d(cx, cy, r_out, r_in, teeth):
    pts = []
    for i in range(teeth):
        a0 = 2 * math.pi * i / teeth
        w = 2 * math.pi / teeth
        for frac, r in ((0.0, r_in), (0.18, r_out), (0.5, r_out), (0.68, r_in)):
            a = a0 + w * frac
            pts.append((cx + math.cos(a) * r, cy + math.sin(a) * r))
    return pts_path(pts, True)


def bell():
    rng = random.Random(5505)
    W, H = 1200, 1500
    p = Plate(W, H, "The fog bell", "The fog bell in its timber frame at the door of the bell house, with its clockwork striker; the lit tower beyond.")
    horizon = 704
    ground = 1176
    p.lin("sky", 0, 0, 0, horizon, [(0, "#0a141e", 1), (0.55, NAVY, 1), (1, "#1f3548", 1)])
    p.add('<rect width="%d" height="%d" fill="url(#sky)"/>' % (W, horizon + 2))
    stars(p, rng, 22, 330, x_min=0, x_max=640, op_scale=0.7)

    # the lamp tower, small, on the far head of the rock
    g = lh_geom(176, 640, 262)
    lx, ly = g["lamp"]
    beam(p, "bm", lx, ly, -15, -3, 900, op=0.5)
    fog_bank(p, rng, 610, 700, 7, (200, 460), (20, 40), (0.14, 0.26), xr=(-100, 760))

    # open sea beyond, and the gut of water between the head and the bell house
    p.lin("sea", 0, horizon, 0, ground + 40, [(0, "#213a4e", 1), (0.3, NAVY2, 1), (1, NAVY, 1)])
    p.add('<rect y="%d" width="%d" height="%d" fill="url(#sea)"/>' % (horizon, W, ground + 40 - horizon))
    p.add('<path d="M0 %d h%d" stroke="%s" stroke-width="1" opacity="0.28"/>' % (horizon, W, BONE))
    swell(p, rng, horizon + 5, 742, 4, op0=0.3, op1=0.22, x0=340, x1=640)
    swell(p, rng, horizon + 5, 900, 6, op0=0.3, op1=0.2, x0=1100, x1=1200)

    # the head that carries the tower, falling to a low neck of rock on the right
    top = [(0, 662), (88, 645), (128, 640), (236, 640), (294, 652), (348, 684), (408, 724), (468, 742),
           (540, 747), (612, 740)]
    foot = [(612, 800), (520, 807), (400, 799), (280, 809), (150, 801), (0, 807)]
    p.lin("rise", 0, 640, 0, 806, [(0, "#1c3144", 1), (1, "#0b151f", 1)])
    p.add('<path d="%s" fill="url(#rise)"/>' % pts_path(top + foot, True))
    p.add('<path d="%s" fill="none" stroke="%s" stroke-width="1.4" stroke-linejoin="round" opacity="0.8"/>' % (pts_path(top), BONE))
    d = ""
    for _ in range(40):
        x = rng.uniform(10, 590)
        ytop = 640
        for (xa, ya), (xb, yb) in zip(top, top[1:]):
            if xa <= x <= xb:
                ytop = ya + (yb - ya) * (x - xa) / (xb - xa)
        if 796 - ytop < 24:
            continue
        y = rng.uniform(ytop + 10, 790)
        ln = rng.uniform(8, 20)
        d += "M%s %s l%s %s " % (n(x), n(y), n(ln * 0.5), n(ln * 0.85))
    p.add('<path d="%s" fill="none" stroke="%s" stroke-width="0.9" stroke-linecap="round" opacity="0.24"/>' % (d.strip(), BONE))
    for k in range(3):
        p.add('<path d="%s" fill="none" stroke="%s" stroke-width="%s" stroke-linecap="round" opacity="%s"/>' % (
            wave_d(-10, 606, 806 + 7 * k, 2.6, 30, rng), BONE, n(1.4 - 0.3 * k), "%.2f" % (0.55 - 0.16 * k)))
    lighthouse(p, g, "night", "lh")
    reflection(p, rng, 176, 832, 1010, 34, AMBER, 0.42, rows=11, grow=0.7)
    swell(p, rng, 836, ground - 30, 12, op0=0.26, op1=0.16, x1=620)
    fog_bank(p, rng, 760, 900, 6, (180, 380), (20, 40), (0.12, 0.24), xr=(-100, 600))
    fog_lines(p, rng, 700, 900, 9, op=0.15, xr=(0, 600))

    # ---- the bell house ----
    wx0, wx1, eave = 600, 1124, 500
    apex = (862, 262)
    wall = [(wx0, ground), (wx0, eave), apex, (wx1, eave), (wx1, ground)]
    p.lin("wall", wx0, 0, wx1, 0, [(0, "#2b4459", 1), (0.5, "#1e3345", 1), (1, "#101d2a", 1)])
    p.add('<path d="%s" fill="url(#wall)" stroke="%s" stroke-width="2" stroke-linejoin="round"/>' % (pts_path(wall, True), BONE))
    # coursed masonry
    dcx, dw, spring = 862, 236, 690      # door
    d = ""
    row = 0
    y = ground - 46
    while y > apex[1] + 30:
        if y >= eave:
            xa, xb = wx0, wx1
        else:
            f = (y - apex[1]) / float(eave - apex[1])
            xa, xb = apex[0] - (apex[0] - wx0) * f, apex[0] + (wx1 - apex[0]) * f
        d += "M%s %s H%s " % (n(xa + 3), n(y + rng.uniform(-1.2, 1.2)), n(xb - 3))
        x = xa + (30 if row % 2 else 68) + rng.uniform(-6, 6)
        while x < xb - 20:
            d += "M%s %s v46 " % (n(x), n(y))
            x += rng.uniform(70, 96)
        y -= 46
        row += 1
    p.add('<path d="%s" fill="none" stroke="%s" stroke-width="1" opacity="0.24"/>' % (d.strip(), BONE))
    # roof verges
    verge = [(wx0 - 52, eave + 24), (apex[0], apex[1] - 26), (wx1 + 52, eave + 24),
             (wx1 + 40, eave + 44), (apex[0], apex[1] + 4), (wx0 - 40, eave + 44)]
    p.add('<path d="%s" fill="#0c1620" stroke="%s" stroke-width="1.8" stroke-linejoin="round"/>' % (pts_path(verge, True), BONE))
    # oculus in the gable
    p.add('<circle cx="%d" cy="408" r="34" fill="#0a121b" stroke="%s" stroke-width="2"/>' % (apex[0], BONE))
    p.add('<path d="M%d 408 h68 M%d 374 v68" stroke="%s" stroke-width="1.3" opacity="0.7"/>' % (apex[0] - 34, apex[0], BONE))
    p.add('<circle cx="%d" cy="408" r="44" fill="none" stroke="%s" stroke-width="1" opacity="0.4"/>' % (apex[0], BONE))
    # the door: an arched opening with warm light far inside
    door = "M%s %d V%d A%s %s 0 0 1 %s %d V%d Z" % (n(dcx - dw / 2.0), ground, spring, n(dw / 2.0), n(dw / 2.0),
                                                  n(dcx + dw / 2.0), spring, ground)
    p.add('<path d="%s" fill="#070d14"/>' % door)
    p.rad("inner", [(0, "#ffd998", 0.75), (0.35, AMBER, 0.32), (1, AMBER, 0)], dcx + 46, 880, 210)
    p.add('<clipPath id="doorclip"><path d="%s"/></clipPath>' % door)
    p.add('<g clip-path="url(#doorclip)"><circle cx="%d" cy="880" r="210" fill="url(#inner)"/>'
          '<path d="M%s 1104 H%s M%s 1140 H%s" stroke="%s" stroke-width="1" opacity="0.22"/></g>' % (
              dcx + 46, n(dcx - dw / 2.0), n(dcx + dw / 2.0), n(dcx - dw / 2.0), n(dcx + dw / 2.0), AMBER))
    # hanging lantern inside
    p.add('<path d="M%d %d V846" stroke="%s" stroke-width="1.2" opacity="0.7"/>' % (dcx + 46, spring - 100, BONE))
    p.add('<path d="M%d 846 h20 l5 12 v30 h-30 v-30 Z" fill="%s" stroke="%s" stroke-width="1.5" stroke-linejoin="round"/>' % (
        dcx + 36, "#ffd998", BONE))
    p.add('<path d="M%d 858 h30 M%d 888 h30" stroke="%s" stroke-width="2.4"/>' % (dcx + 31, dcx + 31, INK))
    p.add('<path d="%s" fill="none" stroke="%s" stroke-width="2.2"/>' % (door, BONE))
    # voussoirs round the arch
    d = ""
    for a in range(180, 361, 18):
        ca, sa = math.cos(math.radians(a)), math.sin(math.radians(a))
        d += "M%s %s L%s %s " % (n(dcx + ca * dw / 2.0), n(spring + sa * dw / 2.0),
                                n(dcx + ca * (dw / 2.0 + 34)), n(spring + sa * (dw / 2.0 + 34)))
    p.add('<path d="%s" stroke="%s" stroke-width="1.2" opacity="0.5"/>' % (d.strip(), BONE))
    p.add('<path d="M%s %d A%s %s 0 0 1 %s %d" fill="none" stroke="%s" stroke-width="1.2" opacity="0.5"/>' % (
        n(dcx - dw / 2.0 - 34), spring, n(dw / 2.0 + 34), n(dw / 2.0 + 34), n(dcx + dw / 2.0 + 34), spring, BONE))

    # ---- ground ----
    gpts = [(0, ground + 26), (120, ground + 14), (250, ground + 4), (420, ground), (640, ground - 2), (900, ground),
            (1080, ground + 3), (1200, ground + 8)]
    p.lin("ground", 0, ground, 0, H, [(0, "#1a2e40", 1), (0.5, "#101d2a", 1), (1, "#09121a", 1)])
    p.add('<path d="%s L1200 %d L0 %d Z" fill="url(#ground)"/>' % (pts_path(gpts), H, H))
    p.add('<path d="%s" fill="none" stroke="%s" stroke-width="1.6" stroke-linejoin="round" opacity="0.85"/>' % (pts_path(gpts), BONE))
    # two worn steps at the door
    for k, (inset, yy) in enumerate([(0, ground), (26, ground + 30)]):
        p.add('<path d="M%s %d h%s v30 h%s Z" fill="#1c3144" stroke="%s" stroke-width="1.5" stroke-linejoin="round" opacity="0.95"/>' % (
            n(dcx - dw / 2.0 - 30 - inset), yy, n(dw + 60 + 2 * inset), n(-(dw + 60 + 2 * inset)), BONE))
    # rock texture: cracks and hatching
    d = ""
    for _ in range(70):
        x = rng.uniform(10, 1190)
        y = rng.uniform(ground + 40, H - 20)
        if dcx - dw / 2.0 - 70 < x < dcx + dw / 2.0 + 70 and y < ground + 70:
            continue
        ln = rng.uniform(10, 30)
        d += "M%s %s l%s %s " % (n(x), n(y), n(ln * 0.7), n(ln * 0.5))
    p.add('<path d="%s" fill="none" stroke="%s" stroke-width="1" stroke-linecap="round" opacity="0.2"/>' % (d.strip(), BONE))
    for pts in ([(40, 1290), (150, 1330), (230, 1318), (360, 1372), (470, 1362)],
                [(620, 1420), (760, 1392), (860, 1410), (1010, 1380), (1150, 1396)],
                [(300, 1250), (380, 1262), (470, 1246)]):
        p.add('<path d="%s" fill="none" stroke="%s" stroke-width="1.2" stroke-linejoin="round" opacity="0.32"/>' % (pts_path(pts), BONE))

    # ---- timber frame ----
    post_l, post_r, pw = 330, 760, 36
    beam_y, beam_h = 404, 44
    timber = "#2a3f52"
    p.lin("timber", 0, 0, 1, 0, [(0, "#3b556b", 1), (1, "#1a2c3d", 1)], user=False)

    def grain(x, y, w, h, vertical=True, count=4):
        d = ""
        for i in range(count):
            if vertical:
                gx = x + w * (i + 1) / (count + 1.0) + rng.uniform(-2, 2)
                y0 = y + rng.uniform(4, h * 0.3)
                d += "M%s %s v%s " % (n(gx), n(y0), n(rng.uniform(h * 0.3, h * 0.65)))
            else:
                gy = y + h * (i + 1) / (count + 1.0) + rng.uniform(-1.5, 1.5)
                x0 = x + rng.uniform(4, w * 0.3)
                d += "M%s %s h%s " % (n(x0), n(gy), n(rng.uniform(w * 0.3, w * 0.65)))
        p.add('<path d="%s" fill="none" stroke="%s" stroke-width="0.9" stroke-linecap="round" opacity="0.3"/>' % (d.strip(), BONE))

    # braces (behind posts)
    for (x0, y0, x1, y1) in [(post_l + pw / 2.0, 560, post_l + 150, beam_y + beam_h),
                             (post_r - pw / 2.0, 560, post_r - 150, beam_y + beam_h)]:
        p.add('<path d="M%s %s L%s %s" stroke="%s" stroke-width="27" stroke-linecap="butt"/>' % (n(x0), n(y0), n(x1), n(y1), BONE))
        p.add('<path d="M%s %s L%s %s" stroke="%s" stroke-width="23" stroke-linecap="butt"/>' % (n(x0), n(y0), n(x1), n(y1), timber))
    for x in (post_l, post_r):
        p.add('<rect x="%s" y="%d" width="%d" height="%d" fill="url(#timber)" stroke="%s" stroke-width="2"/>' % (
            n(x - pw / 2.0), beam_y + 10, pw, ground - beam_y - 4, BONE))
        grain(x - pw / 2.0, beam_y + 60, pw, ground - beam_y - 80, True, 3)
        # stone pad
        p.add('<path d="M%d %d h%d l8 22 h-%d Z" fill="#1c3144" stroke="%s" stroke-width="1.5" stroke-linejoin="round"/>' % (
            x - 34, ground + 2, 68, 84, BONE))
    p.add('<rect x="%d" y="%d" width="%d" height="%d" fill="url(#timber)" stroke="%s" stroke-width="2"/>' % (
        post_l - 62, beam_y, post_r - post_l + 124, beam_h, BONE))
    grain(post_l - 62, beam_y, post_r - post_l + 124, beam_h, False, 4)
    # pegs
    d = ""
    for x in (post_l, post_r):
        d += '<circle cx="%d" cy="%d" r="3.4"/><circle cx="%d" cy="%d" r="3.4"/>' % (x - 8, beam_y + 22, x + 8, beam_y + 22)
    p.add('<g fill="%s" opacity="0.75">%s</g>' % (BONE, d))

    # ---- the bell ----
    bcx, by0, bh, R = 545, 516, 322, 176
    # headstock and straps
    p.add('<path d="M%d %d V%d M%d %d V%d" stroke="%s" stroke-width="7" stroke-linecap="round"/>' % (
        bcx - 66, beam_y + beam_h, by0 - 34, bcx + 66, beam_y + beam_h, by0 - 34, BONE))
    p.add('<rect x="%d" y="%d" width="212" height="36" rx="6" fill="url(#timber)" stroke="%s" stroke-width="2"/>' % (bcx - 106, by0 - 44, BONE))
    grain(bcx - 106, by0 - 44, 212, 36, False, 3)
    p.add('<path d="M%d %d v24 h56 v-24" fill="#1d3246" stroke="%s" stroke-width="2" stroke-linejoin="round"/>' % (bcx - 28, by0 - 8, BONE))
    prof = "M%s %s C%s %s %s %s %s %s C%s %s %s %s %s %s" % (
        n(bcx + 0.30 * R), n(by0), n(bcx + 0.52 * R), n(by0 + 0.02 * bh), n(bcx + 0.44 * R), n(by0 + 0.34 * bh),
        n(bcx + 0.56 * R), n(by0 + 0.62 * bh),
        n(bcx + 0.64 * R), n(by0 + 0.83 * bh), n(bcx + 0.82 * R), n(by0 + 0.9 * bh), n(bcx + R), n(by0 + bh))
    body = prof + " A%s %s 0 0 1 %s %s C%s %s %s %s %s %s C%s %s %s %s %s %s Z" % (
        n(R), n(R * 0.14), n(bcx - R), n(by0 + bh),
        n(bcx - 0.82 * R), n(by0 + 0.9 * bh), n(bcx - 0.64 * R), n(by0 + 0.83 * bh), n(bcx - 0.56 * R), n(by0 + 0.62 * bh),
        n(bcx - 0.44 * R), n(by0 + 0.34 * bh), n(bcx - 0.52 * R), n(by0 + 0.02 * bh), n(bcx - 0.30 * R), n(by0))
    # mouth (inside of the bell) and clapper
    p.add('<ellipse cx="%d" cy="%d" rx="%d" ry="%s" fill="#070d14" stroke="%s" stroke-width="2"/>' % (bcx, by0 + bh, R, n(R * 0.14), BONE))
    p.add('<path d="M%d %d v34" stroke="%s" stroke-width="5" stroke-linecap="round"/>' % (bcx, by0 + bh - 4, BONE))
    p.add('<circle cx="%d" cy="%d" r="17" fill="#22384c" stroke="%s" stroke-width="2"/>' % (bcx, by0 + bh + 44, BONE))
    p.lin("bellf", bcx - R, 0, bcx + R, 0, [(0, "#16283a", 1), (0.3, "#3e5a72", 1), (0.55, "#2a4258", 1), (1, "#0e1a26", 1)])
    p.add('<path d="%s" fill="url(#bellf)" stroke="%s" stroke-width="2.4" stroke-linejoin="round"/>' % (body, BONE))
    # warm light from the door catching the right flank
    p.lin("bellw", bcx, 0, bcx + R, 0, [(0, AMBER, 0), (0.7, AMBER, 0.28), (1, AMBER, 0.1)])
    p.add('<path d="%s" fill="url(#bellw)"/>' % body)
    # moulding wires
    for t, wf in ((0.1, 0.47), (0.14, 0.455), (0.6, 0.552), (0.64, 0.565), (0.9, 0.82), (0.94, 0.9)):
        yb = by0 + t * bh
        w = wf * R
        p.add('<path d="M%s %s Q%d %s %s %s" fill="none" stroke="%s" stroke-width="1.3" opacity="0.7"/>' % (
            n(bcx - w), n(yb), bcx, n(yb + w * 0.2), n(bcx + w), n(yb), BONE))
    # highlight
    p.add('<path d="M%s %s C%s %s %s %s %s %s" fill="none" stroke="%s" stroke-width="3" stroke-linecap="round" opacity="0.35"/>' % (
        n(bcx - 0.3 * R), n(by0 + 0.16 * bh), n(bcx - 0.3 * R), n(by0 + 0.4 * bh), n(bcx - 0.36 * R), n(by0 + 0.56 * bh),
        n(bcx - 0.5 * R), n(by0 + 0.78 * bh), BONE))

    # ---- clockwork striker on the right-hand post ----
    gx, gy = post_r + 4, 640
    # mounting plate
    p.add('<rect x="%d" y="%d" width="112" height="212" rx="8" fill="#16283a" stroke="%s" stroke-width="1.8"/>' % (gx - 56, gy - 86, BONE))
    d = ""
    for (ox, oy) in ((-46, -76), (46, -76), (-46, 116), (46, 116)):
        d += '<circle cx="%d" cy="%d" r="3"/>' % (gx + ox, gy + oy)
    p.add('<g fill="%s" opacity="0.75">%s</g>' % (BONE, d))
    # great wheel
    p.add('<path d="%s" fill="#22384c" stroke="%s" stroke-width="1.8" stroke-linejoin="round"/>' % (gear_d(gx, gy, 66, 56, 22), BONE))
    p.add('<circle cx="%d" cy="%d" r="44" fill="#16283a" stroke="%s" stroke-width="1.4"/>' % (gx, gy, BONE))
    d = ""
    for a in range(0, 360, 60):
        ca, sa = math.cos(math.radians(a + 12)), math.sin(math.radians(a + 12))
        d += "M%s %s L%s %s " % (n(gx + ca * 12), n(gy + sa * 12), n(gx + ca * 44), n(gy + sa * 44))
    p.add('<path d="%s" stroke="%s" stroke-width="3.4" stroke-linecap="round"/>' % (d.strip(), BONE))
    # drum for the weight chain, and arbor
    p.add('<circle cx="%d" cy="%d" r="20" fill="#2c465c" stroke="%s" stroke-width="1.8"/>' % (gx, gy, BONE))
    p.add('<circle cx="%d" cy="%d" r="5" fill="%s"/>' % (gx, gy, BONE))
    # pinion above
    p.add('<path d="%s" fill="#22384c" stroke="%s" stroke-width="1.5" stroke-linejoin="round"/>' % (
        gear_d(gx - 34, gy - 84, 24, 17, 10), BONE))
    p.add('<circle cx="%d" cy="%d" r="4" fill="%s"/>' % (gx - 34, gy - 84, BONE))
    # hammer: pivot, arm, head (resting just clear of the sound bow)
    pvx, pvy = gx - 6, gy + 98
    hx, hy = bcx + R + 22, by0 + bh - 26
    p.add('<path d="M%d %d L%d %d" stroke="#0a121b" stroke-width="13" stroke-linecap="round"/>' % (pvx + 30, pvy - 34, hx, hy))
    p.add('<path d="M%d %d L%d %d" stroke="%s" stroke-width="7" stroke-linecap="round"/>' % (pvx + 30, pvy - 34, hx, hy, BONE))
    ang = math.degrees(math.atan2(hy - (pvy - 34), hx - (pvx + 30)))
    p.add('<rect x="%d" y="%d" width="30" height="46" rx="7" fill="#2c465c" stroke="%s" stroke-width="2" transform="rotate(%s %d %d)"/>' % (
        hx - 15, hy - 23, BONE, n(ang), hx, hy))
    p.add('<circle cx="%d" cy="%d" r="9" fill="#16283a" stroke="%s" stroke-width="2"/>' % (pvx + 30, pvy - 34, BONE))
    # weight on its chain
    chx = gx + 20
    d = ""
    y = gy + 4
    k = 0
    while y < 1010:
        if k % 2 == 0:
            d += '<ellipse cx="%d" cy="%s" rx="3.6" ry="7"/>' % (chx, n(y + 7))
        else:
            d += '<path d="M%d %s v12"/>' % (chx, n(y + 1))
        y += 12
        k += 1
    p.add('<g fill="none" stroke="%s" stroke-width="1.5" stroke-linecap="round">%s</g>' % (BONE, d))
    p.lin("weight", chx - 26, 0, chx + 26, 0, [(0, "#3b566d", 1), (1, "#132231", 1)])
    p.add('<path d="M%d 1012 h16 v10 h-16 Z" fill="#22384c" stroke="%s" stroke-width="1.5"/>' % (chx - 8, BONE))
    p.add('<rect x="%d" y="1022" width="52" height="92" rx="7" fill="url(#weight)" stroke="%s" stroke-width="2"/>' % (chx - 26, BONE))
    p.add('<path d="M%d 1044 h52 M%d 1092 h52" stroke="%s" stroke-width="1.1" opacity="0.55"/>' % (chx - 26, chx - 26, BONE))

    # drifting fog in the foreground
    fog_bank(p, rng, 1150, 1290, 6, (240, 480), (22, 44), (0.1, 0.2))
    fog_lines(p, rng, 1210, 1300, 6, op=0.12)
    vignette(p, 0.6)
    return p.save("bell.svg")


# --------------------------------------------------------------------------
# 6. daybreak.svg
# --------------------------------------------------------------------------

def daybreak():
    rng = random.Random(6606)
    W, H = 1200, 1600
    p = Plate(W, H, "Daybreak", "Dawn at Skerry Vane: the fog lifting, the lamp out, a rowing boat far off on a calm sea.")
    horizon = 992
    dark = "#1b2630"
    p.lin("sky", 0, 0, 0, horizon, [(0, "#e9dcc8", 1), (0.55, "#dcd6c8", 1), (1, "#b9c7cc", 1)])
    p.add('<rect width="%d" height="%d" fill="url(#sky)"/>' % (W, horizon + 2))
    # low sun behind the haze
    sx, sy = 828, 934
    p.rad("sun", [(0, "#fbe9c4", 0.9), (0.3, "#f6d9a6", 0.5), (1, "#f2c98a", 0)], sx, sy, 300)
    p.add('<circle cx="%d" cy="%d" r="300" fill="url(#sun)"/>' % (sx, sy))
    p.add('<circle cx="%d" cy="%d" r="34" fill="#fdf4dc" opacity="0.92"/>' % (sx, sy))
    # high thin cloud, very pale
    fog_bank(p, rng, 770, 880, 6, (260, 520), (10, 20), (0.35, 0.6), color="#f4eee0", gid="mist")
    for _ in range(9):
        y = rng.uniform(760, 900)
        length = rng.uniform(160, 420)
        x = rng.uniform(-40, W - length + 40)
        p.add('<path d="%s" fill="none" stroke="#f7f2e6" stroke-width="1.2" stroke-linecap="round" opacity="0.6"/>' % (
            wave_d(x, x + length, y, 3, 80, rng)))

    # sea
    p.lin("sea", 0, horizon, 0, H, [(0, "#b3c2c7", 1), (0.35, "#a4b6bd", 1), (1, "#8fa3ad", 1)])
    p.add('<rect y="%d" width="%d" height="%d" fill="url(#sea)"/>' % (horizon, W, H - horizon))
    # the sun's path on the water
    reflection(p, rng, sx, horizon + 8, 1270, 60, "#fbeed2", 0.8, rows=20, grow=1.5)
    # long calm reflections
    for _ in range(34):
        t = rng.random()
        y = horizon + 10 + (1290 - horizon) * (t ** 1.3)
        length = rng.uniform(80, 300) * (0.6 + t)
        x = rng.uniform(-40, W - length + 40)
        p.add('<path d="M%s %s h%s" stroke="#eef0ea" stroke-width="%s" stroke-linecap="round" opacity="%s"/>' % (
            n(x), n(y), n(length), n(1 + t), "%.2f" % (rng.uniform(0.25, 0.5) * (1 - 0.6 * t))))
    for _ in range(10):
        y = rng.uniform(1300, 1580)
        length = rng.uniform(200, 460)
        x = rng.uniform(-60, W - length + 60)
        p.add('<path d="M%s %s h%s" stroke="#e6ebe6" stroke-width="1.6" stroke-linecap="round" opacity="0.16"/>' % (n(x), n(y), n(length)))

    # the lighthouse, lamp out, as a clean silhouette
    g = lh_geom(336, 1004, 286)
    water = rock(p, rng, 336, 1004, 330, 30, mode="dawn", uid="rk")
    lighthouse(p, g, "dawn", "lh")
    reflection(p, rng, 336, water + 12, 1236, 30, dark, 0.5, rows=15, grow=0.5)

    # fog lifting off the water in thin bands
    fog_bank(p, rng, 962, 1012, 9, (220, 520), (10, 20), (0.45, 0.75), color="#eef0ea", gid="mist2")
    fog_bank(p, rng, 906, 950, 5, (200, 420), (7, 13), (0.3, 0.5), color="#f4eee0", gid="mist")

    # a rowing boat, far off
    bx, by = 986, 1082
    p.add('<path d="M%d %d q30 13 62 0 l-6 9 q-25 6 -50 0 Z" fill="%s"/>' % (bx - 31, by, dark))
    p.add('<path d="M%d %d l-22 13 M%d %d l24 12" stroke="%s" stroke-width="1.6" stroke-linecap="round"/>' % (bx - 2, by + 1, bx + 6, by + 1, dark))
    p.add('<path d="M%d %d c-1 -11 9 -12 9 -2 l2 6 h-11 Z" fill="%s"/>' % (bx - 1, by + 1, dark))
    p.add('<circle cx="%s" cy="%d" r="3.3" fill="%s"/>' % (n(bx + 3.5), by - 13, dark))
    reflection(p, rng, bx, by + 14, by + 44, 22, dark, 0.3, rows=5, grow=0.2)

    # two gulls, low
    for (gx, gy, s) in ((640, 868, 1.0), (694, 842, 0.7)):
        p.add('<path d="M%s %s q%s %s %s %s q%s %s %s %s" fill="none" stroke="%s" stroke-width="1.5" stroke-linecap="round" opacity="0.7"/>' % (
            n(gx - 13 * s), n(gy), n(7 * s), n(-8 * s), n(13 * s), n(2 * s), n(6 * s), n(-10 * s), n(13 * s), n(-2 * s), dark))
    return p.save("daybreak.svg")


def main():
    os.makedirs(OUT, exist_ok=True)
    for fn in (cover, title_plate, chart, lens, bell, daybreak):
        path = fn()
        print("%-18s %7d bytes" % (os.path.basename(path), os.path.getsize(path)))


if __name__ == "__main__":
    main()
