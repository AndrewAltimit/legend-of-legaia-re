#!/usr/bin/env python3
"""The project teaser: a 1080p video of grids of clips, each scene one side of the repo.

Shots come from four places, all listed in `teaser.toml` beside this file:

* `scene` / `args` / `pad` rows: `legaia-engine play-window` runs, as the
  hero montage shoots them (`site-media.py`'s runner), a frame every `every`
  ticks;
* `str` rows: a movie segment through the port's MDEC decoder;
* `page` rows: a site page in headless Chromium, recorded by
  `teaser-browser.mjs` while its steps drive it;
* `app` rows: a desktop app (Explorer, the launcher, the asset viewer) on a
  private Xvfb display, driven by xdotool and recorded with ffmpeg's x11grab.

Every window opens on that private display, so a run leaves the desktop alone.
Each shot is cached under a hash of its row (and its binary), so an edited
row re-shoots only itself. Scenes then lay the shots out as titled grids, and
the frames are encoded to `--out` (default `captures/teaser/teaser.mp4`).

    scripts/site-media/teaser.py                       # shoot what changed, compose, encode
    scripts/site-media/teaser.py --only ex_arts --sheet   # re-shoot one shot, preview sheet
    scripts/site-media/teaser.py --no-shoot --scenes 3    # recompose one scene from the cache
    scripts/site-media/teaser.py --build               # cargo build the binaries first

Needs `LEGAIA_DISC_BIN` and an `extracted/` tree, a built site (`site/wasm`
and `python3 site/_gen.py`), playwright-core (see `scripts/ci/browser-session.mjs`),
Xvfb, xdotool, ffmpeg and Pillow. The output holds game footage rendered from
the disc: it lives under `captures/` (gitignored) and is not committed.
"""
import argparse
import hashlib
import importlib.util
import json
import math
import os
import shutil
import subprocess
import sys
import time
import tomllib
from concurrent.futures import ProcessPoolExecutor
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

REPO = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
BIN = REPO / "target/release"
W, H = 1920, 1080

_spec = importlib.util.spec_from_file_location("site_media", HERE / "site-media.py")
site_media = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(site_media)


def log(msg):
    print(f"teaser: {msg}", file=sys.stderr, flush=True)


# ---------------------------------------------------------------- shooting

def shot_kind(row):
    if "page" in row:
        return "page"
    if "app" in row:
        return "app"
    if "str" in row:
        return "str"
    return "engine"


def shot_key(row):
    kind = shot_kind(row)
    binary = {"engine": BIN / "legaia-engine", "str": BIN / "mdec"}.get(kind)
    if kind == "app":
        binary = BIN / {"{explorer}": "legaia-explorer", "{engine}": "legaia-engine",
                        "{asset_viewer}": "asset-viewer"}.get(row["app"][0], "")
    stamp = ""
    if binary and binary.exists():
        st = binary.stat()
        stamp = f"{st.st_size}:{st.st_mtime_ns}"
    if kind == "page":
        s = REPO / "site/wasm/SOURCE_STAMP.json"
        stamp = s.read_text() if s.exists() else ""
    blob = json.dumps({k: v for k, v in row.items() if k != "label"}, sort_keys=True) + stamp
    return hashlib.sha256(blob.encode()).hexdigest()[:16]


class Xvfb:
    """A private X display for every window the shots open."""

    def __init__(self, size=(W, H)):
        self.proc = None
        for n in range(91, 120):
            if not Path(f"/tmp/.X{n}-lock").exists():
                self.display = f":{n}"
                break
        self.size = size

    def __enter__(self):
        self.proc = subprocess.Popen(["Xvfb", self.display, "-screen", "0", f"{self.size[0]}x{self.size[1]}x24",
                                      "-nolisten", "tcp"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        time.sleep(1.0)
        if self.proc.poll() is not None:
            sys.exit(f"teaser: Xvfb {self.display} did not start")
        log(f"Xvfb on {self.display}")
        return self

    def __exit__(self, *a):
        self.proc.terminate()
        self.proc.wait(timeout=10)


def subst(s, ctx):
    return s.format(**ctx) if isinstance(s, str) else s


def shoot_engine(row, out, defaults, xdisplay):
    os.environ["DISPLAY"] = xdisplay
    site_media.shoot_engine(row, out, {"every": row.get("every", defaults["every"])},
                            audio_wav=out / "audio.wav")


def shoot_str(row, out, defaults, xdisplay):
    """A movie segment through the port's MDEC decoder, its frames named by
    the 60 Hz tick each lands on (the movie runs at its own `fps`, 15)."""
    raw = out.parent / (out.name + ".str")
    shutil.rmtree(raw, ignore_errors=True)
    raw.mkdir(parents=True)
    site_media.run([str(BIN / "mdec"), "decode-str", str(site_media.extracted_root() / row["str"]), "-o", str(raw),
                    "--start-frame", str(row.get("start_frame", 0)), "--end-frame", str(row.get("end_frame", 0))])
    frames = sorted(raw.glob("*.png"))
    every, rate = row.get("every", defaults["every"]), row.get("fps", 15)
    tick = 0
    while (src := int(tick / 60 * rate)) < len(frames) and tick <= row["to"]:
        if tick >= row.get("from", 0):
            shutil.copy(frames[src], out / f"tick_{tick:05d}.png")
        tick += every
    shutil.rmtree(raw, ignore_errors=True)


def xdo(display, *args):
    return subprocess.run(["xdotool", *[str(a) for a in args]], env=dict(os.environ, DISPLAY=display),
                          check=False, capture_output=True, text=True).stdout


def app_actions(actions, display, origin, t0):
    """Run a desktop shot's pointer and key steps, each at its own time."""
    ox, oy = origin
    for a in actions:
        at = a.get("at", 0)
        delay = t0 + at - time.time()
        if delay > 0:
            time.sleep(delay)
        if "click" in a:
            x, y = a["click"]
            xdo(display, "mousemove", ox + x, oy + y, "click", a.get("button", 1))
        elif "move" in a:
            x0, y0 = a.get("start", a["move"])
            x1, y1 = a["move"]
            n = max(1, int(a.get("over", 0.4) * 30))
            for i in range(1, n + 1):
                xdo(display, "mousemove", ox + x0 + (x1 - x0) * i // n, oy + y0 + (y1 - y0) * i // n)
                time.sleep(1 / 30)
        elif "drag" in a:
            x0, y0, x1, y1 = a["drag"]
            n = max(1, int(a.get("over", 1.0) * 30))
            xdo(display, "mousemove", ox + x0, oy + y0, "mousedown", 1)
            for i in range(1, n + 1):
                xdo(display, "mousemove", ox + x0 + (x1 - x0) * i // n, oy + y0 + (y1 - y0) * i // n)
                time.sleep(1 / 30)
            xdo(display, "mouseup", 1)
        elif "scroll" in a:
            x, y = a.get("at_xy", [0, 0])
            xdo(display, "mousemove", ox + x, oy + y)
            btn = 5 if a["scroll"] > 0 else 4
            for _ in range(abs(a["scroll"])):
                xdo(display, "click", btn)
                time.sleep(a.get("every", 0.08))
        elif "key" in a:
            xdo(display, "key", "--delay", "60", *a["key"].split())
        elif "type" in a:
            xdo(display, "type", "--delay", str(a.get("delay", 80)), a["type"])


def shoot_app(row, out, defaults, xdisplay):
    work = out.parent / (out.name + ".run")
    shutil.rmtree(work, ignore_errors=True)
    (work / "cfg").mkdir(parents=True)
    (work / "data").mkdir()
    ctx = {"disc": os.environ.get("LEGAIA_DISC_BIN", ""), "extracted": str(REPO / "extracted"),
           "explorer": str(BIN / "legaia-explorer"), "engine": str(BIN / "legaia-engine"),
           "asset_viewer": str(BIN / "asset-viewer"), "work": str(work)}
    for f, body in row.get("files", {}).items():
        p = work / f
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(subst(body, ctx))
    cmd = [subst(c, ctx) for c in row["app"]]
    env = dict(os.environ, DISPLAY=xdisplay, XMODIFIERS="@im=none", XDG_CONFIG_HOME=str(work / "cfg"),
               XDG_DATA_HOME=str(work / "data"), HOME=str(work), **{k: subst(v, ctx) for k, v in row.get("env", {}).items()})
    env.pop("WAYLAND_DISPLAY", None)
    with open(work / "app.log", "w") as logf:
        proc = subprocess.Popen(cmd, cwd=row.get("cwd", str(REPO)), env=env, stdout=logf, stderr=subprocess.STDOUT)
    try:
        deadline = time.time() + 60
        win = ""
        while time.time() < deadline and not win:
            time.sleep(0.5)
            win = xdo(xdisplay, "search", "--onlyvisible", "--name", row.get("window", ".")).split()
            win = win[-1] if win else ""
            if proc.poll() is not None:
                sys.exit(f"teaser: {row['name']}: the app exited (see {work / 'app.log'})")
        if not win:
            sys.exit(f"teaser: {row['name']}: no window")
        if row.get("size"):
            xdo(xdisplay, "windowsize", win, *row["size"])
        xdo(xdisplay, "windowmove", win, 0, 0)
        xdo(xdisplay, "windowfocus", win)
        time.sleep(row.get("ready", 5))
        geo = dict(l.strip().split(": ", 1) for l in xdo(xdisplay, "getwindowgeometry", win).splitlines()[1:])
        x, y = (int(v) for v in geo["Position"].split(" ")[0].split(","))
        w, h = (int(v) for v in geo["Geometry"].split("x"))
        w, h = w - w % 2, h - h % 2
        fps = defaults["fps"]
        secs = row["seconds"]
        grab = subprocess.Popen(["ffmpeg", "-loglevel", "error", "-y", "-f", "x11grab", "-draw_mouse",
                                 "1" if row.get("pointer", True) else "0", "-framerate", str(fps),
                                 "-video_size", f"{w}x{h}", "-i", f"{xdisplay}+{x},{y}", "-t", str(secs),
                                 "-start_number", "0", str(out / "f_%05d.png")])
        t0 = time.time()
        app_actions(row.get("actions", []), xdisplay, (x, y), t0)
        grab.wait(timeout=secs + 60)
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            proc.kill()
    if not list(out.glob("f_*.png")):
        sys.exit(f"teaser: {row['name']}: no frames")


def shoot_pages(rows, work, defaults):
    spec = {"site": str(REPO / "site"), "fps": defaults["fps"], "shots": rows}
    f = work / "pages.json"
    f.write_text(json.dumps(spec, indent=1))
    site_media.run(["node", str(HERE / "teaser-browser.mjs"), str(f), str(work / "shots")], cwd=REPO)


def shot_crop(row, work):
    """A page shot's measured `cropTo` box (crop.json), else the row's `crop`."""
    f = work / "shots" / row["name"] / "crop.json"
    if row.get("cropTo") and f.exists():
        return json.loads(f.read_text())
    return row.get("crop")


def _cropped(p, crop):
    im = Image.open(p)
    if crop:
        x, y, w, h = crop
        im = im.crop((x, y, x + w, y + h))
    return im


def deflicker(row, work):
    """Hold the last good frame over a screencast frame whose cropped region
    came back blank (a WebGL canvas grabbed between its clear and its draw).
    A frame is blank when its region is near-uniform while the frame before
    it was not; a shot that is blank for a stretch on purpose (a fade) keeps
    the stretch: runs longer than `deflicker` frames (6 for `true`) are left.
    Every blank frame at the very start is left too (nothing came before).
    Every page shot also gets the one-frame spike test below."""
    from PIL import ImageStat
    out = work / "shots" / row["name"]
    frames = sorted(out.glob("f_*.jpg"))
    crop = shot_crop(row, work)
    def spread(p):
        im = Image.open(p)
        if crop:
            x, y, w, h = crop
            im = im.crop((x, y, x + w, y + h))
        return ImageStat.Stat(im.convert("L").resize((64, 64))).stddev[0]
    sd = [spread(p) for p in frames] if row.get("deflicker") else [99.0] * len(frames)
    run = row["deflicker"] if isinstance(row.get("deflicker"), int) and not isinstance(row["deflicker"], bool) else 6
    fixed, i = 0, 1
    while i < len(frames):
        if sd[i] < 4 and sd[i - 1] >= 4:
            j = i
            while j < len(frames) and sd[j] < 4:
                j += 1
            if j - i <= run:
                for k in range(i, j):
                    shutil.copyfile(frames[i - 1], frames[k])
                    sd[k] = sd[i - 1]
                    fixed += 1
            i = j
        else:
            i += 1
    # A one-frame spike: a frame far from both neighbours while they agree
    # with each other (a blank canvas grab inside page chrome, which the
    # uniform-region test above cannot see). A real cut or flash moves the
    # frames after it too, so it is left.
    lum = [ImageStat.Stat(_cropped(p, crop).convert("L").resize((96, 96))).mean[0] for p in frames]
    for i in range(1, len(frames) - 1):
        a, b, c = lum[i - 1], lum[i], lum[i + 1]
        if abs(b - a) > 6 and abs(b - c) > 6 and abs(a - c) < 3:
            shutil.copyfile(frames[i - 1], frames[i])
            lum[i] = a
            fixed += 1
    if fixed:
        log(f"{row['name']}: held {fixed} blank frame(s)")


def frames_of(row, out):
    """The shot's frames in order, as paths."""
    if shot_kind(row) in ("engine", "str"):
        return site_media.clip_frames(row, out)
    return sorted(p for p in out.iterdir() if p.name.startswith("f_")) if out.exists() else []


# ---------------------------------------------------------------- composing

FONT_SANS = "/usr/share/fonts/truetype/ubuntu/UbuntuSans[wdth,wght].ttf"
FONT_FALLBACK = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"


def font(size, weight="Bold"):
    try:
        f = ImageFont.truetype(FONT_SANS, size)
        f.set_variation_by_name(weight)
        return f
    except OSError:
        return ImageFont.truetype(FONT_FALLBACK, size)


def ease(k):
    k = max(0.0, min(1.0, k))
    return 1 - (1 - k) ** 3


def hexrgb(s):
    s = s.lstrip("#")
    return tuple(int(s[i:i + 2], 16) for i in (0, 2, 4))


def background(style):
    top, bottom = hexrgb(style["bg_top"]), hexrgb(style["bg_bottom"])
    grad = Image.new("RGB", (1, H))
    for y in range(H):
        k = y / (H - 1)
        grad.putpixel((0, y), tuple(int(top[i] + (bottom[i] - top[i]) * k) for i in range(3)))
    bg = grad.resize((W, H))
    glow = Image.new("L", (W, H), 0)
    ImageDraw.Draw(glow).ellipse((-400, -500, 900, 500), fill=60)
    glow = glow.filter(ImageFilter.GaussianBlur(220))
    accent = Image.new("RGB", (W, H), hexrgb(style["accent"]))
    return Image.composite(accent, bg, glow)


def fit(img, size, crop=None, anchor=0.5):
    if crop:
        x, y, w, h = crop
        img = img.crop((x, y, x + w, y + h))
    w, h = size
    sw, sh = img.size
    if sw * h > sh * w:
        nw = sh * w // h
        x0 = int((sw - nw) * 0.5)
        img = img.crop((x0, 0, x0 + nw, sh))
    elif sw * h < sh * w:
        nh = sw * h // w
        y0 = int((sh - nh) * anchor)
        img = img.crop((0, y0, sw, y0 + nh))
    return img.convert("RGB").resize((w, h), Image.LANCZOS)


def rounded_mask(size, r):
    m = Image.new("L", size, 0)
    ImageDraw.Draw(m).rounded_rectangle((0, 0, size[0] - 1, size[1] - 1), r, fill=255)
    return m


def layout_rects(scene, area):
    """The tile rectangles of a scene's layout inside `area` (x, y, w, h)."""
    ax, ay, aw, ah = area
    gap = 22
    n = len(scene["tiles"])
    kind = scene.get("layout", "grid")
    aspect = scene.get("aspect", [4, 3])
    ar = aspect[0] / aspect[1]
    if kind == "feature":
        # One large tile on the left, the rest stacked on the right.
        side = n - 1
        # Solve for the big tile height bh: big w = bh*ar, small h = (bh - gap*(side-1))/side, small w = sh*ar.
        bh = ah
        while True:
            sh = (bh - gap * (side - 1)) / side
            tw = bh * ar + gap + sh * ar
            if tw <= aw:
                break
            bh -= 2
        bw, sw = bh * ar, sh * ar
        x0 = ax + (aw - (bw + gap + sw)) / 2
        y0 = ay + (ah - bh) / 2
        rects = [(x0, y0, bw, bh)]
        for i in range(side):
            rects.append((x0 + bw + gap, y0 + i * (sh + gap), sw, sh))
    else:
        cols = scene.get("cols", min(n, 3))
        rows = math.ceil(n / cols)
        tw = (aw - gap * (cols - 1)) / cols
        th = tw / ar
        if th * rows + gap * (rows - 1) > ah:
            th = (ah - gap * (rows - 1)) / rows
            tw = th * ar
        gw, gh = tw * cols + gap * (cols - 1), th * rows + gap * (rows - 1)
        x0, y0 = ax + (aw - gw) / 2, ay + (ah - gh) / 2
        rects = []
        for i in range(n):
            r, c = divmod(i, cols)
            in_row = min(cols, n - r * cols)
            xr = x0 + (gw - (tw * in_row + gap * (in_row - 1))) / 2
            rects.append((xr + c * (tw + gap), y0 + r * (th + gap), tw, th))
    return [tuple(int(round(v)) for v in r) for r in rects]


TICK_FRAMES = 735  # stereo frames of the capture's mix per 1/60 s tick (LEGAIA_CAPTURE_AUDIO_WAV)
RATE = 44100


def tile_audio(row, frames, work):
    """(wav path, the stereo frame the tile's first video frame starts at), or None."""
    if shot_kind(row) != "engine":
        return None
    wav = work / "shots" / row["name"] / "audio.wav"
    if not wav.exists() or not frames:
        return None
    tick = int(Path(frames[0]).stem.split("_")[1])
    return wav, tick * TICK_FRAMES


def audio_schedule(scene, shots, work, fps):
    """Which tile is heard when: the tiles with sound share the scene in turn.

    Returns [(tile index, start s, end s, appear s)] - the order is the
    scene's `audio` list (tile indexes) or every engine tile in order."""
    tiles = scene.get("tiles", [])
    if scene.get("kind") == "card":
        return []
    order = scene.get("audio")
    if order is None:
        order = [i for i, t in enumerate(tiles) if shot_kind(shots[t["shot"]]) == "engine"]
    if not order:
        return []
    stagger = scene.get("stagger", 0.14)
    start = 0.25 + stagger * min(len(tiles) - 1, 2)
    end = scene["seconds"]
    span = (end - start) / len(order)
    return [(i, start + k * span, start + (k + 1) * span, 0.25 + i * stagger) for k, i in enumerate(order)]


def highlight_at(schedule, n, t, fade=0.25):
    for i, a, b, _ in schedule:
        if i == n and a - fade <= t <= b + fade:
            return min(1.0, (t - (a - fade)) / fade, ((b + fade) - t) / fade)
    return 0.0


def speaker_badge(color, alpha):
    s = 44
    im = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    d.ellipse((0, 0, s - 1, s - 1), fill=(8, 12, 20, int(alpha * 0.85)))
    c = (*color, alpha)
    d.rectangle((11, 17, 17, 27), fill=c)
    d.polygon([(17, 17), (25, 10), (25, 34), (17, 27)], fill=c)
    d.arc((22, 13, 34, 31), -55, 55, fill=c, width=3)
    return im


class SceneRenderer:
    """Renders one scene's frame `i`; built once per worker process."""

    def __init__(self, scene, shots, work, style, fps):
        self.scene, self.style, self.fps = scene, style, fps
        self.bg = background(style)
        self.title_font = font(style["title_size"])
        self.sub_font = font(style["subtitle_size"], "Regular")
        self.label_font = font(style["label_size"], "Medium")
        self.kind = scene.get("kind", "grid")
        top = style["header_height"] if scene.get("title") and self.kind == "grid" else 60
        area = (70, top, W - 140, H - top - 60)
        self.rects = layout_rects(scene, area) if self.kind == "grid" else []
        self.tiles = []
        for t in scene["tiles"]:
            row = shots[t["shot"]]
            frames = frames_of(row, work / "shots" / row["name"])
            if not frames:
                sys.exit(f"teaser: {t['shot']}: no frames")
            skip = int(t.get("skip", 0) * fps)
            self.tiles.append((t, row, frames[skip:] or frames))
        self.schedule = audio_schedule(scene, shots, work, fps)
        self.crops = [shot_crop(row, work) or t.get("crop") for t, row, _ in self.tiles]

    def tile_frame(self, idx, k, size):
        t, row, frames = self.tiles[idx]
        speed = t.get("speed", 1.0)
        j = min(len(frames) - 1, max(0, int(k * speed)))
        return fit(Image.open(frames[j]), size, self.crops[idx], t.get("anchor", 0.5))

    def text(self, d, xy, s, f, fill, alpha):
        if alpha <= 0:
            return
        col = (*fill, int(255 * alpha))
        d.text(xy, s, font=f, fill=col)

    def render(self, i):
        sc, st, fps = self.scene, self.style, self.fps
        t = i / fps
        if self.kind == "card":
            return self.render_card(i)
        img = self.bg.copy()
        over = Image.new("RGBA", (W, H), (0, 0, 0, 0))
        d = ImageDraw.Draw(over)
        # Header: an accent bar, the title and the subtitle slide in.
        k = ease(t / 0.5)
        dx = int((1 - k) * -40)
        if sc.get("title"):
            d.rounded_rectangle((70 + dx, 52, 78 + dx, 150), 3, fill=(*hexrgb(st["gold"]), int(255 * k)))
            self.text(d, (100 + dx, 40), sc["title"], self.title_font, (255, 255, 255), k)
            if sc.get("subtitle"):
                k2 = ease((t - 0.15) / 0.5)
                self.text(d, (102 + int((1 - k2) * -40), 122), sc["subtitle"], self.sub_font, hexrgb(st["muted"]), k2)
        img.paste(over, (0, 0), over)
        stagger = sc.get("stagger", 0.14)
        for n, (x, y, w, h) in enumerate(self.rects):
            appear = 0.25 + n * stagger
            if t < appear:
                continue
            a = ease((t - appear) / 0.45)
            fi = int((t - appear) * fps)
            tile = self.tile_frame(n, fi, (w, h))
            yy = y + int((1 - a) * 36)
            # Shadow and border.
            sh = Image.new("RGBA", (w + 60, h + 60), (0, 0, 0, 0))
            ImageDraw.Draw(sh).rounded_rectangle((30, 36, w + 30, h + 36), 16, fill=(0, 0, 0, int(150 * a)))
            sh = sh.filter(ImageFilter.GaussianBlur(14))
            img.paste(sh, (x - 30, yy - 30), sh)
            mask = rounded_mask((w, h), 14)
            if a < 1:
                mask = mask.point(lambda v, a=a: int(v * a))
            img.paste(tile, (x, yy), mask)
            bd = Image.new("RGBA", (w, h), (0, 0, 0, 0))
            ImageDraw.Draw(bd).rounded_rectangle((0, 0, w - 1, h - 1), 14, outline=(*hexrgb(st["border"]), int(255 * a)), width=2)
            img.paste(bd, (x, yy), bd)
            hl = highlight_at(self.schedule, n, t)
            if hl > 0:
                gold = hexrgb(st["gold"])
                ring = Image.new("RGBA", (w + 12, h + 12), (0, 0, 0, 0))
                ImageDraw.Draw(ring).rounded_rectangle((0, 0, w + 11, h + 11), 19, outline=(*gold, int(255 * hl * a)), width=5)
                img.paste(ring, (x - 6, yy - 6), ring)
                badge = speaker_badge(gold, int(255 * hl * a))
                img.paste(badge, (x + w - badge.size[0] - 14, yy + 14), badge)
            label = self.tiles[n][0].get("label")
            if label:
                la = ease((t - appear - 0.25) / 0.4)
                lf = self.label_font
                tw = int(d.textlength(label, font=lf))
                pill = Image.new("RGBA", (tw + 28, st["label_size"] + 20), (0, 0, 0, 0))
                pd = ImageDraw.Draw(pill)
                pd.rounded_rectangle((0, 0, tw + 27, st["label_size"] + 19), 10, fill=(8, 12, 20, int(205 * la)))
                pd.text((14, 7), label, font=lf, fill=(255, 255, 255, int(255 * la)))
                img.paste(pill, (x + 14, yy + h - pill.size[1] - 14), pill)
        return img

    def render_card(self, i):
        """A full-bleed mosaic of the shots, dimmed and blurred, with centred lines of text."""
        sc, st, fps = self.scene, self.style, self.fps
        t = i / fps
        cols = sc.get("cols", 3)
        n = len(self.tiles)
        rows = math.ceil(n / cols)
        cw, ch = math.ceil(W / cols), math.ceil(H / rows)
        mos = Image.new("RGB", (W, H))
        for idx in range(n):
            r, c = divmod(idx, cols)
            mos.paste(self.tile_frame(idx, i, (cw, ch)), (c * cw, r * ch))
        mos = mos.filter(ImageFilter.GaussianBlur(sc.get("blur", 6)))
        dim = sc.get("dim", 0.62)
        fade_in = ease(t / 0.6) if sc.get("fade_in", True) else 1
        img = Image.blend(mos, Image.new("RGB", (W, H), hexrgb(st["bg_top"])), dim + (1 - dim) * (1 - fade_in))
        over = Image.new("RGBA", (W, H), (0, 0, 0, 0))
        d = ImageDraw.Draw(over)
        lines = sc["lines"]
        heights = [ln.get("size", 48) * 1.35 for ln in lines]
        y = (H - sum(heights)) / 2
        for n_ln, (ln, lh) in enumerate(zip(lines, heights)):
            f = font(ln.get("size", 48), ln.get("weight", "Bold"))
            k = ease((t - 0.3 - n_ln * 0.22) / 0.6)
            tw = d.textlength(ln["text"], font=f)
            col = hexrgb(st.get(ln.get("color", "white"), "#ffffff")) if ln.get("color") else (255, 255, 255)
            d.text(((W - tw) / 2, y + (1 - k) * 24), ln["text"], font=f, fill=(*col, int(255 * k)))
            y += lh
        img.paste(over, (0, 0), over)
        return img


_R = {}


def _render_job(args):
    key, scene, shots, work, style, fps, i, dest = args
    r = _R.get(key)
    if r is None:
        r = _R[key] = SceneRenderer(scene, shots, Path(work), style, fps)
    r.render(i).save(dest, quality=94)
    return dest


def compose(scenes, shots, work, style, fps, jobs, only_scenes=None):
    """Renders every scene's frames, then cross-fades scene into scene."""
    out_dirs = []
    for n, sc in enumerate(scenes):
        d = work / "scenes" / f"{n:02d}"
        out_dirs.append(d)
        if only_scenes is not None and n not in only_scenes:
            continue
        shutil.rmtree(d, ignore_errors=True)
        d.mkdir(parents=True)
        total = int(sc["seconds"] * fps)
        argv = [(n, sc, shots, str(work), style, fps, i, str(d / f"{i:05d}.jpg")) for i in range(total)]
        with ProcessPoolExecutor(jobs) as ex:
            list(ex.map(_render_job, argv, chunksize=8))
        log(f"scene {n} '{sc.get('title') or sc.get('kind')}': {total} frames")
    seq = work / "sequence"
    shutil.rmtree(seq, ignore_errors=True)
    seq.mkdir()
    xf = style["crossfade_frames"]
    k = 0
    tail = []
    for d in out_dirs:
        frames = sorted(d.glob("*.jpg"))
        if tail:
            for j in range(len(tail)):
                a = (j + 1) / (len(tail) + 1)
                Image.blend(Image.open(tail[j]), Image.open(frames[j]), a).save(seq / f"{k:05d}.jpg", quality=94)
                k += 1
            frames = frames[len(tail):]
        body, tail = (frames[:-xf], frames[-xf:]) if d != out_dirs[-1] else (frames, [])
        for f in body:
            os.link(f, seq / f"{k:05d}.jpg")
            k += 1
    log(f"sequence: {k} frames, {k / fps:.1f} s")
    return seq, k


def read_wav(path):
    import numpy as np
    import wave
    with wave.open(str(path)) as w:
        data = np.frombuffer(w.readframes(w.getnframes()), dtype="<i2").astype(np.float32) / 32768
    return data.reshape(-1, 2)


def soundtrack(scenes, shots, work, fps, style, total_frames, dest):
    """The teaser's sound: each scene's heard tile in turn, its own mix at the
    same instant as its picture, or the scene's `bed` (a shot's mix from a
    point on) under a scene with no game tile. Scenes overlap by the
    cross-fade, as the pictures do."""
    import numpy as np
    import wave
    n_total = int(total_frames / fps * RATE) + RATE
    mix = np.zeros((n_total, 2), np.float32)
    cache = {}

    def wav(p):
        if p not in cache:
            cache[p] = read_wav(p)
        return cache[p]

    def place(buf, at, gain, fade_s=0.12):
        if buf.size == 0:
            return
        f = min(len(buf) // 2, int(fade_s * RATE))
        env = np.ones(len(buf), np.float32)
        if f:
            env[:f] = np.linspace(0, 1, f)
            env[-f:] = np.linspace(1, 0, f)
        seg = buf * (env[:, None] * gain)
        end = min(n_total, at + len(seg))
        mix[at:end] += seg[: end - at]

    def level(buf):
        rms = float(np.sqrt(np.mean(buf ** 2))) if buf.size else 0
        return rms

    xf = style["crossfade_frames"]
    target = style.get("audio_rms", 0.12)
    scene_start = 0
    for n, sc in enumerate(scenes):
        frames_in = int(sc["seconds"] * fps)
        t0 = scene_start / fps
        sched = audio_schedule(sc, shots, work, fps)
        tiles = sc.get("tiles", [])
        for i, a, b, appear in sched:
            t = tiles[i]
            row = shots[t["shot"]]
            fr = frames_of(row, work / "shots" / row["name"])
            skip = int(t.get("skip", 0) * fps)
            info = tile_audio(row, fr[skip:] or fr, work)
            if not info:
                continue
            path, origin = info
            data = wav(path)
            lead = 0.12
            s0 = origin + int(((a - lead) - appear) * t.get("speed", 1.0) * RATE)
            s1 = origin + int(((b + lead) - appear) * t.get("speed", 1.0) * RATE)
            buf = data[max(0, s0):max(0, s1)]
            rms = level(buf)
            gain = min(4.0, target / rms) if rms > 1e-4 else 1.0
            place(buf, int((t0 + a - lead) * RATE), gain * t.get("gain", 1.0))
        bed = sc.get("bed")
        if bed and not sched:
            row = shots[bed["shot"]]
            fr = frames_of(row, work / "shots" / row["name"])
            info = tile_audio(row, fr, work)
            if info:
                path, origin = info
                data = wav(path)
                s0 = origin + int(bed.get("skip", 0) * RATE)
                buf = data[s0:s0 + int((sc["seconds"] + 0.4) * RATE)]
                rms = level(buf)
                gain = (min(4.0, target / rms) if rms > 1e-4 else 1.0) * bed.get("gain", 0.6)
                place(buf, int(t0 * RATE), gain, fade_s=0.5)
        scene_start += frames_in - (xf if n < len(scenes) - 1 else 0)
    mix = mix[: int(total_frames / fps * RATE)]
    peak = float(np.max(np.abs(mix))) if mix.size else 0
    if peak > 0.98:
        mix *= 0.98 / peak
    with wave.open(str(dest), "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes((mix * 32767).astype("<i2").tobytes())
    log(f"soundtrack {dest}: {len(mix) / RATE:.1f} s")
    return dest


def encode(seq, fps, out, v):
    out.parent.mkdir(parents=True, exist_ok=True)
    cmd = ["ffmpeg", "-loglevel", "error", "-y", "-framerate", str(fps), "-i", str(seq / "%05d.jpg")]
    music = v.get("audio")
    if music:
        cmd += ["-i", music]
    cmd += ["-c:v", "libx264", "-preset", "slow", "-crf", str(v.get("crf", 17)), "-pix_fmt", "yuv420p",
            "-profile:v", "high", "-movflags", "+faststart"]
    if music:
        n = len(list(seq.glob("*.jpg")))
        dur = n / fps
        cmd += ["-c:a", "aac", "-b:a", "192k", "-ar", "48000",
                "-af", f"loudnorm=I={v.get('loudness', -16)}:TP=-1.5:LRA=11,afade=t=out:st={dur - 2:.2f}:d=2",
                "-shortest"]
    cmd.append(str(out))
    site_media.run(cmd)
    log(f"{out}: {out.stat().st_size / 1e6:.1f} MB")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--manifest", type=Path, default=HERE / "teaser.toml")
    ap.add_argument("--work", type=Path, default=REPO / "captures/teaser")
    ap.add_argument("--out", type=Path, help="the video (default <work>/teaser.mp4)")
    ap.add_argument("--only", help="comma-separated shot names to (re)shoot")
    ap.add_argument("--force", action="store_true", help="re-shoot every shot")
    ap.add_argument("--no-shoot", action="store_true", help="use cached shots only")
    ap.add_argument("--no-compose", action="store_true")
    ap.add_argument("--scenes", help="comma-separated scene indexes to re-render (the others reuse their frames)")
    ap.add_argument("--sheet", action="store_true", help="a preview sheet per shot shot and per scene")
    ap.add_argument("--silent", action="store_true", help="no soundtrack")
    ap.add_argument("--jobs", type=int, default=max(1, (os.cpu_count() or 4) - 2))
    ap.add_argument("--build", action="store_true", help="cargo build the engine, mdec, Explorer and the asset viewer")
    a = ap.parse_args()
    work = a.work.resolve()
    (work / "shots").mkdir(parents=True, exist_ok=True)

    m = tomllib.loads(a.manifest.read_text())
    defaults, style = m["defaults"], m["style"]
    fps = defaults["fps"]
    shots = {r["name"]: r for r in m["shot"]}
    scenes = m["scene"]
    used = {t["shot"] for sc in scenes for t in sc["tiles"]}
    missing = used - shots.keys()
    if missing:
        sys.exit(f"teaser: scenes name unknown shots: {sorted(missing)}")
    only = set(a.only.split(",")) if a.only else None

    if a.build:
        site_media.run(["cargo", "build", "--release", "-p", "legaia-engine-shell", "--bin", "legaia-engine",
                        "-p", "psx-mdec", "--bin", "mdec", "-p", "legaia-explorer", "--bin", "legaia-explorer",
                        "-p", "legaia-asset-viewer", "--bin", "asset-viewer"], cwd=REPO)

    todo = []
    for name in sorted(used, key=lambda n: list(shots).index(n)):
        row = shots[name]
        out = work / "shots" / name
        key = shot_key(row)
        cached = (out / ".key").exists() and (out / ".key").read_text() == key
        if a.no_shoot or (only and name not in only) or (cached and not a.force and not only):
            if not out.exists() and not only and not a.no_compose:
                sys.exit(f"teaser: {name}: no cached frames; drop --no-shoot")
            continue
        todo.append(row)

    if todo:
        pages = [r for r in todo if shot_kind(r) == "page"]
        others = [r for r in todo if shot_kind(r) != "page"]
        with Xvfb() as x:
            for row in others:
                out = work / "shots" / row["name"]
                shutil.rmtree(out, ignore_errors=True)
                out.mkdir(parents=True)
                log(f"shoot {row['name']} ({shot_kind(row)})")
                {"engine": shoot_engine, "str": shoot_str, "app": shoot_app}[shot_kind(row)](row, out, defaults, x.display)
                (out / ".key").write_text(shot_key(row))
        if pages:
            try:
                shoot_pages(pages, work, defaults)
            finally:
                for row in pages:
                    d = work / "shots" / row["name"]
                    if d.exists() and any(d.glob("f_*.jpg")):
                        deflicker(row, work)
                        (d / ".key").write_text(shot_key(row))
        if a.sheet:
            for row in todo:
                fr = frames_of(row, work / "shots" / row["name"])
                step = max(1, len(fr) // 24)
                site_media.sheet(fr[::step], work / f"sheet-{row['name']}.png", cols=6, cell=(320, 200))

    if a.no_compose:
        return
    only_scenes = {int(s) for s in a.scenes.split(",")} if a.scenes else None
    seq, n = compose(scenes, shots, work, style, fps, a.jobs, only_scenes)
    if a.sheet:
        site_media.sheet([seq / f"{i:05d}.jpg" for i in range(0, n, max(1, n // 48))],
                         work / "sheet-teaser.png", cols=8, cell=(320, 180))
    v = dict(m.get("video", {}))
    if not a.silent:
        v["audio"] = str(soundtrack(scenes, shots, work, fps, style, n, work / "soundtrack.wav"))
    encode(seq, fps, a.out or work / "teaser.mp4", v)


if __name__ == "__main__":
    main()
