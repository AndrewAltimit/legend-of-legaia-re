# Overlay pointer-table code

Sister format to [MIPS overlay code](mips-overlay.md). Same family of disc-resident overlay code blobs, but the first chunk is a function/jump-table header instead of an `addiu sp, sp, -X` prologue.

Implementation: `crates/asset/src/overlay_ptr_table.rs`.

## Layout

```text
+0x00   u32  ptr_0      ; address in 0x801C0000..=0x801FFFFF (overlay window)
+0x04   u32  ptr_1      ; same range
...
+N*4    u32  ptr_{N-1}  ; last pointer (still in range)
+(N+1)*4 ...            ; first non-pointer u32 (typically MIPS code, sometimes
                        ; a `jr ra; nop` stub or a leading ASCII string)
```

The pointer-table length `N` ranges from 4 to 64. Tables come in three shapes:
- **Function entry-point tables** - small, monotonic; one slot per public function.
- **Switch dispatch tables** - larger, repeating; emitted by the C compiler for `switch(x)` over a dense integer range.
- **Per-mode actor vtables** - small alphabet of 3 distinct values across N slots.

## Detection

Two checks produce zero overlap with already-named formats:

1. The first u32 is in `0x801C0000..=0x801FFFFF` (the overlay window).
2. The run of consecutive overlay-pointer u32s is between 4 and 64 long.

We do **not** require monotonicity - switch dispatch tables legitimately contain repeating handler addresses.

## Cluster anatomy

All matches cluster in the `0900..=0968_xxx_dat` PROT range - sized 14 KB to 160 KB. Pointer-range distribution:

- 30 entries: `0x801F6Axx`–`0x801F71xx` cluster (small function-entry tables, monotonic, 5–14 entries).
- 12 entries: `0x801F84xx+` cluster (some monotonic, some switch dispatch with repeating handlers).

A handful of entries lead with an ASCII title string before the pointer table — the title is the
**battle special-attack name** the overlay stages, not a song: `0907_xxx_dat.BIN` "Hell's Music"
is **Nighto's summon stager** (capture-pinned on the spell-`0x85` slot of the summon loader's
`903..=913` range; the name also appears in the SCUS spell table — `Hell's Music|Kill or confuse
enemy.` — and in `summon.dat`'s attack-name records, exactly parallel to Gimard's `Burning
Attack`). The earlier "Disco King dance-song" reading is **refuted**: the dance overlay (0980)
contains zero slot-B loader callsites — its music is sequenced BGM via the sound streaming
loader. `0924_xxx_dat.BIN` "Ultimate Rave" and `0927_xxx_dat.BIN` "Dark Eclipse" are the same
attack-titled, stager-shaped family (part-spawn call census matches the pinned stagers); their
loader callsites are computed, so which action ids drive them is still open. All three are in
the static overlay map, slot-B base `0x801F69D8`. **`0957_xxx_dat.BIN` is a different shape**:
its head is a summon string table — `Dies` / `Puera` / `Both` / `Damage` / `Recover` (the summon
`Puera` + effect/target labels) — followed by an absolute-pointer table and code; it is the
slot-B `summon_effect_table` overlay.
See [`static-overlay-pipeline.md`](../tooling/static-overlay-pipeline.md).

## Reading the format

```rust
use legaia_asset::overlay_ptr_table;

if let Some(t) = overlay_ptr_table::detect(buf) {
    println!("Overlay pointer table: {} entries, first=0x{:08x}, last=0x{:08x}",
             t.count, t.first_ptr, t.last_ptr);
}
```

Each entry can be Ghidra-imported via [`scripts/bulk-import-overlays.sh`](../tooling/overlay-capture.md) once the load address is determined; the pointer values bound the load address from above (`<= min_ptr`).

## See also

- [MIPS overlay detection](mips-overlay.md) - the sister detector keyed on the prologue instruction shape.
- [`reference/functions.md`](../reference/functions.md) - the catalogue of traced function entry points.
- [`reference/memory-map.md`](../reference/memory-map.md) - the RAM map showing where overlays load.
