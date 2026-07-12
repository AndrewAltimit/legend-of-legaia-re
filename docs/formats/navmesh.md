# Per-scene primitive scratch buffer at `0x80108EA4`

The 1700-byte cluster at `0x80108EA4..0x80109550` that differs across area-load saves is **not** a 24-byte stride navmesh. It's a per-scene scratch buffer that the renderer fills with assembled GPU primitive data on scene entry - the contents shape varies by scene because the buffer holds whatever the per-scene pre-fill writes for that scene's billboard / decoration / sprite set.

This entry exists to record the negative finding so the next person doesn't re-walk the same path.

## What the data actually looks like

Inspected across `mc1` (field `map01`), `mc2` (in-battle), `mc3` (field `suimon`), the bytes have three shapes that don't match a navmesh table:

- **Repeating constant runs**: in `mc1` (`map01`), addresses `0x80108EA4..0x801095xx` hold `0c 00 0c 00 0c 00 ... 0d 00 0d 00 ... 0e 00 ...` - a uniform u16 sequence with no record structure.
- **GPU-packet shapes** in `mc2` and `mc3`: 12-byte runs that mirror the GP0 packet header + RGB-color + delta-position layout the PSX renderer emits. Bytes like `80 80 80 7E` / `80 80 80 76` are the canonical TMD primitive flag/mode/code/ilen quartet; bytes like `c1 c1 c1` / `cc cc cc` / `f7 f7 f7` are RGB color triplets; signed `0xff` deltas appear at the i16 positions.
- **Cross-save residency**: `mc1`, `mc2`, and `mc3` all show *different* shapes at the same offset. A real navmesh would carry stable per-scene records that other code reads through a known base pointer; this region is overwritten wholesale on scene entry.

## Why the original "navmesh" reading misled

The cluster *was* spotted by diffing two area-load saves and noticing 1700 bytes differed. That's how scene-resident data shows up - but it's also how *any* working buffer that gets repopulated on scene entry shows up. The leading `id` / `kind` interpretation in the first reading happened to fit `mc3` (where some bytes look like 0x00/0x01-shaped record ids), but breaks for `mc1` (uniform `0x0c00`) and `mc2` (battle state, no field rendering active).

## Why no consumer was ever found

A `mednafen-state` pointer-hunt over the full 2 MiB main-RAM window for any u32 value pointing into `0x80108EA4..0x80109550` returns **zero** hits in `mc1`, `mc2`, or `mc3`. There is no RAM cell anywhere - overlay slot, SCUS data segment, kernel stack, or runtime heap - that holds the table base. That's the smoking gun: a real navmesh table would be consulted through a stable base-pointer cell. This buffer is consumed by code that knows its address as a constant (LUI+ADDIU pair compiled into the renderer), not via indirection.

The wider window `0x80108000..0x8010A000` does have 6 external pointers (in `mc1`), but they target neighbouring offsets (`0x80108398`, `0x80108BC8`, `0x80108C2C`, `0x80108C38`) - that's the pre-fill / sprite-batcher reading from data structures *adjacent to* this scratch region, not the scratch region itself.

## Reproducing the negative finding

```bash
./target/release/mednafen-state extract <mc1.mc1> --start 0x80000000 --end 0x80200000 --out /tmp/ram_mc1.bin
./target/release/mednafen-state extract <mc3.mc3> --start 0x80000000 --end 0x80200000 --out /tmp/ram_mc3.bin

python3 scripts/mednafen/pointer-hunt.py into-window /tmp/ram_mc1.bin \
    --target-lo 0x80108EA4 --target-hi 0x80109550 --exclude-self
python3 scripts/mednafen/pointer-hunt.py into-window /tmp/ram_mc3.bin \
    --target-lo 0x80108EA4 --target-hi 0x80109550 --exclude-self
```

Both invocations return zero hits. The narrow `--target-lo`/`--target-hi` bounds match the original reading; widen to `0x80108000..0x8010A000` to surface the adjacent sprite-batcher pointers.

## What the actual navmesh / pathing data is

Real per-scene region / event-trigger data for actor pathing is NOT in this RAM window. The actual systems:

- **General town/field free-movement locomotion + collision** is `FUN_801d01b0` (player controller) + `FUN_801cfe4c` (collision), which sample a per-scene walkability tile map through the base pointer `_DAT_1f8003ec` (grid at `+0x4000`, 4 sub-cell wall bits per byte). See [`subsystems/field-locomotion.md`](../subsystems/field-locomotion.md). This is the resolved answer to "where is the collision data" - a nibble grid keyed on the player tile, not a RAM table in this window.
- A **tile-board grid** (cell `2` = wall) is installed inline in the field-VM event script by op `0x49`, but that drives the puzzle / board minigame mode, not general locomotion (see [`subsystems/tile-board.md`](../subsystems/tile-board.md)).
- Per-scene **region / zone boxes** are 18-byte records at the MAN control block `_DAT_801c6ea4 + 0x4`, queried by player tile via `FUN_801dba20` (bbox in `bytes[1..4]`; `bytes[5..17]` is the region's [camera preset](encounter.md#man-section-3-the-camera-region-table), not pathing data).
- The **encounter-record pointer** lives in actor records at `actor[+0x94]` - see [`subsystems/world-map.md`](../subsystems/world-map.md#encounter-record-installation) for that flow.

## See also

- [`subsystems/field-locomotion.md`](../subsystems/field-locomotion.md) - the real free-movement collision system.
- [Scene bundles](scene-bundles.md) - the scene asset layouts that hold the per-scene grids.
