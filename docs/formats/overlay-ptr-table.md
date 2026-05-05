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
- **Function entry-point tables** — small, monotonic; one slot per public function.
- **Switch dispatch tables** — larger, repeating; emitted by the C compiler for `switch(x)` over a dense integer range.
- **Per-mode actor vtables** — small alphabet of 3 distinct values across N slots.

## Detection

Two checks produce zero overlap with already-named formats:

1. The first u32 is in `0x801C0000..=0x801FFFFF` (the overlay window).
2. The run of consecutive overlay-pointer u32s is between 4 and 64 long.

We do **not** require monotonicity — switch dispatch tables legitimately contain repeating handler addresses.

## Cluster anatomy

All matches cluster in the `0900..=0968_xxx_dat` PROT range — sized 14 KB to 160 KB. Pointer-range distribution:

- 30 entries: `0x801F6Axx`–`0x801F71xx` cluster (small function-entry tables, monotonic, 5–14 entries).
- 12 entries: `0x801F84xx+` cluster (some monotonic, some switch dispatch with repeating handlers).

A handful of entries lead with a Shift-JIS / ASCII title string before the pointer table — `0907_xxx_dat.BIN` "Hell's Music", `0924_xxx_dat.BIN` "Ultimate Rave", `0927_xxx_dat.BIN` "Dark Eclipse", `0957_xxx_dat.BIN`. These are likely the **Disco King dance-minigame** song-data overlays.

## Reading the format

```rust
use legaia_asset::overlay_ptr_table;

if let Some(t) = overlay_ptr_table::detect(buf) {
    println!("Overlay pointer table: {} entries, first=0x{:08x}, last=0x{:08x}",
             t.count, t.first_ptr, t.last_ptr);
}
```

Each entry can be Ghidra-imported via [`scripts/bulk-import-overlays.sh`](../tooling/overlay-capture.md) once the load address is determined; the pointer values bound the load address from above (`<= min_ptr`).
