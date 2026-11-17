#!/usr/bin/env python3
"""Mechanical lint for the DARM example books (standard library only).

It does not replace the compiler. It checks the things that can be verified
without one, against docs/language.md (grammar version 1):

  * every .darml file is UTF-8, starts with `darm 1`, has no tabs in its
    indentation and indents logical lines in steps of 4 spaces;
  * `include` paths exist, stay inside the project and do not form a cycle;
    exactly one `book` with `author` and `language`;
  * every asset path exists, stays inside the project and has the declared
    kind of content (sniffed from the bytes, not the extension); `alt` is
    present on images and videos; posters and captions exist; WebVTT parses;
  * SVG files are well-formed XML, have a viewBox and contain no scripts,
    event handlers, foreignObject, external references or @import;
  * every `id=` is unique and not a reserved target; every `goto`, `goto=`
    and `[label](target)` points at an existing id or a reserved target;
  * every asset used by a block or a cover is declared (unused ones are
    reported as warnings); every `{name}` and every assigned or tested
    variable is a declared state variable (or a component parameter);
  * inline markup is balanced; choices, quizzes and galleries respect the
    documented limits; `at=`/`size=` only appear inside `page`.

With ffprobe on the PATH it also reports codec facts for audio and video and
checks them against size limits. With `--darm PATH` it additionally runs
`darm check` on each project.

Usage:
    python3 examples/tools/lint_examples.py            # lint everything under examples/
    python3 examples/tools/lint_examples.py --stats    # also print word counts and media facts
    python3 examples/tools/lint_examples.py PATH ...   # lint specific entry files or project dirs
Exit status: 0 when there are no errors (warnings do not fail), 1 otherwise.
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import xml.etree.ElementTree as ET

RESERVED_TARGETS = {"cover", "contents", "start", "end"}
KEYWORDS = {"true", "false", "and", "or", "not", "in", "if", "elif", "else"}
ASSET_KINDS = {"image", "video", "audio", "font"}
ACTION_PARENTS = {"button", "option", "on_correct"}
BUILTIN_VALUES = {
    "serif", "sans", "mono", "reflow", "fixed", "smart", "plain", "indent", "spaced",
    "title", "colophon", "contents", "classic", "band", "minimal", "modern",
}
PLACEHOLDER = re.compile(r"⟦(\d+)⟧")          # ⟦n⟧ stands for string literal n
NAME = r"[A-Za-z_][A-Za-z0-9_]*"


class Report:
    def __init__(self):
        self.errors, self.warnings = [], []

    def error(self, where, msg):
        self.errors.append(f"{where}: {msg}")

    def warn(self, where, msg):
        self.warnings.append(f"{where}: {msg}")


# --------------------------------------------------------------------------- lexing

class Line:
    """One logical line: indentation, code with strings replaced by ⟦n⟧, the strings."""
    __slots__ = ("file", "no", "indent", "code", "strings", "parents")

    def __init__(self, file, no, indent, code, strings):
        self.file, self.no, self.indent, self.code, self.strings = file, no, indent, code, strings
        self.parents = []

    @property
    def where(self):
        return f"{self.file}:{self.no}"

    @property
    def head(self):
        m = re.match(NAME, self.code)
        return m.group(0) if m else ""

    def text(self, code=None):
        """Code with the placeholders expanded back to quoted strings (for messages)."""
        return PLACEHOLDER.sub(lambda m: '"' + self.strings[int(m.group(1))][:30] + '"', code or self.code)


def dedent_block(raw):
    lines = raw.split("\n")
    if lines and not lines[0].strip():
        lines = lines[1:]
    if lines and not lines[-1].strip():
        lines = lines[:-1]
    indents = [len(l) - len(l.lstrip(" ")) for l in lines if l.strip()]
    cut = min(indents) if indents else 0
    return "\n".join(l[cut:] if l.strip() else "" for l in lines)


def lex(path, rel, rep):
    """Split a .darml file into logical lines. Returns (lines, ok)."""
    try:
        src = open(path, "rb").read().decode("utf-8")
    except UnicodeDecodeError as e:
        rep.error(rel, f"not valid UTF-8 ({e})")
        return []
    if src.startswith("﻿"):
        src = src[1:]
    src = src.replace("\r\n", "\n")
    first = src.split("\n", 1)[0]
    if not re.fullmatch(r"darm 1\s*", first):
        rep.error(f"{rel}:1", "file must start with the pragma `darm 1`")

    out, i, n, lineno = [], 0, len(src), 1
    code, strings, start_no, indent, depth = [], [], None, 0, 0
    at_line_start = True

    def flush():
        nonlocal code, strings, start_no
        text = "".join(code).strip()
        if text:
            out.append(Line(rel, start_no, indent, text, strings))
        code, strings, start_no = [], [], None

    while i < n:
        c = src[i]
        if at_line_start:
            j = i
            while j < n and src[j] in " \t":
                j += 1
            ws = src[i:j]
            if "\t" in ws:
                rep.error(f"{rel}:{lineno}", "tab in indentation (E0102): use spaces only")
            blank = j >= n or src[j] in "\n#"
            if not blank and start_no is None:
                indent, start_no = len(ws), lineno
                if len(ws) % 4:
                    rep.error(f"{rel}:{lineno}", f"indentation of {len(ws)} spaces is not a multiple of 4")
            i, at_line_start = j, False
            continue
        if c == "\n":
            joined = depth > 0 or "".join(code).rstrip().endswith(",")
            if joined:
                code.append(" ")
            else:
                flush()
            lineno += 1
            i += 1
            at_line_start = True
            continue
        if c == "#":
            while i < n and src[i] != "\n":
                i += 1
            continue
        if src.startswith('"""', i):
            end = src.find('"""', i + 3)
            if end < 0:
                rep.error(f"{rel}:{lineno}", 'unterminated """ text block')
                break
            raw = src[i + 3:end]
            for k, physical in enumerate(raw.split("\n")[1:], 1):
                if "\t" in physical[:len(physical) - len(physical.lstrip())]:
                    rep.error(f"{rel}:{lineno + k}", "tab in the indentation of a text block")
            code.append(f"⟦{len(strings)}⟧")
            strings.append(dedent_block(raw))
            lineno += raw.count("\n")
            i = end + 3
            continue
        if c == '"':
            j, buf = i + 1, []
            while j < n and src[j] != '"':
                if src[j] == "\n":
                    rep.error(f"{rel}:{lineno}", "unterminated string literal")
                    break
                if src[j] == "\\" and j + 1 < n:
                    nxt = src[j + 1]
                    if nxt == "n":
                        buf.append("\n")
                    elif nxt == "t":
                        buf.append("\t")
                    elif nxt == '"':
                        buf.append('"')
                    elif nxt == "u" and src.startswith("{", j + 2):
                        close = src.find("}", j)
                        try:
                            buf.append(chr(int(src[j + 3:close], 16)))
                        except ValueError:
                            rep.error(f"{rel}:{lineno}", "bad \\u{...} escape")
                        j = close - 1
                    else:                       # \\ and the rich-text escapes stay as written
                        buf.append("\\" + nxt)
                    j += 2
                    continue
                buf.append(src[j])
                j += 1
            value = "".join(buf)
            if len(value.encode("utf-8")) > 8192:
                rep.error(f"{rel}:{lineno}", "plain string longer than 8 KiB")
            code.append(f"⟦{len(strings)}⟧")
            strings.append(value)
            i = j + 1
            continue
        if c in "([":
            depth += 1
        elif c in ")]":
            depth -= 1
        code.append(c)
        i += 1
    flush()
    if depth:
        rep.error(rel, "unbalanced brackets")
    return out


def split_args(code):
    """Split `a, b=(1, 2), c` at top-level commas."""
    parts, depth, cur = [], 0, []
    for ch in code:
        if ch in "([":
            depth += 1
        elif ch in ")]":
            depth -= 1
        if ch == "," and depth == 0:
            parts.append("".join(cur).strip())
            cur = []
        else:
            cur.append(ch)
    tail = "".join(cur).strip()
    if tail:
        parts.append(tail)
    return parts


def parse_element(line):
    """Return (name, positional[], named{}, has_block) for an element line."""
    code = line.code
    has_block = code.endswith(":")
    if has_block:
        code = code[:-1]
    m = re.match(NAME, code)
    name = m.group(0)
    positional, named = [], {}
    for part in split_args(code[m.end():].strip()):
        km = re.match(rf"({NAME})\s*=(?!=)\s*(.*)$", part, re.S)
        if km:
            named[km.group(1)] = km.group(2).strip()
        else:
            positional.append(part)
    return name, positional, named, has_block


def string_of(line, token):
    m = PLACEHOLDER.fullmatch(token.strip()) if token else None
    return line.strings[int(m.group(1))] if m else None


# --------------------------------------------------------------------------- project loading

def load_project(entry, root, base, rep):
    """Follow includes from the entry file; return all logical lines in reading order."""
    lines, stack, seen = [], [], set()

    def visit(path):
        real = os.path.realpath(path)
        rel = os.path.relpath(path, base)
        if real in stack:
            rep.error(rel, "include cycle")
            return
        if os.path.commonpath([real, os.path.realpath(root)]) != os.path.realpath(root):
            rep.error(rel, "include escapes the project directory")
            return
        if not os.path.isfile(path):
            rep.error(rel, "included file does not exist")
            return
        if len(stack) >= 16:
            rep.error(rel, "include depth over 16")
            return
        seen.add(real)
        stack.append(real)
        for line in lex(path, rel, rep):
            if line.no == 1 and line.head == "darm":        # the pragma, checked by lex()
                continue
            if line.indent == 0 and line.head == "include":
                target = string_of(line, line.code[len("include"):])
                if target is None:
                    rep.error(line.where, "include needs a string path")
                else:
                    visit(os.path.join(os.path.dirname(path), target))
            else:
                line.file = rel
                lines.append((line, os.path.dirname(path)))
        stack.pop()

    visit(entry)
    return lines, seen


def attach_parents(lines):
    stack = []                                      # (indent, head, line)
    for line, _ in lines:
        while stack and stack[-1][0] >= line.indent:
            stack.pop()
        line.parents = [(h, l) for _, h, l in stack]
        stack.append((line.indent, line.head, line))


# --------------------------------------------------------------------------- content sniffing

def sniff(path):
    with open(path, "rb") as f:
        head = f.read(4096)
    if head.startswith(b"\x89PNG\r\n\x1a\n"):
        return "png"
    if head.startswith(b"\xff\xd8\xff"):
        return "jpeg"
    if head[:6] in (b"GIF87a", b"GIF89a"):
        return "gif"
    if head[:4] == b"RIFF" and head[8:12] == b"WEBP":
        return "webp"
    if head[:4] == b"RIFF" and head[8:12] == b"WAVE":
        return "wav"
    if head[4:8] == b"ftyp":
        brand = head[8:12]
        return "m4a" if brand in (b"M4A ", b"M4B ") else "mp4"
    if head[:4] == b"\x1aE\xdf\xa3":
        return "webm"
    if head[:4] == b"OggS":
        return "ogg"
    if head[:4] == b"fLaC":
        return "flac"
    if head[:3] == b"ID3" or (len(head) > 1 and head[0] == 0xFF and head[1] & 0xE0 == 0xE0):
        return "mp3"
    if head[:4] == b"wOF2":
        return "woff2"
    if head[:4] in (b"\x00\x01\x00\x00", b"true"):
        return "ttf"
    if head[:4] == b"OTTO":
        return "otf"
    text = head.lstrip(b"\xef\xbb\xbf").lstrip()
    if text.startswith(b"WEBVTT"):
        return "vtt"
    if text.startswith(b"<?xml") or text.startswith(b"<svg") or text.startswith(b"<!--"):
        return "svg" if b"<svg" in open(path, "rb").read() else "xml"
    return "unknown"


ALLOWED = {
    "image": {"png", "jpeg", "gif", "webp", "svg"},
    "video": {"mp4", "webm"},
    "audio": {"mp3", "m4a", "mp4", "ogg", "wav", "flac"},
    "font": {"woff2", "ttf", "otf"},
}


def check_svg(path, rel, rep, max_kb):
    size = os.path.getsize(path)
    if size > max_kb * 1024:
        rep.error(rel, f"SVG is {size // 1024} KB, over the {max_kb} KB budget")
    raw = open(path, "rb").read().decode("utf-8", "replace")
    try:
        root = ET.fromstring(raw)
    except ET.ParseError as e:
        rep.error(rel, f"SVG is not well-formed XML: {e}")
        return
    if root.tag.split("}")[-1] != "svg":
        rep.error(rel, "root element is not <svg>")
    if not root.get("viewBox"):
        rep.error(rel, "SVG has no viewBox")
    if "<!DOCTYPE" in raw or "<!ENTITY" in raw:
        rep.error(rel, "SVG has a DOCTYPE/ENTITY declaration")
    if re.search(r"@import", raw):
        rep.error(rel, "SVG uses @import")
    ids = set()
    for el in root.iter():
        tag = el.tag.split("}")[-1]
        if tag in ("script", "foreignObject", "iframe", "object", "embed", "image", "animate",
                   "animateTransform", "animateMotion", "set"):
            rep.error(rel, f"SVG contains forbidden or non-static element <{tag}>")
        if el.get("id"):
            ids.add(el.get("id"))
        for key, value in el.attrib.items():
            attr = key.split("}")[-1]
            if attr.lower().startswith("on"):
                rep.error(rel, f"SVG has event handler attribute `{attr}`")
            if attr == "href" and not value.startswith("#"):
                rep.error(rel, f"SVG has an external reference `{value[:60]}`")
            if re.search(r"url\(\s*['\"]?(?!#)", value):
                rep.error(rel, f"SVG has an external url() in `{attr}`")
            if re.search(r"(?i)javascript:|data:", value):
                rep.error(rel, f"SVG has a javascript:/data: URI in `{attr}`")
            if attr == "font-family" and not set(re.split(r"\s*,\s*", value.strip("'\" "))) <= {
                    "serif", "sans-serif", "monospace", "cursive", "fantasy", "system-ui"}:
                rep.warn(rel, f"SVG text uses a non-generic font family `{value}`")
    for ref in re.findall(r"url\(#([^)]+)\)", raw):
        if ref not in ids:
            rep.error(rel, f"SVG references missing id `#{ref}`")


def parse_vtt(path, rel, rep):
    """Return the list of (start, end) cue times in seconds."""
    text = open(path, "rb").read().decode("utf-8", "replace").lstrip("﻿")
    if not text.startswith("WEBVTT"):
        rep.error(rel, "captions file does not start with WEBVTT")
        return []
    cues, stamp = [], r"(?:(\d+):)?(\d{2}):(\d{2})\.(\d{3})"
    for m in re.finditer(rf"^{stamp}\s+-->\s+{stamp}", text, re.M):
        g = [int(x) if x else 0 for x in m.groups()]
        start = g[0] * 3600 + g[1] * 60 + g[2] + g[3] / 1000
        end = g[4] * 3600 + g[5] * 60 + g[6] + g[7] / 1000
        if end <= start:
            rep.error(rel, f"cue ends before it starts ({m.group(0)})")
        if cues and start < cues[-1][0]:
            rep.error(rel, f"cues are not in order ({m.group(0)})")
        cues.append((start, end))
    if not cues:
        rep.error(rel, "captions file has no cues")
    return cues


def ffprobe(path):
    if not shutil.which("ffprobe"):
        return None
    try:
        out = subprocess.run(
            ["ffprobe", "-v", "error", "-show_format", "-show_streams", "-of", "json", path],
            capture_output=True, text=True, timeout=60, check=True).stdout
        return json.loads(out)
    except (subprocess.SubprocessError, ValueError):
        return None


def faststart(path):
    """True when the moov atom precedes mdat (progressive playback)."""
    with open(path, "rb") as f:
        order, pos, size = [], 0, os.path.getsize(path)
        while pos + 8 <= size:
            f.seek(pos)
            header = f.read(16)
            length, kind = int.from_bytes(header[:4], "big"), header[4:8]
            if length == 1:
                length = int.from_bytes(header[8:16], "big")
            if length < 8:
                break
            order.append(kind)
            pos += length
    return b"moov" in order and b"mdat" in order and order.index(b"moov") < order.index(b"mdat")


def media_facts(path, rel, kind, rep, opts):
    size = os.path.getsize(path)
    info = ffprobe(path)
    fact = {"file": rel, "bytes": size}
    limit = opts.max_video_mb * 1024 * 1024 if kind == "video" else opts.max_audio_kb * 1024
    if size > limit:
        rep.error(rel, f"{kind} file is {size} bytes, over the budget of {limit}")
    if not info:
        return fact
    fact["duration"] = round(float(info["format"].get("duration", 0)), 3)
    for s in info["streams"]:
        if s["codec_type"] == "video":
            fact["video"] = f'{s["codec_name"]} {s.get("profile", "")} {s.get("pix_fmt", "")} ' \
                            f'{s["width"]}x{s["height"]} {s.get("r_frame_rate", "")} fps'
            if kind == "video" and sniff(path) == "mp4":
                if s["codec_name"] != "h264" or s.get("pix_fmt") != "yuv420p":
                    rep.error(rel, f'video stream is {s["codec_name"]}/{s.get("pix_fmt")}, expected h264/yuv420p')
        elif s["codec_type"] == "audio":
            fact["audio"] = f'{s["codec_name"]} {s.get("profile", "")} {s.get("sample_rate")} Hz {s.get("channels")} ch'
    if kind == "video" and "video" not in fact:
        rep.error(rel, "no video stream found")
    if kind == "video" and "audio" not in fact:
        rep.warn(rel, "video has no audio track")
    if sniff(path) in ("mp4", "m4a"):
        fact["faststart"] = faststart(path)
        if not fact["faststart"]:
            rep.warn(rel, "moov atom is after mdat (not faststart)")
    return fact


# --------------------------------------------------------------------------- rich text

def strip_markup(text):
    text = re.sub(r"\\(.)", r"\1", re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", text))
    return re.sub(r"[*`]", "", text)


def check_rich(text, where, rep):
    """Balanced emphasis markers per paragraph; returns the link targets."""
    plain = re.sub(r"\\.", "", text)
    for para in re.split(r"\n\s*\n", plain):
        body = re.sub(r"`[^`\n]*`", "", para)
        body = re.sub(r"\]\([^)]*\)", "]", body)
        if body.count("`") % 2:
            rep.error(where, f"unbalanced backtick in: {para[:50]!r}")
        bold = body.count("**")
        if bold % 2:
            rep.error(where, f"unbalanced ** in: {para[:50]!r}")
        if body.replace("**", "").count("*") % 2:
            rep.error(where, f"unbalanced * in: {para[:50]!r}")
    return re.findall(r"\]\(([^)\s]+)\)", plain)


# --------------------------------------------------------------------------- the lint itself

RICH_ELEMENTS = {"text", "quote", "item", "heading", "explain", "button", "option", "answer",
                 "choice", "quiz", "chapter", "section"}
PROSE_ELEMENTS = {"text", "quote", "item", "heading", "explain"}


def lint_project(entry, root, rep, opts):
    name = os.path.relpath(entry, opts.base)
    lines, files = load_project(entry, root, opts.base, rep)
    attach_parents(lines)
    stats = {"project": name, "files": len(files), "words": 0, "ui_words": 0, "chapters": 0, "media": []}

    books = [l for l, _ in lines if l.indent == 0 and l.head == "book"]
    if len(books) != 1:
        rep.error(name, f"expected exactly one `book`, found {len(books)}")
    meta = {l.code.split("=")[0].strip() for l, _ in lines if l.parents and l.parents[-1][0] == "book"}
    for required in ("author", "language"):
        if books and required not in meta:
            rep.error(books[0].where, f"`book` is missing `{required}`")

    # ---- declarations
    assets, state, components, ids = {}, {}, {}, {}
    for line, base in lines:
        top = line.parents[0][0] if line.parents else None
        if top == "assets" and len(line.parents) == 1:
            m = re.match(rf"({NAME})\s*=\s*({NAME})\((.*)\)$", line.code, re.S)
            if not m or m.group(2) not in ASSET_KINDS:
                rep.error(line.where, f"cannot read asset declaration `{line.text()}`")
                continue
            aname, kind, args = m.group(1), m.group(2), split_args(m.group(3))
            if aname in assets:
                rep.error(line.where, f"asset `{aname}` declared twice")
            named, path = {}, None
            for a in args:
                km = re.match(rf"({NAME})\s*=\s*(.*)$", a, re.S)
                if km:
                    named[km.group(1)] = km.group(2)
                else:
                    path = string_of(line, a)
            assets[aname] = {"kind": kind, "line": line, "used": False}
            if kind in ("image", "video") and not (string_of(line, named.get("alt", "")) or "").strip():
                rep.error(line.where, f"{kind} asset `{aname}` needs a non-empty alt text")
            if "url" in named:
                continue
            if path is None:
                rep.error(line.where, f"asset `{aname}` has no path")
                continue
            extra = []
            if "poster" in named:
                extra.append(("image", string_of(line, named["poster"])))
            for cap in PLACEHOLDER.findall(named.get("captions", "")):
                extra.append(("captions", line.strings[int(cap)]))
            for k, p in [(kind, path)] + extra:
                full = os.path.normpath(os.path.join(base, p or ""))
                rel = os.path.relpath(full, opts.base)
                if os.path.commonpath([os.path.realpath(full), os.path.realpath(root)]) != os.path.realpath(root):
                    rep.error(line.where, f"asset path `{p}` escapes the project directory")
                    continue
                if not os.path.isfile(full):
                    rep.error(line.where, f"asset file `{p}` does not exist (E0401)")
                    continue
                found = sniff(full)
                if k == "captions":
                    cues = parse_vtt(full, rel, rep) if found == "vtt" else rep.error(rel, "captions are not WebVTT") or []
                    probe = ffprobe(os.path.normpath(os.path.join(base, path)))
                    if cues and probe:
                        dur = float(probe["format"].get("duration", 0))
                        if cues[-1][1] > dur + 0.05:
                            rep.error(rel, f"last cue ends at {cues[-1][1]} s, after the media ends ({dur:.3f} s)")
                    stats["media"].append({"file": rel, "bytes": os.path.getsize(full), "cues": len(cues)})
                    continue
                if found not in ALLOWED[k]:
                    rep.error(line.where, f"`{p}` is {found}, not a valid {k} format")
                    continue
                if found == "svg":
                    check_svg(full, rel, rep, opts.max_svg_kb)
                    stats["media"].append({"file": rel, "bytes": os.path.getsize(full)})
                elif k in ("video", "audio"):
                    stats["media"].append(media_facts(full, rel, k, rep, opts))
                else:
                    stats["media"].append({"file": rel, "bytes": os.path.getsize(full), "format": found})
        elif top == "state" and len(line.parents) == 1:
            m = re.match(rf"({NAME})\s*=\s*(.+?)(?:\s+in\s+(.+))?$", line.code)
            if not m:
                rep.error(line.where, f"cannot read state declaration `{line.text()}`")
                continue
            if m.group(1) in state:
                rep.error(line.where, f"state variable `{m.group(1)}` declared twice")
            init = m.group(2).strip()
            kind = "bool" if init in ("true", "false") else "text" if PLACEHOLDER.fullmatch(init) else "int"
            if kind == "text":
                values = [line.strings[int(v)] for v in PLACEHOLDER.findall(m.group(3) or "")]
                if not values or string_of(line, init) not in values:
                    rep.error(line.where, f"text state `{m.group(1)}` must start as a member of its closed set")
                if len(values) > 64:
                    rep.error(line.where, "more than 64 enum values")
            state[m.group(1)] = kind
        elif line.indent == 0 and line.head == "component":
            m = re.match(rf"component\s+({NAME})\((.*)\):$", line.code)
            if not m:
                rep.error(line.where, "cannot read component definition")
                continue
            components[m.group(1)] = [p.split("=")[0].strip() for p in split_args(m.group(2))]

    if len(state) > 256:
        rep.error(name, "more than 256 state variables")

    def known_names(line):
        params = set()
        for head, parent in line.parents:
            if head == "component":
                params |= set(components.get(re.match(rf"component\s+({NAME})", parent.code).group(1), []))
        return params

    def check_expr(expr, line):
        for ident in re.findall(rf"(?<![\w.]){NAME}", PLACEHOLDER.sub(" ", expr)):
            if ident in KEYWORDS or ident in state or ident in known_names(line):
                continue
            rep.error(line.where, f"`{ident}` in `{line.text(expr)}` is not a state variable")

    # ---- blocks
    targets, option_counts, chapter_ids = [], {}, []
    for line, _ in lines:
        heads = [h for h, _ in line.parents]
        top = heads[0] if heads else line.head
        if line.indent == 0 and line.head in ("book", "theme", "assets", "state", "cover", "back_cover", "component"):
            if line.head != "component":
                continue
        if top in ("book", "theme", "assets", "state"):
            continue
        if top in ("cover", "back_cover") and line.head == "image" and "=" in line.code and "(" not in line.code:
            used = line.code.split("=", 1)[1].strip()
            if used not in assets:
                rep.error(line.where, f"unknown asset `{used}` (E0304)")
            elif assets[used]["kind"] != "image":
                rep.error(line.where, f"`{used}` is not an image asset")
            else:
                assets[used]["used"] = True
            continue
        if top == "cover" or (top == "back_cover" and line.head != "text"):
            continue

        # actions and conditionals
        if line.head in ("if", "elif"):
            check_expr(line.code[len(line.head):].rstrip(":"), line)
            continue
        if line.head == "else":
            continue
        am = re.match(rf"({NAME})\s*(=|\+=|-=)\s*(.+)$", line.code)
        if am and heads and heads[-1] in ACTION_PARENTS:
            if am.group(1) not in state:
                rep.error(line.where, f"assignment to undeclared state variable `{am.group(1)}`")
            elif am.group(2) != "=" and state[am.group(1)] != "int":
                rep.error(line.where, f"`{am.group(2)}` needs an int variable")
            check_expr(am.group(3), line)
            continue
        if line.head == "reset":
            var = line.code[len("reset"):].strip()
            if var and var not in state:
                rep.error(line.where, f"reset of undeclared state variable `{var}`")
            continue
        if line.head == "goto":
            parts = split_args(line.code[len("goto"):].strip())
            targets.append((parts[0], line))
            continue
        if line.head == "component":
            continue

        try:
            ename, positional, named, _ = parse_element(line)
        except AttributeError:
            rep.error(line.where, f"cannot read statement `{line.text()}`")
            continue

        if "id" in named:
            ident = named["id"]
            if not re.fullmatch(NAME, ident) or len(ident) > 64:
                rep.error(line.where, f"`{ident}` is not a valid id")
            elif ident in RESERVED_TARGETS:
                rep.error(line.where, f"id `{ident}` is a reserved target")
            elif ident in ids:
                rep.error(line.where, f"id `{ident}` already used at {ids[ident]}")
            else:
                ids[ident] = line.where
        if ename == "chapter":
            stats["chapters"] += 1
            chapter_ids.append((named.get("id"), "when" in named, line))
        if "goto" in named:
            targets.append((named["goto"], line))
        if "when" in named:
            check_expr(named["when"], line)
        if ("at" in named or "size" in named) and "page" not in heads:
            rep.error(line.where, "`at=`/`size=` are only allowed inside `page`")
        if "page" in heads and ename in ("break", "section", "page"):
            rep.error(line.where, f"`{ename}` is not allowed inside `page`")
        if "page" in heads and ename not in ("if", "elif", "else") and not ({"at", "size"} <= set(named)):
            rep.warn(line.where, f"`{ename}` inside `page` has no at=/size= frame")
        if "animate" in named and named["animate"] not in ("fade", "rise", "drop", "slide_left", "slide_right", "zoom"):
            rep.error(line.where, f"unknown animation `{named['animate']}`")
        if "transition" in named and named["transition"] not in ("flip", "fade", "none"):
            rep.error(line.where, f"unknown transition `{named['transition']}`")

        if ename in ("image", "video", "audio", "gallery"):
            want = "image" if ename == "gallery" else ename
            if ename == "gallery" and not 2 <= len(positional) <= 24:
                rep.error(line.where, "a gallery takes 2 to 24 images")
            for used in positional:
                if used in known_names(line):
                    continue
                if used not in assets:
                    rep.error(line.where, f"unknown asset `{used}` (E0304)")
                elif assets[used]["kind"] != want:
                    rep.error(line.where, f"`{used}` is a {assets[used]['kind']} asset, `{ename}` needs {want}")
                else:
                    assets[used]["used"] = True
        if ename in ("option", "answer"):
            parent = line.parents[-1][1] if line.parents else None
            option_counts.setdefault(id(parent), [parent, 0, 0])
            option_counts[id(parent)][1] += 1
            option_counts[id(parent)][2] += named.get("correct") == "true"

        known = (ename in RICH_ELEMENTS or ename in components or top == "back_cover"
                 or ename in ("image", "gallery", "video", "audio", "box"))
        if not known and ename not in ("list", "divider", "space", "break", "page", "on_correct"):
            rep.error(line.where, f"unknown element `{ename}`")
        for idx, value in enumerate(line.strings):
            for target in check_rich(value, line.where, rep):
                if not re.match(r"(https?:|mailto:)", target):
                    targets.append((target, line))
                elif target.startswith("http:"):
                    rep.warn(line.where, f"plain http link `{target}`")
            for var in re.findall(r"(?<!\\)\{([^}]*)\}", value):
                if var not in state and var not in known_names(line):
                    rep.error(line.where, f"`{{{var}}}` is not a state variable or component parameter")
            if len(value.encode("utf-8")) > 65536:
                rep.error(line.where, "text run longer than 64 KiB")
            words = len(re.findall(r"[^\W_]+(?:['’\-][^\W_]+)*", strip_markup(value)))
            is_prose = ename in PROSE_ELEMENTS or ename in components or top == "back_cover"
            first_positional = bool(positional) and positional[0] == f"⟦{idx}⟧"
            if is_prose and first_positional and top != "back_cover":
                stats["words"] += words
            else:
                stats["ui_words"] += words

    for parent, count, correct in option_counts.values():
        if count > 12:
            rep.error(parent.where, "more than 12 options/answers")
        if parent.head == "quiz" and correct == 0:
            rep.error(parent.where, "quiz has no correct answer")
        if parent.head == "choice" and count < 2:
            rep.warn(parent.where, "choice with fewer than two options")

    for target, line in targets:
        if target not in ids and target not in RESERVED_TARGETS:
            rep.error(line.where, f"jump target `{target}` does not exist")
    jumped = {t for t, _ in targets}
    for cid, gated, line in chapter_ids:
        if gated and cid not in jumped:
            rep.warn(line.where, f"gated chapter `{cid}` is never the target of a goto or link")
    if chapter_ids and all(g for _, g, _ in chapter_ids):
        rep.error(name, "every chapter has a `when=` condition (E0313)")
    if not chapter_ids:
        rep.error(name, "a book needs at least one chapter")
    for aname, a in assets.items():
        if not a["used"] and a["kind"] != "font":
            rep.warn(a["line"].where, f"asset `{aname}` is declared but never used (W0603)")
    spaces = {"asset": set(assets), "state variable": set(state), "id": set(ids), "component": set(components)}
    kinds = list(spaces)
    for i, a in enumerate(kinds):
        for b in kinds[i + 1:]:
            for clash in sorted(spaces[a] & spaces[b]):
                rep.warn(name, f"`{clash}` is both a {a} and a {b}")
    stats["ids"], stats["assets"], stats["state"] = len(ids), len(assets), len(state)

    if opts.darm:
        run = subprocess.run([opts.darm, "check", entry], capture_output=True, text=True)
        stats["darm_check"] = run.returncode
        if run.returncode != 0:
            tail = (run.stdout + run.stderr).strip().splitlines()[-12:]
            rep.error(name, "darm check failed:\n      " + "\n      ".join(tail))
    return stats


def discover(base):
    found = []
    for dirpath, dirnames, filenames in os.walk(base):
        dirnames[:] = sorted(d for d in dirnames if not d.startswith(".") and d != "tools")
        if "book.darml" in filenames:
            found.append((os.path.join(dirpath, "book.darml"), dirpath))
            dirnames[:] = []
        elif os.path.basename(dirpath) == "snippets":
            found += [(os.path.join(dirpath, f), dirpath) for f in sorted(filenames) if f.endswith(".darml")]
    return found


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("paths", nargs="*", help="entry .darml files or project directories")
    ap.add_argument("--stats", action="store_true", help="print word counts and media facts")
    ap.add_argument("--darm", help="path to the darm binary; also run `darm check`")
    ap.add_argument("--max-svg-kb", type=int, default=150)
    ap.add_argument("--max-video-mb", type=float, default=3)
    ap.add_argument("--max-audio-kb", type=int, default=600)
    opts = ap.parse_args()
    opts.base = os.path.dirname(here)

    projects = []
    for p in opts.paths:
        p = os.path.abspath(p)
        if os.path.isdir(p):
            projects += discover(p) or [(os.path.join(p, "book.darml"), p)]
        else:
            projects.append((p, os.path.dirname(p)))
    projects = projects or discover(opts.base)

    rep, failed = Report(), 0
    for entry, root in projects:
        before = len(rep.errors)
        stats = lint_project(entry, root, rep, opts)
        status = "ok  " if len(rep.errors) == before else "FAIL"
        failed += status == "FAIL"
        print(f"{status} {stats['project']}: {stats['files']} file(s), {stats['chapters']} chapters, "
              f"{stats['ids']} ids, {stats['assets']} assets, {stats['state']} state vars, "
              f"{stats['words']} prose words (+{stats['ui_words']} in labels, captions and titles)")
        if opts.stats:
            for m in stats["media"]:
                extra = "  ".join(f"{k}={v}" for k, v in m.items() if k not in ("file", "bytes"))
                print(f"       {m['file']}  {m['bytes']} bytes  {extra}")
    for w in rep.warnings:
        print("warning:", w)
    for e in rep.errors:
        print("error:", e)
    print(f"{len(projects)} project(s), {failed} failed, {len(rep.errors)} error(s), {len(rep.warnings)} warning(s)")
    return 1 if rep.errors else 0


if __name__ == "__main__":
    sys.exit(main())
