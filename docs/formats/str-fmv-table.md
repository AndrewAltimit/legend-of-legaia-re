# In-RAM STR FMV file table

The cutscene / MDEC overlay's lookup tables for STR FMV files. Two distinct tables coexist in the str_fmv overlay's data section, with **different roles**:

1. **Compact STR FMV table** at `0x801CAE40` - 6 entries, 24 bytes each, labelled `MV1.STR;1` .. `MV6.STR;1`. Carries a libcd-shaped filename + BCD MSF + size. *This table is dev-shape metadata, not the play-engine source* (see "Runtime mapping vs compact table" below).
2. **FMV dispatch table** at `0x801D0A6C` - 12 entries, 64 bytes each. The play loop (`FUN_801CF098`) reads this; it is the actual play-engine source. Each entry's leading 32-byte record is `[path_ptr, scale_flag, start_frame, end_frame, .., .., width, height]` - a path-string pointer into the **path string table** at the overlay start (`0x801CE818`) plus the frame range that segment plays. Static overlay data, so it decodes straight from the disc (`legaia_asset::fmv_dispatch`).

A third copy of the six MV files appears nearby in full ISO9660 directory-record form (`0x801CCA80`, 56-byte stride); the runtime FMV-state table is the actual lookup, the directory copies are presumably retained for `CdReadDir`-style validation.

## Confidence

**Inferred - structural reading.** The compact-table layout is pinned from a captured FMV-overlay-resident save state. The runtime FMV-state table layout is pinned from the same overlay binary, cross-validated against the play loop's offset reads at `FUN_801CF098 +0x38..+0x60`. The retail trigger range (`0..=8`) is pinned by the per-STR FMV trigger corpus (nine save states, `_DAT_8007BA78 ∈ 0..=8`).

## Compact table layout (`0x801CAE40`, 6 × 24 B)

```text
+0x00  char[12]  filename     "MV1.STR;1\0..." (null-padded; libcd path shape)
+0x0C  u32       reserved     zero across all observed entries
+0x10  u32       bcd_msf      byte 0 = BCD minute, 1 = BCD second,
                              2 = BCD frame, 3 = zero
+0x14  u32       size         file size in bytes (LE)
```

`bcd_msf` is the standard libcd `CdlLOC` representation: each byte is two BCD digits (high nibble = tens, low nibble = ones). The byte order packs into the u32 such that reading `bcd_msf` as 4 LE bytes recovers `[M, S, F, 0]` directly.

Convert to absolute LBA with the standard CD identity:

```text
LBA = ((M * 60) + S) * 75 + F - 150
```

The `-150` accounts for the 2-second pre-gap.

### Pinned compact-table entries

| Idx | Name        | BCD MSF (decimal) | Computed LBA | Disc match (`disc-extract list`) |
|----:|-------------|-------------------|-------------:|---|
| 0   | `MV1.STR;1` | 33:51.53          |   152,228    | **disc `MV2.STR`** (size 5,099,520) |
| 1   | `MV2.STR;1` | 34:24.68          |   154,718    | **disc `MV3.STR`** (size 18,104,320) |
| 2   | `MV3.STR;1` | 36:22.58          |   163,558    | **disc `MV4.STR`** (size 7,045,120) |
| 3   | `MV4.STR;1` | 37:08.48          |   166,998    | **disc `MV5.STR`** (size 13,393,920) |
| 4   | `MV5.STR;1` | 38:35.63          |   173,538    | **disc `MV6.STR`** (size 13,701,120) |
| 5   | `MV6.STR;1` | 19:14.41          |    86,441    | disc `XA15.XA` |

The compact table's name fields are dev-only labels and **do not match what the disc reader resolves at the table's BCD MSF**. The first five entries shift by one against the disc layout (entry [0] "MV1" points at disc MV2, etc.), and entry [5] "MV6" points at the unrelated `XA15.XA`. The compact table is a separate dev/init lookup, not the FMV play engine's resolver.

## Path string table (`0x801CE810`, null-terminated)

The runtime FMV-state slots' path-pointer field (`+0x00`) points into this packed string table. Nine null-padded paths in storage order:

| Path-table offset | String                |
|------------------:|-----------------------|
| `+0x008`          | `\DATA\MOV.STR;1`     |
| `+0x018`          | `\DATA\MOV15.STR;1`   |
| `+0x02C`          | `\MOV\MV1A.STR;1`     |
| `+0x03C`          | `\MOV\MV6.STR;1`      |
| `+0x04C`          | `\MOV\MV5.STR;1`      |
| `+0x05C`          | `\MOV\MV4.STR;1`      |
| `+0x06C`          | `\MOV\MV3.STR;1`      |
| `+0x07C`          | `\MOV\MV2.STR;1`      |
| `+0x08C`          | `\MOV\MV1.STR;1`      |

Three of the nine paths (`\DATA\MOV.STR;1`, `\DATA\MOV15.STR;1`, `\MOV\MV1A.STR;1`) are dev-only - the corresponding files are not on the retail disc.

## Mid-game scene labels (`0x801CE8AC`)

The same overlay data section carries seven CDNAME-shape labels for the mid-game FMV-trigger field scenes the FMV overlay knows about: `town0b`, `map01`, `chitei2`, `map02`, `jou`, `uru2`, `town0e`. These match `legaia_engine_core::scene::FMV_TRIGGER_FIELD_SCENES`.

## FMV dispatch table (`0x801D0A6C`, 12 × 64 B)

The play loop's selector lives at `0x801CECA0`:

```text
0x801CEC94: lh   v0, -0x4588(s0)        ; v0 = (s16) _DAT_8007BA78
0x801CEC9C: sll  v0, v0, 6              ; v0 = fmv_id * 64
0x801CECA0: jal  FUN_801CF098
0x801CECA4:  addu a1, v0, 0x801D0A6C    ; param_2 = &runtime_table[fmv_id]
```

Each slot's leading 32-byte record is `[u32 path_ptr, u32 scale_flag, u32 start_frame, u32 end_frame, u32 reserved, u32 (8), u32 width, u32 height]` (the second 32-byte half is a sibling segment the loop doesn't read for the primary path). The play loop opens the file at `path_ptr`, seeks `(start_frame - 1) * 10` sectors in (the 15 fps cadence), and reads to `end_frame` - which is how one `MVn.STR` carries several cutscenes by frame range. The `path_ptr` resolves into the path-string table at the **overlay start** (`0x801CE818`).

This table is **static initialised data** in the cutscene overlay (PROT 0970), not a runtime-built structure, so it decodes straight from the disc: `legaia_asset::fmv_dispatch::FmvTable::from_str_overlay` reads it (per-`fmv_id` path + frame range + dimensions), pinned by the disc-gated `fmv_dispatch_real` test. The windowed-cutscene player uses the frame range to seek to the right segment (`cutscene_av::fmv_segment_window`), so e.g. `fmv_id 2` correctly starts at `MV3.STR` frame `0x1A5`.

`_DAT_8007BA78` is a `s16` written by the field-VM FMV-trigger op (`0x4C 0xE2 lo hi …`); see [`cutscene.md`](../subsystems/cutscene.md#field-vm-fmv-trigger-op) for the full opcode trace.

### Authoritative runtime mapping

The retail USA build's twelve runtime FMV-state slots resolve as:

| `fmv_id` | path resolved        | notes |
|---------:|----------------------|-------|
| 0        | `\MOV\MV1.STR;1`     | frames 1..0x53a; intro logo (also fired by title-screen attract loop) |
| 1        | `\MOV\MV3.STR;1`     | frames 1..0xe1 (first segment) |
| 2        | `\MOV\MV3.STR;1`     | frames 0x1a5..0x27b (second segment) |
| 3        | `\MOV\MV4.STR;1`     | frames 1..0x152 |
| 4        | `\MOV\MV6.STR;1`     | frames 1..0x297 |
| 5        | `\DATA\MOV15.STR;1`  | dev-only path (file not on retail disc) |
| 6..=11   | `\DATA\MOV.STR;1`    | dev-only path (file not on retail disc) |

`MV2.STR` and `MV5.STR` exist on the retail disc but are **never referenced by any FMV slot** - the runtime FMV play engine never opens them.

The same authoritative mapping ships in `legaia_engine_core::cutscene::fmv_index_to_str_filename` (returns `Some(path)` for `0..=4`, `None` for cut/missing slots).

## Per-STR FMV trigger corpus

The current corpus carries nine save states captured RIGHT before each FMV begins playing, one per `_DAT_8007BA78` value (`fmv_id ∈ 0..=8`). Each save pins:

- `_DAT_8007BA78 = expected_fmv_id` (s16 LE)
- `_DAT_8007B83C = 0x1A` (StrInit game mode)
- `_DAT_8007BAC8 = 2000` (BGM ID; global pool index 0)
- Active scene = `map01` (one of the seven mid-game FMV-trigger field scenes)
- `recover_base()` = `0x80139530` (the `map01` field-pack base)

The `0x4C 0xE2 lo hi` byte sequence does NOT appear in the field-pack RAM region for any save in the corpus - the saves were generated by debug-menu-driven trigger paths, NOT by stepping the field VM through a per-scene FMV trigger op. The corpus pins the trigger-side state across the full `0..=8` range.

The corpus is codified at `legaia_engine_core::capture_observations::cutscene_trigger_corpus` and exercised by the disc-gated test `crates/mednafen/tests/real_saves.rs::cutscene_trigger_corpus_pins_fmv_id_across_nine_saves`.

## Per-scene trigger assignment (disc-sourced)

Which `fmv_id` each scene fires is **inline script data**, not a runtime value: the trigger op carries its id as a literal `i16` operand, so walking every scene MAN's partition-1 scripts with the field-VM disassembler recovers the full assignment straight from the disc (`legaia_engine_core::man_field_scripts::scene_fmv_triggers`, the `0x3F`-destination walk's sibling; pinned by the disc-gated `scene_fmv_triggers_disc` test over all 88 scene MANs):

| Scene (extraction entry) | `fmv_id` | resolves to |
|---|---:|---|
| `0004_town01` | 1 | `MV3.STR` first segment |
| `0095_garmel` | 2 | `MV3.STR` second segment |
| `0606_deroa` | 3 | `MV4.STR` |
| `0706_chitei2` | 3 | `MV4.STR` |
| `0218_dohaty` | 4 | `MV6.STR` |
| `0348_town0d` | 6 | cut (`\DATA\MOV.STR`, not on retail disc) |
| `0435_uru` | 7 | cut |
| `0689_jouine` | 8 | cut |

One trigger op per scene; no other scene MAN carries one. `fmv_id 0` (the `MV1.STR` intro) fires from the title/new-game path rather than a scene script, and `fmv_id 5` (the cut `MOV15.STR` slot) appears in no scene script. The `town0d` / `uru` / `jouine` triggers are **vestigial** - retail scene scripts still firing slots whose movies were cut from the disc.

## Rust API

```rust
use legaia_asset::str_fmv_table;

// Slice the compact table out of a captured main-RAM image.
let off = (0x801CAE40 - 0x80000000) as usize;
let bytes = &main_ram[off..off + 6 * str_fmv_table::ENTRY_STRIDE];

// Parse 6 entries; zero-filled trailing slots are dropped silently.
let entries = str_fmv_table::parse_entries(bytes, 6).expect("table parses");
for entry in &entries {
    println!("{} at LBA {} ({} bytes)", entry.name, entry.lba(), entry.size);
}

// Resolve fmv_id (the value the field VM writes to _DAT_8007BA78)
// to a STR file via the authoritative runtime mapping.
use legaia_engine_core::cutscene::fmv_index_to_str_filename;
assert_eq!(fmv_index_to_str_filename(0), Some("MOV/MV1.STR"));
assert_eq!(fmv_index_to_str_filename(1), Some("MOV/MV3.STR"));
assert_eq!(fmv_index_to_str_filename(5), None); // cut/missing slot

// Cheap signature check (compact table head).
assert!(str_fmv_table::looks_like_str_fmv_table(bytes));
```

## Provenance

| Subject                                    | Source |
|---|---|
| Compact-table layout                       | FMV-overlay-resident save; `legaia_asset::str_fmv_table` |
| BCD MSF semantics                          | PSX-SPX libcd `CdlLOC` definition |
| ISO9660 directory copy at `0x801CCA80`     | FMV-overlay-resident save |
| Path string table at `0x801CE810`          | FMV-overlay binary data section |
| Runtime FMV-state slot pointers            | FMV-overlay binary data section, cross-validated against `FUN_801CF098 +0x00` read |
| `fmv_id ∈ 0..=8` range                     | Per-STR FMV trigger corpus (nine save states); `cutscene_trigger_corpus` |
| Trigger-side state at game mode `0x1A`     | Per-STR FMV trigger corpus |
| Residency signature                        | `legaia_engine_core::capture_observations::str_fmv_overlay::is_resident` |

## See also

- [`subsystems/cutscene.md`](../subsystems/cutscene.md) - the STR game modes and MDEC decode loop.
- [XA audio](xa.md) - the XA-ADPCM audio interleaved with the STR video.
