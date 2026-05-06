# Field-pack format

Magic `0x01059B84` followed by a 97-entry strict schema preceding packed TIMs/TMDs. Four PROT entries match the signature today. Detector + dispatch: `crates/asset/src/field_pack.rs`.

## Layout

```
[preamble — variable size, content shape unknown]
[u32 LE = 0x01059B84]                  <- MAGIC
[97 × u32 LE — schema table, 388 bytes — byte-identical across all field-packs]
[asset region — packed TIMs / TMDs, in some files]
```

The schema slot offsets cover `[0x60..0x16651]` (≈ 91 KB of logical layout). They are anchored on `slots[0] == 0x60` and `slots[96] == 0x16651` and are byte-identical across every field-pack file (MD5 `edcfdf1575889d63d2077c396089d7f3`). The schema is therefore a STATIC abstract layout, not per-file metadata.

## What the four entries look like

| PROT | preamble | schema | asset region | TIMs / TMDs in tim_scan / tmd_scan |
|---|---|---|---|---|
| `0002_gameover_data` | 234 KB | 388 B | 5.7 KB | 2 TIMs + several TMDs |
| `0003_town01` | 233 KB | 388 B | 362 KB | 5 TIMs + 2 TMDs |
| `0004_town01` | 227 KB | 388 B | 226 KB | 5 TIMs + 1 TMD |
| `0005_town01` | 0 B | 388 B | 166 KB | none — schema-indexed data only |

`0005_town01` is the odd one out: the magic sits at offset 0 and there are no packed TIMs/TMDs after the schema. The simplest explanation is that this entry holds the canonical schema-indexed data block on its own — likely a default template that scene-specific entries override piecewise.

## Why the magic isn't load-bearing

A scan of `SCUS_942.54` and every captured overlay (dialog, town, battle action, menu, the 0896 / 0897 / battle-action clusters) for either the `LUI`+`ADDIU/ORI` immediate pair that synthesises `0x01059B84` or the byte sequence `84 9B 05 01` returns zero hits. The runtime never compares against this magic.

That rules out a magic-checked format loader. The most likely interpretation is that field-pack is a build-time layout artefact — the schema describes the in-RAM shape that per-scene code reads at hard-coded slot offsets, and the magic is a sanity marker the disc mastering left behind (or the dev tooling stamped) rather than a runtime parser anchor.

Per-slot interpretation therefore depends on locating the consumer — per-scene code in a field/town overlay that reads from the slot offsets. No such consumer has been captured yet. See [`ghidra/scripts/find_field_pack_magic.py`](../../ghidra/scripts/find_field_pack_magic.py) for the scan that established the magic isn't referenced.

## Tooling

```bash
asset field-pack <PATH>                # show schema + slot sizes
asset field-pack <PATH> --all-slots    # all 97 slot offsets/sizes
asset field-pack-scan <DIR>            # find every field-pack in a PROT dir
```

Use the detector for classification today; full per-slot interpretation is pending a per-scene consumer trace.
