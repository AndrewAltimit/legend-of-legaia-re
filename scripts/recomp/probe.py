#!/usr/bin/env python3
"""Client library + CLI for the Legend of Legaia static-recomp TCP debug server.

The recomp runtime exposes a JSON-over-newline TCP debug protocol (one JSON
object per line, response on the same connection). This module wraps it with
the protocol traps baked in so probe scripts don't re-discover them:

  * ``press`` takes the key ``buttons`` (NOT ``value``; a wrong key is
    silently ignored) with a raw ACTIVE-LOW SIO pad word plus ``frames``.
    Short presses are silently dropped on some screens - default holds are
    >= 30 frames. ``press`` is NON-BLOCKING (it returns before the hold
    elapses); :meth:`RecompClient.press` waits it out by default.
  * ``savestate`` load acks ``{"ok":true}`` and then DROPS the TCP
    connection. :meth:`RecompClient.load_savestate` reconnects and verifies
    the state actually took (scene name @0x8007050C, game mode @0x8007B83C).
  * ``vram_peek`` clamps to <= 128 px wide per call - wider reads are
    chunked transparently.
  * ``dirty_exec_hot`` PCs are KSEG-masked physical - OR ``0x80000000``
    before resolving against overlay VAs (see :func:`hot_pc_to_va`).
  * ``pause`` / ``step`` / ``run_to_frame`` are REMOVED in current runtime
    builds (they return errors); frame-exact observation goes through the
    per-frame ring buffer instead (``set_snapshot`` + ``read_frame_ram`` -
    see ``trace_capture.py``).
  * ``screenshot`` WORKS on a ``--headless`` instance (no X server needed).
    Its ``display disabled`` error reports the GUEST GPU's display-enable
    bit, not the host - it means the game is mid-boot or between attract
    segments, so retry once a picture is up. Screenshot-guided menu
    navigation needs no ``xvfb-run``.
  * Never kill instances via ``pkill -f <pattern>`` (the pattern matches
    your own shell). ``launch`` records the PID; ``kill`` kills by PID.
  * Two instances on one port silently share it and fake responses -
    ``launch`` refuses to start when the port already answers.

Environment contract: ``LEGAIA_RECOMP_DIR`` points at the recomp workspace
(binary at ``build-dbg/Legend_of_Legaia_Recompiled``, ``game.toml`` at the
root); ``LEGAIA_RECOMP_BIOS`` points at a PSX BIOS image. Both overridable
per-invocation via CLI flags. ``LEGAIA_RECOMP_PORT`` sets the default port.
"""

from __future__ import annotations

import argparse
import json
import os
import signal
import socket
import subprocess
import sys
import time

DEFAULT_HOST = "127.0.0.1"
DEFAULT_PORT = int(os.environ.get("LEGAIA_RECOMP_PORT", "4370"))

# Retail globals used for savestate verification (pinned in
# docs/reference/memory-map.md).
SCENE_NAME_ADDR = 0x8007050C  # 8 bytes, NUL-padded ASCII
GAME_MODE_ADDR = 0x8007B83C  # u16, index into the 28-mode table

# Active-low SIO pad word bit positions (standard PSX digital pad).
_BUTTON_BITS = {
    "select": 0,
    "l3": 1,
    "r3": 2,
    "start": 3,  # -> 0xFFF7 (NOT 0xF7FF)
    "up": 4,
    "right": 5,
    "down": 6,
    "left": 7,
    "l2": 8,
    "r2": 9,
    "l1": 10,
    "r1": 11,
    "triangle": 12,
    "circle": 13,
    "cross": 14,
    "square": 15,
}

#: name -> raw active-low SIO word (the value ``press`` wants).
BUTTON_WORDS = {name: 0xFFFF & ~(1 << bit) for name, bit in _BUTTON_BITS.items()}
IDLE_PAD_WORD = 0xFFFF


def button_word(spec: str) -> int:
    """Resolve a button name (``cross``) or hex word (``0xBFFF``) to the raw
    active-low pad word."""
    s = spec.strip().lower()
    if s in BUTTON_WORDS:
        return BUTTON_WORDS[s]
    return int(s, 16)


def hot_pc_to_va(pc: int) -> int:
    """``dirty_exec_hot`` reports KSEG-masked physical PCs; map back to the
    KSEG0 virtual address the overlay maps use."""
    return pc | 0x80000000


class RecompError(RuntimeError):
    """Server returned ``ok: false``."""


class RecompClient:
    """JSON-over-newline client for one recomp debug server.

    The server closes the TCP connection after EVERY response (one
    request per connection, verified live - despite the protocol doc's
    "responses on same connection"), so each :meth:`call` opens a fresh
    connection. Localhost connect cost is negligible next to the frame
    cadence."""

    def __init__(
        self,
        host: str = DEFAULT_HOST,
        port: int = DEFAULT_PORT,
        timeout: float = 20.0,
    ):
        self.host = host
        self.port = port
        self.timeout = timeout
        self._id = 0

    # -- connection ------------------------------------------------------

    def connect(self) -> None:
        """Probe the server once (raises ``OSError`` when unreachable)."""
        s = socket.create_connection((self.host, self.port), timeout=self.timeout)
        s.close()

    def close(self) -> None:
        """No persistent state to drop (kept for call-site symmetry)."""

    def reconnect(self, retries: int = 20, delay: float = 0.5) -> None:
        """Wait for the server to answer (used after savestate load, or when
        waiting for a fresh instance's TCP to come up)."""
        last: Exception | None = None
        for _ in range(retries):
            try:
                self.connect()
                return
            except OSError as e:
                last = e
                time.sleep(delay)
        raise ConnectionError(f"could not reach {self.host}:{self.port}: {last}")

    # -- protocol core ---------------------------------------------------

    def call(self, cmd: str, **params) -> dict:
        """Send one command on a fresh connection, return the parsed
        response. Raises :class:`RecompError` on ``ok: false``; raises
        ``ConnectionError`` when the server is unreachable or drops the
        line mid-exchange (savestate load does this by design - catch it
        and :meth:`reconnect`)."""
        self._id += 1
        req = {"id": self._id, "cmd": cmd}
        req.update(params)
        try:
            s = socket.create_connection((self.host, self.port), timeout=self.timeout)
        except OSError as e:
            raise ConnectionError(f"could not reach {self.host}:{self.port}: {e}") from e
        try:
            s.settimeout(self.timeout)
            s.sendall((json.dumps(req) + "\n").encode())
            buf = b""
            while b"\n" not in buf:
                chunk = s.recv(1 << 16)
                if not chunk:
                    break
                buf += chunk
        except OSError as e:
            raise ConnectionError(f"connection lost during '{cmd}': {e}") from e
        finally:
            s.close()
        if b"\n" not in buf:
            raise ConnectionError(f"server closed connection during '{cmd}'")
        resp = json.loads(buf.decode())
        if not resp.get("ok", False):
            raise RecompError(f"{cmd}: {resp.get('error', resp)}")
        return resp

    # -- basics ----------------------------------------------------------

    def ping(self) -> dict:
        return self.call("ping")

    def frame(self) -> int:
        """Current frame number."""
        return int(self.call("frame")["frame"])

    def read_ram(self, addr: int | str, length: int) -> bytes:
        """Read ``length`` bytes from PS1 address space (single response,
        up to the full 2 MB)."""
        a = addr if isinstance(addr, str) else f"0x{addr:08X}"
        return bytes.fromhex(self.call("read_ram", addr=a, len=length)["hex"])

    def read_u16(self, addr: int) -> int:
        return int.from_bytes(self.read_ram(addr, 2), "little")

    def read_i16(self, addr: int) -> int:
        return int.from_bytes(self.read_ram(addr, 2), "little", signed=True)

    def read_u32(self, addr: int) -> int:
        return int.from_bytes(self.read_ram(addr, 4), "little")

    def read_i32(self, addr: int) -> int:
        return int.from_bytes(self.read_ram(addr, 4), "little", signed=True)

    def scene_name(self) -> str:
        """8-byte scene-name field, NULs mapped to spaces, trimmed (e.g.
        ``"jou ene"``)."""
        raw = self.read_ram(SCENE_NAME_ADDR, 8)
        return raw.replace(b"\x00", b" ").decode("ascii", "replace").strip()

    def game_mode(self) -> int:
        return self.read_u16(GAME_MODE_ADDR)

    # -- input -----------------------------------------------------------

    def press(self, buttons: int | str, frames: int = 30, wait: bool = True) -> dict:
        """Hold a raw active-low pad word for ``frames`` frames.

        ``press`` on the server is non-blocking; with ``wait=True`` (default)
        this method sleeps until the hold has elapsed so back-to-back calls
        don't overwrite each other. Use >= 30-frame holds for confirms.
        """
        word = button_word(buttons) if isinstance(buttons, str) else buttons
        resp = self.call("press", buttons=word, frames=frames)
        if wait:
            time.sleep(frames / 60.0 + 0.15)
        return resp

    def clear_input(self) -> dict:
        return self.call("clear_input")

    # -- savestates ------------------------------------------------------

    def load_savestate(
        self,
        slot: int,
        settle: float = 2.0,
        expect_scene: str | None = None,
        expect_mode: int | None = None,
    ) -> tuple[str, int]:
        """Load savestate ``slot``, ride out the connection drop, reconnect,
        and verify the state took. Returns ``(scene_name, game_mode)``.

        The server acks ``{"ok":true}`` and then drops the TCP connection
        (the load is STAGED - it executes at the next block boundary and
        unwinds the guest, killing every connection, possibly *after* a
        quick reconnect already succeeded); the verification reads retry
        through reconnects until the state settles. A load can also be
        silently wrong (stale slot), so the caller should pass
        ``expect_scene`` / ``expect_mode`` when known.
        """
        self.call("savestate", op="load", slot=slot)
        # The ack raced the unwind; drop our side and reconnect.
        self.close()
        time.sleep(settle)
        deadline = time.monotonic() + 30.0
        while True:
            try:
                self.reconnect()
                scene = self.scene_name()
                mode = self.game_mode()
                break
            except ConnectionError:
                # The staged load executed between our reconnect and the
                # read - reconnect again.
                if time.monotonic() > deadline:
                    raise
                time.sleep(0.5)
        if expect_scene is not None and scene != expect_scene:
            raise RecompError(
                f"savestate slot {slot}: scene {scene!r} != expected {expect_scene!r}"
            )
        if expect_mode is not None and mode != expect_mode:
            raise RecompError(
                f"savestate slot {slot}: mode 0x{mode:X} != expected 0x{expect_mode:X}"
            )
        return scene, mode

    def save_savestate(self, slot: int) -> dict:
        return self.call("savestate", op="save", slot=slot)

    # -- hot counters ----------------------------------------------------

    def hot(self, top: int = 64, clear: bool = False) -> dict:
        """``dirty_exec_hot`` snapshot. Entries carry KSEG-masked physical
        PCs; resolve via :func:`hot_pc_to_va`."""
        params: dict = {"top": top}
        if clear:
            params["clear"] = 1
        return self.call("dirty_exec_hot", **params)

    # -- VRAM ------------------------------------------------------------

    def vram_peek(self, x: int, y: int, w: int, h: int) -> list[list[int]]:
        """Read a 16-bit VRAM rect as rows of pixel ints. The server clamps
        each call to <= 128 px in BOTH dimensions, so wider/taller rects are
        chunked. The response's ``hex`` field is a flat row-major string of
        4-hex-digit pixel *values* (``%04x`` per pixel, not LE byte pairs)."""
        rows: list[list[int]] = [[] for _ in range(h)]
        cy = 0
        while cy < h:
            ch = min(128, h - cy)
            cx = 0
            row_chunks: list[list[int]] = [[] for _ in range(ch)]
            while cx < w:
                cw = min(128, w - cx)
                resp = self.call("vram_peek", x=x + cx, y=y + cy, w=cw, h=ch)
                s = resp["hex"]
                for ry in range(ch):
                    off = ry * cw * 4
                    row_chunks[ry].extend(
                        int(s[off + 4 * i : off + 4 * i + 4], 16) for i in range(cw)
                    )
                cx += cw
            for ry in range(ch):
                rows[cy + ry] = row_chunks[ry]
            cy += ch
        return rows

    # -- per-frame ring buffer --------------------------------------------

    def set_snapshot(self, slot: int, addr: int, size: int) -> dict:
        """Configure per-frame RAM snapshot region (4 slots, <= 128 bytes
        each). The ring records the region every frame; read back with
        :meth:`read_frame_ram`."""
        return self.call("set_snapshot", slot=slot, addr=f"0x{addr:08X}", size=size)

    def read_frame_ram(self, addr: int, length: int, frame: int) -> bytes:
        """Read RAM *as of a specific frame* from the ring buffer. Only
        addresses inside a configured snapshot region resolve."""
        resp = self.call("read_frame_ram", addr=f"0x{addr:08X}", len=length, frame=frame)
        return bytes.fromhex(resp["hex"])

    def history(self) -> dict:
        return self.call("history")


# -- instance management (launch / kill) ----------------------------------


def recomp_dir(override: str | None = None) -> str:
    d = override or os.environ.get("LEGAIA_RECOMP_DIR")
    if not d:
        raise SystemExit(
            "recomp workspace not configured: set LEGAIA_RECOMP_DIR or pass --recomp-dir"
        )
    return os.path.expanduser(d)


def port_answers(host: str, port: int) -> bool:
    try:
        c = RecompClient(host, port, timeout=3.0)
        c.connect()
        c.ping()
        c.close()
        return True
    except (OSError, RecompError, json.JSONDecodeError):
        return False


def launch_instance(
    port: int,
    cache_dir: str,
    recomp_root: str | None = None,
    bios: str | None = None,
    game: str | None = None,
    log_path: str | None = None,
) -> int:
    """Launch a headless recomp instance. Refuses if the port already
    answers (two instances silently share a port and fake responses).
    Returns the PID - record it and kill by PID, never by pattern."""
    if port_answers(DEFAULT_HOST, port):
        raise SystemExit(f"port {port} already answers - refusing to double-launch")
    root = recomp_dir(recomp_root)
    binary = os.path.join(root, "build-dbg", "Legend_of_Legaia_Recompiled")
    game_toml = game or os.path.join(root, "game.toml")
    bios_path = bios or os.environ.get("LEGAIA_RECOMP_BIOS")
    if not bios_path:
        raise SystemExit("BIOS not configured: set LEGAIA_RECOMP_BIOS or pass --bios")
    os.makedirs(cache_dir, exist_ok=True)
    env = dict(os.environ)
    env["PSX_OVERLAY_CACHE_DIR"] = cache_dir
    env.setdefault("PSX_STARVATION_TIMEOUT_US", "0")
    out = open(log_path, "ab") if log_path else subprocess.DEVNULL
    proc = subprocess.Popen(
        [
            binary,
            "--headless",
            "--debug-port",
            str(port),
            "--bios",
            os.path.expanduser(bios_path),
            "--game",
            game_toml,
        ],
        stdout=out,
        stderr=subprocess.STDOUT if log_path else subprocess.DEVNULL,
        env=env,
        # The runtime writes report artifacts (psx_live_snapshot.json, freeze
        # dumps) to its cwd - keep them in the recomp workspace, never in a
        # repo checkout (they carry game RAM).
        cwd=root,
        start_new_session=True,
    )
    return proc.pid


# -- CLI -------------------------------------------------------------------


def _fmt(obj) -> str:
    return json.dumps(obj, indent=1)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--host", default=DEFAULT_HOST)
    ap.add_argument("--port", type=int, default=DEFAULT_PORT)
    sub = ap.add_subparsers(dest="cmd", required=True)

    sub.add_parser("ping", help="heartbeat + frame number")

    p = sub.add_parser("read", help="read RAM bytes as hex")
    p.add_argument("addr", help="hex address, e.g. 0x8007050C")
    p.add_argument("len", nargs="?", type=int, default=16)

    p = sub.add_parser("press", help="hold a pad button (name or hex active-low word)")
    p.add_argument("button", help="cross/circle/start/up/... or a hex word like 0xBFFF")
    p.add_argument("frames", nargs="?", type=int, default=30)
    p.add_argument("--no-wait", action="store_true", help="don't sleep out the hold")

    p = sub.add_parser("load-state", help="load a savestate slot + reconnect + verify")
    p.add_argument("slot", type=int)
    p.add_argument("--expect-scene", help="fail unless the loaded scene matches")
    p.add_argument(
        "--expect-mode", help="fail unless the loaded game mode matches (hex ok)"
    )

    p = sub.add_parser("hot", help="dirty_exec_hot snapshot (PCs are KSEG-masked)")
    p.add_argument("--top", type=int, default=64)
    p.add_argument("--clear", action="store_true")
    p.add_argument("--out", help="write full JSON to file")

    p = sub.add_parser("vram-peek", help="read a VRAM rect (chunked past 128 px)")
    p.add_argument("x", type=int)
    p.add_argument("y", type=int)
    p.add_argument("w", type=int)
    p.add_argument("h", type=int)

    p = sub.add_parser("cmd", help="raw passthrough: JSON object without id")
    p.add_argument("json", help='e.g. \'{"cmd":"gpu_state"}\'')

    p = sub.add_parser("launch", help="launch a headless instance (refuses busy ports)")
    p.add_argument("--recomp-dir", help="recomp workspace (default $LEGAIA_RECOMP_DIR)")
    p.add_argument("--bios", help="PSX BIOS path (default $LEGAIA_RECOMP_BIOS)")
    p.add_argument("--game", help="game.toml path (default <recomp-dir>/game.toml)")
    p.add_argument("--cache-dir", required=True, help="PSX_OVERLAY_CACHE_DIR value")
    p.add_argument("--log", help="append stdout/stderr to this file")
    p.add_argument("--wait-tcp", action="store_true", help="block until TCP answers")

    p = sub.add_parser("kill", help="kill an instance by PID (never by pattern)")
    p.add_argument("pid", type=int)

    args = ap.parse_args(argv)

    if args.cmd == "launch":
        pid = launch_instance(
            args.port, args.cache_dir, args.recomp_dir, args.bios, args.game, args.log
        )
        print(f"PID={pid}")
        if args.wait_tcp:
            c = RecompClient(args.host, args.port)
            c.reconnect(retries=60, delay=2.0)
            print(_fmt(c.ping()))
            c.close()
        return 0

    if args.cmd == "kill":
        os.kill(args.pid, signal.SIGTERM)
        print(f"sent SIGTERM to {args.pid}")
        return 0

    c = RecompClient(args.host, args.port)
    try:
        if args.cmd == "ping":
            r = c.ping()
            r["frame"] = c.frame()
            print(_fmt(r))
        elif args.cmd == "read":
            print(c.read_ram(args.addr, args.len).hex())
        elif args.cmd == "press":
            print(_fmt(c.press(args.button, args.frames, wait=not args.no_wait)))
        elif args.cmd == "load-state":
            expect_mode = int(args.expect_mode, 0) if args.expect_mode else None
            scene, mode = c.load_savestate(
                args.slot, expect_scene=args.expect_scene, expect_mode=expect_mode
            )
            print(f"loaded slot {args.slot}: scene={scene!r} mode=0x{mode:X}")
        elif args.cmd == "hot":
            r = c.hot(top=args.top, clear=args.clear)
            if args.out:
                with open(args.out, "w") as f:
                    json.dump(r, f, indent=1)
                print(f"wrote {args.out}: total={r.get('total')}")
            else:
                print(_fmt(r)[:4000])
        elif args.cmd == "vram-peek":
            rows = c.vram_peek(args.x, args.y, args.w, args.h)
            for row in rows:
                print(" ".join(f"{v:04x}" for v in row))
        elif args.cmd == "cmd":
            print(_fmt(c.call(**json.loads(args.json))))
    finally:
        c.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
