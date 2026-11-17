/**
 * WebGL2 renderer of the book scene.
 *
 * Why WebGL and a deformable mesh: a turning page has to bend, foreshorten,
 * show its reverse side and cast a soft shadow, all at display refresh rate
 * and with the page's real content on it. CSS transforms can only rotate a
 * rigid plane; a mesh whose vertices are recomputed every frame from the
 * leaf model in `flip/geometry.ts` gives true curvature, per-pixel lighting
 * from the surface normal, two-sided texturing and a projected, blurred
 * shadow — while the page content stays an ordinary texture painted once.
 *
 * The camera looks straight down. A perspective camera perpendicular to the
 * page plane maps that plane by a pure scale, so pages at rest are drawn
 * texel-for-pixel (crisp text), while anything lifted off the plane grows
 * towards the viewer with true perspective.
 *
 * Passes per frame:
 *   1. shadow map: leaves in the air are projected along the light onto the
 *      page plane into a small framebuffer and blurred (separable Gaussian);
 *   2. scene: desk, shadows received by the desk, the opened boards, the page
 *      stacks' edges, the two resting pages (which sample the shadow map),
 *      then the flying leaves and boards with depth testing.
 */
import type { FlyerPose, ScenePose, SurfaceKey } from "../flip/controller";
import { MOTION } from "../flip/controller";
import { MESH_COLS, MESH_ROWS, VERTEX_SIZE, boardIndices, buildBoard, buildLeaf, buildPaste, leafIndices } from "../flip/geometry";

export interface SceneView {
  /** Canvas size in device pixels. */
  width: number;
  height: number;
  /** Width of one page at zoom 1, in device pixels (an integer, so resting pages are texel-exact). */
  pagePx: number;
  /** Page height divided by page width. */
  aspect: number;
  /** Where the view centre sits on the canvas, in device pixels from the top-left. */
  centerX: number;
  centerY: number;
  /** Two-page spread (gutter shading on both pages) or single page. */
  spread: boolean;
}

export interface SceneStyle {
  /** Desk colours (centre, edge), linear-ish sRGB triples 0 … 1. */
  desk: [number, number, number];
  deskEdge: [number, number, number];
  paper: [number, number, number];
  /** Cover cloth colour, for board edges and the opened boards under the pages. */
  cloth: [number, number, number];
  dark: boolean;
}

/** Board overhang beyond the page block and board thickness, in page widths. */
export const BOARD_OVERHANG = 0.022;
export const BOARD_THICK = 0.016;
const STACK_MAX = 0.03;

const LIGHT = normalize([-0.38, 0.46, 0.8]);
/** Shadow displacement per unit of height (light from the upper left). */
const SHADOW_SLOPE: [number, number] = [0.3, -0.22];

function normalize(v: [number, number, number]): [number, number, number] {
  const l = Math.hypot(v[0], v[1], v[2]);
  return [v[0] / l, v[1] / l, v[2] / l];
}

const VS = `#version 300 es
layout(location=0) in vec3 aPos;
layout(location=1) in vec3 aNormal;
layout(location=2) in vec2 aUv;
uniform vec4 uView;   // centre (ndc), scale (ndc per world unit at z=0)
uniform vec2 uCam;    // camera x, camera distance
out vec3 vWorld;
out vec3 vNormal;
out vec2 vUv;
void main() {
  float w = (uCam.y - aPos.z) / uCam.y;         // perspective divide: nearer = bigger
  vec2 ndc = uView.xy + vec2(aPos.x - uCam.x, aPos.y) * uView.zw / w;
  float depth = 1.0 - 2.0 * (aPos.z + 0.05) / 1.4;
  gl_Position = vec4(ndc * w, depth * w, w);
  vWorld = aPos;
  vNormal = aNormal;
  vUv = aUv;
}`;

const COMMON_FS = `#version 300 es
precision highp float;
in vec3 vWorld;
in vec3 vNormal;
in vec2 vUv;
out vec4 outColor;
uniform vec3 uLight;
float hash(vec2 p) {
  vec3 q = fract(vec3(p.xyx) * 0.1031);
  q += dot(q, q.yzx + 33.33);
  return fract((q.x + q.y) * q.z);
}
float lit(vec3 n) {
  // 1.0 for a surface facing straight up, so resting pages keep their exact colours.
  return 0.76 + 0.24 * max(dot(n, uLight), 0.0) / uLight.z;
}
float roundRect(vec2 uv, vec2 size, float r) {
  vec2 p = (uv - 0.5) * size;
  vec2 d = abs(p) - size * 0.5 + r;
  return length(max(d, 0.0)) + min(max(d.x, d.y), 0.0) - r;
}`;

/** A page lying flat: texture, gutter shading, paper grain, shadows cast by leaves in the air. */
const FS_PAGE = `${COMMON_FS}
uniform sampler2D uTex;
uniform sampler2D uShadow;
uniform vec4 uShadowRect;
uniform float uShadowOn;
uniform float uGutter;
void main() {
  vec3 c = texture(uTex, vec2(vUv.x, 1.0 - vUv.y)).rgb;
  float u = abs(vWorld.x);
  float gutter = 1.0 - uGutter * (0.26 * exp(-u / 0.030) + 0.09 * exp(-u / 0.15));
  float fore = 1.0 - 0.045 * smoothstep(0.93, 1.0, u);
  float s = texture(uShadow, (vWorld.xy - uShadowRect.xy) / uShadowRect.zw).r * uShadowOn;
  float grain = 1.0 + (hash(gl_FragCoord.xy) - 0.5) * 0.014;
  outColor = vec4(c * gutter * fore * grain * (1.0 - 0.5 * s), 1.0);
}`;

/** A leaf in the air: two-sided, lit from its normal. */
const FS_LEAF = `${COMMON_FS}
uniform sampler2D uFront;
uniform sampler2D uBack;
uniform float uMirror;
uniform vec3 uPaper;
void main() {
  bool front = gl_FrontFacing;
  vec3 n = normalize(vNormal) * (front ? 1.0 : -1.0);
  vec2 uvFront = vec2(vUv.x, 1.0 - vUv.y);
  vec3 c;
  if (front) {
    c = texture(uFront, uvFront).rgb;
  } else if (uMirror > 0.5) {
    // Single-sided leaf: the ink of the front shows through, faint and mirrored.
    c = mix(uPaper, texture(uFront, uvFront).rgb, 0.085);
  } else {
    c = texture(uBack, vec2(1.0 - vUv.x, 1.0 - vUv.y)).rgb;
  }
  float shade = min(lit(n), 1.07);
  // Light scattered through the sheet when it is lit from behind.
  shade += 0.10 * max(-dot(normalize(vNormal) * (front ? 1.0 : -1.0), uLight), 0.0);
  // The sheet darkens where it dives into the spine, and along its very edge.
  float root = 1.0 - 0.20 * exp(-vUv.x / 0.045);
  float edge = min(min(1.0 - vUv.x, vUv.y), 1.0 - vUv.y);
  float rim = 1.0 - 0.10 * (1.0 - smoothstep(0.0, 0.006, edge));
  float grain = 1.0 + (hash(gl_FragCoord.xy) - 0.5) * 0.014;
  outColor = vec4(c * shade * root * rim * grain, 1.0);
}`;

/** Flat-coloured lit surfaces: board edges, the opened boards, page-stack edges. */
const FS_SOLID = `${COMMON_FS}
uniform vec3 uColor;
uniform vec2 uSize;      // world size of the quad, for rounded corners
uniform float uRadius;
uniform float uStripes;  // > 0: draw the edges of a stack of pages
uniform float uFlat;     // 1: ignore lighting (already-lit colours)
void main() {
  float d = roundRect(vUv, uSize, uRadius);
  float a = 1.0 - smoothstep(-0.0015, 0.0015, d);
  if (a <= 0.003) discard;
  vec3 n = normalize(vNormal);
  vec3 c = uColor * mix(lit(n), 1.0, uFlat);
  if (uStripes != 0.0) {
    // Negative stripe counts mean the stack fans out towards u = 0 (left pile).
    float o = uStripes > 0.0 ? vUv.x : 1.0 - vUv.x;
    float line = 0.5 + 0.5 * sin(o * abs(uStripes) * 6.2831853);
    c *= 0.90 + 0.10 * line;
    c *= 1.0 - 0.28 * o * o;
  }
  outColor = vec4(c, a);
}`;

/** The face of a cover board: art, rounded corners, joint groove, a moving sheen. */
const FS_BOARD = `${COMMON_FS}
uniform sampler2D uTex;
uniform vec2 uSize;
uniform float uRadius;
uniform float uFlipU;   // 1: mirror the art horizontally
uniform float uHinge;   // 0: the spine is at u = 0, 1: at u = 1
void main() {
  float d = roundRect(vUv, uSize, uRadius);
  float a = 1.0 - smoothstep(-0.0015, 0.0015, d);
  if (a <= 0.003) discard;
  vec3 n = normalize(vNormal);
  float u = mix(vUv.x, 1.0 - vUv.x, uFlipU);
  vec3 c = texture(uTex, vec2(u, 1.0 - vUv.y)).rgb;
  float hu = mix(vUv.x, 1.0 - vUv.x, uHinge);
  // The joint: a shallow groove where the board hinges on the spine.
  float joint = 1.0 - 0.16 * exp(-pow((hu - 0.035) / 0.012, 2.0));
  float spine = 1.0 - 0.22 * exp(-hu / 0.012);
  // Rounded edges catch less light.
  float bevel = 1.0 - 0.10 * (1.0 - smoothstep(-0.012, -0.002, d));
  vec3 r = reflect(-uLight, n);
  float sheen = pow(max(r.z, 0.0), 22.0) * 0.16;
  float grain = 1.0 + (hash(gl_FragCoord.xy) - 0.5) * 0.02;
  outColor = vec4(c * lit(n) * joint * spine * bevel * grain + sheen, a);
}`;

const FS_DESK = `#version 300 es
precision highp float;
out vec4 outColor;
uniform vec3 uCentre;
uniform vec3 uEdge;
uniform vec2 uRes;
float hash(vec2 p) {
  vec3 q = fract(vec3(p.xyx) * 0.1031);
  q += dot(q, q.yzx + 33.33);
  return fract((q.x + q.y) * q.z);
}
void main() {
  vec2 p = (gl_FragCoord.xy - 0.5 * uRes) / max(uRes.x, uRes.y);
  // Light pools from the upper left, like the lighting on the pages.
  float t = smoothstep(0.0, 0.62, length(p - vec2(-0.03, 0.03)));
  vec3 c = mix(uCentre, uEdge, t);
  c += (hash(gl_FragCoord.xy) - 0.5) * 0.012;
  outColor = vec4(c, 1.0);
}`;

const VS_FULL = `#version 300 es
out vec2 vUv;
void main() {
  vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
  vUv = p;
  gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}`;

/** Shadow map: project along the light onto the page plane. */
const VS_SHADOW = `#version 300 es
layout(location=0) in vec3 aPos;
uniform vec4 uShadowRect;
uniform vec2 uSlope;
out float vHeight;
void main() {
  vec2 p = aPos.xy + uSlope * aPos.z;
  vec2 ndc = (p - uShadowRect.xy) / uShadowRect.zw * 2.0 - 1.0;
  vHeight = aPos.z;
  gl_Position = vec4(ndc, 0.0, 1.0);
}`;

const FS_SHADOW = `#version 300 es
precision highp float;
in float vHeight;
out vec4 outColor;
void main() {
  // Higher sheets cast a lighter (more diffuse) shadow.
  float a = mix(0.95, 0.42, clamp(vHeight / 0.9, 0.0, 1.0));
  outColor = vec4(a, 0.0, 0.0, 1.0);
}`;

const FS_BLUR = `#version 300 es
precision highp float;
in vec2 vUv;
out vec4 outColor;
uniform sampler2D uTex;
uniform vec2 uStep;
void main() {
  float s = texture(uTex, vUv).r * 0.2270270270;
  s += (texture(uTex, vUv + uStep * 1.3846153846).r + texture(uTex, vUv - uStep * 1.3846153846).r) * 0.3162162162;
  s += (texture(uTex, vUv + uStep * 3.2307692308).r + texture(uTex, vUv - uStep * 3.2307692308).r) * 0.0702702703;
  outColor = vec4(s, 0.0, 0.0, 1.0);
}`;

/** Shadows received by the desk, and the soft shadow under the whole book. */
const FS_GROUND = `${COMMON_FS}
uniform sampler2D uShadow;
uniform vec4 uShadowRect;
uniform float uShadowOn;
uniform vec4 uBook;      // book rect on the desk: x0, y0, x1, y1
uniform float uStrength;
void main() {
  vec2 c = 0.5 * (uBook.xy + uBook.zw) + vec2(0.012, -0.016);
  vec2 half_ = 0.5 * (uBook.zw - uBook.xy);
  vec2 d = abs(vWorld.xy - c) - half_ + 0.03;
  float sd = length(max(d, 0.0)) + min(max(d.x, d.y), 0.0) - 0.03;
  float ambient = (1.0 - smoothstep(-0.02, 0.11, sd)) * 0.42 + (1.0 - smoothstep(-0.004, 0.016, sd)) * 0.30;
  float thrown = texture(uShadow, (vWorld.xy - uShadowRect.xy) / uShadowRect.zw).r * uShadowOn * 0.5;
  float a = clamp(max(ambient * step(0.0001, half_.x), thrown), 0.0, 1.0) * uStrength;
  outColor = vec4(0.0, 0.0, 0.0, a);
}`;

interface Program {
  program: WebGLProgram;
  uniforms: Map<string, WebGLUniformLocation | null>;
}

interface Surface {
  texture: WebGLTexture;
  width: number;
  height: number;
  /** Frame counter of the last use, for eviction. */
  used: number;
}

export class GlRenderer {
  readonly gl: WebGL2RenderingContext;
  private programs = new Map<string, Program>();
  private surfaces = new Map<SurfaceKey, Surface>();
  private placeholder: WebGLTexture;
  private vbo: WebGLBuffer;
  private leafIbo: WebGLBuffer;
  private boardIbo: WebGLBuffer;
  private quadIbo: WebGLBuffer;
  private vao: WebGLVertexArrayObject;
  private leafVerts = new Float32Array((MESH_COLS + 1) * (MESH_ROWS + 1) * VERTEX_SIZE);
  private boardVerts = new Float32Array(24 * VERTEX_SIZE);
  private quadVerts = new Float32Array(4 * VERTEX_SIZE);
  private shadowFbo: [WebGLFramebuffer, WebGLFramebuffer];
  private shadowTex: [WebGLTexture, WebGLTexture];
  private shadowSize: [number, number] = [384, 256];
  private view: SceneView = { width: 1, height: 1, pagePx: 1, aspect: 1.4, centerX: 0, centerY: 0, spread: true };
  private frame = 0;
  private anisotropy: { ext: EXT_texture_filter_anisotropic; max: number } | null = null;
  /** Byte budget for page textures; least recently used ones are evicted beyond it. */
  textureBudget = 220 * 1024 * 1024;
  /** Set when the context was lost and restored: every surface must be uploaded again. */
  onContextRestored: (() => void) | null = null;
  lost = false;

  /** Returns null when WebGL2 is not available (the caller falls back to Canvas 2D). */
  static create(canvas: HTMLCanvasElement): GlRenderer | null {
    const gl = canvas.getContext("webgl2", {
      antialias: true,
      alpha: false,
      depth: true,
      stencil: false,
      premultipliedAlpha: true,
      powerPreference: "default",
      preserveDrawingBuffer: false,
    });
    if (!gl) return null;
    try {
      return new GlRenderer(canvas, gl);
    } catch (e) {
      console.warn("WebGL2 initialisation failed, using the 2D renderer", e);
      return null;
    }
  }

  private constructor(
    readonly canvas: HTMLCanvasElement,
    gl: WebGL2RenderingContext,
  ) {
    this.gl = gl;
    this.placeholder = gl.createTexture()!;
    this.vbo = gl.createBuffer()!;
    this.leafIbo = gl.createBuffer()!;
    this.boardIbo = gl.createBuffer()!;
    this.quadIbo = gl.createBuffer()!;
    this.vao = gl.createVertexArray()!;
    this.shadowFbo = [gl.createFramebuffer()!, gl.createFramebuffer()!];
    this.shadowTex = [gl.createTexture()!, gl.createTexture()!];
    this.init();
    canvas.addEventListener("webglcontextlost", (e) => {
      e.preventDefault();
      this.lost = true;
    });
    canvas.addEventListener("webglcontextrestored", () => {
      this.lost = false;
      this.programs.clear();
      this.surfaces.clear();
      this.placeholder = gl.createTexture()!;
      this.vbo = gl.createBuffer()!;
      this.leafIbo = gl.createBuffer()!;
      this.boardIbo = gl.createBuffer()!;
      this.quadIbo = gl.createBuffer()!;
      this.vao = gl.createVertexArray()!;
      this.shadowFbo = [gl.createFramebuffer()!, gl.createFramebuffer()!];
      this.shadowTex = [gl.createTexture()!, gl.createTexture()!];
      this.init();
      this.onContextRestored?.();
    });
  }

  private init(): void {
    const gl = this.gl;
    const compile = (name: string, vs: string, fs: string) => {
      const make = (type: number, src: string) => {
        const s = gl.createShader(type)!;
        gl.shaderSource(s, src);
        gl.compileShader(s);
        if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) {
          throw new Error(`shader ${name}: ${gl.getShaderInfoLog(s) ?? "compile error"}`);
        }
        return s;
      };
      const p = gl.createProgram()!;
      gl.attachShader(p, make(gl.VERTEX_SHADER, vs));
      gl.attachShader(p, make(gl.FRAGMENT_SHADER, fs));
      gl.linkProgram(p);
      if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(`program ${name}: ${gl.getProgramInfoLog(p) ?? "link error"}`);
      this.programs.set(name, { program: p, uniforms: new Map() });
    };
    compile("page", VS, FS_PAGE);
    compile("leaf", VS, FS_LEAF);
    compile("solid", VS, FS_SOLID);
    compile("board", VS, FS_BOARD);
    compile("ground", VS, FS_GROUND);
    compile("desk", VS_FULL, FS_DESK);
    compile("shadow", VS_SHADOW, FS_SHADOW);
    compile("blur", VS_FULL, FS_BLUR);

    gl.bindVertexArray(this.vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.vbo);
    gl.bufferData(gl.ARRAY_BUFFER, this.leafVerts.byteLength, gl.DYNAMIC_DRAW);
    const stride = VERTEX_SIZE * 4;
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 3, gl.FLOAT, false, stride, 0);
    gl.enableVertexAttribArray(1);
    gl.vertexAttribPointer(1, 3, gl.FLOAT, false, stride, 12);
    gl.enableVertexAttribArray(2);
    gl.vertexAttribPointer(2, 2, gl.FLOAT, false, stride, 24);
    gl.bindVertexArray(null);

    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, this.leafIbo);
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, leafIndices(), gl.STATIC_DRAW);
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, this.boardIbo);
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, boardIndices(), gl.STATIC_DRAW);
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, this.quadIbo);
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, new Uint16Array([0, 1, 2, 0, 2, 3]), gl.STATIC_DRAW);

    gl.bindTexture(gl.TEXTURE_2D, this.placeholder);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, 1, 1, 0, gl.RGBA, gl.UNSIGNED_BYTE, new Uint8Array([128, 128, 128, 255]));

    for (let i = 0; i < 2; i++) {
      gl.bindTexture(gl.TEXTURE_2D, this.shadowTex[i]!);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.R8, this.shadowSize[0], this.shadowSize[1], 0, gl.RED, gl.UNSIGNED_BYTE, null);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      gl.bindFramebuffer(gl.FRAMEBUFFER, this.shadowFbo[i]!);
      gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, this.shadowTex[i]!, 0);
    }
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);

    const ext = gl.getExtension("EXT_texture_filter_anisotropic");
    this.anisotropy = ext ? { ext, max: Math.min(4, gl.getParameter(ext.MAX_TEXTURE_MAX_ANISOTROPY_EXT) as number) } : null;
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
  }

  /** Largest texture side the GPU accepts. */
  get maxTextureSize(): number {
    return this.gl.getParameter(this.gl.MAX_TEXTURE_SIZE) as number;
  }

  resize(view: SceneView): void {
    this.view = view;
    if (this.canvas.width !== view.width) this.canvas.width = view.width;
    if (this.canvas.height !== view.height) this.canvas.height = view.height;
  }

  // --- surfaces ---------------------------------------------------------------

  hasSurface(key: SurfaceKey): boolean {
    return this.surfaces.has(key);
  }

  /** Upload (or replace) the image of a surface. */
  setSurface(key: SurfaceKey, source: TexImageSource, width: number, height: number): void {
    if (this.lost) return;
    const gl = this.gl;
    let s = this.surfaces.get(key);
    if (!s) {
      s = { texture: gl.createTexture()!, width, height, used: this.frame };
      this.surfaces.set(key, s);
    }
    s.width = width;
    s.height = height;
    s.used = this.frame;
    gl.bindTexture(gl.TEXTURE_2D, s.texture);
    gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, false);
    gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, false);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, source);
    // Mipmaps keep text calm while a page is foreshortened in mid-turn.
    gl.generateMipmap(gl.TEXTURE_2D);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    if (this.anisotropy) gl.texParameterf(gl.TEXTURE_2D, this.anisotropy.ext.TEXTURE_MAX_ANISOTROPY_EXT, this.anisotropy.max);
    this.evict();
  }

  deleteSurface(key: SurfaceKey): void {
    const s = this.surfaces.get(key);
    if (!s) return;
    this.gl.deleteTexture(s.texture);
    this.surfaces.delete(key);
  }

  /** Drop every surface whose key matches (after a re-layout or theme change). */
  deleteSurfaces(match: (key: SurfaceKey) => boolean): void {
    for (const key of [...this.surfaces.keys()]) if (match(key)) this.deleteSurface(key);
  }

  /** Approximate GPU memory held by page textures, in bytes (mip chain included). */
  get textureBytes(): number {
    let total = 0;
    for (const s of this.surfaces.values()) total += s.width * s.height * 4 * 1.34;
    return total;
  }

  private evict(): void {
    if (this.textureBytes <= this.textureBudget) return;
    const byAge = [...this.surfaces.entries()].sort((a, b) => a[1].used - b[1].used);
    for (const [key, s] of byAge) {
      if (this.textureBytes <= this.textureBudget * 0.8) break;
      if (s.used >= this.frame - 1) break; // in use right now
      this.deleteSurface(key);
    }
  }

  private texture(key: SurfaceKey | null): WebGLTexture {
    if (!key) return this.placeholder;
    const s = this.surfaces.get(key);
    if (!s) return this.placeholder;
    s.used = this.frame;
    return s.texture;
  }

  // --- drawing helpers --------------------------------------------------------

  private use(name: string): Program {
    const p = this.programs.get(name)!;
    this.gl.useProgram(p.program);
    return p;
  }

  private loc(p: Program, name: string): WebGLUniformLocation | null {
    let l = p.uniforms.get(name);
    if (l === undefined) {
      l = this.gl.getUniformLocation(p.program, name);
      p.uniforms.set(name, l);
    }
    return l;
  }

  private camera(p: Program, pose: ScenePose): void {
    const gl = this.gl;
    const v = this.view;
    const scale = v.pagePx * pose.zoom;
    gl.uniform4f(this.loc(p, "uView"), (2 * v.centerX) / v.width - 1, 1 - (2 * v.centerY) / v.height, (2 * scale) / v.width, (2 * scale) / v.height);
    gl.uniform2f(this.loc(p, "uCam"), pose.camX, MOTION.cameraDistance);
    gl.uniform3f(this.loc(p, "uLight"), LIGHT[0], LIGHT[1], LIGHT[2]);
  }

  private bindTex(p: Program, name: string, unit: number, tex: WebGLTexture): void {
    const gl = this.gl;
    gl.activeTexture(gl.TEXTURE0 + unit);
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.uniform1i(this.loc(p, name), unit);
  }

  /** Upload a flat quad on the plane z = `z` and draw it with the bound program. */
  private quad(x0: number, y0: number, x1: number, y1: number, z = 0): void {
    const gl = this.gl;
    const q = this.quadVerts;
    const put = (i: number, x: number, y: number, u: number, v: number) => {
      const o = i * VERTEX_SIZE;
      q[o] = x;
      q[o + 1] = y;
      q[o + 2] = z;
      q[o + 3] = 0;
      q[o + 4] = 0;
      q[o + 5] = 1;
      q[o + 6] = u;
      q[o + 7] = v;
    };
    put(0, x0, y0, 0, 0);
    put(1, x1, y0, 1, 0);
    put(2, x1, y1, 1, 1);
    put(3, x0, y1, 0, 1);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.vbo);
    gl.bufferSubData(gl.ARRAY_BUFFER, 0, q);
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, this.quadIbo);
    gl.drawElements(gl.TRIANGLES, 6, gl.UNSIGNED_SHORT, 0);
  }

  private uploadLeaf(f: FlyerPose, lift: number): void {
    const gl = this.gl;
    buildLeaf(f, this.view.aspect, lift, this.leafVerts);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.vbo);
    gl.bufferSubData(gl.ARRAY_BUFFER, 0, this.leafVerts);
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, this.leafIbo);
  }

  private uploadBoard(f: FlyerPose, lift: number): void {
    const gl = this.gl;
    const a = this.view.aspect;
    buildBoard(f.p, 1 + BOARD_OVERHANG, a + 2 * BOARD_OVERHANG, BOARD_THICK, lift, this.boardVerts);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.vbo);
    gl.bufferSubData(gl.ARRAY_BUFFER, 0, this.boardVerts);
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, this.boardIbo);
  }

  // --- frame ------------------------------------------------------------------

  render(pose: ScenePose, style: SceneStyle): void {
    if (this.lost) return;
    const gl = this.gl;
    const v = this.view;
    const a = v.aspect;
    const h2 = a / 2;
    this.frame++;
    gl.bindVertexArray(this.vao);

    // World rectangle covered by the shadow map: the whole spread plus room for thrown shadows.
    const shadowRect: [number, number, number, number] = [-1.45, -h2 - 0.4, 2.9, a + 0.8];
    const airborne = pose.flyers.length > 0;

    // ---- pass 1: shadow map ---------------------------------------------------
    if (airborne) {
      gl.bindFramebuffer(gl.FRAMEBUFFER, this.shadowFbo[0]!);
      gl.viewport(0, 0, this.shadowSize[0], this.shadowSize[1]);
      gl.disable(gl.DEPTH_TEST);
      gl.disable(gl.CULL_FACE);
      gl.clearColor(0, 0, 0, 1);
      gl.clear(gl.COLOR_BUFFER_BIT);
      // MAX blending: overlapping leaves do not darken the shadow twice.
      gl.enable(gl.BLEND);
      gl.blendEquation(gl.MAX);
      gl.blendFunc(gl.ONE, gl.ONE);
      const sp = this.use("shadow");
      gl.uniform4f(this.loc(sp, "uShadowRect"), ...shadowRect);
      gl.uniform2f(this.loc(sp, "uSlope"), SHADOW_SLOPE[0], SHADOW_SLOPE[1]);
      for (const f of pose.flyers) {
        if (f.board) {
          this.uploadBoard(f, 0);
          gl.drawElements(gl.TRIANGLES, 36, gl.UNSIGNED_SHORT, 0);
        } else {
          this.uploadLeaf(f, 0);
          gl.drawElements(gl.TRIANGLES, MESH_COLS * MESH_ROWS * 6, gl.UNSIGNED_SHORT, 0);
        }
      }
      gl.blendEquation(gl.FUNC_ADD);
      gl.disable(gl.BLEND);
      // Separable blur, two rounds for a wide soft penumbra.
      const bp = this.use("blur");
      for (let round = 0; round < 2; round++) {
        for (let pass = 0; pass < 2; pass++) {
          gl.bindFramebuffer(gl.FRAMEBUFFER, this.shadowFbo[1 - pass]!);
          this.bindTex(bp, "uTex", 0, this.shadowTex[pass]!);
          const spread = 1 + round * 1.5;
          gl.uniform2f(this.loc(bp, "uStep"), pass === 0 ? spread / this.shadowSize[0] : 0, pass === 1 ? spread / this.shadowSize[1] : 0);
          gl.drawArrays(gl.TRIANGLES, 0, 3);
        }
      }
    }

    // ---- pass 2: scene --------------------------------------------------------
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    gl.viewport(0, 0, v.width, v.height);
    gl.disable(gl.DEPTH_TEST);
    gl.depthMask(false);
    gl.disable(gl.BLEND);

    const dp = this.use("desk");
    gl.uniform3f(this.loc(dp, "uCentre"), ...style.desk);
    gl.uniform3f(this.loc(dp, "uEdge"), ...style.deskEdge);
    gl.uniform2f(this.loc(dp, "uRes"), v.width, v.height);
    gl.drawArrays(gl.TRIANGLES, 0, 3);

    gl.enable(gl.BLEND);
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);

    const ov = BOARD_OVERHANG;
    const leftPresent = pose.left !== null || pose.leftBase;
    const rightPresent = pose.right !== null || pose.rightBase;

    // Shadows on the desk: under the book, and those thrown by sheets in the air.
    const gp = this.use("ground");
    this.camera(gp, pose);
    this.bindTex(gp, "uShadow", 0, this.shadowTex[0]!);
    gl.uniform4f(this.loc(gp, "uShadowRect"), ...shadowRect);
    gl.uniform1f(this.loc(gp, "uShadowOn"), airborne ? 1 : 0);
    const bx0 = leftPresent ? -1 - ov : 0;
    const bx1 = rightPresent ? 1 + ov : 0;
    gl.uniform4f(this.loc(gp, "uBook"), bx0, -h2 - ov, bx1, h2 + ov);
    gl.uniform1f(this.loc(gp, "uStrength"), style.dark ? 0.85 : 0.62);
    this.quad(-3.2, -h2 - 1.2, 3.2, h2 + 1.2);

    // The opened boards lying under the page block.
    const sol = this.use("solid");
    this.camera(sol, pose);
    gl.uniform1f(this.loc(sol, "uStripes"), 0);
    gl.uniform1f(this.loc(sol, "uFlat"), 0);
    const cloth = style.cloth;
    const baseColor = (k: number): [number, number, number] => [cloth[0] * k, cloth[1] * k, cloth[2] * k];
    const drawSolid = (x0: number, y0: number, x1: number, y1: number, color: [number, number, number], radius: number, stripes = 0) => {
      gl.uniform3f(this.loc(sol, "uColor"), ...color);
      gl.uniform2f(this.loc(sol, "uSize"), Math.abs(x1 - x0), Math.abs(y1 - y0));
      gl.uniform1f(this.loc(sol, "uRadius"), radius);
      gl.uniform1f(this.loc(sol, "uStripes"), stripes);
      this.quad(x0, y0, x1, y1);
    };
    if (pose.leftBase) drawSolid(-1 - ov, -h2 - ov, 0, h2 + ov, baseColor(0.86), 0.012);
    if (pose.rightBase) drawSolid(0, -h2 - ov, 1 + ov, h2 + ov, baseColor(0.86), 0.012);
    if (pose.leftBase && pose.rightBase) drawSolid(-0.014, -h2 - ov, 0.014, h2 + ov, baseColor(0.62), 0);

    // Edges of the page stacks: the fore-edge fans out as leaves accumulate.
    const edge: [number, number, number] = style.dark
      ? [style.paper[0] * 1.5 + 0.04, style.paper[1] * 1.5 + 0.04, style.paper[2] * 1.5 + 0.04]
      : [style.paper[0] * 0.9, style.paper[1] * 0.89, style.paper[2] * 0.86];
    const stack = (fraction: number) => 0.004 + STACK_MAX * Math.min(1, fraction);
    if (pose.rightBase && pose.right !== null && !pose.rightIsBoard && pose.stackRight > 0) {
      const w = Math.min(stack(pose.stackRight), ov - 0.003);
      drawSolid(1, -h2 + 0.002, 1 + w, h2 - 0.002, edge, 0, Math.max(3, Math.round(w * 900)));
    }
    if (pose.leftBase && pose.left !== null && !pose.leftIsBoard && pose.stackLeft > 0) {
      const w = Math.min(stack(pose.stackLeft), ov - 0.003);
      drawSolid(-1 - w, -h2 + 0.002, -1, h2 - 0.002, edge, 0, -Math.max(3, Math.round(w * 900)));
    }

    // The two resting surfaces.
    const drawResting = (key: SurfaceKey | null, side: -1 | 1, isBoard: boolean) => {
      if (key === null) return;
      if (isBoard) {
        const bp = this.use("board");
        this.camera(bp, pose);
        this.bindTex(bp, "uTex", 0, this.texture(key));
        gl.uniform2f(this.loc(bp, "uSize"), 1 + ov, a + 2 * ov);
        gl.uniform1f(this.loc(bp, "uRadius"), 0.014);
        gl.uniform1f(this.loc(bp, "uFlipU"), 0);
        // The front cover rests on the right with its spine at the left edge;
        // the back cover rests on the left with its spine at the right edge.
        gl.uniform1f(this.loc(bp, "uHinge"), side > 0 ? 0 : 1);
        if (side > 0) this.quad(0, -h2 - ov, 1 + ov, h2 + ov, 0.004);
        else this.quad(-1 - ov, -h2 - ov, 0, h2 + ov, 0.004);
        return;
      }
      const pp = this.use("page");
      this.camera(pp, pose);
      this.bindTex(pp, "uTex", 0, this.texture(key));
      this.bindTex(pp, "uShadow", 1, this.shadowTex[0]!);
      gl.uniform4f(this.loc(pp, "uShadowRect"), ...shadowRect);
      gl.uniform1f(this.loc(pp, "uShadowOn"), airborne ? 1 : 0);
      gl.uniform1f(this.loc(pp, "uGutter"), pose.openness * (v.spread ? 1 : 0.75));
      if (side > 0) this.quad(0, -h2, 1, h2);
      else this.quad(-1, -h2, 0, h2);
    };
    drawResting(pose.left, -1, pose.leftIsBoard);
    drawResting(pose.right, 1, pose.rightIsBoard);

    // Sheets in the air, depth-tested against each other.
    if (airborne) {
      gl.enable(gl.DEPTH_TEST);
      gl.depthFunc(gl.LEQUAL);
      gl.depthMask(true);
      gl.clearDepth(1);
      gl.clear(gl.DEPTH_BUFFER_BIT);
      pose.flyers.forEach((f, i) => {
        // Leaves lower in the right-hand pile sit fractionally lower so they never z-fight.
        const lift = 0.0025 + (pose.flyers.length - i) * 0.0006;
        if (f.board) {
          // Slab quad 0 is the face that looks up while the board rests on the
          // right (the sheet's front), quad 1 the opposite face (its back).
          const coverIsTop = f.front === "front-cover";
          const coverKey = coverIsTop ? f.front : f.back;
          const innerKey = coverIsTop ? f.back : f.front;
          this.uploadBoard(f, lift);
          const bp = this.use("board");
          this.camera(bp, pose);
          this.bindTex(bp, "uTex", 0, this.texture(coverKey));
          gl.uniform2f(this.loc(bp, "uSize"), 1 + ov, a + 2 * ov);
          gl.uniform1f(this.loc(bp, "uRadius"), 0.014);
          gl.uniform1f(this.loc(bp, "uHinge"), 0);
          // Seen from outside, the back cover has its spine on the right.
          gl.uniform1f(this.loc(bp, "uFlipU"), coverIsTop ? 0 : 1);
          gl.drawElements(gl.TRIANGLES, 6, gl.UNSIGNED_SHORT, (coverIsTop ? 0 : 6) * 2);
          // The turned-in cloth of the inner face, and the four edges.
          const sp = this.use("solid");
          this.camera(sp, pose);
          gl.uniform1f(this.loc(sp, "uStripes"), 0);
          gl.uniform1f(this.loc(sp, "uFlat"), 0);
          gl.uniform1f(this.loc(sp, "uRadius"), 0.014);
          gl.uniform2f(this.loc(sp, "uSize"), 1 + ov, a + 2 * ov);
          gl.uniform3f(this.loc(sp, "uColor"), ...baseColor(0.86));
          gl.drawElements(gl.TRIANGLES, 6, gl.UNSIGNED_SHORT, (coverIsTop ? 6 : 0) * 2);
          gl.uniform1f(this.loc(sp, "uRadius"), 0);
          gl.uniform2f(this.loc(sp, "uSize"), 1, 1);
          gl.uniform3f(this.loc(sp, "uColor"), ...baseColor(0.66));
          gl.drawElements(gl.TRIANGLES, 24, gl.UNSIGNED_SHORT, 12 * 2);
          // The endpaper pasted on the inner face, page-sized.
          const lp = this.use("leaf");
          this.camera(lp, pose);
          this.bindTex(lp, "uFront", 0, this.texture(innerKey));
          this.bindTex(lp, "uBack", 1, this.texture(innerKey));
          gl.uniform1f(this.loc(lp, "uMirror"), 0);
          gl.uniform3f(this.loc(lp, "uPaper"), ...style.paper);
          buildPaste(f.p, a, coverIsTop ? -0.0012 : BOARD_THICK + 0.0012, lift, this.quadVerts);
          gl.bindBuffer(gl.ARRAY_BUFFER, this.vbo);
          gl.bufferSubData(gl.ARRAY_BUFFER, 0, this.quadVerts);
          gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, this.quadIbo);
          gl.drawElements(gl.TRIANGLES, 6, gl.UNSIGNED_SHORT, 0);
        } else {
          const lp = this.use("leaf");
          this.camera(lp, pose);
          this.bindTex(lp, "uFront", 0, this.texture(f.front));
          this.bindTex(lp, "uBack", 1, this.texture(f.back));
          gl.uniform1f(this.loc(lp, "uMirror"), f.mirrorBack ? 1 : 0);
          gl.uniform3f(this.loc(lp, "uPaper"), ...style.paper);
          this.uploadLeaf(f, lift);
          gl.drawElements(gl.TRIANGLES, MESH_COLS * MESH_ROWS * 6, gl.UNSIGNED_SHORT, 0);
        }
      });
      gl.disable(gl.DEPTH_TEST);
      gl.depthMask(false);
    }
    gl.bindVertexArray(null);
  }

  dispose(): void {
    const gl = this.gl;
    for (const s of this.surfaces.values()) gl.deleteTexture(s.texture);
    this.surfaces.clear();
    for (const p of this.programs.values()) gl.deleteProgram(p.program);
    this.programs.clear();
  }
}
