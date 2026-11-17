/**
 * Geometry shared by the layout (which sizes widgets and sets their labels)
 * and the painter (which draws their chrome). Pure functions of the widget
 * box, so both sides, and the DOM overlays of the app, agree to the pixel.
 *
 * Division of labour for `button`, `option` and `answer`: the painter draws
 * the control itself (surface, outline, marker, state glyph); the label is
 * emitted by the layout as ordinary text items placed on top, so that it is
 * measured, wrapped and coloured by the same typesetter as the rest of the
 * page. The painter draws the lettering of `audio` and `gallery` itself.
 */

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Minimum height of anything the reader can tap, CSS px. */
export const MIN_TARGET = 44;

/** Label size of a button of height `h`. */
export function buttonLabelSize(h: number): number {
  return Math.round(Math.max(11, Math.min(19, h * 0.335)) * 10) / 10;
}

/** Corner radius of a button of height `h`: a soft rectangle, not a lozenge. */
export function buttonRadius(h: number): number {
  return Math.min(h / 2, Math.max(4, h * 0.2));
}

/**
 * Parts of an `option` / `answer` row, relative to the widget box.
 * `marker` is the radio-like disc (vertically centred); labels start at
 * `textX` and may use `textW`.
 */
export function rowWidgetLayout(w: number, h: number): { marker: Rect; textX: number; textW: number; radius: number } {
  const d = h >= MIN_TARGET ? 20 : Math.max(8, Math.round(h * 0.45));
  const pad = Math.min(14, Math.max(6, w * 0.04));
  const textX = pad + d + Math.min(12, pad);
  return {
    marker: { x: pad, y: (h - d) / 2, w: d, h: d },
    textX,
    textW: Math.max(10, w - textX - pad),
    radius: 6,
  };
}

/**
 * Parts of the audio player, relative to the widget box:
 *
 *   ┌──────────────────────────────────────────────┐
 *   │ (▶)  Title of the recording                  │
 *   │      ────────────────────────────  0:00      │
 *   └──────────────────────────────────────────────┘
 *
 * `play` is the round button, `title` the single line of lettering,
 * `track` the progress line (its box is taller than the stroke so it can be
 * a hit target) and `time` the duration read-out. The live DOM player is
 * expected to place its controls over exactly these rectangles.
 */
export function audioWidgetLayout(w: number, h: number): { play: Rect; title: Rect; track: Rect; time: Rect; radius: number } {
  const pad = Math.max(8, Math.min(16, h * 0.22));
  const d = Math.max(24, Math.min(40, h - 2 * pad));
  const play: Rect = { x: pad, y: (h - d) / 2, w: d, h: d };
  const x = play.x + d + pad;
  const right = Math.max(x + 20, w - pad - 2);
  const timeW = Math.min(48, Math.max(30, w * 0.13));
  const k = Math.min(1.4, h / 56);
  const mid = h / 2;
  // Title and track are stacked around the centre line, so they balance the round button.
  const titleH = 18 * k;
  const trackH = 14 * k;
  return {
    play,
    title: { x, y: mid - titleH - 1 * k, w: right - x, h: titleH },
    track: { x, y: mid + 3 * k, w: Math.max(10, right - x - timeW - 10), h: trackH },
    time: { x: right - timeW, y: mid + 3 * k, w: timeW, h: trackH },
    radius: Math.min(12, h * 0.2),
  };
}

/** Parts of the video box: `frame` is the picture (the whole box) and `play` the centred disc. */
export function videoWidgetLayout(w: number, h: number): { frame: Rect; play: Rect; radius: number } {
  const d = Math.max(28, Math.min(72, Math.min(w, h) * 0.26));
  return {
    frame: { x: 0, y: 0, w, h },
    play: { x: (w - d) / 2, y: (h - d) / 2, w: d, h: d },
    radius: 3,
  };
}

/** Height of the strip under a gallery picture that holds the position dots and the count. */
export function galleryStripHeight(w: number): number {
  return w < 200 ? 18 : 24;
}

/** Parts of the gallery: the picture, and the strip with dots and "1 / n" below it. */
export function galleryWidgetLayout(w: number, h: number): { image: Rect; indicator: Rect; radius: number } {
  const strip = Math.min(galleryStripHeight(w), h * 0.3);
  return {
    image: { x: 0, y: 0, w, h: h - strip },
    indicator: { x: 0, y: h - strip, w, h: strip },
    radius: 3,
  };
}
