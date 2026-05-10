# Cheat databases

Third-party GameShark / Pro-Action-Replay cheat code dumps for
*Legend of Legaia* (NTSC-U) are an unusually rich source of
ground-truth RAM addresses for reverse-engineering work. This page
documents:

1. the encoding rules for the two formats we ingest,
2. how `crates/cheats` parses + classifies them,
3. the citations the rest of the docs use to anchor offsets, and
4. the runtime applier (`legaia-engine play-window --cheat-file`)
   that validates our memory map against retail behaviour.

The data files live under [`data/cheats/`](../../data/cheats/) and
are committed to the repo. They contain no Sony-owned bytes - just
labelled `(addr, value)` pairs published in public cheat databases
since the late 1990s.

## Format 1 — GameShark text dump

`legaia-ntsc-u.gs.txt` is a plain-text dump where each line is one
write:

```text
R I 2 L 0 80084816 64 100 AP
^ ^ ^ ^ ^ ^^^^^^^^ ^^ ^^^^^^^^
| | | | | |        |  description (rest of line)
| | | | | |        value (hex, no 0x)
| | | | | address (8 hex digits, no 0x)
| | | | compression group (always 0)
| | | endianness (L = little)
| | width-in-bytes (1, 2, or 4)
| encoding flag (always I)
read/write classifier (always R for cheat-as-write)
```

The high byte of the address is the GameShark **prefix**:

| Prefix | Meaning | Example |
|---|---|---|
| `0x80` | u16 LE write | `80084816 0064` ⇒ `mem16[0x80084816] = 0x0064` |
| `0x30` | u8 write | `300848A3 0000` ⇒ `mem8[0x800848A3] = 0x00` |
| `0xD0` | conditional: if `mem16 == value`, execute next | `D007B7C0 0100` (Select pressed) |
| `0xE0` | conditional: if `mem16 != value`, execute next | `E007B83C 0003` |

When the prefix is `0x80` the parser uses the field-width column as
ground truth (1 / 2 / 4) and rewrites the prefix accordingly - this
matters because user-edited dumps occasionally desync the prefix
from the column value.

## Format 2 — Mednafen `.cht`

`legaia-ntsc-u.cht` is a TOML-shaped dump where each effect is one
indexed triplet:

```toml
cheat0_desc = "Max Exp (Vahn)"
cheat0_code = "80084708 FFFF+8008470A 0098"
cheat0_enable = false
```

Multi-write effects join their codes with `+`; the parser splits on
that delimiter. Conditional codes use the same `D0` / `E0` prefix
encoding as Format 1.

## Parser + classifier

`crates/cheats` exposes:

| Item | Purpose |
|---|---|
| [`parse_gs_text`] | Format 1 → [`Database`] |
| [`parse_mednafen_cht`] | Format 2 → [`Database`] |
| [`Database::dedupe_identical`] | Drop the "Have 99 Items × 70" duplicate sprawl |
| [`classify_address`] | Map one address to a [`Category`] + detail label |
| [`Category`] | Coarse buckets: CharacterRecord / Inventory / BattleActor / ScriptVmGlobal / CameraGlobal / PadInput / WorldStoryFlag / Minigame / FieldVmCollision / ScratchActiveActor / CodePatch / Unknown |

The companion `cheat-tool` CLI:

```bash
cargo run -p legaia-cheats --bin cheat-tool -- parse data/cheats/legaia-ntsc-u.cht
cargo run -p legaia-cheats --bin cheat-tool -- classify data/cheats/legaia-ntsc-u.gs.txt --dedupe
cargo run -p legaia-cheats --bin cheat-tool -- diff data/cheats/legaia-ntsc-u.gs.txt data/cheats/legaia-ntsc-u.cht
cargo run -p legaia-cheats --bin cheat-tool -- extract-offsets data/cheats/legaia-ntsc-u.cht
cargo run -p legaia-cheats --bin cheat-tool -- offset-table data/cheats/legaia-ntsc-u.cht
```

[`parse_gs_text`]: ../../crates/cheats/src/gs_text.rs
[`parse_mednafen_cht`]: ../../crates/cheats/src/mednafen_cht.rs
[`Database`]: ../../crates/cheats/src/lib.rs
[`Database::dedupe_identical`]: ../../crates/cheats/src/lib.rs
[`classify_address`]: ../../crates/cheats/src/classify.rs
[`Category`]: ../../crates/cheats/src/classify.rs

## Citation table (per category)

What the cheat database tells us about each category. The `Detail`
column is the [`ClassifiedAddress::detail`] string the classifier
emits.

### CharacterRecord

Per-character `0x414`-byte record at `0x80084708 + slot * 0x414`.
Every offset is named in [`docs/formats/save-record.md`](../formats/save-record.md);
the `Detail` strings drop in via the field-name table there.

### Inventory

Inventory array at `0x80085958` + 2-byte stride, 72 slots. The
`Have 99 Items` cheat stamps `0x63` into every count byte; the
`Have Max Items` cheat stamps `0xFF`; `Item Modifier` zeroes the ID
byte. The stride alternates `(id, count)`.

### BattleActor

Per-actor battle record at `0x800EC9E8 + n * 0x2D4`. Pinned offsets:

| Offset | Field | Cheat |
|---:|---|---|
| `+0x14C` | `hp_curr` | "Infinite HP" (per character) |
| `+0x14E` | `hp_max` | (read by reader at battle init) |
| `+0x150` | `mp_curr` | "Infinite MP" |
| `+0x152` | `mp_max` | |
| `+0x172` | `hp_max_settled` | "Infinite HP" second site |
| `+0x174` | `mp_max_settled` | "Infinite MP" second site |

The "second site" pair at `+0x172 / +0x174` is the engine's record
copy that survives `FUN_80042558`'s per-frame stat aggregation
clamp. Cheats target both sites to keep the value pinned across
clamping.

### ScriptVmGlobal

| Address | Cheat | Engine semantic |
|---|---|---|
| `0x8007B450` | "Status Modifier Menu", "Save Anywhere", "Shop Modifier", "End of Game Stat Page" | Menu-request register the menu overlay polls each frame |
| `0x8007B5FC` | "No Random Battles" | Encounter step counter (cheat sets to `0x377` to force trigger or suppress) |
| `0x8007B6A8` | "Save Anywhere (Press Select+X)" | Save-anywhere allow flag |
| `0x8007B7C0` | (cond gate in many cheats) | Pad state register the `D0` / `E0` codes read |
| `0x8007B83C` | "Press R2 For Debug Menu" | Next game-mode register; FUN_801E30E4 sets `0x1A` for FMV |

### CameraGlobal

`0x8007B6F4` is the camera mode word. "Control Camera" and "Small
Maps" cheats target it.

### WorldStoryFlag

The "Access All Towns When You Use Door of Wind" cheat targets
`0x8008575C / 0x8008575E` (a 32-bit visited-towns bitmask). This is
*outside* the per-character records and *outside* the inventory
window - it lives in the dedicated story-flag block at
`0x80085600..0x80085800`.

### Minigame

Mini-game scratch RAM. The fishing minigame uses
`0x801D9168 / 0x801D9274 / 0x801D9298 / 0x801D91CC` for tension /
casting power / life / fish ID; baka fighter at
`0x801DBFC4 / 0x801DBFF0 / 0x801DC06C`; dance points at
`0x801D53CC`; slot machine at `0x801D3CAC`. None of these are wired
into the engine yet - they're recorded as citations only.

### FieldVmCollision

The "Walk Thru Walls" cheat patches four collision-state cells in
the field overlay: `0x801D078C / 0x801D071C / 0x801D065C /
0x801D06BC`. Each is a 1-byte gate the field VM consults during
movement; setting them all to `0x06` disables the collision check.

### ScratchActiveActor

`0x8007A6BC` is shared scratch for the currently-acting character.
Every "Infinite HP / MP" cheat hits this cell first - the runtime
reads it during the per-actor frame tick before applying back to
the per-record copy. Useful to know when chasing post-battle
stat-leak issues.

### CodePatch

A handful of cheats patch `SCUS_942.54` instructions to `0x2400`
(MIPS `nop`). The classifier flags these so we can tell them apart
from RAM cells:

- `0x800422F4` – "Bought Any Item / Find Items You Will Get 99 Quantity"
  patches the inventory-add helper.
- `0x8004309E` – "Infinite Items All Slots" patches the
  count-decrement instruction.
- `0x8004390E` – "Remove Vahn's Chest" patches a draw-call site.
- `0x8007EA96` – "Maxed HP for All Characters" patches an HP-write
  branch.

These are all useful Ghidra anchors; the patched instruction
addresses give us callsite hints the LUI+ADDIU resolver wouldn't.

## Runtime applier

`legaia-engine play-window --cheat-file <PATH>` parses the file via
`legaia_cheats`, builds a per-frame applier via
`legaia_engine_core::cheat_applier::apply`, and dispatches each
write through the `ram_map` registry to the appropriate `World` /
`CharacterRecord` field. Conditional codes are treated as
always-true by default (use `--cheat-strict` to honour them, which
will skip every cheat that gates on a button press the engine
doesn't emulate).

The applier reports per-entry status:

```text
Cheat report (49 entries, 96 writes; 71 applied, 25 skipped):
  ok    Infinite HP (Vahn)               4/4 writes applied
  ok    Infinite Gold (Never Glitchy)    1/1 writes applied
  skip  Walk Thru Walls                  0/4 writes applied (FieldVmCollision unmapped)
  ...
```

Use this to validate that a code change keeps the same set of
cheats applying. If a previously-applying cheat starts failing,
either:

- the engine field it targets was renamed (update `ram_map`), or
- the engine no longer exposes the field (update `WorldField`), or
- the cheat database had stale data (update the citation in the
  format doc).

## See also

- [`docs/formats/save-record.md`](../formats/save-record.md) — full
  record offset table that the cheat citations anchor.
- [`docs/reference/memory-map.md`](memory-map.md) — newly-pinned
  globals from the cheat database.
- [`crates/cheats/README.md`](../../crates/cheats/README.md) —
  parser + CLI reference.
