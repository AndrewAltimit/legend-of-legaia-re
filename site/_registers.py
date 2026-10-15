"""Render the reverse-engineering register pages from their `docs/` sources.

The three register pages - open threads, settled threads, do not re-walk - are
row lists that change every time a thread opens, closes or is corrected. When
the site carried hand-written copies of them, every correction had to be made
twice, and the copy fell behind: hand-picked one-line verdicts kept stating
claims the docs had since overturned. So the rows are no longer copied. Each
site fragment keeps only its framing (header, lede, page-specific intro, the
filter box and its script) and marks where the register goes:

    <!--REGISTER-NAV:settled-->     the "Jump to" area list
    <!--REGISTER:settled-->         every area, every row, every write-up

`site/_gen.py` expands the markers at build time from the current `docs/`, so
the published page cannot disagree with its source. Each expansion opens with a
`<!--register:<kind> sha=...-->` stamp of the source bytes it was rendered
from; `scripts/ci/check-site-doc-mirrors.py` recomputes it to tell a stale
local build from a fresh one, and runs its coverage checks over the expansion.

Rendering is deliberately small: inline code, emphasis, links, paragraphs,
lists, tables, fenced blocks and `###` / `####` headings - the subset the
register docs use. A table row the renderer cannot place is an error, not a
silent drop: `render()` counts the thread rows of every source table and fails
when the page it built carries a different number.
"""

from __future__ import annotations

import hashlib
import html
import re
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
REPO_BLOB = "https://github.com/AndrewAltimit/legend-of-legaia-re/blob/main"

OPEN_DOC = "docs/reference/open-rev-eng-threads.md"
SETTLED_DOC = "docs/reference/re-settled-threads.md"
FALSIFIED_DOC = "docs/reference/re-do-not-re-walk.md"

# Site page each register kind renders onto (site-relative), and its source doc.
PAGE = {
    "open": ("reference/open-rev-eng-threads.html", OPEN_DOC),
    "settled": ("reference/re-settled-threads.html", SETTLED_DOC),
    "falsified": ("reference/re-do-not-re-walk.html", FALSIFIED_DOC),
}

# The site keeps short section ids for areas (inbound links use them); an area
# not listed here gets its heading slug.
AREA_IDS = {
    "open": {
        "recently-corrected": "recently-corrected",
        "field--locomotion": "field",
        "battle--rendering": "battle-rendering",
        "audio--bgm": "audio",
        "title--boot--overlays": "title-boot-overlays",
        "containers--data-blobs": "containers-data-blobs",
        "measurement--tooling": "measurement-tooling",
    },
    "settled": {
        "world-map--kingdom-bundles": "world-map",
        "battle--arts--level-up": "battle",
        "field--locomotion": "field",
        "text--fonts--dialog": "text-dialog",
        "animation": "animation",
        "audio": "audio",
        "title--boot--overlays": "title-boot-overlays",
        "rendering--camera": "rendering-camera",
        "measurement--corpus": "measurement",
    },
    "falsified": {
        "world-map--kingdom-bundles": "world-map",
        "battle--arts--level-up": "battle",
        "audio--sound-driver": "audio",
        "title--boot--overlays": "title-boot-overlays",
        "containers--placeholder-slots": "containers",
        "field--locomotion": "field",
        "no-overlay-function-lives-below-0x801ce818": "overlay-band",
        "menus--ui": "menus-ui",
        "rendering--camera": "rendering-camera",
        "measurement-readings": "measurement",
    },
}

# Sections of a source doc that the site fragment states in its own words
# (claimed there by a hand `data-doc`), so the renderer skips them.
HAND_SECTIONS = {
    "open": {"what-this-page-is-for", "what-an-evidence-grade-means", "status-conventions",
             "how-a-thread-is-laid-out", "adding-a-thread", "related-pages"},
    "settled": {"the-evidence-column", "how-a-thread-is-laid-out", "related-pages"},
    "falsified": {"related-pages"},
}

# One-line area descriptions. They describe what an area covers, not what was
# found in it, so they do not drift with the rows.
BLURBS = {
    "open": {
        "recently-corrected": "Claims that stood on these pages and were overturned. A claim that was wrong once is the cheapest place to look for one that is still wrong.",
        "field--locomotion": "Walking towns and dungeons, scripted events, scene boundaries: the field overlay (PROT 0897) and its script VM.",
        "battle--rendering": "The battle overlay, its screen chrome and the cast-module band; the overworld's rendering questions are filed here too.",
        "audio--bgm": "Music, sound effects and streamed voice: the SEQ sequencer, the VAB banks, and the field-VM opcodes that drive them.",
        "title--boot--overlays": "Boot, the title screen, and which overlay image sits in which RAM window at which moment.",
        "containers--data-blobs": "What a PROT entry's bytes are: the packer's layout, inherited tails, and which image a table belongs to.",
        "measurement--tooling": "The project's own instruments - save states, reference sweeps, census tools - whose answers decide which other rows can be trusted.",
    },
    "falsified": {
        "world-map--kingdom-bundles": "Kingdom bundles and the slot-4 animation bank.",
        "battle--arts--level-up": "Battle load, the action state machine, damage, the HUD, Muscle Dome, boss softlocks.",
        "audio--sound-driver": "Sound driver, SPU transfers, audio-level globals.",
        "title--boot--overlays": "Boot order, the title screen and overlay identity.",
        "containers--placeholder-slots": "PROT entries, placeholder slots and stream layouts - most born of the over-reading entry size.",
        "field--locomotion": "Field overlay: locomotion, move-VM opcodes, the prologue grade, phantom function entries.",
        "no-overlay-function-lives-below-0x801ce818": "Slot A begins at <code>0x801CE818</code>; a \"function\" printed below it belongs to no runtime image.",
        "menus--ui": "The pause menu and the window-widget interpreter.",
        "rendering--camera": "Readings about the draw path - including one where the instrument measuring a defect was producing it.",
        "measurement-readings": "Claims about the project's own instruments. A count can be right while the story attached to it is wrong.",
    },
}

# Per-area reference links on the open-threads page.
OPEN_SEE_ALSO = {
    "field--locomotion": [("../subsystems/script-vm.html", "Field / event VM"),
                          ("../subsystems/field-locomotion.html", "Field locomotion"),
                          ("../subsystems/motion-vm.html", "Motion VMs"),
                          ("../subsystems/renderer.html", "Renderer")],
    "battle--rendering": [("../subsystems/battle.html", "Battle"),
                          ("../subsystems/cast-module.html", "Cast modules"),
                          ("../subsystems/world-map.html", "World map")],
    "audio--bgm": [("../subsystems/audio.html", "Audio subsystem"),
                   ("music-tracks.html", "Music tracks")],
    "title--boot--overlays": [("../subsystems/boot.html", "Boot sequence"),
                              ("../tooling/static-overlay-pipeline.html", "Static overlay pipeline"),
                              ("../tooling/overlay-capture.html", "Overlay capture")],
    "containers--data-blobs": [("../formats/prot.html", "PROT.DAT"),
                               ("../tooling/byte-accounting.html", "Byte accounting"),
                               ("../tooling/disc-coverage.html", "Disc coverage")],
    "measurement--tooling": [("../tooling/mednafen-automation.html", "Mednafen automation"),
                             ("../tooling/pcsx-redux-automation.html", "PCSX-Redux automation")],
}

# The open page's own area <-> the settled / falsified areas that hold what
# closed there.
OPEN_CLOSED_LINKS = {
    "field--locomotion": ("field", "field"),
    "battle--rendering": ("battle", "battle"),
    "audio--bgm": ("audio", "audio"),
    "title--boot--overlays": ("title-boot-overlays", "title-boot-overlays"),
    "containers--data-blobs": ("measurement", "containers"),
    "measurement--tooling": ("measurement", "measurement"),
}

GRADES = ("disassembly", "capture", "decompiled-C", "inference")
ID_MAX = 72
FENCE_RE = re.compile(r"^\s*(```|~~~)")
LIVE_STATUS_RE = re.compile(r"^(open|partial|mostly\s+resolved)\b", re.IGNORECASE)


# ---------------------------------------------------------------------------
# Slugs and ids
# ---------------------------------------------------------------------------

def slug(text: str) -> str:
    """GitHub's heading -> anchor slug (the rule check-md-links.py uses)."""
    text = re.sub(r"<a\s+id=\"[^\"]*\"\s*>\s*</a>", "", text)
    text = re.sub(r"`([^`]*)`", r"\1", text)
    text = re.sub(r"\*\*([^*]*)\*\*", r"\1", text)
    text = re.sub(r"\*([^*]*)\*", r"\1", text)
    text = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", text)
    s = text.strip().lower()
    s = re.sub(r"[^\w\s-]", "", s)
    return s.replace(" ", "-")


class Ids:
    """Unique element ids for one page."""

    def __init__(self) -> None:
        self.used: set[str] = set()

    def make(self, prefix: str, text: str) -> str:
        base = prefix + slug(text)[:ID_MAX]
        cand, n = base, 2
        while cand in self.used:
            cand = f"{base}-{n}"
            n += 1
        self.used.add(cand)
        return cand

    def reserve(self, ident: str) -> None:
        self.used.add(ident)


# ---------------------------------------------------------------------------
# Markdown parsing
# ---------------------------------------------------------------------------

def split_sections(md: str, level: int) -> list[tuple[str, list[str]]]:
    """Split a page at its `#`*level headings (fences respected). The first
    element is `("", lines before the first heading)`."""
    marker = "#" * level + " "
    out: list[tuple[str, list[str]]] = [("", [])]
    in_fence = False
    for line in md.splitlines():
        if FENCE_RE.match(line):
            in_fence = not in_fence
        if not in_fence and line.startswith(marker):
            out.append((line[len(marker):].strip(), []))
            continue
        out[-1][1].append(line)
    return out


def split_cells(line: str) -> list[str]:
    s = line.strip()
    if s.startswith("|"):
        s = s[1:]
    if s.endswith("|") and not s.endswith("\\|"):
        s = s[:-1]
    cells, cur, in_code, i = [], [], False, 0
    while i < len(s):
        c = s[i]
        if c == "\\" and i + 1 < len(s) and s[i + 1] == "|":
            cur.append("|")
            i += 2
            continue
        if c == "`":
            in_code = not in_code
        if c == "|" and not in_code:
            cells.append("".join(cur).strip())
            cur = []
        else:
            cur.append(c)
        i += 1
    cells.append("".join(cur).strip())
    return cells


def is_separator(cells: list[str]) -> bool:
    return all(c and set(c) <= set("-: ") for c in cells)


def parse_blocks(lines: list[str]) -> list[tuple]:
    """A flat block list: ("p", text) / ("table", header, rows) / ("list",
    ordered, items) / ("code", text) / ("h", level, text) / ("quote", lines)."""
    blocks: list[tuple] = []
    i, n = 0, len(lines)
    while i < n:
        line = lines[i]
        s = line.strip()
        if not s or s == "---":
            i += 1
            continue
        if FENCE_RE.match(line):
            j = i + 1
            while j < n and not FENCE_RE.match(lines[j]):
                j += 1
            blocks.append(("code", "\n".join(lines[i + 1:j])))
            i = j + 1
            continue
        m = re.match(r"^(#{3,6})\s+(.*)$", s)
        if m:
            blocks.append(("h", len(m.group(1)), m.group(2)))
            i += 1
            continue
        if s.startswith("|"):
            rows = []
            while i < n and lines[i].strip().startswith("|"):
                rows.append(split_cells(lines[i]))
                i += 1
            header = rows[0]
            body = [r for r in rows[1:] if not is_separator(r)]
            blocks.append(("table", header, body))
            continue
        if s.startswith(">"):
            q = []
            while i < n and lines[i].strip().startswith(">"):
                q.append(lines[i].strip()[1:].lstrip())
                i += 1
            blocks.append(("quote", q))
            continue
        if re.match(r"^([-*]|\d+\.)\s+", s):
            items: list[tuple[int, str]] = []
            ordered = bool(re.match(r"^\d+\.", s))
            while i < n:
                raw = lines[i]
                if not raw.strip():
                    # a blank line ends the list unless an item follows
                    if i + 1 < n and re.match(r"^\s*([-*]|\d+\.)\s+", lines[i + 1]):
                        i += 1
                        continue
                    break
                mm = re.match(r"^(\s*)([-*]|\d+\.)\s+(.*)$", raw)
                if mm:
                    items.append((len(mm.group(1)), mm.group(3)))
                elif raw.startswith(" ") and items:
                    d, t = items[-1]
                    items[-1] = (d, t + " " + raw.strip())
                else:
                    break
                i += 1
            blocks.append(("list", ordered, items))
            continue
        para = []
        while i < n:
            t = lines[i].strip()
            if (not t or t.startswith("|") or FENCE_RE.match(lines[i]) or t == "---"
                    or re.match(r"^#{3,6}\s", t) or re.match(r"^([-*]|\d+\.)\s+", t)
                    or t.startswith(">")):
                break
            para.append(t)
            i += 1
        blocks.append(("p", " ".join(para)))
    return blocks


# ---------------------------------------------------------------------------
# Rendering
# ---------------------------------------------------------------------------

class Ctx:
    """Link resolution for one source doc rendered onto one site page."""

    def __init__(self, src_doc: str, page: str, anchors: dict, site_pages: set[str]):
        self.src_doc = src_doc          # repo-relative path of the markdown source
        self.page = page                # site-relative output page
        self.anchors = anchors          # (repo doc path, slug) -> (site page, id)
        self.site_pages = site_pages    # every generated page, site-relative

    def rel(self, target_page: str) -> str:
        here = self.page.rsplit("/", 1)[0] if "/" in self.page else ""
        there_dir, _, there_name = target_page.rpartition("/")
        if there_dir == here:
            return there_name
        up = "../" * (self.page.count("/"))
        return up + target_page

    def href(self, target: str) -> tuple[str, bool]:
        """(href, external) for a markdown link target."""
        if re.match(r"^[a-z]+:", target):
            return target, True
        path, _, frag = target.partition("#")
        if path:
            base = Path(self.src_doc).parent
            parts: list[str] = []
            for seg in (base / path).as_posix().split("/"):
                if seg == "..":
                    if parts:
                        parts.pop()
                elif seg and seg != ".":
                    parts.append(seg)
            repo_path = "/".join(parts)
        else:
            repo_path = self.src_doc
        key = (repo_path, frag.lower())
        if key in self.anchors:
            page, ident = self.anchors[key]
            if page == self.page and ident:
                return f"#{ident}", False
            return self.rel(page) + (f"#{ident}" if ident else ""), False
        if not frag and repo_path.startswith("docs/") and repo_path.endswith(".md"):
            site = repo_path[len("docs/"):-3] + ".html"
            if site in self.site_pages:
                return self.rel(site), False
        url = f"{REPO_BLOB}/{repo_path}"
        if frag:
            url += f"#{frag}"
        return url, True


def inline(md: str, ctx: Ctx) -> str:
    """Inline markdown -> HTML: code spans, links, bold, italics."""
    md = re.sub(r"<a\s+id=\"[^\"]*\"\s*>\s*</a>", "", md)
    md = md.replace("\\|", "|")
    codes: list[str] = []

    def stash(m: re.Match) -> str:
        codes.append(f"<code>{html.escape(m.group(1), quote=False)}</code>")
        return f"\x00{len(codes) - 1}\x00"

    md = re.sub(r"`([^`]+)`", stash, md)
    md = html.escape(md, quote=False)
    md = md.replace("&lt;br&gt;", "<br>")

    def link(m: re.Match) -> str:
        text, target = m.group(1), html.unescape(m.group(2))
        url, ext = ctx.href(target)
        attrs = ' target="_blank" rel="noopener"' if ext else ""
        return f'<a href="{html.escape(url)}"{attrs}>{text}</a>'

    md = re.sub(r"\[((?:[^\[\]]|\x00\d+\x00)+)\]\(([^)\s]+)\)", link, md)
    md = re.sub(r"\*\*(.+?)\*\*", r"<strong>\1</strong>", md)
    md = re.sub(r"(?<![\w*])\*(?!\s)(.+?)(?<!\s)\*(?![\w*])", r"<em>\1</em>", md)
    md = re.sub(r"(?<![\w])_(?!\s)([^_]+?)(?<!\s)_(?![\w])", r"<em>\1</em>", md)
    return re.sub(r"\x00(\d+)\x00", lambda m: codes[int(m.group(1))], md)


def render_list(items: list[tuple[int, str]], ordered: bool, ctx: Ctx) -> str:
    tag = "ol" if ordered else "ul"
    cls = "" if ordered else ' class="bullets"'
    out, stack = [], []
    for depth, text in items:
        if not stack:
            out.append(f"<{tag}{cls}>")
            stack.append(depth)
        elif depth > stack[-1]:
            out.append("<ul>")
            stack.append(depth)
        else:
            while len(stack) > 1 and depth < stack[-1]:
                out.append("</li></ul>")
                stack.pop()
            out.append("</li>")
        out.append(f"<li>{inline(text, ctx)}")
    while len(stack) > 1:
        out.append("</li></ul>")
        stack.pop()
    out.append(f"</li></{tag}>")
    return "".join(out)


def render_table(header: list[str], rows: list[list[str]], ctx: Ctx) -> str:
    h = "".join(f"<th>{inline(c, ctx)}</th>" for c in header)
    body = []
    for r in rows:
        cells = (r + [""] * len(header))[:max(len(header), len(r))]
        body.append("<tr>" + "".join(f"<td>{inline(c, ctx)}</td>" for c in cells) + "</tr>")
    return ('<div class="table-wrap"><table><thead><tr>' + h + "</tr></thead><tbody>"
            + "".join(body) + "</tbody></table></div>")


def render_blocks(blocks: list[tuple], ctx: Ctx, table_hook=None) -> str:
    out = []
    for b in blocks:
        kind = b[0]
        if kind == "p":
            out.append(f"<p>{inline(b[1], ctx)}</p>")
        elif kind == "h":
            lvl = min(6, b[1] + 1)
            out.append(f"<h{lvl}>{inline(b[2], ctx)}</h{lvl}>")
        elif kind == "code":
            out.append(f"<pre><code>{html.escape(b[1], quote=False)}</code></pre>")
        elif kind == "quote":
            out.append(f"<blockquote><p>{inline(' '.join(b[1]), ctx)}</p></blockquote>")
        elif kind == "list":
            out.append(render_list(b[2], b[1], ctx))
        elif kind == "table":
            done = table_hook(b[1], b[2]) if table_hook else None
            out.append(done if done is not None else render_table(b[1], b[2], ctx))
    return "\n".join(out)


def plain(md: str) -> str:
    md = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", md)
    return re.sub(r"[`*]", "", md).strip()


def paren_short(status: str, word: str) -> str | None:
    """`resolved (X)` -> `X`; `resolved` -> None. Anything else is returned whole."""
    s = status.strip()
    m = re.match(rf"^{word}\b\s*(.*)$", s, re.IGNORECASE)
    if not m:
        return s
    rest = m.group(1).strip()
    if not rest:
        return None
    if rest.startswith("(") and rest.endswith(")"):
        return rest[1:-1].strip()
    return rest.lstrip("-:, ").strip() or None


def grade_cell(evidence: str, ctx: Ctx) -> str:
    def tag(m: re.Match) -> str:
        return f'<span class="tag tag-done">{m.group(1)}</span>'
    out = inline(evidence, ctx)
    return re.sub(r"<code>(" + "|".join(re.escape(g) for g in GRADES) + r")</code>", tag, out)


def reg_row(ident: str, q: str, short: str, why: str, grade: str | None,
            summary: str, extra_attr: str = "") -> str:
    tail = f'<td>{grade}</td>' if grade is not None else ""
    return (f'<tr id="{ident}"{extra_attr}><td>{q}</td><td>{short}'
            f'<details class="disclosure reg-why"><summary>{summary}</summary>'
            f'<div class="disclosure-body">{why}</div></details></td>{tail}</tr>')


# ---------------------------------------------------------------------------
# Per-page renderers
# ---------------------------------------------------------------------------

def _read(rel: str) -> str:
    return (REPO / rel).read_text(encoding="utf-8")


def settled_area_pages() -> list[tuple[str, str, str]]:
    """(area slug, area title, area page repo path) from the index's area table."""
    md = _read(SETTLED_DOC)
    out = []
    for line in md.splitlines():
        m = re.match(r"^\|\s*\[([^\]]+)\]\(#([^)]+)\)\s*\|\s*\[[^\]]*\]\(([^)]+)\)", line)
        if m:
            out.append((m.group(2), m.group(1), "docs/reference/" + m.group(3)))
    return out


def source_files() -> list[str]:
    files = [OPEN_DOC, SETTLED_DOC, FALSIFIED_DOC]
    files += [p for _, _, p in settled_area_pages()]
    return files


def stamp(kind: str) -> str:
    h = hashlib.sha256()
    h.update(Path(__file__).read_bytes())
    for f in source_files():
        h.update(f.encode())
        h.update((REPO / f).read_bytes())
    return f"<!--register:{kind} sha={h.hexdigest()[:16]}-->"


def build_anchors() -> dict:
    """(repo doc path, lower-case slug) -> (site page, element id) for every
    area and write-up the three pages render, plus each doc's page itself."""
    anchors: dict = {}
    for kind, (page, doc) in PAGE.items():
        anchors[(doc, "")] = (page, "")
    # open + falsified: areas and their ### write-ups on one page each
    for kind in ("open", "falsified"):
        page, doc = PAGE[kind]
        ids = Ids()
        for title, lines in split_sections(_read(doc), 2)[1:]:
            s = slug(title)
            if s in HAND_SECTIONS[kind]:
                continue
            aid = AREA_IDS[kind].get(s, s)
            ids.reserve(aid)
            anchors[(doc, s)] = (page, aid)
            for sub, _ in split_sections("\n".join(lines), 3)[1:]:
                anchors[(doc, slug(sub))] = (page, ids.make("w-", sub))
    # settled: index areas + every area page's ### write-ups
    page, doc = PAGE["settled"]
    ids = Ids()
    for area_slug, _title, area_doc in settled_area_pages():
        aid = AREA_IDS["settled"].get(area_slug, area_slug)
        ids.reserve(aid)
        anchors[(doc, area_slug)] = (page, aid)
        anchors[(area_doc, "")] = (page, aid)
        for title, lines in split_sections(_read(area_doc), 2)[1:]:
            for sub, _ in split_sections("\n".join(lines), 3)[1:]:
                ident = ids.make("w-", sub)
                anchors[(area_doc, slug(sub))] = (page, ident)
                anchors[(doc, slug(sub))] = (page, ident)
    return anchors


def _writeups(sections: list[tuple[str, list[str]]], ctx: Ctx, ids_map: dict, doc: str) -> str:
    if not sections:
        return ""
    items = []
    for title, lines in sections:
        ident = ids_map[(doc, slug(title))][1]
        body = render_blocks(parse_blocks(lines), ctx)
        items.append(f'<details class="disclosure reg-writeup" id="{ident}"><summary>'
                     f'{inline(title, ctx)}</summary><div class="disclosure-body">{body}'
                     f"</div></details>")
    return ('<p class="reg-writeups"><strong>Long write-ups in this area</strong></p>\n' + "\n".join(items))


class Counter:
    def __init__(self) -> None:
        self.source = 0
        self.rendered = 0


def _thread_table_hook(kind: str, ctx: Ctx, ids: Ids, counter: Counter):
    """Turn a register table into a filterable reg-table; leave others alone."""

    def hook(header: list[str], rows: list[list[str]]) -> str | None:
        h0 = plain(header[0]).lower() if header else ""
        hl = [plain(c).lower() for c in header]
        if h0 not in ("thread", "reading"):
            return None
        out = []
        for r in rows:
            r = (r + [""] * len(header))[:len(header)]
            counter.source += 1
            q = inline(r[0], ctx)
            ident = ids.make("t-" if kind == "settled" else ("x-" if kind == "falsified" else "o-"), plain(r[0]))
            if kind == "settled" and hl[:4] == ["thread", "status", "evidence", "answer"]:
                short = paren_short(r[1], "resolved")
                short_html = inline(short, ctx) if short else '<span class="reg-kw">resolved</span>'
                why = f"<p>{inline(r[3], ctx)}</p>"
                out.append(reg_row(ident, q, short_html, why, grade_cell(r[2], ctx), "Why"))
            elif kind == "falsified" and hl[:3] == ["thread", "verdict", "why"]:
                short = paren_short(r[1], "falsified")
                short_html = inline(short, ctx) if short else '<span class="reg-kw">falsified</span>'
                why = f"<p>{inline(r[2], ctx)}</p>"
                out.append(reg_row(ident, q, short_html, why, None,
                                   "Why it looked right, and what killed it"))
            elif h0 == "reading" and len(header) == 3:
                why = (f"<p><strong>Why it looked right:</strong> {inline(r[1], ctx)}</p>"
                       f"<p><strong>What is true:</strong> {inline(r[2], ctx)}</p>")
                out.append(reg_row(ident, q, '<span class="reg-kw">falsified</span>', why, None,
                                   "Why it looked right, and what killed it"))
            elif h0 == "reading" and len(header) == 2:
                out.append(reg_row(ident, q, '<span class="reg-kw">falsified</span>',
                                   f"<p>{inline(r[1], ctx)}</p>", None, "What is true"))
            elif kind == "open" and h0 == "thread":
                status = r[1]
                word = re.match(r"^(open|partial|mostly resolved)", plain(status), re.I)
                tagcls = {"open": "tag-open", "partial": "tag-partial"}.get(
                    (word.group(1).lower() if word else ""), "tag-mostly")
                rest = inline(status[len(word.group(1)):].lstrip(" -") if word else status, ctx)
                head = f'<span class="tag {tagcls}">{word.group(1) if word else ""}</span> ' if word else ""
                attr = f' data-row="{slug(r[0])}"' if word and LIVE_STATUS_RE.match(plain(status)) else ""
                close = "".join(f"<p>{inline(c, ctx)}</p>" for c in r[2:])
                out.append(f'<tr id="{ident}"{attr}><td>{q}</td><td>{head}{rest}</td><td>{close}</td></tr>')
            else:
                return None
            counter.rendered += 1
        if kind == "settled":
            thead = "<th>Thread</th><th>Answer</th><th>Grade</th>"
        elif kind == "open":
            thead = "<th>Thread</th><th>Status</th><th>What would close it</th>"
        else:
            thead = "<th>The reading that looked right</th><th>What is true</th>"
        cls = "" if kind == "open" else ' class="reg-table"'
        return (f'<div class="table-wrap"><table{cls}>\n    <thead><tr>' + thead
                + "</tr></thead>\n    <tbody>\n      " + "\n      ".join(out)
                + "\n    </tbody>\n  </table></div>")

    return hook


def _count_thread_rows(md: str) -> int:
    n = 0
    for b in parse_blocks(md.splitlines()):
        if b[0] == "table" and b[1] and plain(b[1][0]).lower() in ("thread", "reading"):
            n += len(b[2])
    return n


def render(kind: str, site_pages: set[str]) -> tuple[str, str]:
    """(nav html, body html) for one register page."""
    page, doc = PAGE[kind]
    anchors = build_anchors()
    ids = Ids()
    for (_, _), (p, ident) in anchors.items():
        if p == page and ident:
            ids.reserve(ident)
    counter = Counter()
    counter_src = 0
    nav, sections = [], []

    def area(area_slug: str, title_md: str, ctx: Ctx, intro: str, body_md: list[str],
             writeups: list, src_doc: str, extra: str = "") -> None:
        aid = AREA_IDS[kind].get(area_slug, area_slug)
        hook = _thread_table_hook(kind, ctx, ids, counter)
        nonlocal counter_src
        counter_src += _count_thread_rows("\n".join(body_md))
        body = render_blocks(parse_blocks(body_md), ctx, hook)
        cls = "doc-section reg-area" if kind != "open" else "doc-section"
        nav.append(f'<a href="#{aid}">{html.escape(plain(title_md))}</a>')
        sections.append(
            f'<section class="{cls}" id="{aid}" data-doc="{area_slug}">\n'
            f"  <h2>{inline(title_md, ctx)}</h2>\n"
            + (f"  <p>{intro}</p>\n" if intro else "")
            + body + "\n" + _writeups(writeups, ctx, anchors, src_doc) + extra
            + "\n</section>")

    if kind in ("open", "falsified"):
        md = _read(doc)
        for title, lines in split_sections(md, 2)[1:]:
            s = slug(title)
            if s in HAND_SECTIONS[kind]:
                continue
            ctx = Ctx(doc, page, anchors, site_pages)
            subs = split_sections("\n".join(lines), 3)
            extra = ""
            if kind == "open" and s in OPEN_SEE_ALSO:
                links = " ".join(f'<a href="{h}">{t}</a>' for h, t in OPEN_SEE_ALSO[s])
                extra = ('\n  <div class="see-also"><span class="see-also-label">Reference</span> '
                         + links + "</div>")
            area(s, title, ctx, BLURBS.get(kind, {}).get(s, ""), subs[0][1], subs[1:], doc, extra)
    else:
        for area_slug, title, area_doc in settled_area_pages():
            amd = _read(area_doc)
            ctx = Ctx(area_doc, page, anchors, site_pages)
            top = split_sections(amd, 2)
            # Intro: the area page's own description, past the register boilerplate.
            intro_paras = [b for b in parse_blocks(top[0][1]) if b[0] == "p"
                           and not b[1].startswith("One area of")
                           and not b[1].startswith("The evidence grades")]
            intro = " ".join(inline(b[1], ctx) for b in intro_paras[:1])
            body_lines: list[str] = []
            writeups: list = []
            for t, lines in top[1:]:
                if slug(t) == "detailed-write-ups":
                    continue
                subs = split_sections("\n".join(lines), 3)
                if slug(t) != "threads":
                    body_lines += ["", f"### {t}", ""]
                body_lines += subs[0][1]
                writeups += subs[1:]
            src = f'<p class="reg-src">Source: <a href="{REPO_BLOB}/{area_doc}" target="_blank" rel="noopener"><code>{area_doc}</code></a></p>'
            area(area_slug, title, ctx, intro, body_lines, writeups, area_doc, "\n" + src)

    if counter.rendered != counter_src:
        raise RuntimeError(
            f"register {kind}: rendered {counter.rendered} thread rows of {counter_src} in the source"
        )
    nav_html = ('<nav class="see-also reg-nav" aria-label="Areas on this page">\n'
                '    <span class="see-also-label">Jump to</span>\n    '
                + "\n    ".join(nav)
                + ('\n    <a href="#find">Filter threads</a>' if kind != "open" else "")
                + "\n  </nav>")
    return nav_html, stamp(kind) + "\n" + "\n\n".join(sections)


def expand(body: str, site_pages: set[str]) -> str:
    """Replace every register marker in a fragment with its rendering."""
    for kind in PAGE:
        nav_m, body_m = f"<!--REGISTER-NAV:{kind}-->", f"<!--REGISTER:{kind}-->"
        if nav_m in body or body_m in body:
            nav, rendered = render(kind, site_pages)
            body = body.replace(nav_m, nav).replace(body_m, rendered)
    return body
