#!/usr/bin/env python3
"""Site media: the hero montage and the game-screenshot tiles, shot from the port.

Every picture comes from `legaia-engine play-window` run headless-style
(`--screenshot-every` into a scratch directory, the window's chrome left out)
in the default enhanced mode a player sees, or from the port's MDEC decoder
(`mdec decode-str`) for a movie. Clips and tiles are listed in `media.toml`
beside this file; edit a row and re-run to re-shoot it.

    scripts/site-media/site-media.py                      # shoot what changed, assemble, write
    scripts/site-media/site-media.py --only gimard --sheet   # re-shoot one clip, preview sheet
    scripts/site-media/site-media.py --no-shoot           # re-assemble from cached frames
    scripts/site-media/site-media.py --build              # cargo build the two binaries first

Output: `site/assets/hero-montage.{webm,mp4,webp}`, `site/img/tiles/hero.jpg`
and every `[[tile]]` file. Frames and previews go under `--work` (default
`captures/site-media/`, gitignored: they are rendered from the disc).
Needs `LEGAIA_DISC_BIN` or an `extracted/` tree, ffmpeg (libvpx-vp9, libx264,
libwebp_anim) and Pillow. Each clip's frames are cached under a hash of its
row and the binary, so an unchanged clip is not re-shot.
"""
import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path

from PIL import Image, ImageDraw

REPO = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
ENGINE = REPO / "target/release/legaia-engine"
MDEC = REPO / "target/release/mdec"
W, H = 960, 720


def log(msg):
    print(f"site-media: {msg}", file=sys.stderr, flush=True)


def run(cmd, **kw):
    log(" ".join(str(c) for c in cmd) if len(cmd) < 40 else f"{cmd[0]} ...")
    subprocess.run(cmd, check=True, **kw)


def clip_key(row, binary):
    """What decides a clip's frames: the run's own fields and the binary."""
    st = binary.stat() if binary.exists() else None
    run_fields = {k: v for k, v in row.items() if k not in ("from", "caption", "cut", "file", "size", "tick", "quality")}
    blob = json.dumps(run_fields, sort_keys=True) + (f"{st.st_size}:{st.st_mtime_ns}" if st else "")
    return hashlib.sha256(blob.encode()).hexdigest()[:16]


def extracted_root():
    root = REPO / "extracted"
    if not (root / "PROT.DAT").exists():
        sys.exit("site-media: extracted/PROT.DAT missing; run legaia-extract first")
    return root


def scope(cmd):
    """Engine runs go under a memory-capped user scope where systemd-run exists."""
    if shutil.which("systemd-run"):
        return ["systemd-run", "--user", "--scope", "-q", "-p", "MemoryMax=24G", *cmd]
    return cmd


def shoot_engine(row, out, defaults):
    """One play-window run, a PNG every `every` ticks up to `to`."""
    work = out.parent / (out.name + ".run")
    shutil.rmtree(work, ignore_errors=True)
    work.mkdir(parents=True)
    # An empty options file gives the window its defaults (the enhanced port),
    # whatever a player's own options file says.
    (work / "legaia-options.toml").write_text("")
    every = row.get("every", defaults["every"])
    cmd = [str(ENGINE), "play-window", "--no-audio", "--extracted-root", str(extracted_root())]
    if row.get("scene"):
        cmd += ["--scene", row["scene"]]
    cmd += row.get("args", [])
    if row.get("pad"):
        cmd += ["--pad-script", row["pad"]]
    if row.get("keys"):
        cmd += ["--key-script", row["keys"]]
    cmd += ["--screenshot-every", str(every), "--screenshot-dir", str(out),
            "--screenshot-last-tick", str(row["to"])]
    env = dict(os.environ, XMODIFIERS="@im=none", **row.get("env", {}))
    timeout = max(120, row["to"] // 20 + 120)
    with open(work / "engine.log", "w") as logf:
        run(["timeout", "--kill-after=10s", f"{timeout}s", *scope(cmd)], cwd=work, env=env,
            stdout=logf, stderr=subprocess.STDOUT)
    shots = sorted(out.glob("tick_*.png"))
    if not shots:
        sys.exit(f"site-media: {row['name']}: no frames (see {work / 'engine.log'})")


def shoot_str(row, out, defaults):
    """A movie segment through the port's MDEC decoder, resampled to the montage rate."""
    raw = out.parent / (out.name + ".str")
    shutil.rmtree(raw, ignore_errors=True)
    raw.mkdir(parents=True)
    src = extracted_root() / row["str"]
    run([str(MDEC), "decode-str", str(src), "-o", str(raw),
         "--start-frame", str(row.get("start_frame", 0)), "--end-frame", str(row.get("end_frame", 0))])
    frames = sorted(raw.glob("*.png"))
    if not frames:
        sys.exit(f"site-media: {row['name']}: decode-str wrote no frames")
    # The movie runs at its own rate (15 fps); name each frame by the montage
    # tick it lands on so the selection below reads both kinds alike.
    fps = row.get("fps", 15)
    tick = 0
    while (src_i := int(tick / 100 * fps)) < len(frames) and tick <= row["to"]:
        shutil.copy(frames[src_i], out / f"tick_{tick:05d}.png")
        tick += defaults["every"]
    shutil.rmtree(raw, ignore_errors=True)


def clip_frames(row, out):
    lo, hi = row.get("from", 0), row["to"]
    shots = sorted(out.glob("tick_*.png"), key=lambda p: int(p.stem[5:]))
    return [p for p in shots if lo <= int(p.stem[5:]) <= hi]


def frame_at(out, tick):
    shots = sorted(out.glob("tick_*.png"), key=lambda p: int(p.stem[5:]))
    return min(shots, key=lambda p: abs(int(p.stem[5:]) - tick))


def fit(img, size):
    """Crop to the target aspect around the centre, then resize."""
    w, h = size
    img = img.convert("RGB")
    sw, sh = img.size
    if sw * h > sh * w:
        nw = sh * w // h
        img = img.crop(((sw - nw) // 2, 0, (sw - nw) // 2 + nw, sh))
    elif sw * h < sh * w:
        nh = sw * h // w
        img = img.crop((0, (sh - nh) // 2, sw, (sh - nh) // 2 + nh))
    return img.resize((w, h), Image.LANCZOS)


def sheet(paths, dest, cols=8, cell=(240, 180), labels=None):
    rows = (len(paths) + cols - 1) // cols
    im = Image.new("RGB", (cols * cell[0], rows * (cell[1] + 14)), "black")
    d = ImageDraw.Draw(im)
    for i, p in enumerate(paths):
        x, y = (i % cols) * cell[0], (i // cols) * (cell[1] + 14)
        im.paste(fit(Image.open(p), cell), (x, y))
        d.text((x + 2, y + cell[1] + 1), labels[i] if labels else Path(p).stem, fill="white")
    im.save(dest)
    log(f"sheet {dest}")


def assemble(clips, work, defaults, outputs):
    """Cross-fade the clips into one frame sequence, then encode the three files."""
    seq = work / "montage"
    shutil.rmtree(seq, ignore_errors=True)
    seq.mkdir(parents=True)
    fade = defaults["crossfade_frames"]
    frames = []
    for row in clips:
        imgs = [fit(Image.open(p), (W, H)) for p in clip_frames(row, work / "clips" / row["name"])]
        if not imgs:
            sys.exit(f"site-media: {row['name']}: no frames in its from..to window")
        cf = 0 if row.get("cut") else fade
        if frames and cf:
            n = min(cf, len(imgs), len(frames))
            tail = frames[-n:]
            frames = frames[:-n]
            for i in range(n):
                a = (i + 1) / (n + 1)
                frames.append(Image.blend(tail[i], imgs[i], a))
            imgs = imgs[n:]
        frames.extend(imgs)
        log(f"{row['name']}: {len(imgs)} frames")
    for i, f in enumerate(frames):
        f.save(seq / f"{i:05d}.png")
    fps = defaults["fps"]
    log(f"montage: {len(frames)} frames, {len(frames) / fps:.1f} s")
    pattern = str(seq / "%05d.png")
    v = defaults["video"]
    run(["ffmpeg", "-loglevel", "error", "-y", "-framerate", str(fps), "-i", pattern,
         "-c:v", "libvpx-vp9", "-b:v", v["webm_bitrate"], "-pass", "1", "-row-mt", "1",
         "-pix_fmt", "yuv420p", "-an", "-f", "null", "-passlogfile", str(work / "vp9"), "/dev/null"])
    run(["ffmpeg", "-loglevel", "error", "-y", "-framerate", str(fps), "-i", pattern,
         "-c:v", "libvpx-vp9", "-b:v", v["webm_bitrate"], "-pass", "2", "-row-mt", "1",
         "-pix_fmt", "yuv420p", "-an", "-passlogfile", str(work / "vp9"), str(outputs["webm"])])
    for p in (1, 2):
        run(["ffmpeg", "-loglevel", "error", "-y", "-framerate", str(fps), "-i", pattern,
             "-c:v", "libx264", "-preset", "slow", "-b:v", v["mp4_bitrate"], "-pass", str(p),
             "-passlogfile", str(work / "x264"), "-pix_fmt", "yuv420p", "-profile:v", "high",
             "-movflags", "+faststart", "-an",
             *(["-f", "null", "/dev/null"] if p == 1 else [str(outputs["mp4"])])])
    wp = defaults["webp"]
    scale = f"scale={wp['width']}:{wp['height']}:flags=lanczos"
    for q in wp["quality"]:
        run(["ffmpeg", "-loglevel", "error", "-y", "-framerate", str(fps), "-i", pattern,
             "-vf", f"fps={wp['fps']},{scale}", "-c:v", "libwebp_anim", "-lossless", "0",
             "-q:v", str(q), "-compression_level", "6", "-loop", "0", "-an", str(outputs["webp"])])
        size = outputs["webp"].stat().st_size
        log(f"webp q{q}: {size / 1e6:.2f} MB")
        if size <= wp["max_bytes"]:
            break
    for k in ("webm", "mp4", "webp"):
        log(f"{outputs[k]}: {outputs[k].stat().st_size / 1e6:.2f} MB")
    return frames


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--manifest", type=Path, default=HERE / "media.toml")
    ap.add_argument("--work", type=Path, default=REPO / "captures/site-media")
    ap.add_argument("--only", help="comma-separated clip/tile names to (re)shoot")
    ap.add_argument("--force", action="store_true", help="re-shoot even when the cache matches")
    ap.add_argument("--no-shoot", action="store_true", help="use cached frames only")
    ap.add_argument("--no-assemble", action="store_true", help="skip the montage")
    ap.add_argument("--no-tiles", action="store_true", help="skip the tiles")
    ap.add_argument("--sheet", action="store_true", help="write a preview sheet per shot clip")
    ap.add_argument("--build", action="store_true", help="cargo build legaia-engine and mdec first")
    a = ap.parse_args()
    a.work = a.work.resolve()

    m = tomllib.loads(a.manifest.read_text())
    defaults = m["defaults"]
    clips = m["clip"]
    tiles = m.get("tile", [])
    only = set(a.only.split(",")) if a.only else None

    if a.build:
        run(["cargo", "build", "--release", "-p", "legaia-engine-shell", "--bin", "legaia-engine",
             "-p", "psx-mdec", "--bin", "mdec"], cwd=REPO)

    # Clips and the tiles that are runs of their own shoot alike.
    shots = clips + [t for t in tiles if "to" in t]
    for row in shots:
        name = row["name"]
        out = a.work / "clips" / name
        binary = MDEC if "str" in row else ENGINE
        key = clip_key(row, binary)
        stamp = out / ".key"
        cached = stamp.exists() and stamp.read_text() == key
        if a.no_shoot or (only and name not in only) or (cached and not a.force and not only):
            if not out.exists():
                sys.exit(f"site-media: {name}: no cached frames; drop --no-shoot")
            continue
        shutil.rmtree(out, ignore_errors=True)
        out.mkdir(parents=True)
        log(f"shoot {name}")
        (shoot_str if "str" in row else shoot_engine)(row, out, defaults)
        stamp.write_text(key)
        if a.sheet:
            fr = clip_frames(row, out)
            step = max(1, len(fr) // 48)
            sheet(fr[::step], a.work / f"sheet-{name}.png")

    site = REPO / "site"
    if not a.no_assemble and not only:
        outputs = {k: site / "assets" / f"hero-montage.{k}" for k in ("webm", "mp4", "webp")}
        frames = assemble(clips, a.work, defaults, outputs)
        poster = defaults.get("poster_frame", 0)
        frames[poster].save(site / "img/tiles/hero.jpg", quality=88, optimize=True)
        n = len(frames)
        sheet([a.work / "montage" / f"{i:05d}.png" for i in range(0, n, max(1, n // 30))],
              a.work / "sheet-montage.png", cols=6)

    if not a.no_tiles:
        written = []
        by_name = {r["name"]: r for r in shots}
        for t in tiles:
            if only and t["name"] not in only and t.get("clip") not in only:
                continue
            src_row = by_name[t.get("clip", t["name"])]
            src = frame_at(a.work / "clips" / src_row["name"], t["tick"])
            dest = site / "img/tiles" / t["file"]
            fit(Image.open(src), tuple(t["size"])).save(dest, quality=t.get("quality", 85), optimize=True)
            log(f"tile {dest.name} <- {src_row['name']} {src.name}")
            written.append(dest)
        if written:
            sheet(written, a.work / "sheet-tiles.png", cols=4, cell=(320, 240),
                  labels=[p.name for p in written])


if __name__ == "__main__":
    main()
