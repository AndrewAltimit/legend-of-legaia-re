#!/usr/bin/env python3
"""Stage a PSX memory-card image carrying exactly one save.

An in-emulator capture that loads a real memory card (rather than a save
state, which carries a stale RAM image and would hide the very disc-data
changes under test) has to navigate the game's load screen with pad
injection. That navigation is only deterministic when the card's slot
layout is known, and the progression cards in this repo carry up to 15
saves each.

So: copy the card, hide every save except the one asked for, and the
load screen shows a single row that a fixed input sequence always
lands on.

Hiding is directory-only - the block payload is left byte-identical, so
`--keep` never edits save data.

    ./isolate_card_save.py --list card.mcd
    ./isolate_card_save.py --keep 14 --out one.mcd card.mcd

Card layout: 16 blocks x 8192 B. Block 0 is the directory - frame 0 is
the "MC" header, frames 1..15 are the 128-byte entries for blocks 1..15.
Entry byte 0 is the state (0x51 first / 0x52 middle / 0x53 last of a
chain; 0xA0..0xA3 free), bytes 4..7 the payload size, 8..9 the next-block
link, 10.. the product code. Byte 127 is the XOR of bytes 0..126.
"""

import argparse
import sys

BLOCK = 8192
FRAME = 128
BLOCKS = 16
STATE_FIRST, STATE_MIDDLE, STATE_LAST = 0x51, 0x52, 0x53
STATE_FREE_FIRST = 0xA0


def entry_off(idx: int) -> int:
    """Byte offset of the directory entry describing block `idx` (1..15)."""
    return idx * FRAME


def xor_checksum(frame: bytes) -> int:
    c = 0
    for b in frame[:127]:
        c ^= b
    return c


def read_entry(card: bytes, idx: int) -> dict:
    off = entry_off(idx)
    e = card[off : off + FRAME]
    name = e[10:30].split(b"\0")[0].decode("ascii", "replace")
    return {
        "index": idx,
        "state": e[0],
        "size": int.from_bytes(e[4:8], "little"),
        "next": int.from_bytes(e[8:10], "little"),
        "name": name,
    }


def save_title(card: bytes, idx: int) -> str:
    """The save's own on-card title.

    Shift-JIS, and Legaia writes it in FULL-WIDTH forms, so a byte-wise
    ASCII read returns mojibake ("k@@k@OP") rather than "LV 01".
    """
    base = idx * BLOCK
    raw = card[base + 4 : base + 68].split(b"\0")[0]
    try:
        t = raw.decode("shift_jis")
    except UnicodeDecodeError:
        t = raw.decode("shift_jis", "replace")
    # Fold full-width ASCII (U+FF01..U+FF5E) back to its narrow form.
    return "".join(
        chr(ord(c) - 0xFEE0) if 0xFF01 <= ord(c) <= 0xFF5E else c for c in t
    ).strip()


def chain(card: bytes, first: int) -> list:
    """Block indices of the save starting at `first`."""
    out, idx, guard = [first], first, 0
    while guard < BLOCKS:
        e = read_entry(card, idx)
        nxt = e["next"]
        if nxt == 0xFFFF or nxt >= BLOCKS - 1:
            break
        idx = nxt + 1
        out.append(idx)
        guard += 1
    return out


def saves(card: bytes) -> list:
    return [
        read_entry(card, i)
        for i in range(1, BLOCKS)
        if read_entry(card, i)["state"] == STATE_FIRST
    ]


def free_entry(card: bytearray, idx: int) -> None:
    off = entry_off(idx)
    card[off] = STATE_FREE_FIRST
    card[off + 4 : off + 8] = (0).to_bytes(4, "little")
    card[off + 8 : off + 10] = (0xFFFF).to_bytes(2, "little")
    for i in range(10, 127):
        card[off + i] = 0
    card[off + 127] = xor_checksum(bytes(card[off : off + FRAME]))


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("card", help="source .mcd/.mcr image")
    ap.add_argument("--list", action="store_true", help="list saves and exit")
    ap.add_argument("--keep", type=int, help="block index of the save to keep")
    ap.add_argument("--out", help="write the isolated card here")
    ap.add_argument(
        "--to-slot",
        type=int,
        default=1,
        help="relocate the kept save to this block (default 1, where the "
        "load grid's cursor starts); 0 leaves it in place",
    )
    args = ap.parse_args()

    data = open(args.card, "rb").read()
    if len(data) < BLOCKS * BLOCK:
        print(f"not a 128K memory card: {len(data)} bytes", file=sys.stderr)
        return 2
    if data[:2] != b"MC":
        print("missing 'MC' directory header", file=sys.stderr)
        return 2

    present = saves(data)
    if args.list or args.keep is None:
        for e in present:
            print(
                f"block {e['index']:2d}  {e['name']:<24} "
                f"chain={chain(data, e['index'])}  {save_title(data, e['index'])}"
            )
        if args.keep is None and not args.list:
            print("\nnothing to do: pass --keep N --out FILE", file=sys.stderr)
        return 0

    if not any(e["index"] == args.keep for e in present):
        print(
            f"block {args.keep} is not the first block of a save "
            f"(have: {[e['index'] for e in present]})",
            file=sys.stderr,
        )
        return 2
    if not args.out:
        print("--keep needs --out", file=sys.stderr)
        return 2

    keep = set(chain(data, args.keep))
    card = bytearray(data)
    for e in present:
        if e["index"] == args.keep:
            continue
        for b in chain(data, e["index"]):
            if b not in keep:
                free_entry(card, b)

    # Relocate to block 1. NB this does NOT change where the save
    # appears in Legaia's load grid: that position follows the save's
    # OWN slot number - the product-code suffix, `BASCUS-94254PRO-13`
    # being slot 14 - not the card block it occupies (measured: moving
    # block 14 to block 1 left the icon in the same cell). To put a save
    # under the load cursor, pick one whose suffix is `-00`.
    # Single-block chains only - every Legaia save is one block, and
    # moving a chain would mean rewriting its links.
    where = args.keep
    if args.to_slot and args.to_slot != args.keep:
        if len(keep) != 1:
            print(
                f"--to-slot skipped: save spans {sorted(keep)}, not one block",
                file=sys.stderr,
            )
        else:
            dst = args.to_slot
            src_off, dst_off = args.keep * BLOCK, dst * BLOCK
            card[dst_off : dst_off + BLOCK] = data[src_off : src_off + BLOCK]
            se, de = entry_off(args.keep), entry_off(dst)
            card[de : de + FRAME] = card[se : se + FRAME]
            card[de + 127] = xor_checksum(bytes(card[de : de + FRAME]))
            if dst != args.keep:
                free_entry(card, args.keep)
            where = dst

    open(args.out, "wb").write(bytes(card))
    kept = read_entry(bytes(card), where)
    print(
        f"{args.out}: kept block {args.keep} ({kept['name']}) "
        f"{'-> block ' + str(where) + ' ' if where != args.keep else ''}"
        f"- {save_title(bytes(card), where)}"
    )
    print(f"visible saves now: {[e['index'] for e in saves(bytes(card))]}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
