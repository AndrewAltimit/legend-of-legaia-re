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

Each entry can be Ghidra-imported via [`scripts/ghidra-analysis/bulk-import-overlays.sh`](../tooling/overlay-capture.md) once a base address is determined. The overlay window is `0x801C0000`–`0x80200000`; each blob loads at a specific offset within that range, determinable from the asset chain that pulls it in.

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

## Address attestation in overlay dumps

A dumped function's *printed* addresses are trustworthy only when the dump's link base was recovered correctly; a filename prefix is not evidence of the base. Two failure modes recur across the battle / minigame overlay dumps and disqualify most raw addresses from being documented at their printed VA. The byte-match arbiter (`scripts/ghidra-analysis/classify-worklist.py --explain <addr>`) resolves the true image per address; trust its `image=` verdict, not the dump filename.

- **Mis-recovered base.** The `bat_back_dat` (PROT 0896) dump family was imported at `0x801C5818` rather than the overlay's true `0x801C0000` load base (visible in the per-dump header as `[overlay_0896 base=0x801C5818]`). Its printed addresses are shifted, and any address above the overlay's real span reads *through* into the adjacent field (0897) and battle-action (0898) overlays. Running the arbiter over the 0896 address set resolves most entries to field(897) / battle(898) bodies, or to interior / shared-tail fragments (no `addiu sp,sp,-N` prologue, a body that exits via `j` into another routine's epilogue), not to a standalone `bat_back_dat` function. A dump whose header shows this base cannot be cited by its printed VA.

- **Relocation-duplicate re-images.** The debug-menu (0971), fishing (0977 / 0978 "other_game") and menu (0899) overlays re-image a shared render / minigame library at a different base per game-mode context - the same `0900`<->`0901` / `0965`<->`0967` re-imaging pattern noted in [`functions.md`](../reference/functions.md). Their function bodies are byte-identical modulo relocation to already-dumped bodies in debug_menu(0970) / fishing(0972) / menu(0899); the arbiter reports each such address as a `DUPLICATE` of the canonical VA. Only `REAL` self-entry bodies at a recovered base attest a distinct function at their printed address.

## See also

- [Static overlay-extraction pipeline](../tooling/static-overlay-pipeline.md) - extracts these (and the big scene overlays) from the disc at their recovered base, with identity attached from the PROT entry.
- [Overlay pointer table](overlay-ptr-table.md) - the sister detector for overlay code with leading pointer tables.
- [`reference/functions.md`](../reference/functions.md) - the catalogue of traced function entry points.
- [`reference/memory-map.md`](../reference/memory-map.md) - the RAM map showing where overlays load.
