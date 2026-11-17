#!/usr/bin/env python3
"""Generate the twelve ink-and-wash plates of «Atlas de mareas».

Original artwork, drawn procedurally. Standard library only, deterministic
(every plate has its own seeded random generator), and the output is plain
static SVG: no scripts, no external references, no embedded rasters.

    python3 tools/make_atlas_art.py            # all plates
    python3 tools/make_atlas_art.py caseta     # only the named plate(s)

Files are written to ../atlas-de-mareas/art/ relative to this script.
"""
import math
import os
import random
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.normpath(os.path.join(HERE, "..", "atlas-de-mareas", "art"))

INK = "#26302f"
PAPER = "#efe7d6"
BLUE = "#7d99a3"
PALE = "#c5d3d3"
SAND = "#dcc9a2"
TERRA = "#a5482e"
TAU = math.pi * 2


# --------------------------------------------------------------------------
# geometry helpers
# --------------------------------------------------------------------------

def n(v):
    s = "%.1f" % v
    if s.endswith(".0"):
        s = s[:-2]
    return "0" if s == "-0" else s


def esc(s):
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def resample(pts, step):
    out = [pts[0]]
    for (x0, y0), (x1, y1) in zip(pts, pts[1:]):
        k = max(1, int(round(math.hypot(x1 - x0, y1 - y0) / step)))
        for i in range(1, k + 1):
            t = i / k
            out.append((x0 + (x1 - x0) * t, y0 + (y1 - y0) * t))
    return out


def curve_cmds(pts, closed=False):
    """Catmull-Rom spline through pts as cubic Bezier commands (no leading M)."""
    m = len(pts)
    if m == 2 and not closed:
        return "L%s %s" % (n(pts[1][0]), n(pts[1][1]))

    def P(i):
        return pts[i % m] if closed else pts[max(0, min(m - 1, i))]

    d = []
    for i in range(m if closed else m - 1):
        p0, p1, p2, p3 = P(i - 1), P(i), P(i + 1), P(i + 2)
        d.append("C%s %s %s %s %s %s" % (
            n(p1[0] + (p2[0] - p0[0]) / 6), n(p1[1] + (p2[1] - p0[1]) / 6),
            n(p2[0] - (p3[0] - p1[0]) / 6), n(p2[1] - (p3[1] - p1[1]) / 6),
            n(p2[0]), n(p2[1])))
    return "".join(d)


def smooth(pts, closed=False):
    return "M%s %s" % (n(pts[0][0]), n(pts[0][1])) + curve_cmds(pts, closed) + ("Z" if closed else "")


def circ(cx, cy, r, k=None, a0=0.0, a1=TAU):
    """Points on a circle arc; a full circle omits the duplicate end point."""
    full = abs(abs(a1 - a0) - TAU) < 1e-9
    if k is None:
        k = max(8, int(abs(a1 - a0) * r / 30))
    cnt = k if full else k + 1
    return [(cx + r * math.cos(a0 + (a1 - a0) * i / k), cy + r * math.sin(a0 + (a1 - a0) * i / k))
            for i in range(cnt)]


def ell(cx, cy, rx, ry, k=None, rot=0.0, a0=0.0, a1=TAU):
    full = abs(abs(a1 - a0) - TAU) < 1e-9
    if k is None:
        k = max(10, int(abs(a1 - a0) * max(rx, ry) / 30))
    cnt = k if full else k + 1
    c, s = math.cos(rot), math.sin(rot)
    out = []
    for i in range(cnt):
        a = a0 + (a1 - a0) * i / k
        x, y = rx * math.cos(a), ry * math.sin(a)
        out.append((cx + x * c - y * s, cy + x * s + y * c))
    return out


def inside(pt, poly):
    x, y = pt
    c = False
    j = len(poly) - 1
    for i in range(len(poly)):
        xi, yi = poly[i]
        xj, yj = poly[j]
        if (yi > y) != (yj > y) and x < (xj - xi) * (y - yi) / (yj - yi) + xi:
            c = not c
        j = i
    return c


def ribbon(center, halfw):
    """Closed polygon around a centre line with per-point half widths."""
    left, right = [], []
    m = len(center)
    for i, (x, y) in enumerate(center):
        x0, y0 = center[max(0, i - 1)]
        x1, y1 = center[min(m - 1, i + 1)]
        dx, dy = x1 - x0, y1 - y0
        L = math.hypot(dx, dy) or 1.0
        nx, ny = -dy / L, dx / L
        h = halfw[i] if isinstance(halfw, (list, tuple)) else halfw
        left.append((x + nx * h, y + ny * h))
        right.append((x - nx * h, y - ny * h))
    return left + right[::-1]


def offset(center, dist):
    out = []
    m = len(center)
    for i, (x, y) in enumerate(center):
        x0, y0 = center[max(0, i - 1)]
        x1, y1 = center[min(m - 1, i + 1)]
        dx, dy = x1 - x0, y1 - y0
        L = math.hypot(dx, dy) or 1.0
        out.append((x - dy / L * dist, y + dx / L * dist))
    return out


def lerp_list(vals, k):
    """Linear interpolation of a short list of values to k samples."""
    out = []
    for i in range(k):
        t = i / (k - 1) * (len(vals) - 1)
        a = min(int(t), len(vals) - 2)
        out.append(vals[a] + (vals[a + 1] - vals[a]) * (t - a))
    return out


def crescent(cx, cy, R, dx, dy, r2, k=48):
    """Outline of a disc (cx,cy,R) minus a disc centred (cx+dx, cy+dy) radius r2."""
    ox, oy = cx + dx, cy + dy
    outer = [(cx + R * math.cos(TAU * i / k), cy + R * math.sin(TAU * i / k)) for i in range(k)]
    keep = [math.hypot(x - ox, y - oy) >= r2 for x, y in outer]
    start = next(i for i in range(k) if keep[i] and not keep[i - 1])
    arc1 = []
    i = start
    while keep[i % k]:
        arc1.append(outer[i % k])
        i += 1
    inner = [(ox + r2 * math.cos(TAU * i / k), oy + r2 * math.sin(TAU * i / k)) for i in range(k)]
    keep2 = [math.hypot(x - cx, y - cy) <= R for x, y in inner]
    start2 = next(i for i in range(k) if keep2[i] and not keep2[i - 1])
    arc2 = []
    i = start2
    while keep2[i % k]:
        arc2.append(inner[i % k])
        i += 1
    return arc1 + arc2[::-1]


# --------------------------------------------------------------------------
# plate
# --------------------------------------------------------------------------

class Plate:
    def __init__(self, name, w, h, seed, paper=True, inset=40):
        self.name, self.w, self.h = name, w, h
        self.r = random.Random(seed)
        self.defs = []
        self.body = []
        self.uid = 0
        self.paper = paper
        self.inset = inset
        self.blurs = set()

    # -- low level ---------------------------------------------------------
    def raw(self, s):
        self.body.append(s)

    def nid(self, prefix="c"):
        self.uid += 1
        return "%s%d" % (prefix, self.uid)

    def rough(self, pts, step=26, amp=1.1, closed=False):
        src = list(pts) + ([pts[0]] if closed else [])
        q = resample(src, step)
        if closed:
            q = q[:-1]
        out = []
        last = len(q) - 1
        for i, (x, y) in enumerate(q):
            if not closed and i in (0, last):
                out.append((x, y))
            else:
                out.append((x + self.r.gauss(0, amp), y + self.r.gauss(0, amp)))
        return out

    def hand(self, pts, step=26, amp=1.1, closed=False):
        return smooth(self.rough(pts, step, amp, closed), closed)

    def sharp(self, pts, step=40, amp=0.9, closed=True):
        """Wobbly edges that still meet in crisp corners."""
        d = ["M%s %s" % (n(pts[0][0]), n(pts[0][1]))]
        seq = list(pts) + ([pts[0]] if closed else [])
        for a, b in zip(seq, seq[1:]):
            d.append(curve_cmds(self.rough([a, b], step, amp)))
        if closed:
            d.append("Z")
        return "".join(d)

    def blur(self, sd):
        if sd not in self.blurs:
            self.blurs.add(sd)
            self.defs.append('<filter id="b%d" x="-20%%" y="-20%%" width="140%%" height="140%%">'
                             '<feGaussianBlur stdDeviation="%d"/></filter>' % (sd, sd))
        return ' filter="url(#b%d)"' % sd

    def path(self, d, w=2.0, color=INK, fill="none", op=None, dash=None, fop=None, extra=""):
        s = '<path d="%s"' % d
        if fill != "none":
            s += ' fill="%s"' % fill
            if fop is not None:
                s += ' fill-opacity="%s"' % n2(fop)
        if w and color:
            s += ' stroke="%s"' % color if color != INK else ""
            s += ' stroke-width="%s"' % n(w) if abs(w - 2) > 1e-9 else ""
        else:
            s += ' stroke="none"'
        if op is not None:
            s += ' opacity="%s"' % n2(op)
        if dash:
            s += ' stroke-dasharray="%s"' % dash
        self.body.append(s + extra + "/>")

    def ink(self, pts, w=2.0, color=INK, step=26, amp=1.1, closed=False, fill="none",
            op=None, dash=None, fop=None, sharp=False):
        d = self.sharp(pts, step, amp, closed) if sharp else self.hand(pts, step, amp, closed)
        self.path(d, w, color, fill, op, dash, fop)

    def wash(self, pts, color=BLUE, op=0.4, step=40, amp=2.5, edge=0.3, blur=0, sharp=False, raw=False):
        if raw:
            d = smooth(pts, True)
        elif sharp:
            d = self.sharp(pts, step, amp, True)
        else:
            d = self.hand(pts, step, amp, True)
        s = '<path d="%s" fill="%s" fill-opacity="%s"' % (d, color, n2(op))
        if edge:
            s += ' stroke="%s" stroke-opacity="%s" stroke-width="2.4"' % (color, n2(edge))
        else:
            s += ' stroke="none"'
        if blur:
            s += self.blur(blur)
        self.body.append(s + "/>")

    def dots(self, pts, w=1.6, color=INK, op=None):
        if not pts:
            return
        d = "".join("M%s %sh.1" % (n(x), n(y)) for x, y in pts)
        self.path(d, w, color, op=op)

    def segs(self, segments, w=1.0, color=INK, op=None, dash=None):
        """Many straight strokes in a single path."""
        if not segments:
            return
        d = "".join("M%s %sL%s %s" % (n(a[0]), n(a[1]), n(b[0]), n(b[1])) for a, b in segments)
        self.path(d, w, color, op=op, dash=dash)

    def hatch(self, poly, ang=45.0, gap=8.0, w=0.8, color=INK, op=0.5, raw=True):
        cid = self.nid("h")
        d = smooth(poly, True) if not raw else "M" + "L".join("%s %s" % (n(x), n(y)) for x, y in poly) + "Z"
        self.defs.append('<clipPath id="%s"><path d="%s"/></clipPath>' % (cid, d))
        xs = [q[0] for q in poly]
        ys = [q[1] for q in poly]
        cx, cy = (min(xs) + max(xs)) / 2, (min(ys) + max(ys)) / 2
        R = math.hypot(max(xs) - min(xs), max(ys) - min(ys)) / 2 + 4
        a = math.radians(ang)
        ux, uy = math.cos(a), math.sin(a)
        vx, vy = -uy, ux
        segments = []
        t = -R
        while t <= R:
            j0, j1 = self.r.uniform(-1, 1), self.r.uniform(-1, 1)
            segments.append(((cx + vx * (t + j0) - ux * R, cy + vy * (t + j0) - uy * R),
                             (cx + vx * (t + j1) + ux * R, cy + vy * (t + j1) + uy * R)))
            t += gap * self.r.uniform(0.8, 1.2)
        self.raw('<g clip-path="url(#%s)">' % cid)
        self.segs(segments, w, color, op)
        self.raw("</g>")

    def text(self, x, y, s, size=22, italic=True, anchor="start", fill=INK, ls=0, rot=0,
             family="serif", weight=None, op=None):
        a = '<text x="%s" y="%s" font-family="%s" font-size="%s"' % (n(x), n(y), family, n(size))
        if italic:
            a += ' font-style="italic"'
        if weight:
            a += ' font-weight="%s"' % weight
        if anchor != "start":
            a += ' text-anchor="%s"' % anchor
        if ls:
            a += ' letter-spacing="%s"' % n(ls)
        if rot:
            a += ' transform="rotate(%s %s %s)"' % (n(rot), n(x), n(y))
        if op is not None:
            a += ' opacity="%s"' % n2(op)
        a += ' fill="%s" stroke="none">%s</text>' % (fill, esc(s))
        self.body.append(a)

    def arrow(self, a, b, w=1.6, color=INK, head=11, op=None):
        ang = math.atan2(b[1] - a[1], b[0] - a[0])
        h1 = (b[0] - head * math.cos(ang - 0.42), b[1] - head * math.sin(ang - 0.42))
        h2 = (b[0] - head * math.cos(ang + 0.42), b[1] - head * math.sin(ang + 0.42))
        self.segs([(a, b), (h1, b), (h2, b)], w, color, op)

    # -- output ------------------------------------------------------------
    def svg(self):
        w, h, i = self.w, self.h, self.inset
        out = ['<svg xmlns="http://www.w3.org/2000/svg" width="%d" height="%d" viewBox="0 0 %d %d">' % (w, h, w, h)]
        if self.paper:
            self.defs.append('<clipPath id="fr"><rect x="%d" y="%d" width="%d" height="%d"/></clipPath>'
                             % (i + 9, i + 9, w - 2 * i - 18, h - 2 * i - 18))
        if self.defs:
            out.append("<defs>" + "".join(self.defs) + "</defs>")
        if self.paper:
            out.append('<rect width="%d" height="%d" fill="%s"/>' % (w, h, PAPER))
        out.append('<g fill="none" stroke="%s" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">' % INK)
        if self.paper:
            out.append('<g clip-path="url(#fr)">')
        out.extend(self.body)
        if self.paper:
            out.append("</g>")
            body, self.body = self.body, []
            self.ink([(i, i), (w - i, i), (w - i, h - i), (i, h - i)], w=1.8, step=170, amp=0.5, sharp=True, closed=True)
            j = i + 9
            self.ink([(j, j), (w - j, j), (w - j, h - j), (j, h - j)], w=0.7, step=170, amp=0.4, sharp=True, closed=True)
            out.extend(self.body)
            self.body = body
        out.append("</g></svg>")
        return "\n".join(out) + "\n"

    def save(self):
        os.makedirs(OUT, exist_ok=True)
        path = os.path.join(OUT, self.name + ".svg")
        data = self.svg()
        with open(path, "w", encoding="utf-8", newline="\n") as fh:
            fh.write(data)
        return path, len(data.encode("utf-8"))


def n2(v):
    s = "%.2f" % v
    s = s.rstrip("0").rstrip(".")
    return s if s else "0"


# --------------------------------------------------------------------------
# shared motifs
# --------------------------------------------------------------------------

def moon_glyph(p, x, y, r, phase, dark="left"):
    """phase: new | full | half (dark side given)."""
    c = circ(x, y, r)
    if phase == "new":
        p.ink(c, w=1.6, closed=True, fill=INK, amp=0.4, step=14)
    else:
        p.ink(c, w=1.6, closed=True, fill=PAPER, amp=0.4, step=14)
        if phase == "half":
            sweep = 0 if dark == "left" else 1
            p.path("M%s %sA%s %s 0 0 %d %s %sZ" % (n(x), n(y - r), n(r), n(r), sweep, n(x), n(y + r)),
                   w=0, color=None, fill=INK)


def compass(p, x, y, r):
    p.ink(circ(x, y, r * 0.78), w=1.0, closed=True, amp=0.5, step=20)
    p.ink(circ(x, y, r * 0.84), w=0.6, closed=True, amp=0.5, step=20)
    ticks = []
    for i in range(32):
        a = TAU * i / 32
        r0 = r * (0.70 if i % 4 else 0.64)
        ticks.append(((x + r0 * math.cos(a), y + r0 * math.sin(a)),
                      (x + r * 0.78 * math.cos(a), y + r * 0.78 * math.sin(a))))
    p.segs(ticks, 0.8)
    for i, (rr, wd) in enumerate([(0.56, 0.11)] * 4):
        a = TAU * (i + 0.5) / 4
        tip = (x + r * rr * math.cos(a), y + r * rr * math.sin(a))
        l = (x + r * wd * math.cos(a + TAU / 4), y + r * wd * math.sin(a + TAU / 4))
        rt = (x + r * wd * math.cos(a - TAU / 4), y + r * wd * math.sin(a - TAU / 4))
        p.path("M%s %sL%s %sL%s %sZ" % (n(l[0]), n(l[1]), n(tip[0]), n(tip[1]), n(rt[0]), n(rt[1])),
               w=1.0, fill=PAPER)
        p.path("M%s %sL%s %sL%s %sZ" % (n(x), n(y), n(tip[0]), n(tip[1]), n(rt[0]), n(rt[1])),
               w=0, color=None, fill=BLUE)
    for i in range(4):
        a = TAU * i / 4 - TAU / 4
        rr = 1.0 if i == 0 else 0.9
        tip = (x + r * rr * math.cos(a), y + r * rr * math.sin(a))
        l = (x + r * 0.13 * math.cos(a + TAU / 4), y + r * 0.13 * math.sin(a + TAU / 4))
        rt = (x + r * 0.13 * math.cos(a - TAU / 4), y + r * 0.13 * math.sin(a - TAU / 4))
        p.path("M%s %sL%s %sL%s %sZ" % (n(l[0]), n(l[1]), n(tip[0]), n(tip[1]), n(rt[0]), n(rt[1])),
               w=1.2, fill=PAPER)
        p.path("M%s %sL%s %sL%s %sZ" % (n(x), n(y), n(tip[0]), n(tip[1]), n(rt[0]), n(rt[1])),
               w=0, color=None, fill=TERRA if i == 0 else INK)
    p.ink(circ(x, y, r * 0.07), w=1.0, closed=True, fill=PAPER, amp=0.2, step=6)
    p.text(x, y - r - 12, "N", size=r * 0.36, italic=False, anchor="middle")


def bird(p, x, y, s=1.0, flip=False, feeding=False):
    """A curlew in ink silhouette."""
    f = -1 if flip else 1

    def T(pts):
        return [(x + f * a * s, y + b * s) for a, b in pts]

    body = [(-40, 2), (-26, -6), (-10, -13), (8, -15), (22, -11), (30, -3), (24, 7), (8, 12), (-10, 11), (-26, 7)]
    p.path(smooth(T(body), True), w=0, color=None, fill=INK)
    if feeding:
        neck = [(20, -8), (30, -4), (38, 6)]
        head = (41, 9)
        bill = [(44, 11), (52, 20), (56, 32)]
    else:
        neck = [(20, -9), (27, -20), (30, -31)]
        head = (32, -34)
        bill = [(37, -34), (50, -33), (62, -27), (70, -19)]
    p.path(smooth(T(neck)), w=6.5 * s)
    hx, hy = T([head])[0]
    p.path(smooth(circ(hx, hy, 6.2 * s, 8), True), w=0, color=None, fill=INK)
    p.path(smooth(T(bill)), w=1.7 * s)
    legs = [(T([(-4, 10)])[0], T([(-7, 44)])[0]), (T([(6, 10)])[0], T([(9, 44)])[0]),
            (T([(-7, 44)])[0], T([(0, 45)])[0]), (T([(9, 44)])[0], T([(16, 45)])[0])]
    p.segs(legs, 1.5 * s)


# --------------------------------------------------------------------------
# 1 · cubierta
# --------------------------------------------------------------------------

def cubierta():
    p = Plate("cubierta", 1200, 1200, 101, paper=False)
    NIGHT, GOLD, CREAM = "#16303a", "#c9a45c", "#f1e7d0"
    p.defs.append('<clipPath id="disc"><circle cx="600" cy="600" r="524"/></clipPath>')
    p.defs.append('<radialGradient id="halo" gradientUnits="userSpaceOnUse" cx="430" cy="318" r="430">'
                  '<stop offset="0" stop-color="%s" stop-opacity=".26"/>'
                  '<stop offset=".45" stop-color="%s" stop-opacity=".07"/>'
                  '<stop offset="1" stop-color="%s" stop-opacity="0"/></radialGradient>' % (CREAM, CREAM, CREAM))
    p.raw('<circle cx="600" cy="600" r="560" fill="%s" stroke="none"/>' % NIGHT)
    p.raw('<g clip-path="url(#disc)">')
    p.raw('<circle cx="430" cy="318" r="430" fill="url(#halo)" stroke="none"/>')
    HOR = 628
    # sea: a slightly lighter band below the horizon
    p.raw('<rect x="0" y="%d" width="1200" height="600" fill="#1d3d49" stroke="none" opacity=".75"/>' % HOR)
    # stars
    stars = []
    while len(stars) < 46:
        x, y = p.r.uniform(110, 1090), p.r.uniform(110, 560)
        if math.hypot(x - 600, y - 600) > 500 or math.hypot(x - 430, y - 318) < 160:
            continue
        if 630 < x < 880 and y > 380:
            continue
        stars.append((x, y))
    p.dots(stars[:30], 2.6, GOLD, 0.9)
    p.dots(stars[30:], 4.2, CREAM, 0.9)
    # moon
    p.path(smooth(crescent(430, 318, 104, 46, -20, 94, 64), True), w=2.2, color=CREAM, fill=CREAM)
    p.ink(circ(430, 318, 128), w=1.2, color=GOLD, closed=True, amp=0.6, op=0.7, dash="2 10")
    # horizon
    p.ink([(70, HOR), (640, HOR)], w=2.0, color=CREAM, step=60, amp=0.8)
    p.ink([(842, HOR), (1130, HOR)], w=2.0, color=CREAM, step=60, amp=0.8)
    # sandbar contours
    for k in range(7):
        rx, ry = 96 + k * 82, 26 + k * 33
        pts = []
        cnt = 22 + k * 5
        ph = p.r.uniform(0, TAU)
        for i in range(cnt):
            a = TAU * i / cnt
            wob = 1 + 0.035 * math.sin(3 * a + ph) + 0.03 * math.sin(5 * a + ph * 2) + p.r.gauss(0, 0.006)
            pts.append((575 + k * 9 + rx * wob * math.cos(a), 905 + k * 4 + ry * wob * math.sin(a)))
        if k == 0:
            p.path(smooth(pts, True), w=2.4, color=GOLD, fill=GOLD, fop=0.45)
        else:
            p.path(smooth(pts, True), w=2.2 if k % 2 else 1.5, color=CREAM if k % 2 else GOLD,
                   op=max(0.5, 1 - k * 0.08))
    # wave strokes
    for (wx, wy, wl) in [(205, 668, 120), (330, 712, 90), (905, 676, 130), (1010, 722, 80), (470, 676, 70)]:
        pts = [(wx + wl * t / 6, wy - 7 * math.sin(math.pi * (t % 2) / 1) * 0 - (6 if t % 2 else 0)) for t in range(7)]
        p.path(smooth(pts), w=2.0, color=CREAM, op=0.85)
    # tide house on stilts
    bx0, bx1, by0, by1 = 668, 812, 446, 548
    stilts = []
    for sx in (682, 724, 756, 798):
        stilts.append(((sx, by1), (sx + p.r.uniform(-2, 2), 716)))
    p.segs(stilts, 3.2, CREAM)
    p.segs([((682, 570), (724, 640)), ((724, 570), (682, 640)), ((756, 570), (798, 640)), ((798, 570), (756, 640))],
           1.6, GOLD)
    p.path(p.sharp([(bx0, by0), (bx1, by0), (bx1, by1), (bx0, by1)], amp=0.5), w=2.6, color=CREAM, fill=NIGHT)
    planks = [((xx, by0 + 6), (xx, by1 - 5)) for xx in range(bx0 + 18, bx1 - 6, 18)]
    p.segs(planks, 1.0, CREAM, 0.45)
    p.path(p.sharp([(bx0 - 26, by0 + 4), (740, 376), (bx1 + 26, by0 + 4)], amp=0.5), w=2.6, color=CREAM, fill=CREAM, fop=0.92)
    p.segs([((642, by1 + 3), (838, by1 + 3))], 4.0, CREAM)
    p.path("M700 478h30v30h-30Z", w=1.6, color=GOLD, fill=GOLD, fop=0.9)
    p.path("M762 482h30v66h-30Z", w=1.8, color=CREAM)
    p.segs([((740, 376), (740, 330))], 2.0, CREAM)
    p.path("M740 332L776 342L740 352Z", w=1.4, color=GOLD, fill=GOLD)
    # ladder and reflections
    p.segs([((838, 551), (862, 700)), ((852, 551), (876, 700))] +
           [((841 + i * 3.4, 572 + i * 21), (855 + i * 3.4, 572 + i * 21)) for i in range(6)], 1.6, CREAM, 0.9)
    refl = []
    for i in range(9):
        yy = 730 + i * 13
        half = 70 - i * 6 + p.r.uniform(-8, 8)
        cx = 740 + p.r.uniform(-10, 10)
        refl.append(((cx - half, yy), (cx - half * 0.2, yy)))
        refl.append(((cx + half * 0.1, yy), (cx + half, yy)))
    p.segs(refl, 1.6, CREAM, 0.5)
    p.raw("</g>")
    p.raw('<circle cx="600" cy="600" r="560" stroke="%s" stroke-width="7"/>' % GOLD)
    p.raw('<circle cx="600" cy="600" r="538" stroke="%s" stroke-width="2"/>' % GOLD)
    return p


# --------------------------------------------------------------------------
# 2 · mapa de Ardora
# --------------------------------------------------------------------------

def mapa():
    p = Plate("mapa-ardora", 1600, 1100, 202)
    W, H = 1600, 1100
    coastN = p.rough([(372, 20), (384, 150), (364, 270), (392, 372), (418, 428)], 30, 3.0)
    northB = p.rough([(418, 428), (470, 404), (560, 378), (700, 338), (900, 318), (1100, 330),
                      (1300, 378), (1470, 452), (1620, 478)], 34, 3.0)
    southB = p.rough([(1620, 566), (1470, 580), (1300, 612), (1100, 690), (900, 752), (700, 768),
                      (560, 742), (460, 690), (412, 640)], 34, 3.0)
    coastS = p.rough([(412, 640), (398, 720), (410, 830), (382, 950), (372, 1090)], 30, 3.0)
    north = coastN + northB[1:]
    south = southB + coastS[1:]
    sea = [(-40, -40)] + coastN + [(436, 530)] + coastS + [(-40, H + 40)]
    estuary = northB + southB
    chan_c = [(236, 622), (330, 612), (420, 604), (560, 610), (720, 640), (860, 628), (1010, 580),
              (1200, 528), (1400, 520), (1620, 522)]
    chan_c = resample(chan_c, 40)
    chan_w = lerp_list([40, 34, 30, 28, 26, 24, 20, 16, 13, 11], len(chan_c))
    chan = ribbon(chan_c, chan_w)
    viuda_c = resample([(1004, 584), (1056, 524), (1130, 466), (1230, 434), (1330, 438), (1392, 456)], 34)
    viuda_w = lerp_list([13, 11, 9, 7, 5, 2], len(viuda_c))
    viuda = ribbon(viuda_c, viuda_w)

    def dist_to(center, pt):
        return min(math.hypot(pt[0] - c[0], pt[1] - c[1]) for c in center)

    def is_water(pt):
        return inside(pt, sea) or inside(pt, estuary)

    labels = [(540, 548, 760, 596), (1060, 378, 1300, 418), (676, 700, 834, 734), (862, 660, 1080, 690)]

    def in_label(pt):
        return any(a <= pt[0] <= c and b <= pt[1] <= d for a, b, c, d in labels)

    # tidal flats
    p.raw('<path d="%s" fill="%s" fill-opacity=".55" stroke="none"/>' % (smooth(estuary, True), SAND))
    st = []
    while len(st) < 1500:
        q = (p.r.uniform(400, 1560), p.r.uniform(300, 780))
        if not inside(q, estuary) or in_label(q):
            continue
        if dist_to(chan_c, q) < 44 or dist_to(viuda_c, q) < 18:
            continue
        st.append(q)
    p.dots(st, 1.6, INK, 0.6)
    # water: sea and channels share one group so overlaps do not darken
    p.raw('<g opacity=".62" stroke="none" fill="#9db5bb">')
    p.raw('<path d="%s"/>' % smooth(sea, True))
    p.raw('<path d="%s"/>' % smooth(chan, True))
    p.raw('<path d="%s"/>' % smooth(viuda, True))
    p.raw("</g>")
    # deeper sea wash away from the coast
    p.defs.append('<linearGradient id="deep" gradientUnits="userSpaceOnUse" x1="40" y1="0" x2="340" y2="0">'
                  '<stop offset="0" stop-color="%s" stop-opacity=".42"/>'
                  '<stop offset="1" stop-color="%s" stop-opacity="0"/></linearGradient>' % (BLUE, BLUE))
    p.raw('<path d="%s" fill="url(#deep)" stroke="none"/>' % smooth(sea, True))
    # water lining along the open coast
    for dist_, w_, op_ in ((10, 1.0, 0.75), (23, 0.8, 0.55), (40, 0.7, 0.38), (62, 0.6, 0.25)):
        for seg in (coastN[:-1], coastS[1:]):
            base = [seg[0]] + [((a[0] + b[0] + c[0] + d[0] + e[0]) / 5, (a[1] + b[1] + c[1] + d[1] + e[1]) / 5)
                               for a, b, c, d, e in zip(seg, seg[1:], seg[2:], seg[3:], seg[4:])][::2] + [seg[-1]]
            off = offset(base, dist_)
            if not inside(off[len(off) // 2], sea):
                off = offset(base, -dist_)
            p.path(smooth(off), w=w_, color=INK, op=op_ * 0.8)
    # depth contours in the channels
    for sgn in (-1, 1):
        off = [(c[0] - sgn * 0, c[1]) for c in chan_c]
        o2 = offset(chan_c, 1)
        off = [(c[0] + (o[0] - c[0]) * sgn * w * 0.45, c[1] + (o[1] - c[1]) * sgn * w * 0.45)
               for c, o, w in zip(chan_c, o2, chan_w)]
        p.path(smooth(off[1:]), w=0.8, op=0.55, dash="7 6")
    p.path(smooth(chan_c[1:14]), w=0.7, op=0.4, dash="2 6")
    # ebb delta: dotted contour round the mouth
    p.path(smooth([(352, 400), (318, 470), (296, 560), (300, 650), (340, 720), (372, 760)]), w=1.0, op=0.5, dash="1.5 7")
    # channel edges
    k0 = next(i for i, c in enumerate(chan_c) if c[0] > 436)
    p.path(smooth(chan[:len(chan_c)][k0:]), w=1.0, op=0.75)
    p.path(smooth(chan[len(chan_c):][:len(chan_c) - k0]), w=1.0, op=0.75)
    p.path(smooth(viuda[:len(viuda_c)]), w=0.9, op=0.7)
    p.path(smooth(viuda[len(viuda_c):]), w=0.9, op=0.7)
    # coast and bank lines
    p.path(smooth(north), w=2.6)
    p.path(smooth(south), w=2.6)
    # hachures on the land side
    hach = []
    for line, step_, ln in ((north, 9, 11), (south, 9, 11)):
        dense = resample(line, step_)
        for i in range(1, len(dense) - 1):
            x, y = dense[i]
            dx, dy = dense[i + 1][0] - dense[i - 1][0], dense[i + 1][1] - dense[i - 1][1]
            L = math.hypot(dx, dy) or 1
            nx, ny = -dy / L, dx / L
            if is_water((x + nx * 8, y + ny * 8)):
                nx, ny = -nx, -ny
            l = ln * p.r.uniform(0.55, 1.25)
            if x > 430:
                l *= 0.7
            hach.append(((x + nx * 2.5, y + ny * 2.5), (x + nx * (2.5 + l) + p.r.uniform(-1.5, 1.5), y + ny * (2.5 + l))))
    p.segs(hach, 0.8, INK, 0.7)
    # the bar
    bar_c = resample([(416, 436), (424, 480), (430, 524), (432, 566)], 22)
    bar = ribbon(bar_c, lerp_list([13, 17, 15, 6], len(bar_c)))
    p.path(smooth(bar, True), w=1.6, fill=SAND)
    bd = []
    while len(bd) < 60:
        q = (p.r.uniform(400, 452), p.r.uniform(436, 570))
        if inside(q, bar):
            bd.append(q)
    p.dots(bd, 1.4, INK, 0.7)
    # Bajo de las Ánimas
    rx, ry = 172, 716
    p.ink(circ(rx, ry, 40), w=1.0, closed=True, dash="1.5 6", amp=0.5)
    cr = []
    for (ax, ay) in ((-12, -8), (10, -12), (2, 10), (18, 8), (-16, 12)):
        cr.append(((rx + ax - 5, ry + ay), (rx + ax + 5, ry + ay)))
        cr.append(((rx + ax, ry + ay - 5), (rx + ax, ry + ay + 5)))
    p.segs(cr, 1.5)
    # salt pans
    px0, py0, cw, ch = 716, 150, 74, 62
    p.wash([(px0 - 12, py0 - 12), (px0 + 4 * cw + 12, py0 - 12), (px0 + 4 * cw + 12, py0 + 2 * ch + 12),
            (px0 - 12, py0 + 2 * ch + 12)], SAND, 0.4, sharp=True, edge=0)
    for cix in range(4):
        for ciy in range(2):
            x0, y0 = px0 + cix * cw + 4, py0 + ciy * ch + 4
            rect = [(x0, y0), (x0 + cw - 8, y0), (x0 + cw - 8, y0 + ch - 8), (x0, y0 + ch - 8)]
            p.path(p.sharp(rect, amp=0.7), w=1.3, fill="#9db5bb", fop=0.25 + 0.14 * ((cix * 2 + ciy * 3) % 4))
    p.ink([(px0 - 12, py0 - 12), (px0 + 4 * cw + 12, py0 - 12), (px0 + 4 * cw + 12, py0 + 2 * ch + 12),
           (px0 - 12, py0 + 2 * ch + 12)], w=1.6, sharp=True, closed=True)
    p.ink([(800, py0 + 2 * ch + 12), (806, 300), (798, 330)], w=1.2, color=BLUE, amp=1.5, step=14)
    # Ardora: houses
    hs = []
    rows = [((742, 796), (960, 780), 11), ((756, 826), (948, 812), 10), ((790, 856), (930, 846), 7), ((812, 884), (900, 878), 4)]
    for (a, b, cnt) in rows:
        for i in range(cnt):
            t = (i + p.r.uniform(-0.2, 0.2)) / max(1, cnt - 1)
            hx, hy = a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t + p.r.uniform(-4, 4)
            hw, hh = p.r.uniform(11, 17), p.r.uniform(8, 11)
            ang = math.atan2(b[1] - a[1], b[0] - a[0]) + p.r.uniform(-0.12, 0.12)
            c, s = math.cos(ang), math.sin(ang)
            cor = [(hx + c * dx - s * dy, hy + s * dx + c * dy)
                   for dx, dy in ((-hw / 2, -hh / 2), (hw / 2, -hh / 2), (hw / 2, hh / 2), (-hw / 2, hh / 2))]
            hs.append("M" + "L".join("%s %s" % (n(u), n(v)) for u, v in cor) + "Z")
    p.path("".join(hs), w=0.8, fill=INK)
    # church
    p.path("M872 836h20v14h-20Z", w=1.2, fill=PAPER)
    p.segs([((882, 836), (882, 818)), ((876, 824), (888, 824))], 1.5)
    # roads
    p.path(smooth([(950, 800), (1040, 850), (1160, 930), (1300, 960), (1560, 968)]), w=1.1, dash="9 6", op=0.75)
    p.path(smooth([(830, 890), (822, 960), (800, 1070)]), w=1.1, dash="9 6", op=0.75)
    # pier and hut
    pa, pb = (838, 764), (850, 664)
    p.segs([((pa[0] - 3.5, pa[1]), (pb[0] - 3.5, pb[1])), ((pa[0] + 3.5, pa[1]), (pb[0] + 3.5, pb[1]))], 1.3)
    p.segs([((pa[0] + (pb[0] - pa[0]) * t - 3.5, pa[1] + (pb[1] - pa[1]) * t),
             (pa[0] + (pb[0] - pa[0]) * t + 3.5, pa[1] + (pb[1] - pa[1]) * t)) for t in [i / 10 for i in range(11)]], 0.9)
    p.path("M842 648h17v17h-17Z", w=1.4, color=INK, fill=TERRA)
    p.segs([((862, 662), (874, 672))], 0.9, TERRA)
    # marsh tufts
    tufts = []
    tries = 0
    centres = []
    while len(centres) < 120 and tries < 6000:
        tries += 1
        q = (p.r.uniform(1090, 1545), p.r.uniform(200, 930))
        if is_water(q) or is_water((q[0], q[1] - 18)) or is_water((q[0], q[1] + 18)) or is_water((q[0] - 20, q[1])):
            continue
        inN = q[1] < 480 and q[0] > 1240 and q[1] > 215 + (1545 - q[0]) * 0.15
        inS = q[1] > 600 and q[1] < 905 and q[0] > 1090 + (q[1] - 600) * 0.25
        if not (inN or inS):
            continue
        if 1128 < q[0] < 1552 and 772 < q[1] < 812:
            continue
        if any(math.hypot(q[0] - c[0], q[1] - c[1]) < 30 for c in centres):
            continue
        centres.append(q)
    for (tx, ty) in centres:
        tufts.append(((tx - 8, ty), (tx + 8, ty)))
        for dx, top in ((-5, 9), (0, 13), (5, 9)):
            tufts.append(((tx + dx * 0.5, ty - 2), (tx + dx, ty - 2 - top)))
    p.segs(tufts, 0.9, INK, 0.8)
    # labels
    p.text(206, 250, "Mar abierto", size=36, ls=5, anchor="middle")
    p.text(388, 506, "Barra de los Zarapitos", size=21, anchor="end")
    p.text(172, 786, "Bajo de las Ánimas", size=20, anchor="middle")
    p.text(650, 584, "Caño Grande", size=25, anchor="middle", rot=6, ls=2)
    p.text(1180, 408, "Caño de la Viuda", size=20, anchor="middle", rot=-3)
    p.text(864, 124, "Salinas Viejas", size=24, anchor="middle")
    p.text(850, 950, "Ardora", size=44, italic=False, anchor="middle", ls=8)
    p.text(828, 726, "Muelle Largo", size=19, anchor="end")
    p.text(878, 682, "Caseta del mareógrafo", size=19, fill=TERRA)
    p.text(1340, 800, "Marisma del Cigüeñal", size=26, anchor="middle", ls=1)
    # compass and scale
    compass(p, 196, 936, 78)
    sx, sy = 1290, 1012
    p.path("M%d %dh220v9h-220Z" % (sx, sy), w=1.1)
    p.path("M%d %dh55v9h-55ZM%d %dh55v9h-55Z" % (sx, sy, sx + 110, sy), w=0, color=None, fill=INK)
    p.text(sx, sy - 9, "0", size=17, italic=False, anchor="middle")
    p.text(sx + 220, sy - 9, "1 km", size=17, italic=False, anchor="middle")
    return p


# --------------------------------------------------------------------------
# 3 · la caseta
# --------------------------------------------------------------------------

def caseta():
    p = Plate("caseta", 1600, 1000, 303)
    HOR, WL, DECK = 452, 664, 548
    p.defs.append('<linearGradient id="sea" x1="0" y1="0" x2="0" y2="1">'
                  '<stop offset="0" stop-color="%s" stop-opacity=".5"/>'
                  '<stop offset="1" stop-color="%s" stop-opacity=".62"/></linearGradient>' % (PALE, BLUE))
    # sky
    for (cx, cy, rx, ry, op) in ((420, 210, 300, 34, 0.5), (1130, 150, 330, 30, 0.45), (1300, 268, 210, 22, 0.4),
                                 (760, 318, 260, 18, 0.3)):
        p.wash(ell(cx, cy, rx, ry, 14), PALE, op, edge=0, blur=8, amp=6)
    p.path(smooth(circ(1236, 214, 34, 16), True), w=1.4, color=TERRA, fill=TERRA, fop=0.82)
    # far shore
    prof = [(40, HOR)]
    x = 40
    while x < 1560:
        hgt = 10 + 14 * (0.5 + 0.5 * math.sin(x / 170.0)) + p.r.uniform(-3, 3)
        if 560 < x < 760:
            hgt *= 0.4
        prof.append((x, HOR - hgt))
        x += p.r.uniform(40, 80)
    prof.append((1560, HOR - 8))
    prof.append((1560, HOR))
    p.wash(prof, BLUE, 0.5, edge=0, raw=True)
    p.path(smooth(prof[1:-1]), w=1.0, op=0.75)
    trees = []
    for i in range(26):
        tx = p.r.choice([p.r.uniform(80, 520), p.r.uniform(820, 1540)])
        trees.append(((tx, HOR - 12), (tx, HOR - 22 - p.r.uniform(0, 12))))
    p.segs(trees, 1.0, INK, 0.6)
    # water
    p.raw('<rect x="40" y="%d" width="1520" height="%d" fill="url(#sea)" stroke="none"/>' % (HOR, 1000 - HOR))
    p.segs([((49, HOR), (1551, HOR))], 1.1, INK, 0.7)
    y = HOR + 16
    while y < 950:
        t = (y - HOR) / (950 - HOR)
        cnt = int(9 - 4 * t)
        for _ in range(cnt):
            x0 = p.r.uniform(50, 1540)
            ln = (22 + 110 * t) * p.r.uniform(0.6, 1.3)
            pts = [(x0 + ln * i / 3, y + p.r.uniform(-1.5, 1.5) * (0.6 + t)) for i in range(4)]
            p.path(smooth(pts), w=0.7 + 1.0 * t, op=0.35 + 0.35 * t)
        y += 13 + 30 * t
    # sand spit in the foreground
    spit = [(40, 872), (150, 842), (300, 832), (470, 846), (640, 880), (800, 930), (900, 1010), (40, 1010)]
    d_spit = smooth(spit[:7])
    p.raw('<path d="%sL40 1010Z" fill="%s" stroke="none"/>' % (d_spit, PAPER))
    p.raw('<path d="%sL40 1010Z" fill="%s" fill-opacity=".75" stroke="none"/>' % (d_spit, SAND))
    p.path(d_spit, w=1.4, op=0.8)
    sp = []
    while len(sp) < 150:
        q = (p.r.uniform(50, 880), p.r.uniform(840, 950))
        if inside(q, spit):
            sp.append(q)
    p.dots(sp, 1.4, INK, 0.45)
    # pier
    piles = [706, 842, 978, 1114, 1250, 1386, 1522]
    hut_piles = [404, 482, 560, 628]
    for px in hut_piles + piles:
        p.path(p.sharp([(px - 7, DECK + 12), (px + 7, DECK + 12), (px + 6, WL), (px - 6, WL)], amp=0.6, closed=False),
               w=2.0, fill=PAPER)
        p.segs([((px - 2, DECK + 20), (px - 2.5, WL - 6))], 0.8, INK, 0.5)
        # reflection
        ry = WL + 5
        k = 0
        while ry < WL + 78:
            half = 7 + k * 1.3
            off = p.r.uniform(-4, 4)
            p.path(smooth([(px - half + off, ry), (px + off, ry + 2), (px + half + off, ry)]), w=1.5, op=0.6 - k * 0.05)
            ry += 9 + k * 1.5
            k += 1
        p.path(smooth([(px - 13, WL + 1), (px, WL + 4), (px + 13, WL + 1)]), w=1.1, op=0.8)
    braces = []
    for a, b in zip(piles[::2], piles[1::2]):
        braces += [((a + 6, DECK + 22), (b - 6, WL - 16)), ((b - 6, DECK + 22), (a + 6, WL - 16))]
    braces += [((410, DECK + 22), (476, WL - 16)), ((476, DECK + 22), (410, WL - 16)),
               ((566, DECK + 22), (622, WL - 16)), ((622, DECK + 22), (566, WL - 16))]
    p.segs(braces, 1.2, INK, 0.8)
    # deck
    p.path(p.sharp([(372, DECK), (1570, DECK), (1570, DECK + 13), (372, DECK + 13)], step=90, amp=0.7), w=2.2, fill=SAND)
    p.segs([((x, DECK + 2), (x, DECK + 11)) for x in range(400, 1560, 34)], 0.7, INK, 0.5)
    # railing
    rail = [((px, DECK), (px, DECK - 40)) for px in piles] + [((650, DECK), (650, DECK - 40))]
    p.segs(rail, 2.0)
    p.ink([(650, DECK - 40), (1570, DECK - 40)], w=1.8, step=90, amp=0.8)
    p.ink([(650, DECK - 20), (1570, DECK - 20)], w=1.0, step=90, amp=0.8, op=0.7)
    # hut
    hx0, hx1, hy0 = 416, 612, 398
    p.path(p.sharp([(hx0, hy0), (hx1, hy0), (hx1, DECK), (hx0, DECK)], amp=0.6), w=2.6, fill=PAPER)
    p.wash([(hx0, hy0), (hx1, hy0), (hx1, DECK), (hx0, DECK)], SAND, 0.55, sharp=True, edge=0)
    p.segs([((x + p.r.uniform(-0.8, 0.8), hy0 + 8), (x + p.r.uniform(-0.8, 0.8), DECK - 3)) for x in range(hx0 + 16, hx1 - 4, 16)],
           0.8, INK, 0.55)
    p.wash([(hx1 - 44, hy0 + 4), (hx1 - 2, hy0 + 4), (hx1 - 2, DECK - 2), (hx1 - 44, DECK - 2)], BLUE, 0.22, sharp=True, edge=0)
    roof = [(hx0 - 28, hy0 + 6), (514, 316), (hx1 + 28, hy0 + 6)]
    p.path(p.sharp(roof, amp=0.6), w=2.8, fill=PAPER)
    p.wash(roof, BLUE, 0.6, sharp=True, edge=0)
    sh = []
    for i in range(1, 6):
        t = i / 6
        sh.append(((514 - (514 - hx0 + 28) * t + 8, 316 + (hy0 + 6 - 316) * t), (514 + (hx1 + 28 - 514) * t - 8, 316 + (hy0 + 6 - 316) * t)))
    p.segs(sh, 0.9, INK, 0.5)
    p.segs([((hx0 - 30, hy0 + 8), (hx1 + 30, hy0 + 8))], 3.0)
    # door, window, lamp
    p.path(p.sharp([(528, 452), (580, 452), (580, DECK), (528, DECK)], amp=0.5, closed=False), w=2.0, fill=BLUE, fop=0.45)
    p.dots([(572, 506)], 4)
    p.path(p.sharp([(444, 448), (496, 448), (496, 496), (444, 496)], amp=0.5), w=2.0, fill=PALE)
    p.segs([((470, 448), (470, 496)), ((444, 472), (496, 472))], 1.2)
    p.segs([((440, 500), (500, 500))], 2.4)
    # vane and pennant
    p.segs([((514, 316), (514, 250))], 2.0)
    p.path(smooth([(514, 252), (540, 256), (562, 252), (540, 266), (514, 270)]) + "Z", w=1.2, color=TERRA, fill=TERRA)
    p.segs([((500, 284), (528, 284)), ((514, 284), (514, 284))], 1.4)
    # tide staff on the outer pile
    sx0, sx1, sy0, sy1 = 380, 398, DECK + 16, WL + 2
    p.path(p.sharp([(sx0, sy0), (sx1, sy0), (sx1, sy1), (sx0, sy1)], amp=0.4), w=1.6, fill=PAPER)
    blocks = []
    yy = sy0 + 4
    k = 0
    while yy + 8 < sy1:
        xa = sx0 + 1.5 if k % 2 == 0 else (sx0 + sx1) / 2
        blocks.append("M%s %sh%sv8h-%sZ" % (n(xa), n(yy), n((sx1 - sx0) / 2 - 1.5), n((sx1 - sx0) / 2 - 1.5)))
        yy += 8
        k += 1
    p.path("".join(blocks), w=0, color=None, fill=INK)
    p.path("M%d %dl-16 -7v14Z" % (sx0 - 3, DECK + 58), w=1.0, color=TERRA, fill=TERRA)
    p.path(smooth([(sx0 - 12, WL + 2), (389, WL + 5), (sx1 + 10, WL + 2)]), w=1.1, op=0.8)
    # ladder
    p.segs([((640, DECK + 13), (640, WL + 10)), ((656, DECK + 13), (656, WL + 10))] +
           [((640, yv), (656, yv)) for yv in range(DECK + 28, WL + 6, 16)], 1.4)
    # birds
    bird(p, 196, 800, 1.25)
    bird(p, 330, 792, 1.05, flip=True, feeding=True)
    bird(p, 470, 806, 1.15)
    bird(p, 690, 880, 0.95, feeding=True)
    # flying pair, far
    for (fx, fy, fs) in ((980, 250, 1.0), (1040, 228, 0.8), (912, 282, 0.7)):
        p.path(smooth([(fx - 20 * fs, fy - 2 * fs), (fx - 9 * fs, fy - 9 * fs), (fx, fy), (fx + 10 * fs, fy - 10 * fs), (fx + 22 * fs, fy - 5 * fs)]),
               w=1.6 * fs, op=0.85)
    return p


# --------------------------------------------------------------------------
# 4 · sizigia y cuadratura
# --------------------------------------------------------------------------

def earth(p, x, y, r, erx, ery):
    env = ell(x, y, erx, ery, 28)
    p.wash(env, BLUE, 0.42, edge=0, raw=True)
    p.path(smooth(env, True), w=1.3, color=BLUE)
    p.path(smooth(circ(x, y, r, 18), True), w=2.4, fill=PAPER)
    cid = p.nid("e")
    p.defs.append('<clipPath id="%s"><circle cx="%s" cy="%s" r="%s"/></clipPath>' % (cid, n(x), n(y), n(r - 1)))
    p.raw('<g clip-path="url(#%s)">' % cid)
    for blob in ([(-0.55, -0.5), (-0.1, -0.7), (0.25, -0.45), (0.1, -0.1), (-0.25, 0.05), (-0.6, -0.15)],
                 [(0.2, 0.15), (0.6, 0.05), (0.85, 0.4), (0.5, 0.75), (0.15, 0.6)],
                 [(-0.9, 0.35), (-0.5, 0.4), (-0.35, 0.8), (-0.8, 0.9)]):
        pts = [(x + a * r, y + b * r) for a, b in blob]
        p.wash(pts, SAND, 0.9, edge=0, step=14, amp=1.0)
        p.ink(pts, w=1.0, closed=True, step=14, amp=1.0, op=0.8)
    p.raw("</g>")


def sun(p, x, y, r):
    p.wash(circ(x, y, r, 20), SAND, 0.85, edge=0, raw=True)
    p.path(smooth(circ(x, y, r, 20), True), w=2.4)
    rays = []
    for i in range(28):
        a = TAU * i / 28
        r0, r1 = r + 12, r + (34 if i % 2 == 0 else 22)
        rays.append(((x + r0 * math.cos(a), y + r0 * math.sin(a)), (x + r1 * math.cos(a), y + r1 * math.sin(a))))
    p.segs(rays, 1.6)


def range_staff(p, x, y, half, rng_half):
    p.wash([(x - 13, y - rng_half), (x + 13, y - rng_half), (x + 13, y + rng_half), (x - 13, y + rng_half)],
           BLUE, 0.5, sharp=True, edge=0, amp=0.5)
    p.segs([((x, y - half), (x, y + half))], 2.0)
    p.segs([((x - (9 if i % 5 == 0 else 5), y - half + i * (2 * half / 20)), (x + (9 if i % 5 == 0 else 5), y - half + i * (2 * half / 20)))
            for i in range(21)], 1.0)
    p.arrow((x + 34, y + 6), (x + 34, y + rng_half), 1.6, TERRA, 10)
    p.arrow((x + 34, y - 6), (x + 34, y - rng_half), 1.6, TERRA, 10)
    p.segs([((x + 22, y - rng_half), (x + 46, y - rng_half)), ((x + 22, y + rng_half), (x + 46, y + rng_half))], 1.2, TERRA)


def sizigia():
    p = Plate("sizigia", 1600, 1000, 404)
    y1, y2 = 318, 764
    SX, EX = 222, 900
    # row 1
    p.text(96, 124, "Sizigia · mareas vivas", size=36)
    p.segs([((98, 144), (170, 144))], 3.0, TERRA)
    p.segs([((SX + 130, y1), (1330, y1))], 1.0, INK, 0.6, dash="3 9")
    sun(p, SX, y1, 80)
    earth(p, EX, y1, 64, 156, 84)
    moon_glyph(p, 610, y1, 25, "new")
    moon_glyph(p, 1210, y1, 25, "full")
    p.wash(circ(1203, y1 - 6, 7, 8), PALE, 0.9, edge=0, raw=True)
    p.wash(circ(1218, y1 + 8, 5, 8), PALE, 0.9, edge=0, raw=True)
    p.text(SX, y1 + 152, "Sol", size=24, anchor="middle")
    p.text(610, y1 + 66, "Luna nueva", size=24, anchor="middle")
    p.text(EX, y1 + 130, "Tierra", size=24, anchor="middle")
    p.text(1210, y1 + 66, "Luna llena", size=24, anchor="middle")
    range_staff(p, 1420, y1, 100, 84)
    # divider
    p.ink([(96, 520), (760, 520)], w=0.9, step=80, amp=0.6, op=0.6)
    p.ink([(840, 520), (1504, 520)], w=0.9, step=80, amp=0.6, op=0.6)
    p.path("M800 510L810 520L800 530L790 520Z", w=1.0, fill=TERRA, color=TERRA)
    # row 2
    p.text(96, 596, "Cuadratura · mareas muertas", size=36)
    p.segs([((98, 616), (170, 616))], 3.0, TERRA)
    p.segs([((SX + 130, y2), (EX - 110, y2))], 1.0, INK, 0.6, dash="3 9")
    p.segs([((EX, y2 - 112), (EX, y2 - 150))], 1.0, INK, 0.6, dash="3 9")
    sun(p, SX, y2, 80)
    earth(p, EX, y2, 64, 92, 102)
    moon_glyph(p, EX, y2 - 180, 25, "half", dark="right")
    arc = circ(EX, y2, 136, 12, math.pi, math.pi * 1.5)
    p.path(smooth(arc), w=1.0, op=0.7)
    p.text(EX - 122, y2 - 112, "90°", size=20, italic=False, anchor="middle")
    p.text(SX, y2 + 152, "Sol", size=24, anchor="middle")
    p.text(EX, y2 + 146, "Tierra", size=24, anchor="middle")
    p.text(EX + 46, y2 - 172, "Cuarto creciente", size=24)
    range_staff(p, 1420, y2, 100, 36)
    return p


# --------------------------------------------------------------------------
# 5 · curva de marea
# --------------------------------------------------------------------------

def curva():
    p = Plate("curva-marea", 1600, 900, 505)
    X0, X1, YM = 210, 1500, 470
    T = 12.42

    def X(t):
        return X0 + (X1 - X0) * t / 360.0

    def A(t):
        return 158 + 66 * math.cos(TAU * (t - 42) / 336.0)

    def Y(t):
        return YM - A(t) * math.cos(TAU * (t - 3) / T)

    # envelope wash
    top = [(X(t), YM - A(t)) for t in range(0, 361, 12)]
    bot = [(X(t), YM + A(t)) for t in range(360, -1, -12)]
    p.raw('<path d="%sL%s %s%sZ" fill="%s" fill-opacity=".55" stroke="none"/>'
          % (smooth(top), n(bot[0][0]), n(bot[0][1]), curve_cmds(bot), PALE))
    p.path(smooth(top), w=1.1, color=BLUE, dash="8 7")
    p.path(smooth(bot[::-1]), w=1.1, color=BLUE, dash="8 7")
    # axes
    p.segs([((X0, 214), (X0, 770)), ((X0, 770), (X1 + 10, 770))], 1.6)
    p.segs([((X0 - 7, YM + k * 56), (X0, YM + k * 56)) for k in range(-4, 5)], 1.1)
    p.segs([((X(d * 24), 770), (X(d * 24), 780)) for d in range(0, 16)], 1.1)
    for d in range(1, 16):
        p.text(X((d - 0.5) * 24), 808, str(d), size=21, italic=False, anchor="middle")
    p.text(X1 + 10, 842, "días", size=21, anchor="end")
    # mean level
    p.segs([((X0, YM), (X1, YM))], 1.3, TERRA, dash="10 7")
    p.text(X0 - 16, YM + 7, "nivel medio", size=22, anchor="end", fill=TERRA)
    # the curve
    pts = []
    t = 0.0
    while t <= 360.0001:
        pts.append((X(t), Y(t)))
        t += T / 10
    p.path(smooth(pts), w=2.4)
    # moons
    for t_, ph in ((12, "new"), (186, "half"), (348, "full")):
        moon_glyph(p, X(t_), 118, 21, ph, dark="left")
        p.segs([((X(t_), 148), (X(t_), 166))], 1.0, INK, 0.6)
    p.text(X(42), 196, "mareas vivas", size=24, anchor="middle")
    p.text(X(210), 196, "mareas muertas", size=24, anchor="middle")
    p.text(X1, 196, "mareas vivas", size=24, anchor="end")
    # pleamar / bajamar
    th = 3 + 8 * T
    p.segs([((X(th), Y(th) - 8), (X(th) + 26, Y(th) - 40))], 1.0)
    p.dots([(X(th), Y(th))], 7, TERRA)
    p.text(X(th) + 32, Y(th) - 44, "pleamar", size=23)
    tl = 3 + T / 2 + 20 * T
    p.segs([((X(tl), Y(tl) + 8), (X(tl) + 26, Y(tl) + 44))], 1.0)
    p.dots([(X(tl), Y(tl))], 7, TERRA)
    p.text(X(tl) + 32, Y(tl) + 58, "bajamar", size=23)
    return p


# --------------------------------------------------------------------------
# 6 · el mareógrafo (corte)
# --------------------------------------------------------------------------

def callout(p, num, cx, cy, tx, ty):
    ang = math.atan2(ty - cy, tx - cx)
    p.segs([((cx + 21 * math.cos(ang), cy + 21 * math.sin(ang)), (tx, ty))], 1.2, TERRA)
    p.dots([(tx, ty)], 6, TERRA)
    p.raw('<circle cx="%s" cy="%s" r="20" fill="%s" stroke="%s" stroke-width="1.8"/>' % (n(cx), n(cy), PAPER, TERRA))
    p.text(cx, cy + 8.5, str(num), size=25, italic=False, anchor="middle", fill=TERRA)


def mareografo():
    p = Plate("mareografo", 1200, 1500, 606)
    WL, BED, DECK, FLOOR = 1046, 1368, 678, 660
    TX0, TX1, TW = 330, 430, 9          # stilling well
    # water outside
    surf = [(40, WL)]
    x = 40
    while x < 1170:
        x += 46
        surf.append((x, WL + (5 if (x // 46) % 2 else -5) + p.r.uniform(-2, 2)))
    water = surf + [(1170, 1470), (40, 1470)]
    p.defs.append('<linearGradient id="wg" gradientUnits="userSpaceOnUse" x1="0" y1="%d" x2="0" y2="%d">'
                  '<stop offset="0" stop-color="%s" stop-opacity=".34"/>'
                  '<stop offset="1" stop-color="%s" stop-opacity=".66"/></linearGradient>' % (WL, BED, BLUE, BLUE))
    p.raw('<path d="%sL1170 1470L40 1470Z" fill="url(#wg)" stroke="none"/>' % smooth(surf))
    p.path(smooth(surf), w=1.6)
    rip = []
    for i in range(46):
        rx, ry = p.r.uniform(60, 1130), p.r.uniform(WL + 26, BED - 40)
        ln = p.r.uniform(26, 70)
        rip.append(((rx, ry), (rx + ln, ry + p.r.uniform(-1.5, 1.5))))
    p.segs(rip, 0.9, INK, 0.35)
    # sea bed
    bed = [(40, BED + 6), (180, BED - 10), (330, BED + 4), (520, BED - 4), (700, BED + 10), (880, BED - 8),
           (1040, BED + 2), (1170, BED - 6)]
    p.raw('<path d="%sL1170 1470L40 1470Z" fill="%s" stroke="none"/>' % (smooth(bed), PAPER))
    p.raw('<path d="%sL1170 1470L40 1470Z" fill="%s" fill-opacity=".85" stroke="none"/>' % (smooth(bed), SAND))
    p.path(smooth(bed), w=1.8)
    bd = [(p.r.uniform(55, 1145), p.r.uniform(BED + 16, 1445)) for _ in range(240)]
    p.dots(bd, 1.6, INK, 0.5)
    # pier piles and braces
    piles = [610, 850, 1090]
    p.segs([((620, DECK + 60), (840, WL - 60)), ((840, DECK + 60), (620, WL - 60)),
            ((860, DECK + 60), (1080, WL - 60)), ((1080, DECK + 60), (860, WL - 60))], 1.6, INK, 0.85)
    for px in piles:
        p.path(p.sharp([(px - 12, DECK + 18), (px + 12, DECK + 18), (px + 11, BED + 6), (px - 11, BED + 6)],
                       amp=0.7, closed=False), w=2.2, fill=PAPER)
        p.wash([(px + 2, DECK + 20), (px + 11, DECK + 20), (px + 10, BED + 4), (px + 2, BED + 4)], BLUE, 0.3, sharp=True, edge=0, amp=0.5)
        p.path(smooth([(px - 22, WL + 2), (px, WL + 6), (px + 22, WL + 2)]), w=1.2)
    # deck (with the opening for the well)
    for (a, b) in ((170, TX0 - 2), (TX1 + 2, 1175)):
        p.path(p.sharp([(a, FLOOR), (b, FLOOR), (b, DECK), (a, DECK)], step=120, amp=0.6), w=2.2, fill=SAND)
    p.segs([((x, FLOOR + 3), (x, DECK - 3)) for x in list(range(196, TX0 - 6, 38)) + list(range(TX1 + 30, 1160, 38))], 0.8, INK, 0.5)
    p.segs([((x, DECK), (x, DECK + 16)) for x in (200, 300, 470, 730, 970)], 5.0, INK, 0.85)
    # the stilling well
    p.path("M%d %dV%dH%dV%dZ" % (TX0, FLOOR, 1336, TX1, FLOOR), w=0, color=None, fill=PAPER)
    p.path("M%d %dV%dH%dV%dZ" % (TX0 + TW, WL, 1336 - TW, TX1 - TW, WL), w=0, color=None, fill="url(#wg)")
    for (a, b) in ((TX0, TX0 + TW), (TX1 - TW, TX1)):
        p.path("M%d %dV%dH%dV%dZ" % (a, FLOOR, 1336, b, FLOOR), w=1.4, fill=INK, fop=0.55)
    p.path("M%d %dH%dV%dH%dZ" % (TX0, 1336 - TW, TX1, 1336, TX0), w=1.4, fill=INK, fop=0.55)
    # inlet near the bottom (left wall)
    p.path("M%d 1278H%dV1296H%dZ" % (TX0 - 1, TX0 + TW + 1, TX0 - 1), w=0, color=None, fill="#b4c4c7")
    p.segs([((TX0, 1278), (TX0 + TW, 1278)), ((TX0, 1296), (TX0 + TW, 1296))], 1.4)
    p.arrow((262, 1287), (318, 1287), 1.4, INK, 9)
    p.arrow((352, 1287), (388, 1287), 1.2, INK, 8, op=0.7)
    p.segs([((TX0 + TW, WL), (TX1 - TW, WL))], 1.6)
    # straps holding the well to the deck framing
    for yy in (760, 900):
        p.segs([((TX0 - 6, yy), (TX1 + 6, yy))], 3.0)
    # float
    fx0, fx1, fy0, fy1 = 350, 410, WL - 26, WL + 20
    fd = "M%d %dA30 9 0 0 1 %d %dV%dA30 9 0 0 1 %d %dZ" % (fx0, fy0 + 8, fx1, fy0 + 8, fy1 - 8, fx0, fy1 - 8)
    p.path(fd, w=0, color=None, fill=PAPER)
    p.path(fd, w=2.2, fill=SAND, fop=0.95)
    p.path("M%d %dA30 9 0 0 0 %d %d" % (fx0, fy0 + 8, fx1, fy0 + 8), w=1.2)
    p.segs([((380, fy0 - 1), (380, fy0 + 8))], 3.0)
    # hut: section walls, floor is the deck
    HX0, HX1, HT = 250, 900, 306
    p.path("M%d %dH%dV%dH%dZ" % (HX0 + 14, HT, HX1 - 14, FLOOR, HX0 + 14), w=0, color=None, fill=PALE, fop=0.32)
    for (a, b) in ((HX0, HX0 + 14), (HX1 - 14, HX1)):
        rect = [(a, HT), (b, HT), (b, FLOOR), (a, FLOOR)]
        p.path(p.sharp(rect, amp=0.4, step=120), w=2.0, fill=PAPER)
        p.hatch(rect, ang=45, gap=7, w=0.9, op=0.8)
    # back wall boards and a window
    p.segs([((x, HT + 22), (x, FLOOR - 4)) for x in range(HX0 + 60, HX1 - 20, 46)], 0.7, INK, 0.28)
    # roof
    roof = [(HX0 - 40, HT + 4), (575, 150), (HX1 + 40, HT + 4)]
    p.path(p.sharp(roof, amp=0.7, step=120), w=2.8, fill=PAPER)
    p.wash(roof, BLUE, 0.55, sharp=True, edge=0, amp=0.6)
    sh = []
    for i in range(1, 7):
        t = i / 7
        yy = 150 + (HT + 4 - 150) * t
        sh.append(((575 - (575 - HX0 + 40) * t + 10, yy), (575 + (HX1 + 40 - 575) * t - 10, yy)))
    p.segs(sh, 0.9, INK, 0.5)
    p.path(p.sharp([(HX0 - 44, HT), (HX1 + 44, HT), (HX1 + 44, HT + 16), (HX0 - 44, HT + 16)], amp=0.5, step=160), w=2.2, fill=SAND)
    # wire: float -> pulley -> counterweight
    PX, PY, PR = 420, 404, 40
    p.segs([((380, fy0 + 1), (380, PY)), ((460, PY), (460, 548))], 1.5)
    p.path("M380 %dA%d %d 0 0 1 460 %d" % (PY, PR, PR, PY), w=1.5)
    p.segs([((PX - 9, HT + 16), (PX - 4, PY)), ((PX + 9, HT + 16), (PX + 4, PY))], 2.2)
    p.path(smooth(circ(PX, PY, PR - 4, 16), True), w=2.4, fill=PAPER)
    p.path(smooth(circ(PX, PY, PR - 13, 12), True), w=1.0)
    p.segs([((PX + 6 * math.cos(a), PY + 6 * math.sin(a)), (PX + (PR - 13) * math.cos(a), PY + (PR - 13) * math.sin(a)))
            for a in [TAU * i / 6 + 0.3 for i in range(6)]], 1.3)
    p.path(smooth(circ(PX, PY, 13, 10), True), w=1.6, fill=SAND)
    p.dots([(PX, PY)], 5)
    # counterweight
    p.path("M452 548H468L474 612H446Z", w=1.8, fill=INK, fop=0.8)
    p.segs([((460, 540), (460, 548))], 3)
    # recording drum on its clockwork base
    DX0, DX1, DY0, DY1 = 620, 780, 392, 566
    p.path(p.sharp([(596, 578), (804, 578), (804, FLOOR), (596, FLOOR)], amp=0.5, step=120), w=2.2, fill=PAPER)
    p.wash([(596, 578), (804, 578), (804, FLOOR), (596, FLOOR)], SAND, 0.8, sharp=True, edge=0, amp=0.4)
    p.path(smooth(circ(700, 619, 28, 14), True), w=1.8, fill=PAPER)
    p.segs([((700 + 22 * math.cos(TAU * i / 12), 619 + 22 * math.sin(TAU * i / 12)),
             (700 + 26 * math.cos(TAU * i / 12), 619 + 26 * math.sin(TAU * i / 12))) for i in range(12)], 1.0)
    p.segs([((700, 619), (700, 601)), ((700, 619), (713, 626))], 1.8)
    p.dots([(700, 619)], 4)
    p.path("M%d %dV%dA80 15 0 0 0 %d %dV%dA80 15 0 0 0 %d %dZ" % (DX0, DY0, DY1, DX1, DY1, DY0, DX0, DY0), w=0, color=None, fill="#f6f0e2")
    p.path("M%d %dV%dA80 15 0 0 0 %d %dV%d" % (DX0, DY0, DY1, DX1, DY1, DY0), w=2.2)
    p.path(smooth(ell(700, DY0, 80, 15, 18), True), w=2.2, fill=PALE)
    p.wash([(DX1 - 36, DY0 + 10), (DX1 - 1, DY0 + 2), (DX1 - 1, DY1 - 2), (DX1 - 36, DY1 + 9)], BLUE, 0.28, sharp=True, edge=0, amp=0.4)
    for gy in (430, 468, 506, 544):
        p.path("M%d %dA80 15 0 0 0 %d %d" % (DX0, gy, DX1, gy), w=0.6, op=0.35)
    tr = []
    for i in range(41):
        th = math.radians(-82 + 164 * i / 40)
        tr.append((700 + 79 * math.sin(th), 484 + 15 * math.cos(th) + (30 + 9 * math.sin(th * 1.3 + 1)) * math.sin(th * 5.2 + 0.6)))
    p.path(smooth(tr), w=1.6, color=TERRA)
    p.segs([((700, DY0 - 15), (700, DY0 - 30))], 3)
    p.segs([((700, DY1 + 15), (700, 578))], 5)
    # pen on its guide rod, moved by a cord from the pulley shaft
    RX = 822
    p.segs([((RX, 352), (RX, 578))], 2.2)
    p.segs([((RX - 12, 578), (RX + 12, 578))], 3)
    p.path(smooth(circ(RX - 9, 350, 9, 8), True), w=1.6, fill=PAPER)
    p.segs([((PX + 5, PY - 12), (RX - 11, 341.5))], 1.1)
    pen_y = tr[-1][1]
    p.segs([((RX - 18, 350), (RX - 18, pen_y - 12))], 1.1)
    p.path("M%d %sh24v22h-24Z" % (RX - 22, n(pen_y - 12)), w=1.8, fill=SAND)
    p.segs([((RX - 22, pen_y), (DX1 + 6, pen_y))], 2.0)
    p.path("M%d %sl-9 -4v8Z" % (DX1 + 6, n(pen_y)), w=1.0, fill=INK)
    # callouts
    callout(p, 1, 196, 880, TX0 + 4, 880)
    callout(p, 2, 520, 1000, fx1 - 4, WL - 8)
    callout(p, 3, 316, 360, PX - 27, PY - 20)
    callout(p, 4, 538, 600, 471, 590)
    callout(p, 5, 550, 470, DX0 + 3, 470)
    callout(p, 6, 862, 520, RX + 1, pen_y + 6)
    return p


# --------------------------------------------------------------------------
# 7 · el escandallo
# --------------------------------------------------------------------------

def rope(p, pts, w=7.0, twist=True):
    d = smooth(pts)
    p.path(d, w=w)
    p.path(d, w=w - 3.0, color=PAPER)
    if twist:
        dense = resample(pts, 9)
        tk = []
        for i in range(1, len(dense) - 1):
            x, y = dense[i]
            dx, dy = dense[i + 1][0] - dense[i - 1][0], dense[i + 1][1] - dense[i - 1][1]
            L = math.hypot(dx, dy) or 1
            ux, uy = dx / L, dy / L
            nx, ny = -uy, ux
            h = (w - 3.0) / 2
            tk.append(((x + nx * h - ux * 2.2, y + ny * h - uy * 2.2), (x - nx * h + ux * 2.2, y - ny * h + uy * 2.2)))
        p.segs(tk, 0.8, INK, 0.75)


def escandallo():
    p = Plate("escandallo", 1200, 1500, 707)
    LX = 600
    # soft ground shadow
    p.wash(ell(LX + 6, 1322, 150, 20, 14), BLUE, 0.28, edge=0, blur=8, amp=1)
    p.wash(ell(300, 1326, 230, 26, 14), BLUE, 0.25, edge=0, blur=8, amp=1)
    # the peg
    PEG = (LX - 14, 176)
    # rope: coil -> up to the peg -> down to the lead
    drape = [(392, 1232), (372, 1090), (352, 900), (358, 700), (398, 500), (476, 320), (548, 210), (582, 168), (604, 164), (LX + 2, 190)]
    rope(p, drape)
    hang = [(LX + 2, 190), (LX, 300), (LX, 430), (LX, 548)]
    rope(p, hang)
    p.path(smooth(circ(PEG[0], PEG[1], 24, 14), True), w=2.4, fill=PAPER)
    p.wash(circ(PEG[0], PEG[1], 24, 14), SAND, 0.9, edge=0, raw=True)
    p.path(smooth(circ(PEG[0], PEG[1], 15, 10), True), w=1.0, op=0.7)
    p.path(smooth(circ(PEG[0], PEG[1], 6, 8), True), w=1.0, op=0.7)
    rope(p, [(574, 160), (590, 154), (606, 162), (609, 180)], twist=False)
    # marks on the hanging part
    def tag(x, y, tails, side=1):
        pts = [(x, y - 7), (x + side * 26, y - 9), (x + side * 26, y + 9), (x, y + 7)]
        p.path(p.sharp(pts, amp=0.5), w=1.5, fill=SAND)
        for i in range(tails):
            off = (i - (tails - 1) / 2) * 9
            p.path(smooth([(x + side * 26, y + off * 0.6), (x + side * 44, y + off + 3), (x + side * 60, y + off * 1.6 + 12)]),
                   w=4.2)
            p.path(smooth([(x + side * 26, y + off * 0.6), (x + side * 44, y + off + 3), (x + side * 60, y + off * 1.6 + 12)]),
                   w=2.2, color=SAND)
        p.segs([((x - 5, y - 8), (x + 5, y - 8)), ((x - 5, y + 8), (x + 5, y + 8))], 2.2)

    def knot(x, y):
        p.path(smooth([(x - 9, y - 7), (x + 8, y - 9), (x + 11, y + 2), (x + 5, y + 10), (x - 8, y + 8), (x - 11, y)], True),
               w=1.8, fill=PAPER)
        p.segs([((x - 8, y - 2), (x + 8, y - 5)), ((x - 8, y + 5), (x + 8, y + 2))], 1.2)

    tag(LX, 258, 2)
    knot(LX, 332)
    # red bunting
    p.path(smooth([(LX, 398), (LX + 30, 392), (LX + 62, 402), (LX + 88, 396), (LX + 84, 424), (LX + 58, 430), (LX + 28, 420), (LX, 426)], True),
           w=1.4, color=TERRA, fill=TERRA, fop=0.9)
    p.segs([((LX - 5, 398), (LX + 5, 398)), ((LX - 5, 426), (LX + 5, 426))], 2.2)
    knot(LX, 486)
    tag(366, 800, 3, side=-1)
    knot(354, 960)
    # white calico on the drape
    p.path(smooth([(425, 428), (452, 422), (482, 432), (506, 426), (503, 452), (478, 458), (450, 448), (424, 454)], True),
           w=1.5, fill="#f6f0e2")
    p.segs([((419, 430), (430, 427)), ((417, 455), (429, 453))], 2.2)
    # the lead
    TOP, BOT = 590, 1250
    def half(y):
        t = max(0.0, (y - TOP) / (BOT - TOP))
        return 26 + 46 * t ** 0.9
    left = [(LX - half(y), y) for y in range(TOP, BOT + 1, 60)]
    right = [(LX + half(y), y) for y in range(TOP, BOT + 1, 60)]
    hb = half(BOT)
    body = left + [(LX - hb * 0.72, BOT + 17), (LX, BOT + 24), (LX + hb * 0.72, BOT + 17)] + right[::-1] + [(LX + 14, TOP - 14), (LX - 14, TOP - 14)]
    d = smooth(body, True)
    p.path(d, w=0, color=None, fill=PAPER)
    p.path(d, w=0, color=None, fill=BLUE, fop=0.5)
    # facets
    fl = [(LX - half(y) * 0.42, y) for y in range(TOP - 6, BOT + 1, 60)] + [(LX - hb * 0.4, BOT + 21)]
    fr = [(LX + half(y) * 0.42, y) for y in range(TOP - 6, BOT + 1, 60)] + [(LX + hb * 0.4, BOT + 21)]
    lit = fl + fr[::-1]
    p.raw('<path d="M%sZ" fill="%s" fill-opacity=".45" stroke="none"/>' % ("L".join("%s %s" % (n(a), n(b)) for a, b in lit), PAPER))
    shade = fr + [(LX + hb * 0.72, BOT + 17)] + right[::-1]
    p.hatch(shade, ang=62, gap=8, w=0.9, op=0.6)
    p.path(smooth(fl), w=1.1, op=0.8)
    p.path(smooth(fr), w=1.1, op=0.8)
    p.path(d, w=2.8)
    # hollow of the base, seen through
    p.path(smooth([(LX - hb * 0.5, BOT + 14), (LX - hb * 0.3, BOT - 22), (LX, BOT - 34), (LX + hb * 0.3, BOT - 22), (LX + hb * 0.5, BOT + 14)]),
           w=1.2, dash="6 6", op=0.85)
    # nicks and dents
    p.segs([((LX - 40, 900), (LX - 30, 910)), ((LX - 52, 1100), (LX - 44, 1114)), ((LX + 8, 760), (LX + 14, 772)),
            ((LX - 10, 1010), (LX - 2, 1018))], 1.1, INK, 0.7)
    # eye and the bend
    p.path(smooth(ell(LX, TOP - 30, 17, 23, 12), True), w=6.0)
    p.path(smooth(ell(LX, TOP - 30, 17, 23, 12), True), w=2.6, color="#9db0b5")
    rope(p, [(LX, 536), (LX - 8, 556), (LX - 3, 574), (LX + 8, 560), (LX + 2, 540)], twist=False)
    knot(LX, 532)
    # coil
    for i in range(6):
        cy = 1300 - i * 13
        rx, ry = 170 - i * 9, 40 - i * 2
        e = ell(300 + p.r.uniform(-4, 4), cy, rx, ry, 22, rot=p.r.uniform(-0.03, 0.03))
        d2 = smooth(e, True)
        p.path(d2, w=8.5)
        p.path(d2, w=5.5, color=PAPER)
        tk = []
        dense = resample(e + [e[0]], 10)
        for j in range(1, len(dense) - 1):
            x, y = dense[j]
            dx, dy = dense[j + 1][0] - dense[j - 1][0], dense[j + 1][1] - dense[j - 1][1]
            L = math.hypot(dx, dy) or 1
            ux, uy = dx / L, dy / L
            tk.append(((x - uy * 2.6 - ux * 2.4, y + ux * 2.6 - uy * 2.4), (x + uy * 2.6 + ux * 2.4, y - ux * 2.6 + uy * 2.4)))
        p.segs(tk, 0.8, INK, 0.7)
    rope(p, [(392, 1232), (398, 1246), (410, 1252)], twist=False)
    # inset: the armed base
    IX, IY, IR = 930, 1010, 168
    p.segs([((LX + hb * 0.6, BOT + 6), (IX - IR * 0.86, IY + IR * 0.52))], 1.0, INK, 0.7, dash="2 7")
    p.path(smooth(circ(IX, IY, IR, 28), True), w=2.4, fill=PAPER)
    cid = p.nid("i")
    p.defs.append('<clipPath id="%s"><circle cx="%d" cy="%d" r="%d"/></clipPath>' % (cid, IX, IY, IR - 2))
    p.raw('<g clip-path="url(#%s)">' % cid)
    bx, by = IX, IY + 30
    wall = [(bx - 118, by), (bx - 100, by - 200), (bx + 100, by - 200), (bx + 118, by)]
    p.raw('<path d="M%d %dL%d %dL%d %dL%d %dZ" fill="%s" fill-opacity=".5" stroke="none"/>'
          % (wall[0][0], wall[0][1], wall[1][0], wall[1][1], wall[2][0], wall[2][1], wall[3][0], wall[3][1], BLUE))
    p.hatch([(bx + 40, by - 200), (bx + 100, by - 200), (bx + 118, by), (bx + 46, by + 40)], ang=62, gap=8, w=0.9, op=0.55)
    p.segs([((bx - 118, by), (bx - 100, by - 200)), ((bx + 118, by), (bx + 100, by - 200)),
            ((bx - 48, by - 30), (bx - 40, by - 200)), ((bx + 48, by - 30), (bx + 40, by - 200))], 1.6)
    p.path(smooth(ell(bx, by, 118, 58, 22), True), w=2.6, fill="#b4c4c7")
    tal = ell(bx, by + 4, 82, 36, 18)
    p.path(smooth(tal, True), w=1.8, fill="#f6f0e2")
    p.wash(ell(bx + 8, by + 12, 60, 20, 12), SAND, 0.6, edge=0, raw=True)
    gr = []
    while len(gr) < 90:
        q = (p.r.uniform(bx - 80, bx + 80), p.r.uniform(by - 30, by + 40))
        if inside(q, ell(bx, by + 4, 72, 29, 18)):
            gr.append(q)
    p.dots(gr[:60], 2.2, INK, 0.8)
    p.dots(gr[60:78], 3.6, INK, 0.85)
    p.dots(gr[78:], 3.0, TERRA, 0.9)
    for (sx, sy, rot) in ((bx - 34, by + 2, 0.4), (bx + 30, by + 14, -0.5), (bx + 6, by - 12, 2.2)):
        cockle(p, sx, sy, 11, rot, ribs=5)
    p.raw("</g>")
    p.path(smooth(circ(IX, IY, IR, 28), True), w=2.4)
    return p


def cockle(p, x, y, r, rot=0.0, ribs=9, wash=True):
    c, s = math.cos(rot), math.sin(rot)

    def T(a, b):
        return (x + a * c - b * s, y + a * s + b * c)

    hinge = (0, r * 0.62)
    arc = []
    k = ribs * 2
    for i in range(k + 1):
        a = math.radians(-168 + 156 * i / k)
        rr = r * (1.0 if i % 2 else 0.95)
        arc.append((rr * math.cos(a) * 1.06, rr * math.sin(a) * 0.95 + r * 0.12))
    out = [T(-r * 0.5, r * 0.5)] + [T(a, b) for a, b in arc] + [T(r * 0.5, r * 0.5), T(*hinge)]
    d = smooth(out, True)
    p.path(d, w=max(1.1, r * 0.06), fill="#d9ddd4" if wash else PAPER)
    hx, hy = T(*hinge)
    p.segs([((hx, hy), T(arc[i][0] * 0.96, arc[i][1] * 0.96)) for i in range(1, k, 2)], max(0.7, r * 0.035), INK, 0.75)


# --------------------------------------------------------------------------
# 8 · el reloj de mareas
# --------------------------------------------------------------------------

def reloj():
    p = Plate("reloj-mareas", 1200, 1500, 808)
    CX, CY, RO, RI = 600, 800, 440, 380
    p.wash(circ(CX + 30, CY + 36, RO + 6, 26), BLUE, 0.26, edge=0, blur=8, raw=True)
    # nail and ring
    p.path(smooth(circ(CX, CY - RO - 26, 27, 12), True), w=6.5)
    p.path(smooth(circ(CX, CY - RO - 26, 27, 12), True), w=3.0, color=SAND)
    p.segs([((CX - 2, CY - RO - 56), (CX + 9, CY - RO - 66))], 5.0)
    p.dots([(CX - 2, CY - RO - 55)], 12)
    p.path("M%d %dh36v26h-36Z" % (CX - 18, CY - RO - 6), w=2.0, fill=SAND)
    # wooden case
    p.path(smooth(circ(CX, CY, RO, 40), True), w=0, color=None, fill=PAPER)
    p.path(smooth(circ(CX, CY, RO, 40), True), w=0, color=None, fill=SAND, fop=0.95)
    grain = []
    for i in range(46):
        rr = p.r.uniform(RI + 8, RO - 8)
        a0 = p.r.uniform(0, TAU)
        a1 = a0 + p.r.uniform(0.25, 0.9)
        p.path(smooth(circ(CX, CY, rr, 6, a0, a1)), w=0.8, op=0.38)
    p.wash(crescent(CX, CY, RO - 2, -22, -26, RO - 6, 60), BLUE, 0.3, edge=0, raw=True)
    p.path(smooth(circ(CX, CY, RO, 40), True), w=3.0)
    p.path(smooth(circ(CX, CY, RI, 36), True), w=2.4, fill="#f6f0e2")
    p.path(smooth(circ(CX, CY, RI - 10, 36), True), w=0.9, op=0.7)
    p.wash(crescent(CX, CY, RI - 2, 16, 20, RI - 8, 60), BLUE, 0.16, edge=0, raw=True)
    # ticks
    major, minor = [], []
    for i in range(60):
        a = TAU * i / 60 - TAU / 4
        r0 = RI - 54 if i % 5 == 0 else RI - 36
        seg = ((CX + r0 * math.cos(a), CY + r0 * math.sin(a)), (CX + (RI - 22) * math.cos(a), CY + (RI - 22) * math.sin(a)))
        (major if i % 5 == 0 else minor).append(seg)
    p.segs(major, 3.2)
    p.segs(minor, 1.1)
    p.path(smooth(circ(CX, CY, RI - 22, 36), True), w=1.2)
    # numerals
    for k in range(1, 6):
        for side in (1, -1):
            a = side * TAU * k / 12 - TAU / 4
            rr = RI - 104
            p.text(CX + rr * math.cos(a), CY + rr * math.sin(a) + 21, str(6 - k if side == 1 else k), size=62,
                   italic=False, anchor="middle")
    # markers at high and low water
    p.path("M%d %dl-17 -30h34Z" % (CX, CY - RI + 96), w=1.6, fill=INK)
    p.path("M%d %dl-17 30h34Z" % (CX, CY + RI - 96), w=1.6)
    p.text(CX, CY - 146, "PLEAMAR", size=44, italic=False, anchor="middle", ls=10)
    p.text(CX, CY + 188, "BAJAMAR", size=44, italic=False, anchor="middle", ls=10)
    # falling / rising arrows
    r3 = 206
    arc_r = circ(CX, CY, r3, 12, math.radians(-38), math.radians(38))
    arc_l = circ(CX, CY, r3, 12, math.radians(142), math.radians(218))
    p.path(smooth(arc_r), w=1.3, op=0.8)
    p.path(smooth(arc_l), w=1.3, op=0.8)
    p.arrow(arc_r[-2], arc_r[-1], 1.3, INK, 13, op=0.8)
    p.arrow(arc_l[-2], arc_l[-1], 1.3, INK, 13, op=0.8)
    # wave ornament
    for j, yy in enumerate((CY + 84, CY + 102)):
        pts = [(CX - 84 + 12 * i + j * 6, yy + (5 if i % 2 else -5)) for i in range(14 if j == 0 else 13)]
        p.path(smooth(pts), w=1.6, color=BLUE)
    # the single hand
    ang = math.radians(51) - TAU / 4
    ux, uy = math.cos(ang), math.sin(ang)
    vx, vy = -uy, ux
    L, T_ = 296, 86
    hand = [(CX + ux * L, CY + uy * L), (CX + ux * 60 + vx * 15, CY + uy * 60 + vy * 15), (CX + vx * 9, CY + vy * 9),
            (CX - ux * T_ + vx * 5, CY - uy * T_ + vy * 5), (CX - ux * (T_ + 12), CY - uy * (T_ + 12)),
            (CX - ux * T_ - vx * 5, CY - uy * T_ - vy * 5), (CX - vx * 9, CY - vy * 9), (CX + ux * 60 - vx * 15, CY + uy * 60 - vy * 15)]
    p.raw('<path d="M%sZ" fill="%s" fill-opacity=".22" stroke="none" transform="translate(9 11)"/>'
          % ("L".join("%s %s" % (n(a), n(b)) for a, b in hand), INK))
    p.path("M" + "L".join("%s %s" % (n(a), n(b)) for a, b in hand) + "Z", w=1.8, fill=TERRA)
    p.path(smooth(circ(CX - ux * (T_ - 26), CY - uy * (T_ - 26), 13, 10), True), w=1.8, fill=PAPER)
    p.path(smooth(circ(CX, CY, 20, 12), True), w=2.2, fill=SAND)
    p.dots([(CX, CY)], 8)
    return p


# --------------------------------------------------------------------------
# 9 · rastrillo y cesta
# --------------------------------------------------------------------------

def footprint(p, x, y, ang, s=1.0, color=BLUE, op=0.4, line=True):
    c, sn = math.cos(ang), math.sin(ang)

    def T(pts):
        return [(x + (a * c - b * sn) * s, y + (a * sn + b * c) * s) for a, b in pts]

    boot = [(42, 0), (36, -13), (16, -17), (-6, -13), (-22, -10), (-40, -12), (-52, -8), (-56, 0), (-52, 9), (-40, 12),
            (-22, 8), (-6, 10), (16, 15), (36, 12)]
    d = smooth(T(boot), True)
    if line:
        p.path(d, w=1.0, fill=color, fop=op)
        p.segs([(T([(-24, -8)])[0], T([(-24, 6)])[0]), (T([(8, -13)])[0], T([(8, 11)])[0]), (T([(22, -13)])[0], T([(22, 11)])[0])],
               0.9, INK, 0.45)
    else:
        p.path(d, w=0, color=None, fill=color, fop=op)


def rastrillo():
    p = Plate("rastrillo", 1600, 1000, 909)
    p.raw('<rect x="40" y="40" width="1520" height="920" fill="%s" fill-opacity=".5" stroke="none"/>' % SAND)
    # the retreating water at the top
    edge = [(40, 150)]
    x = 40
    while x < 1570:
        x += p.r.uniform(60, 110)
        edge.append((x, 150 + 22 * math.sin(x / 160.0) + p.r.uniform(-7, 7)))
    p.raw('<path d="%sL1570 30L40 30Z" fill="%s" stroke="none"/>' % (smooth(edge), PAPER))
    p.raw('<path d="%sL1570 30L40 30Z" fill="%s" fill-opacity=".55" stroke="none"/>' % (smooth(edge), BLUE))
    p.path(smooth(edge), w=1.8)
    p.path(smooth([(a, b + 13 + 4 * math.sin(a / 70.0)) for a, b in edge]), w=1.0, op=0.5, dash="46 18")
    p.path(smooth([(a, b - 30 + 5 * math.sin(a / 90.0)) for a, b in edge]), w=0.9, op=0.4, dash="60 40")
    p.path(smooth([(a, b - 58 + 4 * math.sin(a / 50.0)) for a, b in edge]), w=0.8, op=0.3, dash="30 50")
    # pools
    pools = [(560, 318, 190, 38), (1220, 262, 220, 30), (640, 868, 250, 40), (1392, 560, 130, 26), (118, 560, 58, 20)]
    for (cx, cy, rx, ry) in pools:
        e = [(cx + rx * math.cos(TAU * i / 16) * (1 + p.r.uniform(-0.12, 0.12)),
              cy + ry * math.sin(TAU * i / 16) * (1 + p.r.uniform(-0.2, 0.2))) for i in range(16)]
        p.wash(e, PALE, 0.9, edge=0, raw=True)
        p.wash(e, BLUE, 0.22, edge=0, raw=True)
        p.path(smooth(e, True), w=1.0, op=0.55, dash="70 14")
    # ripples in the sand
    y = 210
    row = 0
    rip = []
    while y < 940:
        x = 60 + (row % 2) * 40 + p.r.uniform(0, 40)
        while x < 1520:
            ln = p.r.uniform(60, 170)
            pts = [(x + ln * i / 4, y + (4 if i % 2 else -3) + p.r.uniform(-2, 2)) for i in range(5)]
            rip.append(smooth(pts))
            x += ln + p.r.uniform(40, 110)
        y += 36
        row += 1
    p.path("".join(rip), w=0.9, op=0.3)
    # footprints going out to the water
    fy, i = 900.0, 0
    while fy > 196:
        s = 1.0 - i * 0.05
        fx = 196 + (900 - fy) * 0.115
        footprint(p, fx + (27 if i % 2 else -27) * s, fy, -TAU / 4 + 0.1 + (0.12 if i % 2 else -0.12), s)
        fy -= 84 * s
        i += 1
    # rake
    A, B = (1452, 232), (640, 566)
    dx, dy = B[0] - A[0], B[1] - A[1]
    L = math.hypot(dx, dy)
    ux, uy = dx / L, dy / L
    vx, vy = -uy, ux
    def Q(al, ac):
        return (B[0] + ux * al + vx * ac, B[1] + uy * al + vy * ac)
    # shadow
    sh = [Q(-L, -5), Q(0, -6), Q(0, 6), Q(-L, 5)]
    p.raw('<path d="M%sZ" fill="%s" fill-opacity=".25" stroke="none" transform="translate(10 16)"/>'
          % ("L".join("%s %s" % (n(a), n(b)) for a, b in sh), INK))
    handle = [Q(-L, -5.5), Q(10, -7.5), Q(10, 7.5), Q(-L, 5.5)]
    p.path(p.sharp(handle, amp=0.6, step=140), w=2.0, fill=PAPER)
    p.wash(handle, SAND, 0.95, sharp=True, edge=0, amp=0.5)
    p.segs([(Q(-L + 30, 1.5), Q(-L * 0.55, 2)), (Q(-L * 0.5, -2), Q(-40, -2.5))], 0.8, INK, 0.5)
    p.path(smooth(circ(A[0], A[1], 6, 8), True), w=1.6, fill=SAND)
    # head
    HW = 132
    p.segs([(Q(-150, 0), Q(8, -HW + 14)), (Q(-150, 0), Q(8, HW - 14))], 3.0)
    teeth = []
    for i in range(13):
        ac = -HW + 8 + (2 * HW - 16) * i / 12
        t0, t1, t2 = Q(18, ac), Q(46, ac + 1), Q(70, ac + 4)
        p.path(smooth([t0, t1, t2]), w=4.4)
        p.path(smooth([t0, t1, (t2[0] - ux * 5, t2[1] - uy * 5)]), w=1.6, color="#9db0b5")
    bar = [Q(2, -HW), Q(22, -HW), Q(22, HW), Q(2, HW)]
    p.raw('<path d="M%sZ" fill="%s" fill-opacity=".25" stroke="none" transform="translate(10 16)"/>'
          % ("L".join("%s %s" % (n(a), n(b)) for a, b in bar), INK))
    p.path(p.sharp(bar, amp=0.5, step=120), w=2.2, fill=PAPER)
    p.wash(bar, SAND, 0.95, sharp=True, edge=0, amp=0.4)
    p.dots([Q(12, -HW + 16), Q(12, HW - 16), Q(12, 0)], 4.5)
    # furrows left by the rake
    fur = []
    for i in range(13):
        ac = -HW + 8 + (2 * HW - 16) * i / 12
        fur.append((Q(84, ac + 5), Q(84 + p.r.uniform(70, 150), ac + 5)))
    p.segs(fur, 1.2, INK, 0.45)
    # basket
    BX, BY = 1150, 690
    RX_, RY_ = 196, 58
    base_c, brx, bry = (BX, BY + 176), 150, 40
    p.wash(ell(BX + 30, BY + 206, 210, 40, 16), INK, 0.2, edge=0, blur=8, raw=True)
    front_rim = ell(BX, BY, RX_, RY_, 16, a0=0, a1=math.pi)
    front_base = ell(base_c[0], base_c[1], brx, bry, 16, a0=0, a1=math.pi)
    body = front_rim[::-1] + front_base
    d = smooth([(BX - RX_, BY)] + [(BX - RX_ + (RX_ - brx) * t - 4 * math.sin(math.pi * t), BY + 176 * t) for t in (0.33, 0.66)]
               + front_base[::-1] + [(BX + RX_ - (RX_ - brx) * t + 4 * math.sin(math.pi * t), BY + 176 * t) for t in (0.66, 0.33)]
               + [(BX + RX_, BY)]) + curve_cmds(front_rim) + "Z"
    d_out = smooth([(BX - RX_, BY)] + [(BX - RX_ + (RX_ - brx) * t - 4 * math.sin(math.pi * t), BY + 176 * t) for t in (0.33, 0.66)]
                   + front_base[::-1] + [(BX + RX_ - (RX_ - brx) * t + 4 * math.sin(math.pi * t), BY + 176 * t) for t in (0.66, 0.33)]
                   + [(BX + RX_, BY)])
    # inside of the basket (back wall) and the catch
    rim_full = ell(BX, BY, RX_, RY_, 28)
    p.path(smooth(rim_full, True), w=0, color=None, fill=PAPER)
    p.path(smooth(rim_full, True), w=0, color=None, fill=BLUE, fop=0.45)
    cid = p.nid("k")
    p.defs.append('<clipPath id="%s"><path d="%s"/></clipPath>' % (cid, smooth(rim_full, True)))
    p.raw('<g clip-path="url(#%s)">' % cid)
    shells = []
    for row_ in range(4):
        yy = BY - 14 + row_ * 21
        cnt = 7 + row_
        for i in range(cnt):
            shells.append((BX - 168 + 336 * (i + 0.5) / cnt + p.r.uniform(-8, 8), yy + p.r.uniform(-5, 5), p.r.uniform(-1.2, 1.2)))
    for (sx, sy, rot) in shells:
        cockle(p, sx, sy, 24, rot, ribs=6)
    p.raw("</g>")
    # woven body
    p.raw('<path d="%s" fill="%s" stroke="none"/>' % (d, PAPER))
    p.raw('<path d="%s" fill="%s" fill-opacity=".95" stroke="none"/>' % (d, SAND))
    cid2 = p.nid("k")
    p.defs.append('<clipPath id="%s"><path d="%s"/></clipPath>' % (cid2, d))
    p.raw('<g clip-path="url(#%s)">' % cid2)
    p.wash([(BX + 60, BY), (BX + 210, BY - 10), (BX + 210, BY + 240), (BX + 40, BY + 240)], BLUE, 0.3, edge=0, raw=True)
    stakes = []
    ns = 17
    for i in range(ns + 1):
        a = math.pi * i / ns
        top = (BX + RX_ * math.cos(a), BY + RY_ * math.sin(a))
        bot = (base_c[0] + brx * math.cos(a), base_c[1] + bry * math.sin(a))
        stakes.append((top, bot))
    p.segs(stakes, 1.3, INK, 0.75)
    nb = 9
    for j in range(1, nb):
        t = j / nb
        rxj, ryj = RX_ + (brx - RX_) * t - 4 * math.sin(math.pi * t), RY_ + (bry - RY_) * t
        cyj = BY + 176 * t
        weave = []
        for i in range(ns):
            if (i + j) % 2:
                a0, a1 = math.pi * (i + 0.08) / ns, math.pi * (i + 0.92) / ns
                am = (a0 + a1) / 2
                p0 = (BX + rxj * math.cos(a0), cyj + ryj * math.sin(a0))
                pm = (BX + rxj * math.cos(am), cyj + ryj * math.sin(am) + 3.5)
                p1 = (BX + rxj * math.cos(a1), cyj + ryj * math.sin(a1))
                weave.append("M%s %sQ%s %s %s %s" % (n(p0[0]), n(p0[1]), n(pm[0]), n(pm[1] + 3), n(p1[0]), n(p1[1])))
        p.path("".join(weave), w=2.6, op=0.85)
    p.raw("</g>")
    p.path(d_out, w=2.6)
    # rim and hoop handle
    p.path(smooth(rim_full, True), w=7.0)
    p.path(smooth(rim_full, True), w=3.6, color=SAND)
    hoop = [(BX - RX_ + 4, BY - 2), (BX - RX_ + 26, BY - 120), (BX - 90, BY - 214), (BX, BY - 238), (BX + 90, BY - 214),
            (BX + RX_ - 26, BY - 120), (BX + RX_ - 4, BY - 2)]
    p.path(smooth(hoop), w=9.0)
    p.path(smooth(hoop), w=5.6, color=SAND)
    p.segs([((BX - RX_ - 4, BY + 3), (BX - RX_ + 14, BY - 6)), ((BX + RX_ + 4, BY + 3), (BX + RX_ - 14, BY - 6))], 4.0)
    # scattered cockles
    for (sx, sy, rr, rot) in ((470, 742, 34, 0.3), (566, 838, 28, -0.9), (742, 716, 30, 2.0), (836, 868, 36, 0.9),
                              (902, 760, 26, -2.4), (390, 874, 30, 1.5), (1420, 880, 30, -0.4), (930, 318, 24, 0.8)):
        p.wash(ell(sx + 6, sy + 9, rr * 1.05, rr * 0.6, 10), INK, 0.18, edge=0, raw=True)
        cockle(p, sx, sy, rr, rot)
    return p


# --------------------------------------------------------------------------
# 10 · salicornia
# --------------------------------------------------------------------------

LEAF = "#d3dbd2"      # pale wash over paper, opaque so that stems overlap cleanly
BLUSH = "#cc9a83"     # terracotta wash over paper


def capsule(a, b, hw, prof=((0.0, 0.5), (0.2, 0.95), (0.55, 1.0), (0.85, 0.9), (1.0, 0.6))):
    dx, dy = b[0] - a[0], b[1] - a[1]
    L = math.hypot(dx, dy) or 1.0
    ux, uy = dx / L, dy / L
    vx, vy = -uy, ux
    left = [(a[0] + ux * t * L + vx * hw * k, a[1] + uy * t * L + vy * hw * k) for t, k in prof]
    right = [(a[0] + ux * t * L - vx * hw * k, a[1] + uy * t * L - vy * hw * k) for t, k in prof]
    return left + right[::-1]


def salicornia():
    p = Plate("salicornia", 1100, 1500, 1010)
    BASE = (440, 1190)
    segs = []
    joints = []

    def grow(start, ang, nseg, slen, hw, depth):
        x, y = start
        for i in range(nseg):
            terminal = i == nseg - 1
            ln = slen * (1.3 if terminal else 1.0) * p.r.uniform(0.94, 1.06)
            end = (x + ln * math.cos(ang), y + ln * math.sin(ang))
            segs.append(((x, y), end, hw * (1 - 0.035 * i) * (0.85 if terminal else 1.0), depth, terminal))
            x, y = end
            if depth == 0:
                joints.append(end)
            if not terminal:
                if depth == 0 and i >= 1:
                    cn = max(1, min(5, int(round((nseg - 1 - i) * 0.55))))
                    for side in (-1, 1):
                        grow((x, y), ang + side * math.radians(47 + p.r.uniform(-4, 4)), cn, slen * 0.82, hw * 0.82, 1)
                elif depth == 1 and nseg >= 4 and i == 1:
                    for side in (-1, 1):
                        grow((x, y), ang + side * math.radians(40 + p.r.uniform(-4, 4)), 1, slen * 0.74, hw * 0.8, 2)
            ang += (-math.pi / 2 - ang) * (0.2 if depth else 0.3) + p.r.uniform(-0.03, 0.03)

    grow(BASE, -math.pi / 2 + 0.03, 12, 80, 13.5, 0)
    # ground
    GY = BASE[1]
    p.wash(ell(450, GY + 18, 290, 26, 16), SAND, 0.75, edge=0, blur=8, amp=2)
    p.ink([(170, GY + 2), (330, GY)], w=1.4, step=40, amp=1.2)
    p.ink([(356, GY + 1), (410, GY)], w=1.4, step=40, amp=1.0)
    p.ink([(474, GY), (640, GY + 2)], w=1.4, step=40, amp=1.2)
    p.ink([(668, GY + 2), (730, GY + 1)], w=1.2, step=40, amp=1.0)
    p.dots([(p.r.uniform(180, 720), p.r.uniform(GY + 10, GY + 46)) for _ in range(90)], 1.6, INK, 0.5)
    # root
    root = [(440, GY), (436, GY + 40), (444, GY + 78), (432, GY + 116), (438, GY + 150), (428, GY + 186)]
    p.path(smooth(root[:3]), w=6.0)
    p.path(smooth(root[:3]), w=3.2, color="#e6dcc6")
    p.path(smooth(root[2:5]), w=3.6)
    p.path(smooth(root[4:]), w=1.8)
    for (rx, dy_, sgn, ln) in ((437, 28, -1, 80), (439, 44, 1, 96), (442, 70, -1, 110), (440, 88, 1, 74),
                               (434, 112, -1, 70), (436, 128, 1, 88), (436, 152, -1, 46), (432, 168, 1, 40)):
        ry = GY + dy_
        pts = [(rx, ry)]
        for i in range(1, 5):
            pts.append((rx + sgn * ln * i / 4 + p.r.uniform(-4, 4), ry + 7 * i + ln * 0.06 * i * i / 4 + p.r.uniform(-4, 4)))
        p.path(smooth(pts), w=1.3 if ln > 70 else 1.0, op=0.9)
        if ln > 70:
            m = pts[2]
            p.path(smooth([m, (m[0] + sgn * 14, m[1] + 22), (m[0] + sgn * 18, m[1] + 44)]), w=0.9, op=0.8)
    # stems: far branches first
    for depth in (2, 1, 0):
        for (a, b, hw, d_, term) in segs:
            if d_ != depth:
                continue
            shape = capsule(a, b, hw)
            p.path(smooth(shape, True), w=1.5 if depth == 0 else 1.25, fill=LEAF)
            if term:
                # autumn blush on the tip, and the faint rings of the fertile spike
                dx, dy = b[0] - a[0], b[1] - a[1]
                L = math.hypot(dx, dy)
                ux, uy = dx / L, dy / L
                tip = capsule((a[0] + dx * 0.42, a[1] + dy * 0.42), (b[0] - ux * 1.2, b[1] - uy * 1.2), hw * 0.82,
                              ((0.0, 0.9), (0.4, 0.98), (0.8, 0.86), (1.0, 0.5)))
                p.path(smooth(tip, True), w=0, color=None, fill=TERRA, fop=0.55)
                tk = []
                for t in (0.3, 0.5, 0.7):
                    cx, cy = a[0] + dx * t, a[1] + dy * t
                    tk.append(((cx - uy * hw * 0.7, cy + ux * hw * 0.7), (cx + uy * hw * 0.7, cy - ux * hw * 0.7)))
                p.segs(tk, 0.8, INK, 0.55)
            elif depth == 0:
                dx, dy = b[0] - a[0], b[1] - a[1]
                L = math.hypot(dx, dy)
                ux, uy = dx / L, dy / L
                p.segs([((a[0] + dx * 0.3 + uy * hw * 0.35, a[1] + dy * 0.3 - ux * hw * 0.35),
                         (a[0] + dx * 0.75 + uy * hw * 0.35, a[1] + dy * 0.75 - ux * hw * 0.35))], 0.8, INK, 0.4)
    # enlarged detail: two segments with their flowers
    DX = 850

    def big(y0, y1, hw):
        shape = [(DX - hw * 0.62, y0), (DX - hw * 0.98, y0 - (y0 - y1) * 0.22), (DX - hw, y0 - (y0 - y1) * 0.6),
                 (DX - hw * 0.94, y1 + 26), (DX - hw * 0.8, y1 - 16),          # left scale-leaf tip
                 (DX - hw * 0.42, y1 + 12), (DX, y1 + 20), (DX + hw * 0.42, y1 + 12),
                 (DX + hw * 0.8, y1 - 16), (DX + hw * 0.94, y1 + 26),          # right tip
                 (DX + hw, y0 - (y0 - y1) * 0.6), (DX + hw * 0.98, y0 - (y0 - y1) * 0.22), (DX + hw * 0.62, y0)]
        d = smooth(shape, True)
        p.path(d, w=2.4, fill=LEAF)
        cid = p.nid("s")
        p.defs.append('<clipPath id="%s"><path d="%s"/></clipPath>' % (cid, d))
        p.raw('<g clip-path="url(#%s)">' % cid)
        p.segs([((DX + hw * 0.52 + i * 9, y1 - 20), (DX + hw * 0.52 + i * 9 - 26, y0 + 10)) for i in range(8)], 0.9, INK, 0.5)
        p.raw("</g>")
        p.path(smooth([(DX - hw * 0.5, y1 + 30), (DX - hw * 0.52, (y0 + y1) / 2), (DX - hw * 0.4, y0 - 12)]), w=0.9, op=0.45)
        # flower triads sunk in the upper part of the segment
        p.raw('<g clip-path="url(#%s)">' % cid)
        for fx in (DX - hw * 0.2, DX + hw * 0.98):
            fy = y1 + (y0 - y1) * 0.3
            for (ox, oy, rx, ry) in ((-19, 17, 9, 12.5), (19, 17, 9, 12.5), (0, 0, 13, 18)):
                p.path(smooth(ell(fx + ox, fy + oy, rx, ry, 10), True), w=1.3, fill=PAPER)
                p.dots([(fx + ox - 2.5, fy + oy - 2), (fx + ox + 3, fy + oy + 2.5)], 3.2, TERRA)
        p.raw("</g>")
        p.path(d, w=2.4)

    big(1000, 780, 56)
    big(792, 580, 52)
    # the cut ends
    p.path(smooth(ell(DX, 1002, 35, 8, 12), True), w=1.8, fill=PAPER)
    p.path(smooth(capsule((DX, 592), (DX, 500), 40, ((0.0, 0.62), (0.3, 0.95), (0.75, 1.0), (1.0, 0.9))), True), w=2.2, fill=LEAF)
    p.path(smooth(ell(DX, 500, 36, 8, 12), True), w=1.8, fill=PAPER)
    # leader from the plant
    tgt = joints[1]
    p.path(smooth([(tgt[0] + 30, tgt[1] - 8), (640, 930), (DX - 72, 900)]), w=1.1, op=0.75, dash="2 7")
    p.path(smooth(circ(tgt[0], tgt[1], 30, 14), True), w=1.2, color=TERRA)
    # scale strokes
    p.segs([((DX + 96, 580), (DX + 96, 1000)), ((DX + 88, 580), (DX + 104, 580)), ((DX + 88, 1000), (DX + 104, 1000))], 0.9, INK, 0.6)
    # label
    p.text(DX, 1316, "Salicornia", size=46, anchor="middle")
    p.segs([((DX - 36, 1340), (DX + 36, 1340))], 3.0, TERRA)
    return p


# --------------------------------------------------------------------------
# 11 · zonación de la marisma
# --------------------------------------------------------------------------

def catmull(pts, k=10):
    out = []
    m = len(pts)
    for i in range(m - 1):
        p0, p1, p2, p3 = pts[max(0, i - 1)], pts[i], pts[i + 1], pts[min(m - 1, i + 2)]
        for j in range(k):
            t = j / k
            t2, t3 = t * t, t * t * t
            out.append(tuple(0.5 * ((2 * p1[c]) + (-p0[c] + p2[c]) * t + (2 * p0[c] - 5 * p1[c] + 4 * p2[c] - p3[c]) * t2
                                    + (-p0[c] + 3 * p1[c] - 3 * p2[c] + p3[c]) * t3) for c in (0, 1)))
    out.append(pts[-1])
    return out


def zonacion():
    p = Plate("zonacion", 1600, 900, 1111)
    UP = 56
    prof = [(30, 790), (200, 772), (330, 730), (450, 648), (560, 568), (680, 510), (800, 474), (950, 444), (1100, 412),
            (1250, 382), (1400, 354), (1570, 338)]
    prof = [(a, b - UP) for a, b in prof]
    dense = catmull(prof, 10)

    def ground(x):
        for (x0, y0), (x1, y1) in zip(dense, dense[1:]):
            if x0 <= x <= x1:
                return y0 + (y1 - y0) * (x - x0) / (x1 - x0)
        return dense[-1][1]

    def cross(level):
        for (x0, y0), (x1, y1) in zip(dense, dense[1:]):
            if y0 >= level >= y1:
                return x0 + (x1 - x0) * (y0 - level) / (y0 - y1)
        return dense[-1][0]

    levels = [(nm, lv - UP) for nm, lv in (("bajamar viva", 732), ("bajamar muerta", 654), ("nivel medio", 564),
                                           ("pleamar muerta", 476), ("pleamar viva", 394))]
    NM, PV = 564 - UP, 394 - UP
    xNM, xPV = cross(NM), cross(PV)

    def poly(pts):
        return "M" + "L".join("%s %s" % (n(a), n(b)) for a, b in pts) + "Z"

    under_nm = [(30, NM), (xNM, NM)] + [q for q in dense if q[0] < xNM][::-1]
    p.raw('<path d="%s" fill="%s" fill-opacity=".5" stroke="none"/>' % (poly(under_nm), BLUE))
    upper = [(30, PV), (xPV, PV)] + [q for q in dense if xNM <= q[0] < xPV][::-1] + [(xNM, NM), (30, NM)]
    p.raw('<path d="%s" fill="%s" fill-opacity=".5" stroke="none"/>' % (poly(upper), PALE))
    # ground
    gpoly = dense + [(1570, 900), (30, 900)]
    p.raw('<path d="%s" fill="%s" stroke="none"/>' % (poly(gpoly), PAPER))
    p.raw('<path d="%s" fill="%s" fill-opacity=".85" stroke="none"/>' % (poly(gpoly), SAND))
    st = []
    while len(st) < 520:
        q = (p.r.uniform(50, 1550), p.r.uniform(280, 850))
        g = ground(q[0])
        if q[1] > g + 8 and (q[1] < g + 130 or p.r.random() < 0.25):
            st.append(q)
    p.dots(st, 1.5, INK, 0.45)
    hk = []
    for x in range(60, 1550, 13):
        g = ground(x)
        ln = p.r.uniform(7, 15)
        hk.append(((x, g + 4), (x - ln * 0.6, g + 4 + ln)))
    p.segs(hk, 0.8, INK, 0.55)
    p.path("M" + "L".join("%s %s" % (n(a), n(b)) for a, b in dense), w=2.6)
    # levels
    for name, lv in levels:
        xc = cross(lv)
        p.segs([((49, lv), (xc, lv))], 1.1, INK, 0.85, dash="10 7")
        p.dots([(xc, lv)], 7, TERRA)
        p.text(68, lv - 10, name, size=20)
    # plants
    def zostera(x):
        y = ground(x)
        for i in range(6):
            bx = x + p.r.uniform(-12, 12)
            ln = p.r.uniform(54, 92)
            lean = p.r.uniform(0.35, 0.9)
            pts = [(bx, y + 3), (bx + ln * 0.2 * lean, y - ln * 0.45), (bx + ln * 0.75 * lean, y - ln * 0.8), (bx + ln * 1.2 * lean, y - ln * 0.86)]
            p.path(smooth(pts), w=4.6, color=BLUE)
            p.path(smooth(pts), w=1.0, op=0.9)

    def spartina(x):
        y = ground(x)
        for i in range(10):
            a = -math.pi / 2 + (i - 4.5) * 0.085 + p.r.uniform(-0.04, 0.04)
            ln = p.r.uniform(58, 96)
            bx = x + (i - 4.5) * 2.2
            mid = (bx + ln * 0.5 * math.cos(a) , y - ln * 0.5)
            tip = (bx + ln * math.cos(a) * 1.25, y + ln * math.sin(a))
            p.path(smooth([(bx, y + 2), mid, tip]), w=1.5)
            if i % 3 == 1:
                p.path(smooth([(mid[0] + (tip[0] - mid[0]) * 0.45, mid[1] + (tip[1] - mid[1]) * 0.45), tip]), w=3.4)

    def tube(pts, w=5.6):
        p.path(smooth(pts), w=w)
        p.path(smooth(pts), w=w - 2.6, color=LEAF)

    def glass(x):
        y = ground(x)
        h = p.r.uniform(46, 60)
        tube([(x, y + 2), (x + 1, y - h)])
        p.dots([(x + 1, y - h)], 4.4, TERRA)
        for t, ln in ((0.25, 26), (0.5, 22), (0.72, 15)):
            for sgn in (-1, 1):
                b0 = (x, y - h * t)
                pts = [b0, (x + sgn * ln * 0.6, y - h * t - ln * 0.35), (x + sgn * ln * 0.8, y - h * t - ln * 1.05)]
                tube(pts, 5.0)
                p.dots([pts[-1]], 4.0, TERRA)
        tube([(x, y + 2), (x + 1, y - h)])

    def halimione(x):
        y = ground(x)
        for i in range(7):
            a = -math.pi / 2 + (i - 3) * 0.36 + p.r.uniform(-0.08, 0.08)
            ln = p.r.uniform(42, 60) * (1 - abs(i - 3) * 0.06)
            pts = [(x, y + 2)]
            for k in range(1, 6):
                t = k / 5
                pts.append((x + ln * t * math.cos(a), y + ln * t * math.sin(a) + 14 * t * t * abs(math.cos(a))))
            p.path(smooth(pts), w=1.3)
            for k in range(1, 6):
                px_, py_ = pts[k]
                ang = math.atan2(pts[k][1] - pts[k - 1][1], pts[k][0] - pts[k - 1][0])
                for sgn in (-1, 1):
                    la = ang + sgn * 1.0
                    cx_, cy_ = px_ + 7 * math.cos(la), py_ + 7 * math.sin(la)
                    p.path(smooth(ell(cx_, cy_, 7.5, 3.4, 8, rot=la), True), w=0.9, fill=LEAF)

    def limonium(x):
        y = ground(x)
        for i in range(6):
            a = math.pi + (i + 0.5) * math.pi / 6
            a = -abs(a) if False else a
            lx, ly = x + 20 * math.cos(a), y - 4 + 9 * math.sin(a) * 0.9
            p.path(smooth(ell(lx, ly - 4, 17, 6, 10, rot=math.atan2(9 * math.sin(a), 20 * math.cos(a)) * 0.6), True), w=1.0, fill=LEAF)
        buds = []
        for j in range(3):
            a = -math.pi / 2 + (j - 1) * 0.3 + p.r.uniform(-0.06, 0.06)
            h = p.r.uniform(62, 86)
            top = (x + h * math.cos(a), y + h * math.sin(a))
            p.path(smooth([(x, y - 4), ((x + top[0]) / 2 + 3, (y + top[1]) / 2), top]), w=1.2)
            for kk in range(4):
                ta = a + (kk - 1.5) * 0.5
                ln = p.r.uniform(20, 32)
                start = (x + h * 0.7 * math.cos(a) + (top[0] - x) * 0.0, y + h * 0.7 * math.sin(a))
                tip = (start[0] + ln * math.cos(ta) * 1.2, start[1] + ln * math.sin(ta) * 0.9)
                p.path(smooth([start, ((start[0] + tip[0]) / 2, (start[1] + tip[1]) / 2 - 3), tip]), w=0.9)
                for t in (0.55, 0.78, 1.0):
                    buds.append((start[0] + (tip[0] - start[0]) * t, start[1] + (tip[1] - start[1]) * t - 3))
        p.dots(buds, 4.2, TERRA, 0.9)

    def juncus(x):
        y = ground(x)
        stems, heads = [], []
        for i in range(15):
            a = -math.pi / 2 + (i - 7) * 0.045 + p.r.uniform(-0.02, 0.02)
            ln = p.r.uniform(92, 132)
            bx = x + (i - 7) * 1.6
            tip = (bx + ln * math.cos(a), y + ln * math.sin(a))
            stems.append(((bx, y + 2), tip))
            if i % 2 == 0:
                hx, hy = bx + ln * 0.8 * math.cos(a), y + ln * 0.8 * math.sin(a)
                sg = 1 if i > 7 else -1
                heads += [((hx, hy), (hx + sg * 7, hy - 6)), ((hx, hy), (hx + sg * 9, hy - 1)), ((hx, hy), (hx + sg * 5, hy - 10))]
        p.segs(stems, 1.5)
        p.segs(heads, 2.4, INK, 0.9)

    for (fx, fy, fs) in ((420, 196, 1.0), (486, 170, 0.8), (352, 226, 0.7)):
        p.path(smooth([(fx - 20 * fs, fy - 2 * fs), (fx - 9 * fs, fy - 9 * fs), (fx, fy), (fx + 10 * fs, fy - 10 * fs),
                       (fx + 22 * fs, fy - 5 * fs)]), w=1.6 * fs, op=0.85)
    bands = [("seda de mar", zostera, (238, 300, 362), 300), ("espartina", spartina, (524, 580, 636), 580),
             ("salicornia", glass, (748, 796, 844), 796), ("verdolaga marina", halimione, (950, 1040), 995),
             ("lavanda de mar", limonium, (1150, 1206, 1262), 1206), ("junco", juncus, (1372, 1430, 1488), 1430)]
    for name, fn, xs, cx in bands:
        for x in xs:
            fn(x)
        g = ground(cx)
        p.segs([((cx, g + 20), (cx, g + 36))], 1.0, INK, 0.7)
        p.text(cx, g + 62, name, size=22, anchor="middle")
    return p


# --------------------------------------------------------------------------
# 12 · ardora (mar de ardora, noche)
# --------------------------------------------------------------------------

def ardora():
    p = Plate("ardora", 1600, 1000, 1212)
    NIGHT, GLOW, DARK, CREAM = "#1c2b36", "#bfe6dc", "#0f1a21", "#f1e7d0"
    HOR = 372
    box = [(66, 66), (1534, 66), (1534, 934), (66, 934)]
    d_box = p.sharp(box, step=110, amp=1.6)
    p.defs.append('<clipPath id="nt"><path d="%s"/></clipPath>' % d_box)
    p.defs.append('<linearGradient id="sky" gradientUnits="userSpaceOnUse" x1="0" y1="66" x2="0" y2="%d">'
                  '<stop offset="0" stop-color="%s" stop-opacity="0"/>'
                  '<stop offset="1" stop-color="%s" stop-opacity=".3"/></linearGradient>' % (HOR, BLUE, BLUE))
    p.defs.append('<radialGradient id="mh" gradientUnits="userSpaceOnUse" cx="1236" cy="196" r="170">'
                  '<stop offset="0" stop-color="%s" stop-opacity=".24"/>'
                  '<stop offset=".5" stop-color="%s" stop-opacity=".08"/>'
                  '<stop offset="1" stop-color="%s" stop-opacity="0"/></radialGradient>' % (CREAM, CREAM, CREAM))
    p.raw('<path d="%s" fill="%s" stroke="none"/>' % (d_box, NIGHT))
    p.raw('<g clip-path="url(#nt)">')
    p.raw('<rect x="60" y="60" width="1480" height="%d" fill="url(#sky)" stroke="none"/>' % (HOR - 60))
    p.raw('<circle cx="1236" cy="196" r="170" fill="url(#mh)" stroke="none"/>')
    # sea, a shade deeper
    p.raw('<rect x="60" y="%d" width="1480" height="600" fill="#15222b" stroke="none"/>' % HOR)
    p.segs([((66, HOR), (1534, HOR))], 1.0, PALE, 0.45)
    # stars and moon
    stars = []
    while len(stars) < 70:
        q = (p.r.uniform(90, 1510), p.r.uniform(86, HOR - 24))
        if math.hypot(q[0] - 1236, q[1] - 196) > 90:
            stars.append(q)
    p.dots(stars[:48], 2.0, PALE, 0.75)
    p.dots(stars[48:], 3.4, CREAM, 0.9)
    p.path(smooth(crescent(1236, 196, 50, 22, -9, 47, 56), True), w=1.0, color=CREAM, fill=CREAM)
    # moon glitter on the far water
    gl = []
    for i in range(22):
        yy = HOR + 8 + i * 4.2
        half = 6 + i * 2.2 + p.r.uniform(-3, 3)
        cx = 1236 + p.r.uniform(-8, 8)
        gl.append(((cx - half, yy), (cx + half, yy)))
    p.segs(gl, 1.0, CREAM, 0.28)
    # wet sand
    shore = [(40, 772), (260, 742), (520, 734), (800, 760), (1060, 806), (1320, 842), (1570, 858)]
    p.raw('<path d="%sL1570 960L40 960Z" fill="#27343a" stroke="none"/>' % smooth(shore))
    sheen = [(a, b + 26 + 6 * math.sin(a / 130.0)) for a, b in shore]
    p.path(smooth(sheen), w=1.0, color=PALE, op=0.22)
    # glow layer (blurred) then crisp sparks
    blur = p.blur(7)
    glow, sparks, big = [], [], []

    def crest(pts, breaks, w):
        dense = catmull(pts, 8)
        run = []
        for i, q in enumerate(dense):
            t = i / (len(dense) - 1)
            if any(a < t < b for a, b in breaks):
                if len(run) > 2:
                    emit(run, w)
                run = []
            else:
                run.append(q)
        if len(run) > 2:
            emit(run, w)

    def emit(run, w):
        d = "M" + "L".join("%s %s" % (n(a), n(b)) for a, b in run)
        glow.append((d, w * 5))
        p.path(d, w=w, color=PALE, op=0.55)
        for (x, y) in run:
            for _ in range(int(2 + w * 1.6)):
                q = (x + p.r.uniform(-12, 12), y + abs(p.r.gauss(0, 4.5 * w)) + 1)
                (big if p.r.random() < 0.16 else sparks).append(q)

    crest([(120, 446), (360, 440), (600, 448), (860, 442), (1120, 450), (1480, 444)], [(0.18, 0.3), (0.52, 0.66), (0.84, 0.92)], 1.0)
    crest([(66, 520), (300, 510), (560, 524), (840, 512), (1100, 528), (1534, 516)], [(0.3, 0.42), (0.7, 0.8)], 1.4)
    crest([(66, 622), (280, 604), (520, 620), (760, 604), (940, 622)], [(0.42, 0.5)], 1.9)
    crest([(1440, 700), (1490, 692), (1534, 700)], [], 1.9)
    crest([(a, b - 14 + 5 * math.sin(a / 90.0)) for a, b in shore], [(0.2, 0.26), (0.58, 0.66)], 2.3)
    # footprints along the wet sand
    fp = [(214, 880, -0.12), (300, 838, -0.1), (398, 868, -0.06), (492, 824, -0.02), (596, 856, 0.04), (694, 818, 0.1), (800, 856, 0.14),
          (902, 828, 0.18)]
    for i, (fx, fy, fa) in enumerate(fp):
        s = 0.8
        footprint(p, fx, fy, fa, s, GLOW, 0.3 + 0.07 * i, line=False)
        glow.append(("M%s %sh%s" % (n(fx - 26), n(fy), n(44)), 12))
        for _ in range(8 + i * 2):
            sparks.append((fx + p.r.uniform(-32, 24), fy + p.r.uniform(-7, 7)))
    # boat
    hull = [(1004, 566), (1060, 590), (1180, 600), (1310, 598), (1396, 578), (1424, 556), (1392, 628), (1300, 652), (1150, 654), (1052, 636)]
    p.path(smooth(hull, True), w=0, color=None, fill=DARK)
    p.path(smooth(hull[:6]), w=1.6, color=PALE, op=0.6)
    p.segs([((1150, 599), (1150, 650)), ((1290, 598), (1290, 648))], 1.0, PALE, 0.18)
    wl = [(1040, 646), (1150, 664), (1300, 662), (1404, 634)]
    crest(wl, [], 1.5)
    # oar and its swirl
    ob, oe = (1196, 590), (1012, 716)
    p.segs([(ob, oe)], 7.0, DARK)
    p.segs([(ob, oe)], 2.6, "#8fa3a8")
    blade = [(1016, 708), (990, 716), (950, 746), (938, 768), (964, 768), (1008, 740), (1024, 722)]
    p.path(smooth(blade, True), w=1.6, color=PALE, fill="#3a4b52")
    SWX, SWY = 952, 762
    for (r0, r1, a0, turns, w) in ((16, 58, 0.4, 0.8, 2.0), (30, 86, 3.3, 0.7, 1.6), (52, 118, 1.6, 0.55, 1.2)):
        pts = []
        for i in range(15):
            t = i / 14
            a = a0 + turns * TAU * t
            r = r0 + (r1 - r0) * t
            pts.append((SWX + r * math.cos(a) * 1.5, SWY + r * math.sin(a) * 0.52))
        d = smooth(pts)
        glow.append((d, w * 6))
        p.path(d, w=w, color=GLOW, op=0.85)
        for (x, y) in pts:
            for _ in range(4):
                (big if p.r.random() < 0.25 else sparks).append((x + p.r.uniform(-12, 12), y + p.r.uniform(-6, 6)))
    for _ in range(46):
        a, r = p.r.uniform(0, TAU), abs(p.r.gauss(0, 60))
        sparks.append((SWX + r * math.cos(a) * 1.5, SWY + r * math.sin(a) * 0.5))
    # drips from the oar
    for t in (0.55, 0.7, 0.84):
        sparks.append((ob[0] + (oe[0] - ob[0]) * t, ob[1] + (oe[1] - ob[1]) * t + 14))
    p.raw('<g%s opacity=".5">' % blur)
    for d, w in glow:
        p.path(d, w=w, color=GLOW)
    p.raw("</g>")
    p.dots(sparks, 2.2, GLOW, 0.9)
    p.dots(big, 4.0, GLOW, 0.95)
    p.raw("</g>")
    return p


PLATES = [cubierta, mapa, caseta, sizigia, curva, mareografo, escandallo, reloj, rastrillo, salicornia, zonacion, ardora]


def main(argv):
    want = set(argv)
    for fn in PLATES:
        plate = fn()
        if want and plate.name not in want and fn.__name__ not in want:
            continue
        path, size = plate.save()
        print("%-22s %4dx%-4d %7d bytes" % (os.path.basename(path), plate.w, plate.h, size))


if __name__ == "__main__":
    main(sys.argv[1:])
