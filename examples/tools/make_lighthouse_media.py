#!/usr/bin/env python3
"""Generate the original media of the example book "The Lighthouse Protocol".

Everything is synthesised procedurally (no source footage, samples or fonts):

    the-lighthouse-protocol/media/
        beam.mp4    12 s  night sea, lighthouse, two rotating beams (loops)
        beam.jpg          poster frame (first flash, t = 3.0 s)
        beam.en.vtt       captions
        signal.mp4  14 s  a light in the fog: three long flashes, twice
        signal.jpg        poster frame (during the second flash)
        signal.en.vtt     captions
        sea.m4a     30 s  sea ambience
        bell.m4a    24 s  fog bell, three strokes
        horn.m4a    18 s  distant reed horn, three long notes, twice
        horn.en.vtt       captions for horn.m4a

Requirements: Python 3.10+, numpy, ffmpeg (libx264 + aac) on PATH.
Usage:        python3 examples/tools/make_lighthouse_media.py [--only beam,sea,...]
              python3 examples/tools/make_lighthouse_media.py --stills DIR
                  (debug: writes a few PNG frames of each video and exits)

The script is deterministic: fixed random seeds, bit-exact muxing flags and no
timestamps, so the same machine and ffmpeg build reproduce identical files.

Video: frames are computed with numpy (smooth plane-wave fog, analytic
anti-aliased shapes, volumetric beams splatted as separable Gaussians) and
piped to ffmpeg as raw RGB. Audio: filtered noise with swell envelopes (sea),
decaying inharmonic partials (bell) and a band-limited reed tone (horn), each
loudness-normalised to about -19 LUFS with a true peak below -1.5 dBTP.
"""
from __future__ import annotations

import argparse
import json
import math
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
MEDIA = HERE.parent / "the-lighthouse-protocol" / "media"
FFMPEG = shutil.which("ffmpeg") or "/opt/homebrew/bin/ffmpeg"

W, H, FPS = 960, 540, 24
SR = 48000
F32 = np.float32
TAU = 2.0 * math.pi


def rgb(hex_):
    return np.array([int(hex_[i:i + 2], 16) for i in (1, 3, 5)], dtype=F32) / 255.0


NAVY1 = rgb("#0e1a26")
NAVY2 = rgb("#17293a")
FOG = rgb("#8fa3ad")
AMBER = rgb("#f2b45a")
BONE = rgb("#e9e4d6")
INK = np.array([0.022, 0.034, 0.047], dtype=F32)

# Event timings shared by picture, sound and captions -----------------------
BEAM_DUR = 12.0
BEAM_FLASHES = (3.0, 5.5)            # the two beams face the viewer
BEAM_SEPARATION = math.radians(75.0)
BEAM_BELL_AT = 8.5
BEAM_CRESTS = [(1.5, 1.00, 1.1, 2.0), (6.3, 0.70, 1.2, 1.7), (10.9, 0.40, 1.0, 1.8)]

SIGNAL_DUR = 14.0
SIGNAL_ON = [(1.0, 2.2), (2.8, 4.0), (4.6, 5.8), (7.6, 8.8), (9.4, 10.6), (11.2, 12.4)]

SEA_DUR = 30.0
SEA_CRESTS = [(3.2, 1.00, 1.6, 2.6), (7.3, 0.35, 0.9, 1.4), (10.4, 0.85, 1.6, 2.5),
              (14.3, 0.30, 0.9, 1.4), (17.2, 1.00, 1.5, 2.7), (21.5, 0.40, 1.0, 1.5),
              (24.6, 0.90, 1.6, 2.5), (28.3, 0.25, 0.8, 1.2)]

BELL_DUR = 24.0
BELL_STROKES = (1.0, 9.0, 17.0)

HORN_DUR = 18.0
HORN_NOTES = [(1.5, 3.1), (3.8, 5.4), (6.1, 7.7), (10.0, 11.6), (12.3, 13.9), (14.6, 16.2)]


# ===========================================================================
# Audio
# ===========================================================================

def bfilter(x, lo=None, hi=None, order=2, tilt=0.0):
    """Zero-phase Butterworth-magnitude band filter (FFT domain), optional tilt."""
    n = len(x)
    spec = np.fft.rfft(x)
    f = np.fft.rfftfreq(n, 1.0 / SR)
    f[0] = f[1]
    g = np.ones_like(f)
    if tilt:
        g *= (f / 1000.0) ** tilt
    if lo:
        g *= 1.0 / np.sqrt(1.0 + (lo / f) ** (2 * order))
    if hi:
        g *= 1.0 / np.sqrt(1.0 + (f / hi) ** (2 * order))
    g[0] = 0.0
    return np.fft.irfft(spec * g, n)


def unit(x):
    return x / math.sqrt(float(np.mean(x * x)) + 1e-20)


def smooth01(a, b, x):
    t = np.clip((x - a) / (b - a), 0.0, 1.0)
    return t * t * (3.0 - 2.0 * t)


def fade(x, t_in, t_out):
    n = len(x)
    env = np.ones(n)
    a, b = int(t_in * SR), int(t_out * SR)
    if a:
        env[:a] = 0.5 - 0.5 * np.cos(np.pi * np.arange(a) / a)
    if b:
        env[n - b:] = 0.5 + 0.5 * np.cos(np.pi * np.arange(b) / b)
    return x * (env[:, None] if x.ndim == 2 else env)


def swell(t, crests, shift=0.0, rise_k=1.0, decay_k=1.0):
    """Sum of wave envelopes: Gaussian rise to the crest, exponential drain."""
    e = np.zeros_like(t)
    for t0, amp, rise, decay in crests:
        d = t - (t0 + shift)
        e += amp * np.where(d < 0.0, np.exp(-(d / (rise * rise_k)) ** 2),
                            np.exp(-np.maximum(d, 0.0) / (decay * decay_k)))
    return e


def sea(dur, crests, seed):
    """Stereo sea: low body, mid wash on each swell, soft hiss on the crests."""
    rng = np.random.default_rng(seed)
    n = int(round(dur * SR))
    t = np.arange(n) / SR

    def layer(lo, hi, tilt):
        mid = unit(bfilter(rng.standard_normal(n), lo, hi, 2, tilt))
        left = unit(bfilter(rng.standard_normal(n), lo, hi, 2, tilt))
        right = unit(bfilter(rng.standard_normal(n), lo, hi, 2, tilt))
        return np.stack([0.8 * mid + 0.6 * left, 0.8 * mid + 0.6 * right], axis=1)

    body = layer(70.0, 480.0, -0.5)
    wash = layer(280.0, 2600.0, -0.6)
    hiss = layer(2400.0, 8500.0, -0.8)
    gurgle = 1.0 + 0.22 * unit(bfilter(rng.standard_normal(n), 0.4, 3.0, 2))
    out = np.zeros((n, 2))
    for ch, shift, pan in ((0, -0.14, 1.06), (1, 0.14, 0.94)):
        env = swell(t, crests, shift)
        slow = swell(t, crests, shift - 0.30, 1.25, 1.15)
        fizz = swell(t, crests, shift + 0.40, 0.55, 1.35)
        out[:, ch] = pan * (0.36 * body[:, ch] * (0.14 + 0.86 * slow ** 1.2)
                            + 0.50 * wash[:, ch] * gurgle * (0.03 + env ** 1.4)
                            + 0.13 * hiss[:, ch] * (0.02 + fizz ** 1.5))
    return out


BELL_PARTIALS = [  # ratio to the strike note, amplitude, decay (s), beat (Hz)
    (0.500, 0.50, 6.5, 0.31), (1.000, 1.00, 4.2, 0.57), (1.183, 0.50, 3.0, 0.83),
    (1.506, 0.22, 2.0, 0.71), (2.000, 0.55, 1.7, 1.13), (2.514, 0.14, 1.0, 1.40),
    (3.011, 0.16, 0.75, 1.70), (4.166, 0.07, 0.45, 2.10), (5.433, 0.04, 0.30, 2.60),
]


def bell_stroke(dur, seed, f0=349.2):
    """One stroke of a bronze fog bell: inharmonic partials with slow beating."""
    rng = np.random.default_rng(seed)
    n = int(round(dur * SR))
    t = np.arange(n) / SR
    y = np.zeros(n)
    for ratio, amp, tau, beat in BELL_PARTIALS:
        f = f0 * ratio
        p1, p2 = rng.uniform(0.0, TAU, 2)
        y += amp * np.exp(-t / tau) * (np.sin(TAU * f * t + p1)
                                       + 0.55 * np.sin(TAU * (f + beat) * t + p2)) / 1.55
    y *= 1.0 - np.exp(-t / 0.003)
    clang = unit(bfilter(rng.standard_normal(n), 900.0, 3800.0, 2)) * np.exp(-t / 0.010)
    return y + 0.22 * clang


def reverb(x, rt, mix, seed, predelay=0.02, hi=3500.0):
    """Mono or stereo in, stereo out: convolution with decaying filtered noise."""
    if x.ndim == 1:
        x = np.stack([x, x], axis=1)
    n = len(x)
    m = int(rt * 1.3 * SR)
    pre = int(predelay * SR)
    size = 1 << int(math.ceil(math.log2(n + m + pre)))
    out = np.zeros((n, 2))
    for ch in range(2):
        rng = np.random.default_rng(seed + ch)
        ir = rng.standard_normal(m) * np.exp(-6.91 * np.arange(m) / SR / rt)
        ir = bfilter(ir, 150.0, hi, 1)
        ir /= math.sqrt(float(np.sum(ir * ir)))
        ir = np.concatenate([np.zeros(pre), ir])
        wet = np.fft.irfft(np.fft.rfft(x[:, ch], size) * np.fft.rfft(ir, size), size)[:n]
        out[:, ch] = x[:, ch] + mix * wet
    return out


def horn_note(dur, seed, f0=196.0, tail=0.25):
    """A mellow reed horn note that speaks slowly and settles up to pitch."""
    rng = np.random.default_rng(seed)
    n = int(round((dur + tail) * SR))
    t = np.arange(n) / SR
    env = smooth01(0.0, 0.28, t) * (1.0 - smooth01(dur - 0.15, dur + tail, t))
    env = env * (1.0 + 0.03 * np.sin(TAU * 5.3 * t + rng.uniform(0, TAU)))
    fi = f0 * (1.0 - 0.016 * np.exp(-t / 0.10) - 0.006 * smooth01(dur - 0.15, dur + tail, t))
    phase = TAU * np.cumsum(fi) / SR
    y = np.zeros(n)
    for detune, level in ((1.0, 1.0), (1.003, 0.12)):       # a faint second reed
        for h in range(1, 13):
            a = level * (1.0 if h % 2 else 0.7) / h ** 1.15 / math.sqrt(1.0 + (h * f0 / 1300.0) ** 4)
            y += a * np.sin(h * detune * phase + rng.uniform(0, TAU))
    breath = 0.03 * unit(bfilter(rng.standard_normal(n), 300.0, 1500.0, 2))
    return (y / 1.5 + breath) * env


def horn_track(dur, notes, seed, echo_gain=0.24, wet=0.9):
    """Horn notes placed on a timeline, pushed into the distance (stereo)."""
    n = int(round(dur * SR))
    dry = np.zeros(n)
    for i, (a, b) in enumerate(notes):
        note = horn_note(b - a, seed + i)
        k = int(round(a * SR))
        m = min(len(note), n - k)
        dry[k:k + m] += note[:m]
    dry = bfilter(dry, 90.0, 2000.0, 1)
    d = int(0.38 * SR)
    echo = np.zeros(n)
    echo[d:] = echo_gain * bfilter(dry, None, 900.0, 1)[:n - d]
    return reverb(dry + echo, 2.4, wet, seed + 100, predelay=0.035, hi=2200.0)


def place(dst, src, at):
    k = int(round(at * SR))
    m = min(len(src), len(dst) - k)
    dst[k:k + m] += src[:m]


def build_sea_audio():
    return fade(sea(SEA_DUR, SEA_CRESTS, 101), 2.0, 2.5)


def build_bell_audio():
    n = int(round(BELL_DUR * SR))
    dry = np.zeros(n)
    for i, (at, strength) in enumerate(zip(BELL_STROKES, (1.0, 0.90, 0.96))):
        place(dry, strength * bell_stroke(7.9, 300 + i), at)
    wet = reverb(dry, 1.8, 0.32, 310)
    bed = sea(BELL_DUR, [(5.2, 0.8, 1.6, 2.6), (13.4, 0.9, 1.5, 2.8), (21.0, 0.7, 1.4, 2.4)], 320)
    return fade(wet + 0.036 * bed, 0.6, 2.0)


def build_horn_audio():
    horn = horn_track(HORN_DUR, HORN_NOTES, 400)
    bed = sea(HORN_DUR, [(0.6, 0.6, 1.2, 2.0), (8.7, 0.9, 1.4, 2.4), (16.9, 0.6, 1.2, 2.0)], 420)
    return fade(horn + 0.26 * bed, 0.8, 1.5)


def build_beam_audio():
    n = int(round(BEAM_DUR * SR))
    dry = np.zeros(n)
    place(dry, bell_stroke(BEAM_DUR - BEAM_BELL_AT, 500), BEAM_BELL_AT)
    wet = reverb(dry, 1.8, 0.32, 510)
    return fade(0.34 * sea(BEAM_DUR, BEAM_CRESTS, 520) + 0.55 * wet, 0.6, 1.2)


def build_signal_audio():
    horn = horn_track(SIGNAL_DUR, SIGNAL_ON, 600, echo_gain=0.12, wet=0.6)   # short gaps: keep the notes distinct
    bed = sea(SIGNAL_DUR, [(0.2, 0.6, 1.0, 2.0), (6.6, 0.8, 1.2, 2.2), (13.2, 0.6, 1.2, 2.0)], 620)
    return fade(horn + 0.28 * bed, 0.6, 1.0)


def run(cmd, **kw):
    return subprocess.run(cmd, check=True, **kw)


def measure(args):
    """EBU R128 integrated loudness / true peak / range of an ffmpeg input."""
    p = run([FFMPEG, "-hide_banner", "-nostats", *args, "-vn",
             "-af", "loudnorm=print_format=json", "-f", "null", "-"],
            capture_output=True, text=True)
    blob = p.stderr[p.stderr.rindex("{"):p.stderr.rindex("}") + 1]
    j = json.loads(blob)
    return float(j["input_i"]), float(j["input_tp"]), float(j["input_lra"])


RAW_IN = ["-f", "f32le", "-ar", str(SR), "-ac", "2"]


def normalise(x, tmp, target=-19.0, ceiling=-2.2):
    """Plain gain to the target loudness; back off if the true peak is too hot."""
    x = x * (0.5 / float(np.max(np.abs(x))))
    raw = Path(tmp) / "measure.f32"
    x.astype("<f4").tofile(raw)
    loud, peak, _ = measure([*RAW_IN, "-i", str(raw)])
    gain = min(target - loud, ceiling - peak)
    return x * 10.0 ** (gain / 20.0)


BITEXACT = ["-map_metadata", "-1", "-fflags", "+bitexact", "-flags:a", "+bitexact"]


def encode_audio(x, out, tmp):
    raw = Path(tmp) / "audio.f32"
    x.astype("<f4").tofile(raw)
    run([FFMPEG, "-y", "-loglevel", "error", *RAW_IN, "-i", str(raw),
         "-c:a", "aac", "-b:a", "128k", "-movflags", "+faststart", *BITEXACT, str(out)])


# ===========================================================================
# Video helpers
# ===========================================================================

class WaveField:
    """Smooth random field: a sum of plane waves, periodic in time.

    cos(kx x + ky y + w t) is separable, so a frame costs two small matmuls.
    Every temporal frequency is a whole number of cycles per `period`, which
    makes the field loop seamlessly.
    """

    def __init__(self, rng, count, lam_x, lam_y, period, cycles, drift=1.0):
        kx = TAU / rng.uniform(lam_x[0], lam_x[1], count) * rng.choice([-1.0, 1.0], count)
        ky = TAU / rng.uniform(lam_y[0], lam_y[1], count) * rng.choice([-1.0, 1.0], count)
        om = TAU / period * rng.choice(cycles, count)
        om = -np.sign(kx) * np.abs(om) * drift          # drift towards +x
        amp = rng.uniform(0.6, 1.0, count)
        amp /= math.sqrt(float(np.sum(amp * amp)) / 2.0)
        x = np.arange(W)
        y = np.arange(H)
        self.ax = (kx[:, None] * x[None, :] + rng.uniform(0, TAU, count)[:, None])
        self.om = om[:, None]
        self.cy = (np.cos(ky[:, None] * y[None, :]) * amp[:, None]).T.astype(F32)
        self.sy = (np.sin(ky[:, None] * y[None, :]) * amp[:, None]).T.astype(F32)

    def at(self, t):
        a = self.ax + self.om * t
        return self.cy @ np.cos(a).astype(F32) - self.sy @ np.sin(a).astype(F32)


def fog01(field, lo=-1.3, hi=1.7):
    return smooth01(lo, hi, field).astype(F32)


def poly_alpha(X, Y, pts):
    """Anti-aliased coverage of a convex polygon (any winding)."""
    pts = np.asarray(pts, dtype=np.float64)
    area = 0.0
    for i in range(len(pts)):
        x0, y0 = pts[i]
        x1, y1 = pts[(i + 1) % len(pts)]
        area += x0 * y1 - x1 * y0
    sgn = 1.0 if area > 0 else -1.0
    sd = np.full(X.shape, -1e9, dtype=F32)
    for i in range(len(pts)):
        x0, y0 = pts[i]
        x1, y1 = pts[(i + 1) % len(pts)]
        ex, ey = x1 - x0, y1 - y0
        ln = math.hypot(ex, ey)
        d = sgn * ((X - x0) * ey - (Y - y0) * ex) / ln
        sd = np.maximum(sd, d.astype(F32))
    return np.clip(0.5 - sd, 0.0, 1.0)


def rect_alpha(X, Y, x0, y0, x1, y1):
    return poly_alpha(X, Y, [(x0, y0), (x1, y0), (x1, y1), (x0, y1)])


def disc_alpha(X, Y, cx, cy, r):
    return np.clip(r + 0.5 - np.hypot(X - cx, Y - cy), 0.0, 1.0).astype(F32)


def over(img, alpha, colour):
    a = alpha[..., None]
    return img * (1.0 - a) + np.asarray(colour, dtype=F32) * a


def gauss2(x, y, cx, cy, r):
    """Separable 2-D Gaussian as an outer product."""
    gx = np.exp(-((x - cx) / r) ** 2).astype(F32)
    gy = np.exp(-((y - cy) / r) ** 2).astype(F32)
    return gy[:, None] * gx[None, :]


def finish(img, vignette, dither):
    out = img * vignette[..., None]
    return np.clip(out * 255.0 + dither, 0.0, 255.0).astype(np.uint8)


def make_vignette(strength):
    x = (np.arange(W, dtype=F32) - W / 2) / (W / 2)
    y = (np.arange(H, dtype=F32) - H / 2) / (H / 2)
    r2 = (x[None, :] ** 2 + y[:, None] ** 2) / 2.0
    return (1.0 - strength * r2 ** 1.2).astype(F32)


def make_dither(seed):
    rng = np.random.default_rng(seed)
    return (rng.random((H, W, 1)) + rng.random((H, W, 1)) - 0.5).astype(F32)  # triangular


# ===========================================================================
# Scene 1: the beam
# ===========================================================================

class BeamScene:
    dur = BEAM_DUR
    HY = 352.0            # horizon
    LX = 610.0            # tower axis
    BASE = 340.0          # tower foot
    SC = 1.3              # lighthouse scale

    def __init__(self):
        rng = np.random.default_rng(1207)
        self.x = np.arange(W, dtype=F32)
        self.y = np.arange(H, dtype=F32)
        X, Y = np.meshgrid(self.x, self.y)
        self.X, self.Y = X, Y
        self.vignette = make_vignette(0.42)
        self.dither = make_dither(5)
        hy = self.HY

        # --- sky ------------------------------------------------------------
        g = np.clip(self.y / hy, 0.0, 1.0) ** 1.5
        sky = (NAVY1 * 0.78)[None, :] * (1.0 - g[:, None]) + (NAVY2 * 1.06)[None, :] * g[:, None]
        self.sky = np.repeat(sky[:, None, :], W, axis=1).astype(F32)
        # fog density profile: thickest just above the horizon
        self.dens = (0.22 + 0.78 * np.exp(-((self.y - (hy - 30.0)) / 120.0) ** 2)).astype(F32)

        # --- stars (three twinkle groups) -------------------------------------
        self.stars = np.zeros((3, H, W), dtype=F32)
        for i in range(64):
            sx = rng.uniform(10, W - 10)
            sy = rng.uniform(8, 250)
            mag = rng.uniform(0.0, 1.0) ** 2.6
            r = 0.75 + 0.9 * mag
            x0, x1 = int(sx) - 6, int(sx) + 7
            y0, y1 = max(int(sy) - 6, 0), int(sy) + 7
            x0 = max(x0, 0)
            patch = np.exp(-(((X[y0:y1, x0:x1] - sx) ** 2 + (Y[y0:y1, x0:x1] - sy) ** 2) / r ** 2))
            self.stars[i % 3, y0:y1, x0:x1] += (0.16 + 0.60 * mag) * patch
        self.star_fade = (1.0 - 0.85 * np.exp(-((self.y - hy) / 150.0) ** 2)).astype(F32)[:, None]

        # --- fog fields -----------------------------------------------------
        self.fog_far = WaveField(rng, 18, (420, 1500), (130, 420), self.dur, [1, 1, 1, 2])
        self.fog_near = WaveField(rng, 16, (220, 700), (70, 190), self.dur, [1, 2, 2, 3])

        # --- sea ------------------------------------------------------------
        self.sy0 = int(hy)
        ys = self.y[self.sy0:]
        p = (ys - hy) / (H - hy)
        self.p = p
        zs = 1.0 / (p + 0.30)
        xw = (self.x[None, :] - W / 2) * zs[:, None] * 0.011
        zw = (zs * 9.0)[:, None]
        k = 16
        kx = rng.uniform(0.25, 1.1, k) * rng.choice([-1.0, 1.0], k)
        kz = rng.uniform(2.2, 6.5, k)
        amp = rng.uniform(0.6, 1.0, k)
        amp /= math.sqrt(float(np.sum(amp * amp)) / 2.0)
        ph = rng.uniform(0, TAU, k)
        phase = kx[:, None, None] * xw[None] + kz[:, None, None] * zw[None] + ph[:, None, None]
        self.sea_c = (amp[:, None, None] * np.cos(phase)).astype(F32)
        self.sea_s = (amp[:, None, None] * np.sin(phase)).astype(F32)
        self.sea_om = TAU / self.dur * rng.choice([1.0, 2.0, 2.0, 3.0], k)
        sg = p[:, None] ** 0.8
        self.sea_base = ((NAVY2 * 0.80)[None, None, :] * (1.0 - sg[..., None])
                         + (NAVY1 * 0.55)[None, None, :] * sg[..., None]).astype(F32)
        self.sea_base = np.repeat(self.sea_base, W, axis=1)
        self.glint_gain = (0.25 + 0.75 * p[:, None] ** 0.7).astype(F32) * \
            smooth01(0.0, 0.10, p)[:, None].astype(F32)
        # lamp reflection: a column under the lantern, widening towards the viewer
        self.refl = np.exp(-((self.x[None, :] - self.LX) / (16.0 + 70.0 * p[:, None])) ** 2).astype(F32)
        self.refl *= (1.0 - 0.55 * p[:, None]).astype(F32)
        self.haze = (0.85 * np.exp(-(ys - hy) / 20.0)).astype(F32)[:, None, None]

        # --- rock and lighthouse (static layer with alpha) --------------------
        self._build_lighthouse(rng)

        # --- lamp -------------------------------------------------------------
        lx, ly = self.LX, self.lamp_y
        self.glow = 0.75 * gauss2(self.x, self.y, lx, ly, 11.0) + \
            0.14 * gauss2(self.x, self.y, lx, ly, 42.0)
        dx = np.abs(self.x - lx)
        dy = np.abs(self.y - ly)
        streak = np.exp(-(dy[:, None] / 2.6) ** 2) * np.exp(-dx[None, :] / 150.0)
        streak_v = np.exp(-(dx[None, :] / 2.0) ** 2) * np.exp(-dy[:, None] / 46.0)
        self.glare = (7.0 * gauss2(self.x, self.y, lx, ly, 9.0)
                      + 1.5 * gauss2(self.x, self.y, lx, ly, 34.0)
                      + 0.60 * gauss2(self.x, self.y, lx, ly, 72.0)
                      + 0.50 * gauss2(self.x, self.y, lx, ly, 120.0)
                      + 0.16 * gauss2(self.x, self.y, lx, ly, 330.0)
                      + 0.55 * streak + 0.30 * streak_v).astype(F32)
        # beam sample points: spacing grows with the cone radius
        s, pts = 0.0, []
        while s < 1500.0:
            r = 4.0 + 0.058 * s
            pts.append((s, r, 0.5 * r))
            s += 0.5 * r
        arr = np.array(pts)
        self.bs, self.br = arr[:, 0], arr[:, 1]
        self.bw = arr[:, 2] * np.exp(-arr[:, 0] / 560.0) / (math.pi * arr[:, 1] ** 2)
        self.k_lamp = (np.array([1.0, 0.76, 0.42]) * 1.0).astype(F32)

    # ------------------------------------------------------------------
    def _build_lighthouse(self, rng):
        X, Y = self.X, self.Y
        lx, base, sc = self.LX, self.BASE, self.SC

        def px(u):
            return lx + u * sc

        def py(v):
            return base - v * sc

        rgbl = np.zeros((H, W, 3), dtype=F32)
        alpha = np.zeros((H, W), dtype=F32)

        def paint(a, colour):
            nonlocal rgbl, alpha
            rgbl = over(rgbl, a, colour)
            alpha = alpha + a * (1.0 - alpha)

        # rock: a low faceted skerry, a crag to the right and an outlier to the left
        water = self.HY + 5.0
        d = self.x - lx
        knots = [(-232, 0), (-222, 6), (-208, 9), (-198, 4), (-190, 0), (-168, 0), (-150, 8),
                 (-128, 7), (-112, 17), (-90, 14), (-72, 24), (-48, 21), (-33, 19), (33, 19),
                 (42, 30), (58, 39), (70, 30), (88, 32), (104, 17), (122, 18), (138, 8), (158, 5),
                 (172, 0)]
        prof = np.interp(d, [k[0] for k in knots], [k[1] for k in knots], left=0.0, right=0.0)
        top = water - prof
        solid = smooth01(0.3, 1.6, prof)[None, :]
        a_rock = np.clip(Y - top[None, :] + 0.5, 0.0, 1.0) * np.clip(water + 1.5 - Y, 0.0, 1.0) * solid
        slope = np.gradient(prof)
        facing = np.clip(0.5 + 1.6 * slope, 0.15, 1.0)[None, :]      # faces turned to the left catch the sky
        rim = (np.exp(-np.maximum(Y - top[None, :], 0.0) / 3.5) * facing)[..., None]
        rock_col = INK * 0.85 + (FOG * 0.22) * rim
        paint(a_rock.astype(F32), rock_col.astype(F32))
        self.rock_top = top
        self.water = water
        self.rock_solid = solid.astype(F32)

        # tower, with cylindrical shading (dim light from the left) and a band
        v_top = 150.0
        a_tower = poly_alpha(X, Y, [(px(-24), py(-4)), (px(24), py(-4)), (px(15), py(v_top)), (px(-15), py(v_top))])
        v = (base - Y) / sc
        hw = 24.0 - 9.0 * np.clip(v / v_top, 0.0, 1.0)
        u = np.clip((X - lx) / sc / hw, -1.0, 1.0)
        lit = np.clip(0.5 - 0.5 * u, 0.0, 1.0) ** 1.6
        tower = (INK * 1.5)[None, None, :] * (1.0 - lit[..., None]) + (BONE * 0.30)[None, None, :] * lit[..., None]
        band = smooth01(52.0, 54.0, v) * (1.0 - smooth01(96.0, 98.0, v))
        tower = tower * (1.0 - 0.62 * band[..., None])
        tower = tower * (0.80 + 0.20 * np.clip(v / v_top, 0.0, 1.0))[..., None]
        paint(a_tower, tower.astype(F32))
        # door and two lit windows
        paint(rect_alpha(X, Y, px(-3.2), py(11), px(3.2), py(-4)), INK * 0.6)
        paint(disc_alpha(X, Y, px(0), py(11), 3.2 * sc), INK * 0.6)
        for wv in (38.0, 112.0):
            paint(rect_alpha(X, Y, px(-2.0), py(wv + 7), px(2.0), py(wv)), AMBER * 0.62)
        self.windows = [(px(0), py(41.5)), (px(0), py(115.5))]
        # corbel, deck, railing
        paint(poly_alpha(X, Y, [(px(-15), py(144)), (px(15), py(144)), (px(24), py(154)), (px(-24), py(154))]), INK * 0.9)
        paint(rect_alpha(X, Y, px(-25.5), py(158), px(25.5), py(154)), INK * 1.25)
        paint(rect_alpha(X, Y, px(-25.5), py(169.4), px(25.5), py(168.2)), INK * 1.25)
        for i in range(-4, 5):
            uu = i * 6.2
            paint(rect_alpha(X, Y, px(uu) - 0.6, py(168.5), px(uu) + 0.6, py(158)), INK * 1.25)
        # lantern room: lit glazing, dark mullions and sill
        paint(rect_alpha(X, Y, px(-10.5), py(191), px(10.5), py(158)), INK * 0.9)
        glass = rect_alpha(X, Y, px(-8.8), py(188.5), px(8.8), py(161.5))
        gy = np.clip((Y - py(188.5)) / (py(161.5) - py(188.5)), 0.0, 1.0)
        pane = (AMBER * 0.95)[None, None, :] * (0.72 + 0.28 * np.sin(np.pi * gy))[..., None]
        paint(glass, pane.astype(F32))
        for uu in (-2.95, 2.95):
            paint(rect_alpha(X, Y, px(uu) - 0.75, py(189), px(uu) + 0.75, py(161)), INK * 0.8)
        # roof, vent ball, lightning spike
        paint(poly_alpha(X, Y, [(px(-13), py(190)), (px(13), py(190)), (px(3.6), py(203)), (px(-3.6), py(203))]), INK * 0.95)
        paint(rect_alpha(X, Y, px(-13.6), py(191.6), px(13.6), py(189.6)), INK * 1.3)
        paint(disc_alpha(X, Y, px(0), py(205.5), 3.0 * sc), INK * 0.95)
        paint(rect_alpha(X, Y, px(0) - 0.8, py(217), px(0) + 0.8, py(205)), INK * 0.95)

        self.lh_rgb, self.lh_a = rgbl, alpha[..., None]
        self.lamp_y = float(py(175.0))
        # foam along the foot of the rock, and the rock's dark reflection below it
        self.foam = (np.exp(-((Y - water - 0.5) / 1.8) ** 2) * solid).astype(F32)
        self.foam_x = self.x * 0.13
        below = np.clip(Y - water, 0.0, None)
        self.rock_shade = (0.62 * np.exp(-below / (4.0 + 0.45 * prof[None, :])) * solid
                           * (Y >= water)).astype(F32)[int(self.HY):]

    # ------------------------------------------------------------------
    def beam(self, phi):
        c, s = math.cos(phi), math.sin(phi)
        cx = self.LX + self.bs * c
        cy = self.lamp_y - self.bs * s * 0.035
        gx = np.exp(-((self.x[None, :] - cx[:, None]) / self.br[:, None]) ** 2).astype(F32)
        gy = (np.exp(-((self.y[None, :] - cy[:, None]) / self.br[:, None]) ** 2)
              * (self.bw * (0.72 + 0.40 * s))[:, None]).astype(F32)
        return gy.T @ gx

    def frame(self, t):
        hy = self.sy0
        far = fog01(self.fog_far.at(t))
        near = fog01(self.fog_near.at(t))
        dens = self.dens[:, None] * (0.12 + 0.88 * far)

        phi1 = math.pi / 2 + TAU * (t - BEAM_FLASHES[0]) / self.dur
        phi2 = phi1 - BEAM_SEPARATION
        sigma = math.radians(7.5)

        def facing(phi):
            d = (phi - math.pi / 2 + math.pi) % TAU - math.pi
            return math.exp(-(d / sigma) ** 2)

        flash = facing(phi1) + facing(phi2)

        # sky, stars, ambient fog
        tw = [0.82 + 0.18 * math.sin(TAU * (n * t / self.dur) + ph)
              for n, ph in ((3, 0.0), (4, 2.1), (5, 4.2))]
        stars = (tw[0] * self.stars[0] + tw[1] * self.stars[1] + tw[2] * self.stars[2])
        stars = stars * self.star_fade * (1.0 - 0.75 * far)
        img = self.sky + stars[..., None] * BONE
        img = img + (dens * (0.085 + 0.070 * flash))[..., None] * FOG

        # sea
        ct = np.cos(self.sea_om * t).astype(F32)
        st = np.sin(self.sea_om * t).astype(F32)
        field = np.tensordot(ct, self.sea_c, axes=1) - np.tensordot(st, self.sea_s, axes=1)
        glint = smooth01(0.75, 2.1, field) * self.glint_gain
        spark = smooth01(0.15, 1.6, field) * self.refl
        sea = self.sea_base + (glint * 0.26)[..., None] * FOG
        sea = sea + (spark * (0.10 + 0.55 * min(flash, 1.0)))[..., None] * AMBER
        horizon = img[hy - 1:hy, :, :]
        sea = sea * (1.0 - self.haze) + horizon * self.haze
        img[hy:] = sea * (1.0 - self.rock_shade * (0.80 + 0.20 * np.clip(field, -1.0, 1.0)))[..., None]

        # rock + lighthouse, foam at the waterline
        img = img * (1.0 - self.lh_a) + self.lh_rgb
        swash = 0.5 + 0.5 * np.sin(self.foam_x + TAU * t / self.dur * 2.0) * np.sin(self.foam_x * 0.37 - TAU * t / self.dur)
        img = img + (self.foam * (0.05 + 0.16 * swash)[None, :].astype(F32))[..., None] * BONE

        # low mist drifting across the water and the foot of the rock
        mist = 0.42 * np.exp(-((self.y - (self.HY + 6.0)) / 30.0) ** 2)[:, None] * near
        img = img * (1.0 - mist[..., None]) + (mist * (0.21 + 0.10 * flash))[..., None] * FOG

        # lamp: steady glow, two volumetric beams lighting the fog, lens glare
        beams = self.beam(phi1) + self.beam(phi2)
        light = self.glow + 30.0 * beams * (0.74 + 0.52 * near) + flash * self.glare
        img = 1.0 - (1.0 - img) * np.exp(-light[..., None] * self.k_lamp)
        return finish(img, self.vignette, self.dither)


# ===========================================================================
# Scene 2: the signal
# ===========================================================================

class SignalScene:
    dur = SIGNAL_DUR
    SX, SY = 322.0, 318.0

    def __init__(self):
        rng = np.random.default_rng(4410)
        self.x = np.arange(W, dtype=F32)
        self.y = np.arange(H, dtype=F32)
        X, Y = np.meshgrid(self.x, self.y)
        self.vignette = make_vignette(0.55)
        self.dither = make_dither(9)
        # background: fog bank, brightest in a band above the unseen sea
        g = np.exp(-((self.y - 300.0) / 190.0) ** 2)
        top = NAVY1 * 0.80
        mid = NAVY2 * 0.62 + FOG * 0.115
        bg = top[None, :] * (1.0 - g[:, None]) + mid[None, :] * g[:, None]
        bg = bg * (1.0 - 0.45 * smooth01(360.0, 540.0, self.y))[:, None]
        self.bg = np.repeat(bg[:, None, :], W, axis=1).astype(F32)
        self.profile = (0.35 + 0.65 * g).astype(F32)[:, None]
        self.fog_far = WaveField(rng, 18, (480, 1600), (170, 520), self.dur, [1, 1, 2])
        self.fog_near = WaveField(rng, 18, (190, 620), (80, 230), self.dur, [1, 2, 3, 3])

        # gallery rail (foreground silhouette)
        bow = 0.00006 * (self.x - W / 2) ** 2
        y_top = 438.0 + bow
        y_mid = 487.0 + bow * 1.15
        a = np.clip(Y - y_top[None, :] + 0.5, 0, 1) * np.clip(y_top[None, :] + 10.0 - Y + 0.5, 0, 1)
        a = np.maximum(a, np.clip(Y - y_mid[None, :] + 0.5, 0, 1) * np.clip(y_mid[None, :] + 5.0 - Y + 0.5, 0, 1))
        a = np.maximum(a, np.clip(Y - 527.0 + 0.5, 0, 1))
        self.posts = [60.0 + 120.0 * i for i in range(8)]
        rim_side = np.zeros((H, W), dtype=F32)
        for xb in self.posts:
            bar = np.clip(3.5 + 0.5 - np.abs(X - xb), 0, 1) * np.clip(Y - (y_top[None, :] + 5.0), 0, 1)
            a = np.maximum(a, bar)
            side = 1.0 if xb < self.SX else -1.0          # the edge facing the light
            edge = np.exp(-((X - (xb + side * 3.2)) / 1.1) ** 2) * np.clip(Y - (y_top[None, :] + 10.0), 0, 1) \
                * np.clip(527.0 - Y, 0, 1)
            rim_side += (edge * math.exp(-((xb - self.SX) / 330.0) ** 2)).astype(F32)
        self.rail_a = a.astype(F32)[..., None]
        rim = np.exp(-((Y - (y_top[None, :] + 0.8)) / 1.2) ** 2).astype(F32)
        rim_mid = np.exp(-((Y - (y_mid[None, :] + 0.6)) / 1.0) ** 2).astype(F32)
        near_light = np.exp(-((self.x - self.SX) / 300.0) ** 2).astype(F32)[None, :]
        self.rim_cool = (rim * 0.20 + rim_mid * 0.09)[..., None] * FOG
        self.rim_warm = ((rim * 0.50 + rim_mid * 0.20) * near_light + rim_side * 0.22)[..., None] * AMBER
        self.k_lamp = np.array([1.0, 0.74, 0.40], dtype=F32)

    @staticmethod
    def level(t):
        v = 0.0
        for a, b in SIGNAL_ON:
            if a <= t <= b:
                v = float(smooth01(a, a + 0.10, t) * (1.0 - smooth01(b - 0.10, b, t)))
        return v

    def frame(self, t):
        far = fog01(self.fog_far.at(t))
        near = fog01(self.fog_near.at(t))
        lvl = self.level(t)
        img = self.bg + ((0.105 * far + 0.060 * near) * self.profile)[..., None] * FOG

        # the light rides a slow swell
        sx = self.SX + 1.5 * math.sin(TAU * t / 14.0)
        sy = self.SY + 3.0 * math.sin(TAU * t / 7.0 + 0.6)
        if lvl > 0.0:
            x, y = self.x, self.y
            halo = (6.0 * gauss2(x, y, sx, sy, 5.0) + 1.8 * gauss2(x, y, sx, sy, 17.0)
                    + 0.62 * gauss2(x, y, sx, sy, 58.0) * (0.45 + 0.9 * near)
                    + 0.30 * gauss2(x, y, sx, sy, 185.0) * (0.35 + 1.1 * far))
            # a broken reflection on the water below
            dy = y - (sy + 30.0)
            rows = np.where(dy > 0, np.exp(-np.maximum(dy, 0) / 34.0), 0.0) * \
                (0.55 + 0.45 * np.sin(y * 1.9 + TAU * t * 0.9) * np.sin(y * 0.43 - TAU * t * 0.35))
            wob = 2.5 * np.sin(y * 0.31 + TAU * t * 0.5)
            refl = rows[:, None] * np.exp(-((x[None, :] - sx - wob[:, None]) / 7.0) ** 2)
            halo = halo + 0.42 * refl.astype(F32) * (0.5 + 0.6 * near)
            img = 1.0 - (1.0 - img) * np.exp(-(lvl * halo)[..., None] * self.k_lamp)

        rail = (INK * 0.55) + self.rim_cool + lvl * self.rim_warm
        img = img * (1.0 - self.rail_a) + rail * self.rail_a
        return finish(img, self.vignette, self.dither)


# ===========================================================================
# Encoding, captions, driver
# ===========================================================================

def encode_video(scene, audio, out, tmp):
    raw = Path(tmp) / "audio.f32"
    audio.astype("<f4").tofile(raw)
    frames = int(round(scene.dur * FPS))
    cmd = [FFMPEG, "-y", "-loglevel", "error",
           "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{W}x{H}", "-r", str(FPS), "-i", "pipe:0",
           *RAW_IN, "-i", str(raw),
           "-vf", "scale=out_color_matrix=bt709:out_range=tv:flags=accurate_rnd+full_chroma_int,format=yuv420p",
           "-c:v", "libx264", "-profile:v", "high", "-preset", "slow", "-crf", "19", "-g", "48",
           "-x264-params", "aq-mode=3:aq-strength=1.1",
           "-color_primaries", "bt709", "-color_trc", "bt709", "-colorspace", "bt709", "-color_range", "tv",
           "-c:a", "aac", "-b:a", "128k", "-t", f"{scene.dur:.3f}",
           "-movflags", "+faststart", *BITEXACT, "-flags:v", "+bitexact", str(out)]
    proc = subprocess.Popen(cmd, stdin=subprocess.PIPE)
    for i in range(frames):
        proc.stdin.write(scene.frame(i / FPS).tobytes())
    proc.stdin.close()
    if proc.wait() != 0:
        raise SystemExit(f"ffmpeg failed for {out}")


def write_image(frame, out, fmt_args):
    run([FFMPEG, "-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgb24",
         "-s", f"{W}x{H}", "-i", "pipe:0", "-frames:v", "1", *fmt_args,
         "-map_metadata", "-1", "-fflags", "+bitexact", str(out)], input=frame.tobytes())


def write_poster(scene, t, out):
    write_image(scene.frame(t), out, ["-q:v", "2", "-pix_fmt", "yuvj420p"])


def stamp(t):
    ms = int(round(t * 1000))
    return f"{ms // 3600000:02d}:{ms // 60000 % 60:02d}:{ms // 1000 % 60:02d}.{ms % 1000:03d}"


def write_vtt(out, cues):
    lines = ["WEBVTT", ""]
    for i, (a, b, text) in enumerate(cues, 1):
        lines += [str(i), f"{stamp(a)} --> {stamp(b)}", text, ""]
    Path(out).write_text("\n".join(lines), encoding="utf-8")


def blast_cues(notes, first, again):
    ordinal = ("first", "second", "third")
    cues = []
    for i, (a, b) in enumerate(notes):
        lead = first if i < 3 else again
        cues.append((a, b, f"[{lead}: {ordinal[i % 3]} long blast]"))
    return cues


BEAM_CUES = [
    (0.0, 3.4, "[Swell washing over rock]"),
    (3.4, 5.2, "[Water draining from the ledges]"),
    (5.2, 8.3, "[A second, smaller swell]"),
    (8.5, 12.0, "[Fog bell: one stroke, ringing away]"),
]
_sig = blast_cues(SIGNAL_ON, "Distant horn", "Horn again")
SIGNAL_CUES = [(0.0, 1.0, "[Quiet sea under fog]"), *_sig[:3],
               (5.8, 7.6, "[Pause. Only the sea]"), *_sig[3:],
               (12.4, 14.0, "[The horn falls silent]")]
_horn = blast_cues(HORN_NOTES, "Distant horn", "Horn again")
HORN_CUES = [(0.0, 1.5, "[Quiet sea under fog]"), *_horn[:3],
             (7.7, 10.0, "[Pause. Only the sea]"), *_horn[3:],
             (16.2, 18.0, "[The horn falls silent]")]

BEAM_POSTER_T = BEAM_FLASHES[0]
SIGNAL_POSTER_T = 3.4

TARGETS = ("sea", "bell", "horn", "beam", "signal")


def stills(directory):
    directory = Path(directory)
    directory.mkdir(parents=True, exist_ok=True)
    beam, signal = BeamScene(), SignalScene()
    for t in (0.0, 1.5, 2.75, 3.0, 4.25, 5.5, 7.0, 9.5):
        write_image(beam.frame(t), directory / f"beam_{t:05.2f}.png", [])
    for t in (0.5, 1.05, 1.6, 3.4, 6.5, 10.0):
        write_image(signal.frame(t), directory / f"signal_{t:05.2f}.png", [])


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--only", default=",".join(TARGETS), help="comma-separated subset of: " + ", ".join(TARGETS))
    ap.add_argument("--stills", metavar="DIR", help="write debug PNG frames to DIR and exit")
    args = ap.parse_args()
    if args.stills:
        stills(args.stills)
        return
    only = [s.strip() for s in args.only.split(",") if s.strip()]
    unknown = [s for s in only if s not in TARGETS]
    if unknown:
        raise SystemExit(f"unknown target(s): {', '.join(unknown)}")
    MEDIA.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="darm-media-") as tmp:
        if "sea" in only:
            encode_audio(normalise(build_sea_audio(), tmp), MEDIA / "sea.m4a", tmp)
        if "bell" in only:
            encode_audio(normalise(build_bell_audio(), tmp), MEDIA / "bell.m4a", tmp)
        if "horn" in only:
            encode_audio(normalise(build_horn_audio(), tmp), MEDIA / "horn.m4a", tmp)
            write_vtt(MEDIA / "horn.en.vtt", HORN_CUES)
        if "beam" in only:
            scene = BeamScene()
            encode_video(scene, normalise(build_beam_audio(), tmp), MEDIA / "beam.mp4", tmp)
            write_poster(scene, BEAM_POSTER_T, MEDIA / "beam.jpg")
            write_vtt(MEDIA / "beam.en.vtt", BEAM_CUES)
        if "signal" in only:
            scene = SignalScene()
            encode_video(scene, normalise(build_signal_audio(), tmp), MEDIA / "signal.mp4", tmp)
            write_poster(scene, SIGNAL_POSTER_T, MEDIA / "signal.jpg")
            write_vtt(MEDIA / "signal.en.vtt", SIGNAL_CUES)
    for name in sorted(p.name for p in MEDIA.iterdir()):
        print(f"{(MEDIA / name).stat().st_size:>9}  {name}")


if __name__ == "__main__":
    sys.exit(main())
