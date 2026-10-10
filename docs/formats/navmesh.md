# Per-scene primitive scratch buffer at `0x80108EA4`

**Negative finding: this is not a format.** The 1700-byte cluster at `0x80108EA4..0x80109550` that differs between area-load saves is not a 24-byte-stride navmesh. It is a per-scene scratch buffer the renderer refills with assembled GPU primitive data on scene entry. The page exists so the reading is not re-derived; the game's real collision and region data is listed [below](#what-the-actual-navmesh--pathing-data-is).

## Evidence

Confidence: Inferred (from three save states; no consumer is disassembled).

| Observation | What it shows |
|---|---|
| `mc1` (field `map01`) holds a uniform `u16` run `0c 00 0c 00 ... 0d 00 ... 0e 00` | No record structure. |
| `mc2` (in battle) and `mc3` (field `suimon`) hold 12-byte runs shaped like GP0 packets | `80 80 80 7E` / `80 80 80 76` flag-mode-code-ilen quartets, RGB triplets (`c1 c1 c1`, `cc cc cc`, `f7 f7 f7`), signed `0xff` deltas at the `i16` positions. |
| All three saves show a different shape at the same offset | The window is overwritten wholesale on scene entry. |
| A pointer hunt over the full 2 MiB of main RAM finds **zero** `u32` values pointing into the window, in all three saves | No base-pointer cell exists; the consumer knows the address as a compiled `lui` / `addiu` constant. |
| The wider window `0x80108000..0x8010A000` has 6 external pointers in `mc1` | They target neighbours (`0x80108398`, `0x80108BC8`, `0x80108C2C`, `0x80108C38`): adjacent sprite-batcher structures, not this region. |

Diffing two area-load saves surfaces any buffer repopulated on scene entry, not only scene data. The record-id reading fits `mc3` alone, where some bytes look like `0x00` / `0x01` ids; it breaks on `mc1` and on `mc2`, where no field rendering is active.

## Reproducing the negative finding

```bash
./target/release/mednafen-state extract <mc1.mc1> --start 0x80000000 --end 0x80200000 --out /tmp/ram_mc1.bin
./target/release/mednafen-state extract <mc3.mc3> --start 0x80000000 --end 0x80200000 --out /tmp/ram_mc3.bin

python3 scripts/mednafen/pointer-hunt.py into-window /tmp/ram_mc1.bin \
    --target-lo 0x80108EA4 --target-hi 0x80109550 --exclude-self
python3 scripts/mednafen/pointer-hunt.py into-window /tmp/ram_mc3.bin \
    --target-lo 0x80108EA4 --target-hi 0x80109550 --exclude-self
```

Both invocations return zero hits. Widen the bounds to `0x80108000..0x8010A000` to surface the adjacent sprite-batcher pointers.

## What the actual navmesh / pathing data is

Per-scene collision, region and trigger data is not in this RAM window. The systems that carry it:

- **General town/field free-movement locomotion + collision** is `FUN_801d01b0` (player controller) + `FUN_801cfe4c` (collision), which sample a per-scene walkability tile map through the base pointer `_DAT_1f8003ec` (grid at `+0x4000`, 4 sub-cell wall bits per byte). See [`subsystems/field-locomotion.md`](../subsystems/field-locomotion.md). The collision data is a nibble grid keyed on the player tile.
- A **tile-board grid** (cell `2` = wall) is installed inline in the field-VM event script by op `0x49`, but that drives the puzzle / board minigame mode, not general locomotion (see [`subsystems/tile-board.md`](../subsystems/tile-board.md)).
- Per-scene **region / zone boxes** are 18-byte records at the MAN control block `_DAT_801c6ea4 + 0x4`, queried by player tile via `FUN_801dba20` (bbox in `bytes[1..4]`; `bytes[5..17]` is the region's [camera preset](encounter.md#man-section-3-the-camera-region-table), not pathing data).
- The **encounter-record pointer** lives in actor records at `actor[+0x94]` - see [`subsystems/world-map.md`](../subsystems/world-map.md#encounter-record-installation) for that flow.

## See also

- [`subsystems/field-locomotion.md`](../subsystems/field-locomotion.md) - the real free-movement collision system.
- [Scene bundles](scene-bundles.md) - the scene asset layouts that hold the per-scene grids.
