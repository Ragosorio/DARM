/**
 * TypeScript mirror of the DARM IR (crates/darm-ir, IR version 1.x).
 *
 * The reader never sees DARM source: it receives this JSON from the package,
 * after the WASM core has validated it. Keep this file in sync with
 * `schemas/darm-ir.schema.json` by hand when the IR changes: nothing checks
 * it automatically (the WASM validation is what protects the reader).
 */

export type Color = string; // "#rrggbb"

export interface Book {
  ir_version: string;
  grammar_version: number;
  meta: Meta;
  theme: Theme;
  resources: Resource[];
  state: StateVar[];
  cover: Covers;
  chapters: Chapter[];
  toc: TocEntry[];
  anchors: Record<string, Anchor>;
}

export interface Meta {
  id: string;
  title: string;
  subtitle?: string;
  authors: string[];
  language: string;
  edition?: string;
  version?: string;
  publisher?: string;
  published?: string;
  description?: string;
  rights?: string;
  credits?: Credit[];
  layout: "reflow" | "fixed";
  page_aspect: number;
  offline: boolean;
  network?: string[];
  generator: string;
}

export interface Credit {
  role: string;
  name: string;
}

export interface Theme {
  paper: Color;
  ink: Color;
  accent: Color;
  dark?: Palette;
  body_font: FontRef;
  heading_font: FontRef;
  paragraph: "indent" | "spaced";
  justify: boolean;
  hyphenate: boolean;
  drop_caps: boolean;
}

export interface Palette {
  paper: Color;
  ink: Color;
  accent: Color;
}

export type BuiltinFont = "serif" | "sans" | "mono";
export type FontRef = { source: "builtin"; family: BuiltinFont } | { source: "resource"; name: string };

export type ResourceKind = "image" | "video" | "audio" | "captions" | "font";

export interface Resource {
  name: string;
  kind: ResourceKind;
  source: { location: "packaged"; entry: string } | { location: "remote"; url: string };
  media_type: string;
  size: number;
  sha256: string;
  width?: number;
  height?: number;
  duration_ms?: number;
  alt?: string;
  title?: string;
  poster?: string;
  captions?: CaptionTrack[];
  transcript?: string;
}

export interface CaptionTrack {
  resource: string;
  lang: string;
  label: string;
}

export type StateVar =
  | { name: string; type: "int"; initial: number; min: number; max: number }
  | { name: string; type: "bool"; initial: boolean }
  | { name: string; type: "enum"; initial: string; values: string[] };

export interface Covers {
  front: FrontCover;
  back: BackCover;
}

export type CoverStyle = "classic" | "band" | "minimal" | "modern";

export interface FrontCover {
  style: CoverStyle;
  color: Color;
  ink: Color;
  accent: Color;
  image?: string;
  full_bleed?: boolean;
  show_text: boolean;
  title: string;
  subtitle?: string;
  author: string;
  generated?: boolean;
}

export interface BackCover {
  color: Color;
  ink: Color;
  accent: Color;
  blurb?: Span[][];
  tagline?: string;
  image?: string;
  generated?: boolean;
}

export type ChapterKind = "title" | "colophon" | "contents" | "chapter";

export interface Chapter {
  id: string;
  kind: ChapterKind;
  title: string;
  number?: number;
  opener?: boolean;
  when?: Expr;
  blocks?: Block[];
}

export interface TocEntry {
  title: string;
  target: string;
  level: 1 | 2;
  number?: number;
  when?: Expr;
}

export interface Anchor {
  chapter: string;
  block?: string;
}

export type Align = "left" | "center" | "right" | "justify";
export type TextStyle = "body" | "lead" | "small" | "caption" | "dropcap" | "verse" | "mono" | "display";
export type AnimKind = "fade" | "rise" | "drop" | "slide_left" | "slide_right" | "zoom";
export type Transition = "flip" | "fade" | "none";

export interface Anim {
  kind: AnimKind;
  delay_ms: number;
  duration_ms: number;
}

export interface Frame {
  x: number;
  y: number;
  w: number;
  h: number;
}

interface BlockBase {
  id: string;
  align?: Align;
  anim?: Anim;
  frame?: Frame;
  color?: Color;
  scale?: number;
}

export type Block = BlockBase &
  (
    | { type: "paragraph"; spans: Span[]; style: TextStyle }
    | { type: "heading"; spans: Span[]; level: 2 | 3 }
    | { type: "quote"; paragraphs: Span[][]; attribution?: Span[] }
    | { type: "list"; ordered: boolean; items: Span[][] }
    | { type: "image"; resource: string; caption?: Span[]; width: number; fit: "contain" | "cover" }
    | { type: "gallery"; resources: string[]; caption?: Span[] }
    | { type: "video"; resource: string; caption?: Span[]; autoplay?: boolean; loop?: boolean; muted?: boolean }
    | { type: "audio"; resource: string; caption?: Span[] }
    | { type: "divider"; style: "rule" | "ornament" | "blank" }
    | { type: "space"; lines: number }
    | { type: "page_break" }
    | { type: "box"; style: "rule" | "tint" | "bar"; title?: string; blocks: Block[] }
    | { type: "button"; label: string; actions: Action[] }
    | { type: "choice"; prompt: Span[]; options: ChoiceOption[] }
    | { type: "quiz"; question: Span[]; answers: QuizAnswer[]; explanation?: Span[]; on_correct?: Action[] }
    | { type: "group"; when?: Expr; blocks: Block[] }
    | { type: "page"; background?: Color; blocks: Block[] }
  );

export type BlockOf<T extends Block["type"]> = Extract<Block, { type: T }>;

export interface Span {
  text?: string;
  bold?: boolean;
  italic?: boolean;
  code?: boolean;
  link?: Link;
  var?: string;
  break?: boolean;
}

export type Link = { kind: "anchor"; target: string } | { kind: "url"; href: string };

export interface ChoiceOption {
  label: string;
  when?: Expr;
  actions: Action[];
}

export interface QuizAnswer {
  label: string;
  correct?: boolean;
}

export type Action =
  | { do: "set"; var: string; value: Expr }
  | { do: "goto"; target: string; transition?: Transition }
  | { do: "reset"; var?: string };

export type BinOp = "add" | "sub" | "mul" | "eq" | "ne" | "lt" | "le" | "gt" | "ge" | "and" | "or";

export type Expr =
  | { op: "int"; value: number }
  | { op: "bool"; value: boolean }
  | { op: "str"; value: string }
  | { op: "var"; name: string }
  | { op: "not"; expr: Expr }
  | { op: "neg"; expr: Expr }
  | { op: "bin"; kind: BinOp; left: Expr; right: Expr };

export type Value = number | boolean | string;
export type State = Record<string, Value>;

export const RESERVED_TARGETS = ["cover", "start", "contents", "end"] as const;
