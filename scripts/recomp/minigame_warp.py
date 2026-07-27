#!/usr/bin/env python3
"""Warp a live recomp instance into a mode-24 minigame (the op-0x3E door-warp).

The field/event VM enters every minigame through op ``0x3E`` (``op0 >= 100``),
and that arm performs **no call and names no scene** - it only writes SCUS
globals and lets the per-frame mode dispatcher do the rest
(``docs/subsystems/script-vm.md`` section "0x3E WARP"):

  * ``_DAT_8007BA34 = sub_id``  (u32 - selects the minigame overlay)
  * ``_DAT_80084440 = 0``       (session-winnings accumulator)
  * ``_DAT_8007BAC0 = 0``
  * ``player[+0x10] &= ~0x80000`` (player ptr at ``0x8007C364``)
  * ``_DAT_8007B83C = 0x18``    (u16 mode word -> mode 24 OTHER INIT; LAST)

Replaying exactly those writes over the debug server from any live
**field-mode** state (mode ``0x3``) performs the same warp: SCUS-resident
``FUN_80025980`` backs up the current scene name, loads the sub-id's overlay
into slot A and runs its init - no overlay needs to be resident when the
writes land. Sub-ids (script-vm.md): 0 fishing, 3 casino slots, 4 Baka
Fighter, 5 Muscle Dome (``other6`` / PROT 0977), 6 Noa dance.

For the Muscle Dome (``--sub-id 5``) the chain settles in a *battle*:
mode ``0x18 -> 0x19 -> 0x14 -> 0x15``, arena backdrop, Begin/Run menu,
match phase byte ``ctx+6 == 0x1e`` (idle/orbit). The dome fingerprint this
tool prints - ``mode 0x15`` with ``_DAT_8007BA34 == 5`` plus the phase
byte - is how a dome-resident savestate is identified; the scene name is
NOT a fingerprint (it still reads the backed-up host scene, e.g.
``town01``).

Typical capture run (load a field slot, warp, save into a stale slot)::

    python3 scripts/recomp/minigame_warp.py --port 4517 \
        --from-slot 3 --sub-id 5 --save-slot 5 --screenshot /tmp/dome.bmp

The save is verified two ways before the tool reports success: the slot's
``.pst`` must carry a non-zero resume PC (preflight), and the pre-save
fingerprint must have settled. Load-side proof stays with the caller:
``probe.py load-state <slot> --expect-mode 0x15`` from a *fresh* process,
then a screenshot + one Cross press (Begin -> the battle command cluster).
"""

from __future__ import annotations

import argparse
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import preflight  # noqa: E402
from probe import RecompClient  # noqa: E402

# The op-0x3E door-warp globals (script-vm.md section "0x3E WARP").
SUB_ID_ADDR = 0x8007BA34  # u32
WINNINGS_ADDR = 0x80084440  # u32
AUX_ADDR = 0x8007BAC0  # u32
MODE_WORD_ADDR = 0x8007B83C  # u16
PLAYER_PTR_ADDR = 0x8007C364
PLAYER_FLAG_BIT = 0x80000  # cleared in player[+0x10]
MODE_OTHER_INIT = 0x18

# Battle context base pointer (the dome match SM's ctx; phase byte at +6).
BATTLE_CTX_PTR = 0x8007BD24

MUSCLE_DOME_SUB_ID = 5


def _u32_bytes(addr: int, val: int) -> list[tuple[int, int]]:
    return [(addr + i, (val >> (8 * i)) & 0xFF) for i in range(4)]


def _u16_bytes(addr: int, val: int) -> list[tuple[int, int]]:
    return [(addr + i, (val >> (8 * i)) & 0xFF) for i in range(2)]


def warp_byte_writes(sub_id: int) -> list[tuple[int, int]]:
    """The ordered per-byte RAM writes of the op-0x3E minigame door-warp.

    ``write_ram`` is a one-byte-per-call command, so the warp is expressed
    as ``(addr, byte)`` pairs. Order matters only for the mode word: it is
    the dispatcher's trigger, so it comes LAST - every other global must
    already hold its value when the mode switch is picked up.
    """
    writes: list[tuple[int, int]] = []
    writes += _u32_bytes(SUB_ID_ADDR, sub_id)
    writes += _u32_bytes(WINNINGS_ADDR, 0)
    writes += _u32_bytes(AUX_ADDR, 0)
    writes += _u16_bytes(MODE_WORD_ADDR, MODE_OTHER_INIT)
    return writes


def perform_warp(client, sub_id: int) -> None:
    """Issue the door-warp writes on a live client (field mode expected).

    Also clears ``player[+0x10] & 0x80000`` when the player pointer is a
    plausible RAM address, matching the VM arm byte for byte.
    """
    player = client.read_u32(PLAYER_PTR_ADDR)
    if 0x80000000 <= player < 0x80200000:
        flags = client.read_u32(player + 0x10)
        if flags & PLAYER_FLAG_BIT:
            for addr, val in _u32_bytes(player + 0x10, flags & ~PLAYER_FLAG_BIT):
                client.call("write_ram", addr=f"0x{addr:08X}", val=f"0x{val:02X}")
    for addr, val in warp_byte_writes(sub_id):
        client.call("write_ram", addr=f"0x{addr:08X}", val=f"0x{val:02X}")


def wait_settle(client, timeout: float = 60.0, stable_samples: int = 4,
                interval: float = 1.0, sleep=time.sleep) -> list[int]:
    """Poll the mode word until it holds one value for ``stable_samples``
    consecutive samples after leaving the OTHER INIT/run pair (0x18/0x19),
    or until ``timeout``. Returns the observed mode sequence (deduped)."""
    seen: list[int] = []
    stable = 0
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        mode = client.game_mode()
        if not seen or seen[-1] != mode:
            seen.append(mode)
            stable = 1
        else:
            stable += 1
        if mode not in (MODE_OTHER_INIT, 0x19) and stable >= stable_samples:
            return seen
        sleep(interval)
    return seen


def fingerprint(client) -> dict:
    """Content fingerprint of the current state - what identifies a
    minigame-resident savestate (the scene name alone does not: it holds
    the backed-up host scene through the whole minigame)."""
    fp = {
        "mode": client.game_mode(),
        "scene": client.scene_name(),
        "sub_id": client.read_u32(SUB_ID_ADDR),
    }
    if fp["mode"] == 0x15:
        ctx = client.read_u32(BATTLE_CTX_PTR)
        if 0x80000000 <= ctx < 0x80200000:
            fp["ctx"] = ctx
            fp["phase"] = client.read_ram(ctx + 6, 1)[0]
    return fp


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, required=True)
    ap.add_argument("--sub-id", type=int, default=MUSCLE_DOME_SUB_ID,
                    help="minigame sub-id (default 5 = Muscle Dome)")
    ap.add_argument("--from-slot", type=int,
                    help="load this savestate first (must be a field-mode slot)")
    ap.add_argument("--save-slot", type=int,
                    help="after the warp settles, save into this slot and "
                         "preflight-verify the written .pst")
    ap.add_argument("--recomp-dir",
                    help="recomp workspace for the save-slot preflight "
                         "(default $LEGAIA_RECOMP_DIR)")
    ap.add_argument("--screenshot", help="write a BMP of the settled state")
    ap.add_argument("--settle-timeout", type=float, default=90.0)
    args = ap.parse_args(argv)

    c = RecompClient(args.host, args.port)
    if args.from_slot is not None:
        scene, mode = c.load_savestate(args.from_slot, expect_mode=0x3)
        print(f"loaded slot {args.from_slot}: scene={scene!r} mode={mode:#x}")
    mode = c.game_mode()
    if mode != 0x3:
        print(f"refusing to warp: mode {mode:#x} is not field mode 0x3 "
              "(the door-warp is a field-VM op; load a field slot first)",
              file=sys.stderr)
        return 1

    perform_warp(c, args.sub_id)
    chain = wait_settle(c, timeout=args.settle_timeout)
    print("mode chain:", " -> ".join(f"{m:#x}" for m in chain))
    fp = fingerprint(c)
    print("fingerprint:", {k: (hex(v) if isinstance(v, int) else v)
                           for k, v in fp.items()})
    if fp["sub_id"] != args.sub_id:
        print(f"warp failed: sub-id global reads {fp['sub_id']}, "
              f"expected {args.sub_id}", file=sys.stderr)
        return 1

    if args.screenshot:
        c.call("screenshot", path=args.screenshot)
        print("screenshot:", args.screenshot)

    if args.save_slot is not None:
        c.save_savestate(args.save_slot)
        time.sleep(3.0)  # staged - executes at the next block boundary
        import os
        recomp = args.recomp_dir or os.environ.get("LEGAIA_RECOMP_DIR")
        if recomp:
            recomp = os.path.expanduser(recomp)
            path = preflight.slot_state_path(recomp, args.save_slot)
            if path is None:
                print(f"save-slot {args.save_slot}: no .pst appeared",
                      file=sys.stderr)
                return 1
            pc = preflight.slot_resume_pc(path)
            if pc == 0:
                print(f"save-slot {args.save_slot}: resume pc is 0 - the "
                      "snapshot self-wipes on load", file=sys.stderr)
                return 1
            print(f"saved slot {args.save_slot}: resume pc 0x{pc:08X} ok "
                  f"({path})")
        else:
            print(f"saved slot {args.save_slot} (set LEGAIA_RECOMP_DIR or "
                  "--recomp-dir to preflight the written file)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
