# Page-turn animation

How DARM turns a page, why it is built this way, how it degrades, and how to
change it. Code: [`apps/reader/src/flip/geometry.ts`](../apps/reader/src/flip/geometry.ts)
(shape of a leaf), [`flip/controller.ts`](../apps/reader/src/flip/controller.ts)
(physics and the stack of sheets), [`gl/renderer.ts`](../apps/reader/src/gl/renderer.ts)
(WebGL2), [`fallback/renderer2d.ts`](../apps/reader/src/fallback/renderer2d.ts)
(Canvas 2D), [`ui/stage.ts`](../apps/reader/src/ui/stage.ts) (gestures, frame loop).

## 1. The reference video

`~/Desktop/vid.mov` was opened and analysed frame by frame with `ffprobe` and
`ffmpeg` (contact sheets of every 5th frame, a strip of 12 consecutive frames,
and one full-resolution frame). Facts:

| Property | Observation |
|---|---|
| Container | H.264 744×746 + AAC, 3.8 s, 119 video frames (≈ 30 fps effective) |
| Content | A hand-drawn (cel style) open book in three-quarter view: cream pages, red cover boards, black outline |
| Motion | Leaves turn continuously from right to left; **four to five leaves are in the air at once**, each at a different phase, fanned from the spine |
| Cycle | The drawing repeats about every 10 frames (≈ 0.33 s between successive leaves); one leaf takes roughly a second to cross |
| Shape of a leaf | Hinged at the spine. The **root leaves first and the free edge trails**: early in the turn the sheet bows backwards; near vertical it carries a slight S-bend; at the end the belly lands first and the tip whips over last |
| Light | Flat fills with a peach band on the page beneath each lifted leaf (a cast shadow that is widest near the spine) and a darker tone in the gutter |
| Thickness | Page block and boards are drawn with visible edges; the book reads as an object, not a plane |

The video is a reference for the *feel of the leaves*, not a specification of
look, and it is not shipped or played anywhere in the product. What DARM takes
from it:

1. a leaf is a **flexible sheet hinged at the spine**, not a rigid card and not a
   sticker being peeled;
2. in a free turn **the spine leads and the edge lags**;
3. **several leaves can be airborne at once**, in stacking order;
4. each leaf **darkens the page under it**, most strongly near the gutter;
5. the book has **thickness**: boards, page-stack edges.

What it deliberately does not take: the three-quarter camera (text must stay
readable while reading, see §4), the outline style, the constant looping.

## 2. The model of a leaf

A leaf's cross-section, perpendicular to the spine, is described by its
tangent angle θ(u) at arc length u ∈ [0, 1] from the spine (0 = flat on the
right-hand stack, π = flat on the left). Positions come from integrating the
tangent:

```
x(u) = ∫₀ᵘ cos θ      z(u) = ∫₀ᵘ sin θ
```

Consequences, each covered by a unit test in `tests/flip.test.ts`:

* **the sheet never stretches** — every cross-section has length exactly 1
  whatever θ is, which is the main reason it reads as paper;
* **it never sinks into the book** — θ ∈ [0, π] ⇒ z ≥ 0;
* it is attached at the spine at all times (x(0) = z(0) = 0).

θ is a travelling wave with two parameters, progress `p` and shape `λ`:

```
θ(u) = π · E( p·(1+|λ|) − |λ|·d(u) )        E = smoothstep, clamped
d(u) = 1 − u   if λ > 0     (free edge leads)
d(u) = u       if λ < 0     (spine leads)
```

| λ | Family | Used for |
|---|---|---|
| `+2.4` | **Curl.** The page peels from its free edge and rolls over a fold that travels towards the spine; 1/λ is the fold width. | Dragging with finger or mouse, and the release that follows |
| `−0.42` | **Throw.** The leaf is driven from its root; the edge trails, the sheet bows, and the tip whips over at the end — the behaviour seen in the video. | Taps, keys, programmatic jumps, riffles |
| `0` | **Rigid.** Pure rotation. | Cover boards |

At p = 0 and p = 1 every family is exactly flat, so a leaf coincides with the
resting page at both ends and there is no pop. λ is itself animated, so a leaf
caught in mid-throw eases into the curl family instead of snapping. A leaf
travelling backwards uses the mirrored family (θ ↦ π − θ, p ↦ 1 − p), so the
same part of the sheet leads in both directions.

**Corner lift.** The row at height v along the spine uses
`p + κ·(v − v_grab)·sin(πp)`. κ comes from where the page was grabbed (a
corner leads) and from vertical finger travel, and the `sin` taper removes the
skew at both ends. This is an approximation of a cone-shaped bend (rows keep
their own length exactly; distances between rows change slightly), chosen over
an exact developable cone because it keeps the spine attached for every κ.

**Mesh.** 56 × 20 quads per leaf, rebuilt on the CPU every frame (a few
thousand trigonometric evaluations) and uploaded with `bufferSubData`. Doing
this on the CPU keeps one implementation of the shape for the WebGL renderer,
the Canvas 2D fallback, the drag solver and the tests.

## 3. Motion

`FlipController` models the book as a stack of *sheets* — front board, paper
leaves, back board — and a position `pos` (how many sheets lie on the left).
Sheets in the air are *flyers*; each is a **critically damped spring** on `p`:

```
p″ = ω²(target − p) − 2ω p′          semi-implicit Euler, sub-stepped at ≤ 1/240 s
```

| Constant | Value | Meaning |
|---|---|---|
| `omegaThrow` | 13.5 rad/s | free turn; settles in ≈ 0.45 s |
| `omegaRelease` | 16 rad/s | after a drag, starting from the finger's velocity |
| `omegaBoard` | 8.6 rad/s | cover boards are heavier |
| `omegaHeld` | 46 rad/s | smoothing while a finger holds the page |
| `shapeRate` | 14 /s | relaxation of λ and κ |
| `gap` | 0.035 | minimum separation between adjacent airborne leaves (vanishes at both ends) |
| `stagger` | 50 ms | delay between leaves of a riffle |
| `maxRiffle` | 6 | leaves animated for one long jump |

Why springs rather than fixed-duration tweens: a turn can be **reversed,
grabbed or released in mid-air** with continuous position and velocity; the
same constants govern every turn, so the motion is consistent; and the
simulation is frame-rate independent (tested at 30, 60 and 120 Hz).

* **Drag.** On pointer-down nothing moves; once the pointer has travelled 7 px
  sideways the sheet under it is grabbed. The controller solves, by bisection,
  for the `p` at which the *grabbed point of the paper* appears under the
  pointer (perspective included), so the page tracks the finger rather than
  following a fraction of the screen. Release completes the turn if
  `p + 0.16·p′ > 0.5` (position plus a little momentum: a quick flick turns the
  page from anywhere), otherwise the page falls back. `pointercancel` always
  returns it.
* **Several at once.** Each `next`/`prev` launches (or re-targets) one flyer;
  flyers are kept in stacking order with a minimum separation, so quick taps
  fan the leaves as in the reference.
* **Jumps.** A long jump opens the cover if needed, riffles at most six
  leaves (the ones in the middle carry a "ghost" texture of grey text lines,
  only the first and last faces are real pages), and closes onto the back
  cover if that is the destination. Arrival takes well under two seconds for
  any distance.
* **Opening and closing.** The front and back covers are sheets like any other,
  but rigid and thick. While the front board swings, the camera pans from the
  centre of the closed book to the centre of the spread and the view zooms from
  90 % to 100 %; the reverse happens when the back board closes over the last
  page. A returning reader sees the cover, then the book opens by itself and
  riffles to the saved page.

## 4. Rendering

**Technique: a deformable mesh in WebGL2, textured with pages painted on a 2D
canvas.** Reasons:

* CSS 3D transforms can rotate a rigid rectangle; they cannot bend it, shade it
  per pixel or show a correctly mapped reverse side. A pre-rendered video or
  sprite cannot follow a finger or show the book's actual pages.
* Painting pages on a canvas (rather than laying them out as HTML) is what
  makes them usable as textures in every engine; rasterising live DOM into a
  texture is not portable (WebKit taints canvases drawn from `foreignObject`).
* WebGPU would work equally well but is not yet available in every target
  webview; nothing here needs more than WebGL2.

**Camera.** Straight down, perspective, at 5.2 page-widths. A perspective
camera perpendicular to a plane maps that plane by a pure scale, so **resting
pages are drawn texel-for-pixel** (page sizes are snapped to even device
pixels) and text is as sharp as the canvas it was painted on, while anything
lifted off the plane grows towards the viewer. The projection is written by
hand in the vertex shader (`w = (D − z)/D`), which gives perspective-correct
texture interpolation on the bent mesh.

**Passes per frame** (`GlRenderer.render`):

1. *Shadow map* (only while something is airborne): flyers are projected along
   the light direction onto the page plane into a 384×256 R8 framebuffer with
   MAX blending (overlapping leaves do not double the shadow; higher parts
   write a lighter value), then blurred with two rounds of a separable 9-tap
   Gaussian.
2. *Scene*: desk gradient → shadows received by the desk (a soft analytic
   shadow under the book plus the shadow map) → opened boards → fanned edges of
   the two page stacks (their width follows how many leaves lie on each side) →
   the two resting pages → flyers with depth testing.

**Shading.**

* Resting page: texture × gutter falloff (two exponentials, strongest at the
  spine) × a slight fall-off at the fore-edge × the shadow map × ±0.7 % paper
  grain. The lighting term is normalised so a surface facing straight up keeps
  its exact colours.
* Flying leaf: two-sided. The reverse shows the next page (spread mode) or the
  front's ink faintly mirrored through the paper (single-page mode). Diffuse
  lighting from the mesh normal with a light from the upper left, a small
  back-lit transmission term, darkening where the sheet dives into the spine,
  and a thin darker rim along the edges.
* Boards: cover art with rounded corners, a joint groove beside the spine, a
  bevel, cloth grain and a moving sheen; four edge faces and the turned-in
  cloth give them thickness; the endpaper is a page-sized sheet pasted inside.
* Textures carry mipmaps and up to 4× anisotropy so text stays calm while a
  page is foreshortened.

**Cost.** Nothing is rendered while the book is at rest (render on demand).
During a turn each frame rebuilds one mesh per flyer and issues about a dozen
draw calls. Page textures are painted when first needed and pre-warmed for the
neighbouring sheets during idle time; they live in an LRU cache with a byte
budget (220 MiB by default, and each page is capped at about 5 megapixels).

## 5. Degradation

| Situation | Behaviour |
|---|---|
| No WebGL2, or shader/context creation fails | `Canvas2dRenderer`: the same leaf model, orthographic. Each leaf is drawn as 44 vertical slices of the page image placed along the curve and shaded from the slice normal; soft rectangular shadows; no perspective, no corner skew. Forced with `?renderer=2d` |
| WebGL context lost | Rendering pauses; on restore, programs and buffers are rebuilt and every surface is repainted |
| `prefers-reduced-motion`, or Motion → Reduced in settings | No flight: the position changes at once with a 140 ms opacity dip; swipes, taps, keys and the scrubber all still work; entrance animations of blocks show their final state |
| Low memory / very large pages | Texture resolution is reduced before anything else (`textureScale`), then least-recently-used pages are evicted |
| Background tab | `requestAnimationFrame` stops; the spring integrator clamps the time step to 100 ms, so nothing jumps on return |

## 6. Measuring

* `tests/flip.test.ts` (Vitest, no browser): geometry invariants, drag
  tracking accuracy, turn duration, frame-rate independence, stacking order,
  reversal, riffle bounds, and a 4 000-step random-input run.
* The end-to-end suite records the interval between animation frames during
  real turns in each browser engine (`window.darm.diagnostics().frameTimes`)
  and reports median and 95th percentile; see `docs/testing.md` for the
  numbers measured on the development machine. No fixed frame rate is
  promised: on capable hardware a turn runs at the display's refresh rate; on
  weaker hardware the springs simply take larger time steps.
* `harness/flip.html` is a content-free playground with frozen poses for
  screenshots: `?pos=3&freeze=0.42&lambda=2.4&kappa=-0.3&vgrab=0.1`,
  `&fan=4`, `&dark=1`, `&2d=1`, `&mode=single`.

## 7. Changing or adding animations

* **Feel**: every constant is in `MOTION` (`flip/controller.ts`). Raise
  `omegaThrow` for snappier turns; move `lambdaThrow` towards 0 for stiffer
  paper, further negative for floppier; change `lambdaDrag` to widen or tighten
  the curl.
* **A new shape family**: `tangent()` in `flip/geometry.ts` is the only place
  that defines the curve. Keep θ within [0, π] and exactly 0 / π at p = 0 / 1
  and the tests in §6 will hold for it.
* **Lighting**: the fragment shaders at the top of `gl/renderer.ts`
  (`FS_PAGE`, `FS_LEAF`, `FS_BOARD`). The fallback mirrors only the diffuse
  term.
* **Entrance animations of blocks** (`animate=` in the language) are a separate
  mechanism: the painter draws a block's items with eased opacity/offset/scale
  as a function of time since the page was shown (`paint/page.ts`), and the
  reader repaints that page's texture each frame while one is running. To add
  a kind: extend `AnimKind` in `crates/darm-ir`, the `ANIMATIONS` table in
  `crates/darm-compiler/src/spec.rs`, `ir.ts`, and the painter.

## 8. Known limits

* The corner-lift is an approximation, not an exact developable surface; with
  extreme skew, text near the corner shears slightly.
* Leaves do not cast shadows on each other, only on the resting pages, the
  boards and the desk.
* A leaf has no thickness of its own (paper is drawn as a surface with a rim);
  only boards are solid.
* The fallback renderer has no perspective and no skew.
* There is no page-turn sound.
