#!/usr/bin/env python3
"""Extract the party's assembled battle meshes from a captured RAM image.

Input is the 2 MiB main-RAM dump written by
`autorun_battle_mesh_dump.lua` (see `capture_battle_mesh.sh`). This walks
the runtime structures the battle loader builds and writes, per party
slot, the mesh the game is actually drawing - which is the thing a disc
patch is trying to change, and the thing no offline decode can prove on
its own.

Layout (docs/formats/character-mesh.md, docs/subsystems/battle.md):

    party_base = u32 @ 0x8007B824             -- index into the TMD table
    blob       = u32 @ 0x8007C018 + (party_base + slot)*4
    u32 @ blob            == 0x80000002       -- Legaia TMD magic
    nobj       = u32 @ blob + 8
    object[i]  = blob + 12 + i*0x1C           -- absolute pointers
                 +0x00 vertex top, +0x04 vertex count (8-byte i16 x,y,z,pad)
    hdr_base   = blob - 0x18                  -- == u32 @ (rec0 + 0x50)
                 [0 .. nobj)   bone tag per object
                 [nobj .. )    attach bone of each equipment extra

Emits `slot<N>.json` (counts, bone tags, per-object vertex bounds and a
vertex-pool digest) and `slot<N>.obj` for eyeballing, next to the input.

    python3 decode_battle_mesh.py captures/battle_mesh/<stamp>/ram.bin
"""

import argparse
import hashlib
import json
import os
import struct
import sys

RAM_BASE = 0x80000000
RAM_SIZE = 2 * 1024 * 1024
TMD_MAGIC = 0x80000002
TMD_TABLE = 0x8007C018
PARTY_BASE = 0x8007B824
REC0_TABLE = 0x801C9360
ACTOR_TABLE = 0x801C9370
PARTY_SLOTS = 0x8007BD10
OBJ_STRIDE = 0x1C


class Ram:
    def __init__(self, data: bytes):
        self.d = data

    def off(self, va: int) -> int:
        return va & 0x1FFFFF

    def u8(self, va: int) -> int:
        return self.d[self.off(va)]

    def u32(self, va: int) -> int:
        o = self.off(va)
        return struct.unpack_from("<I", self.d, o)[0]

    def i16(self, va: int) -> int:
        return struct.unpack_from("<h", self.d, self.off(va))[0]

    def ok(self, va: int, span: int = 4) -> bool:
        o = va & 0x1FFFFF
        return (va & 0xFFFFFF) < RAM_SIZE and o + span <= len(self.d)


def read_mesh(ram: Ram, blob: int) -> dict:
    nobj = ram.u32(blob + 8)
    if not 0 < nobj < 64:
        raise ValueError(f"implausible object count {nobj} at 0x{blob:08x}")
    hdr = blob - 0x18
    objects = []
    for i in range(nobj):
        e = blob + 12 + i * OBJ_STRIDE
        vtop = ram.u32(e + 0x00)
        vcnt = ram.u32(e + 0x04)
        obj = {
            "index": i,
            "bone_tag": ram.u8(hdr + i),
            "vertex_count": vcnt,
            "vertex_top": vtop,
        }
        if 0 < vcnt < 4096 and ram.ok(vtop, vcnt * 8):
            vs = []
            for v in range(vcnt):
                p = vtop + v * 8
                vs.append((ram.i16(p), ram.i16(p + 2), ram.i16(p + 4)))
            obj["vertices"] = vs
            xs, ys, zs = zip(*vs)
            obj["bbox"] = [
                [min(xs), min(ys), min(zs)],
                [max(xs), max(ys), max(zs)],
            ]
            obj["centroid"] = [
                round(sum(xs) / len(vs), 2),
                round(sum(ys) / len(vs), 2),
                round(sum(zs) / len(vs), 2),
            ]
        else:
            obj["vertices"] = []
            obj["note"] = "vertex pool out of range"
        objects.append(obj)
    pool = b"".join(
        struct.pack("<3h", *v) for o in objects for v in o["vertices"]
    )
    return {
        "blob": blob,
        "header_base": hdr,
        "object_count": nobj,
        "vertex_total": sum(len(o["vertices"]) for o in objects),
        "vertex_digest": hashlib.sha256(pool).hexdigest()[:32],
        "objects": objects,
    }


def find_blobs(ram: Ram) -> list:
    """The three party mesh pointers, table-first with a scan fallback."""
    out = []
    base = ram.u32(PARTY_BASE) & 0xFFFF
    for slot in range(3):
        idx = base + slot
        cand = None
        if idx < 64:
            p = ram.u32(TMD_TABLE + idx * 4)
            if ram.ok(p) and ram.u32(p) == TMD_MAGIC:
                cand = p
        out.append((slot, idx, cand))
    if all(c is None for _, _, c in out):
        # Party base was not what we assumed - take the first three TMD
        # pointers the table holds instead, and say so.
        found = []
        for i in range(64):
            p = ram.u32(TMD_TABLE + i * 4)
            if ram.ok(p) and ram.u32(p) == TMD_MAGIC:
                found.append((i, p))
        out = [(s, i, p) for s, (i, p) in enumerate(found[:3])]
        print(f"note: party base 0x{base:x} yielded no TMDs; scanned table -> {out}")
    return out


def write_obj(path: str, mesh: dict) -> None:
    with open(path, "w") as f:
        f.write(f"# assembled battle mesh, {mesh['object_count']} objects\n")
        for o in mesh["objects"]:
            f.write(f"o obj{o['index']:02d}_bone{o['bone_tag']}\n")
            for x, y, z in o["vertices"]:
                # y-down in game space; flip for a viewer's sanity.
                f.write(f"v {x} {-y} {z}\n")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("ram", help="ram.bin from the capture")
    ap.add_argument("--out", help="output dir (default: alongside ram.bin)")
    args = ap.parse_args()

    data = open(args.ram, "rb").read()
    if len(data) < RAM_SIZE:
        print(f"short RAM image: {len(data)} bytes", file=sys.stderr)
        return 2
    ram = Ram(data)
    out = args.out or os.path.dirname(os.path.abspath(args.ram))
    os.makedirs(out, exist_ok=True)

    print(f"party base index : 0x{ram.u32(PARTY_BASE):08x}")
    for slot in range(3):
        print(
            f"slot {slot}: member={ram.u8(PARTY_SLOTS + slot)} "
            f"rec0=0x{ram.u32(REC0_TABLE + slot * 4):08x} "
            f"actor=0x{ram.u32(ACTOR_TABLE + slot * 4):08x}"
        )

    any_ok = False
    for slot, idx, blob in find_blobs(ram):
        if blob is None:
            print(f"slot {slot}: no TMD at table index {idx} - skipped")
            continue
        try:
            mesh = read_mesh(ram, blob)
        except ValueError as e:
            print(f"slot {slot}: {e}")
            continue
        any_ok = True
        rec0 = ram.u32(REC0_TABLE + slot * 4)
        seats = ram.u32(rec0 + 0x50) if ram.ok(rec0 + 0x50) else 0
        mesh["rec0"] = rec0
        mesh["rec0_header_ptr"] = seats
        mesh["header_matches_rec0"] = seats == mesh["header_base"]
        print(
            f"slot {slot}: blob=0x{blob:08x} objects={mesh['object_count']} "
            f"verts={mesh['vertex_total']} digest={mesh['vertex_digest']} "
            f"rec0_link={'ok' if mesh['header_matches_rec0'] else 'MISMATCH'}"
        )
        print(
            "  bone tags: "
            + " ".join(str(o["bone_tag"]) for o in mesh["objects"])
        )
        json_path = os.path.join(out, f"slot{slot}.json")
        with open(json_path, "w") as f:
            json.dump(mesh, f, indent=1)
        write_obj(os.path.join(out, f"slot{slot}.obj"), mesh)
        print(f"  wrote {json_path} and slot{slot}.obj")

    if not any_ok:
        print("no party meshes decoded - was the dump taken inside a battle?", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
