# MIPS overlay code

Static disc copies of runtime overlays - small subsystem code blobs that load into the `0x801C0000+` overlay window. Distinct from the major full-scene overlays (title / town / battle / options); these are smaller specialised blobs from the `0901..=0969_xxx_dat` PROT range.

Implementation: `crates/asset/src/mips_overlay.rs`.

## Layout

```text
+0x00   u32  0x27BDFFXX        ; addiu sp, sp, -X (negative stack adjust)
+0x04   u32  prologue follow-up; sw ra/s* (0xAFB?_00XX), or another addiu /
                                ; lui / sw / R-type - the second instruction
                                ; of the entry function's prologue
+0x08   ...                    ; rest of the overlay code blob
```

## Detection

Three checks together produce zero false positives across the corpus:

1. `u32_le[0] & 0xFFFF_FF00 == 0x27BD_FF00` - `addiu sp, sp, -X`.
2. `(u32_le[0] & 0xFF) ∈ [0x80, 0xF8]` - only accept reasonable stack adjustments (8 to 128 bytes).
3. `u32_le[1]`'s 6-bit MIPS opcode field is one of the common function-prologue follow-ups (`sw`, `addiu`, `lui`, `lw`, R-type, ldc/sdc).

## Cluster anatomy

All matches cluster in the `0901..=0969` PROT range - sized 14 KB to 37 KB (one outlier at 163 KB for `0969_xxx_dat.BIN`). The `find-overlay` heuristic ranks these as MIPS-code-shape candidates; this detector formalises that ranking into the categorize pipeline.

Each entry can be Ghidra-imported via [`scripts/bulk-import-overlays.sh`](../tooling/overlay-capture.md) once a base address is determined. The overlay window is `0x801C0000`–`0x80200000`; each blob loads at a specific offset within that range, determinable from the asset chain that pulls it in.

## Likely subsystems

Based on size and the `xxx_dat` clustering: cutscenes, world-map, menu screens, mini-games, or per-scene specialised code that doesn't fit in the main town-field overlay.

## Reading the format

```rust
use legaia_asset::mips_overlay;

if let Some(m) = mips_overlay::detect(buf) {
    println!("MIPS overlay: stack frame = {} bytes; second op = {:#04x}",
             m.stack_frame_bytes, m.second_op);
}
```
