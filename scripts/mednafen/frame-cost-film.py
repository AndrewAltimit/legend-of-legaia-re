#!/usr/bin/env python3
"""Step a mednafen library state frame by frame and log what each frame cost.

One CSV row per emulated frame: libetc's VSync counter, the main loop's frame counter
`gp+0x480`, the action state `ctx[+0x07]`, the module phase `ctx[+0x279]`, the party
size `ctx[+0x00]`, the frame step `0x1F800393`, the frame-cost words `gp+0x3A8` (the
hblanks to the CPU's end of frame) and `gp+0x3AC` (after `DrawSync`, the ring's entry),
the ring's newest entry, the GPU draw-time model (`gpu_cost.py`) of the table the GPU
was handed last (`*_drawn`) and of the one the CPU builds (`*_cur`), in hblanks, and the
CPU's PC and `ra` at the frame boundary. Numbers only: it changes nothing and gates
nothing. Steps the emulator as `drive-timeline.py` does (advance-frame and save-state
keys through xdotool on a private Xvfb).

    scripts/mednafen/frame-cost-film.py zenoir_summon_mid_cast --frames 500 --out zenoir.csv

`scripts/ci/frame-cost-agreement.py` reads the CSV. See docs/subsystems/battle-stage-camera.md,
"What a real console's frame costs".
"""
import argparse, gzip, os, shutil, struct, subprocess, sys, tempfile, time, tomllib, csv
from pathlib import Path
REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(Path(__file__).resolve().parent))
from gpu_cost import ot_cost, HBLANK_CPU  # noqa: E402
import importlib.util  # noqa: E402
spec = importlib.util.spec_from_file_location("audio_pcm", REPO / "scripts" / "ci" / "audio-pcm.py")
audio_pcm = importlib.util.module_from_spec(spec); sys.modules["audio_pcm"] = audio_pcm; spec.loader.exec_module(audio_pcm)
u32 = lambda r, a: struct.unpack_from('<I', r, a & 0x1FFFFF)[0]

def row(path):
    raw = path.read_bytes()
    try: b = gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw
    except Exception: return None
    n = b'MainRAM.data8'; i = b.find(bytes([len(n)]) + n)
    if i < 0: return None
    ram = b[i + 1 + len(n) + 4:][:0x200000]
    if len(ram) != 0x200000: return None
    sp = b.find(b'ScratchRAM.data8'); scr = b[sp + 20: sp + 20 + 0x400]
    ctx = u32(ram, 0x8007BD24)
    otlen = struct.unpack_from('<H', scr, 0x3A6)[0]; idx = struct.unpack_from('<h', ram, 0x7B74C)[0] & 1
    g = []
    for r in (idx ^ 1, idx):
        base = u32(ram, 0x8007BF30 + r * 0x74 + 0x70)
        gpu, dma, st = ot_cost(ram, base + otlen * 4 - 4, limit=40000) if otlen else (0, 0, {'poly': 0})
        g += [round(gpu / HBLANK_CPU, 1), round(dma / HBLANK_CPU, 1), st['poly']]
    cur = u32(ram, 0x8007B758) & 0xF
    newest = struct.unpack_from('<h', ram, 0x84098 + 2 * ((cur - 1) & 15))[0]
    cx = ctx & 0x1FFFFF
    pi = b.find(bytes([9]) + b'BACKED_PC'); pc = struct.unpack_from('<I', b, pi + 10 + 4)[0] if pi >= 0 else 0
    gi = b.find(bytes([3]) + b'GPR'); ra = struct.unpack_from('<I', b, gi + 4 + 4 + 31 * 4)[0] if gi >= 0 else 0
    return dict(vsync=u32(ram, 0x8007A894), frame=u32(ram, 0x8007B798), st7=ram[cx + 7], phase=ram[cx + 0x279],
                party=ram[cx], step=scr[0x393], cpu=u32(ram, 0x8007B6C0), tot=u32(ram, 0x8007B6C4), newest=newest,
                gpu_drawn=g[0], dma_drawn=g[1], polys_drawn=g[2], gpu_cur=g[3], dma_cur=g[4], polys_cur=g[5], pc=hex(pc), ra=hex(ra))

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('state'); ap.add_argument('--frames', type=int, default=120)
    ap.add_argument('--out', required=True); ap.add_argument('--display', type=int, default=389)
    ap.add_argument('--settle', type=float, default=0.15)
    a = ap.parse_args()
    disc = Path(os.environ["LEGAIA_DISC_BIN"])
    manifest = tomllib.loads((REPO / "scripts" / "scenarios.toml").read_text())
    state = audio_pcm.state_file(a.state, REPO / "saves" / "library", manifest)
    if state is None: print('no state', a.state); return 1
    rows = []
    with tempfile.TemporaryDirectory(prefix="frame-cost-film-") as t:
        tmp = Path(t); stage, home = tmp / "disc", tmp / "home"
        (home / "mcs").mkdir(parents=True); (home / "firmware").mkdir(); stage.mkdir()
        (stage / disc.name).symlink_to(disc)
        shutil.copy(disc.with_suffix(".cue"), stage / disc.with_suffix(".cue").name)
        fw = Path.home() / ".mednafen" / "firmware" / "scph5501.bin"
        shutil.copy(fw, home / "firmware" / fw.name)
        shutil.copy(state, home / "mcs" / "state.mca")
        r0 = row(state); r0['src'] = 'state'; rows.append(r0)
        env = dict(os.environ, MEDNAFEN_HOME=str(home), SDL_AUDIODRIVER="dummy", MEDNAFEN_ALLOWMULTI="1", DISPLAY=f":{a.display}")
        server = subprocess.Popen(["Xvfb", f":{a.display}", "-screen", "0", "800x600x24"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        time.sleep(1)
        emu = subprocess.Popen(["mednafen", "-autosave", "1", "-filesys.fname_state", "state.%X", "-sound.driver", "sdl",
                                "-video.driver", "softfb", "-video.fs", "0", "-psx.input.port1", "gamepad",
                                str(stage / disc.with_suffix(".cue").name)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        time.sleep(a.settle)
        key = lambda k: subprocess.run(["xdotool", "key", k], env=env, capture_output=True)
        key("Pause"); time.sleep(0.2)
        saved, seen = home / "mcs" / "state.mc0", None
        try:
            for _ in range(a.frames):
                key("alt+a"); time.sleep(0.03); key("F5")
                for _ in range(100):
                    time.sleep(0.03)
                    if saved.exists() and saved.stat().st_mtime_ns != seen:
                        time.sleep(0.03)
                        r = row(saved)
                        if r: seen = saved.stat().st_mtime_ns; r['src'] = 'adv'; rows.append(r); break
        finally:
            emu.terminate(); time.sleep(0.5); emu.kill(); server.terminate()
    with open(a.out, 'w', newline='') as f:
        w = csv.DictWriter(f, fieldnames=list(rows[0].keys())); w.writeheader(); w.writerows(rows)
    print(a.state, len(rows), '->', a.out)

if __name__ == '__main__':
    sys.exit(main())
