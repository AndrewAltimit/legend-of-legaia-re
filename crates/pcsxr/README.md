# legaia-pcsxr

PCSX-Redux save-state (`.sstate`) main-RAM reader - the bridge that lets the
cataloged PCSX-Redux **playthrough anchors** (`s1_newgame_field` ..
`s5_tetsu_battle`, captured by the `scripts/pcsx-redux/` probe harness) feed the
engine's disc-gated oracle tests the same way the mednafen `.mc` saves already do.

## What it does

A `.sstate` is `gzip(rawsstate)`, where `rawsstate` is PCSX-Redux's
protobuf-encoded state. The reader doesn't need the protobuf schema: it gunzips
the file and locates the 2 MiB main RAM **format-agnostically** by reusing the
SCUS anchor search ([`legaia_mednafen::extract::main_ram_via_anchor`]) - matching
a string known to live in the loaded SCUS region (e.g. `h:\prot\cdname.dat`) in
both the SCUS binary and the decompressed payload to derive the RAM base. (For the
captured anchors the RAM happens to start at payload offset `0x27`, but the anchor
search makes the reader robust to that.)

```rust
let st = legaia_pcsxr::SaveState::from_path(path)?;       // gunzip + anchor search
assert_eq!(st.scene_name(), "town01");
assert_eq!(st.game_mode(), 0x03);                          // 0x03 field, 0x15 battle
let (x, z) = st.player_pos().unwrap();                     // player+0x14/+0x18 as i16
```

`SaveState` exposes `main_ram()` + KSEG0 virtual-address readers
(`u8_at`/`u16_at`/`i16_at`/`u32_at`) plus the convenience accessors above. The
position fields are read as `i16` - the facing word at `player+0x16` sits between
`+0x14` (X) and `+0x18` (Z), so a `u32` read would fold it into the coordinate.

## How it composes

Disc-gated oracle tests load `saves/library/pcsx-redux/<fingerprint>.sstate`
(resolved from `scripts/scenarios.toml`) through this reader and compare the
engine's behaviour against the captured retail facts. The anchor search reads
`extracted/SCUS_942.54` (or `$LEGAIA_SCUS`).

- `crates/engine-core/tests/s4_warp_endstate_oracle.rs` - the engine's intra-town
  walk-touch warp lands where the S4 capture recorded.
- `crates/engine-core/tests/opening_progression_oracle.rs` - S1..S5 codify the
  retail opening progression (scene / game-mode / player position).
- `crates/pcsxr/tests/anchor_load.rs` - the reader's own oracle (reads back each
  anchor's pinned facts).

Depends only on `legaia-mednafen` (for the anchor search) + `flate2`; never on
`legaia-engine-core`, so the dev-only oracle edges stay acyclic. No Sony bytes are
committed - the `.sstate` files are gitignored, local-only under `saves/library/`.
