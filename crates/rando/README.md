# legaia-rando

Randomizer / disc patcher for a user-supplied Legend of Legaia disc.

Edits gameplay data on the user's own `.bin` and writes it back: monster item
drops, random-encounter formations, and treasure-chest contents. It is
Track-1-adjacent tooling — it does **not** touch the clean-room engine — and it
ships only code: no game bytes are embedded or committed, and every test that
needs real data is disc-gated.

See [`docs/tooling/randomizer.md`](../../docs/tooling/randomizer.md) for the
full design.

## Modules

| Module | Role |
|---|---|
| `rng` | Version-stable `SplitMix64` PRNG. A published seed always reproduces a run, independent of any external generator's algorithm (the first output for seed 0 is pinned by a test). |
| `items` | The valid item-id pool from the SCUS item-name table (`legaia_asset::item_names`), so a randomized drop is always an item the game has a name and handler for. |
| `drops` | The drop-table planner. `plan_drops` reassigns the monsters that currently drop something, in `Shuffle` mode (redistribute the existing drops — preserves the economy) or `Random` mode (draw from the item pool). Deterministic in `(current drops, pool, seed, mode)`. |
| `monster` | Re-pack a monster slot in the `battle_data` archive (PROT 867). `repack_slot` decompresses the `0x14000`-byte slot, hands the decoded record to an in-place mutator, recompresses with `legaia_lzs::compress`, and zero-pads back to the original slot size so no offset moves. `set_drop` is the drop-id + chance wrapper. |
| `disc` | `DiscPatcher`: own a mutable disc image, locate PROT.DAT + read its TOC, and apply same-size PROT-entry edits via the Mode 2/2352 sector write-back in `legaia_iso::write`. `patch_monster_slot` / `monster_slot` are the `battle_data` helpers; `patch_prot_entry` is the generic form. |
| `encounter` | Random-encounter randomizer. `SceneEncounters::locate` finds a scene bundle's MAN inside a PROT entry and decompresses it; `randomize` rewrites the formation monster ids from the scene's own id pool (so every monster stays scene-loaded) in `Shuffle` (redistribute) or `Random` (draw from pool) mode; `repack` recompresses and reports whether it fits the original footprint. |
| `chest` | Treasure-chest / scripted item-gift randomizer. Gives go through field-VM op `0x39` (`[0x39, item_id]`, inline operand) in the MAN partition-1 interaction scripts, usually **after** the inline dialogue that announces the item. `give_item_sites` walks each record's script with the Track-1 field-VM disassembler, **skipping `0x1F` dialogue segments** so it reaches the post-announcement give, and bounds each walk to the record's extent so it never mis-reads a `0x39` data byte; `SceneChests::locate` bundles the sites with the decoded MAN for rewriting (275 sites / 50 scenes on the retail disc). A chest also names its item in a **separate** dialogue token `0xC2 <id>` (the "There is a {item}…" announcement) distinct from the `0x39` grant, so `give_sites_and_display_tokens` recovers each site's `0xC2` tokens and `SceneChests::set_site` rewrites the operand **and** those tokens together — the flavor text tracks what the chest actually grants. |
| `steal` | Steal-item randomizer (the Evil God Icon). `StealEdits::locate` reads the static `SCUS_942.54` steal table (`DAT_80077828`, per-monster `[chance, item]`, see [`steal-table.md`](../../docs/formats/steal-table.md)); `plan` reuses the drop planner to reassign the item for every stealable monster (`Shuffle` redistributes the existing steal-item multiset, `Random` draws from the item pool), and `item_patches` emits same-size single-byte SCUS edits that touch the **item** only — the steal chance is preserved. No LZS re-pack, so nothing is ever skipped. |
| `door` | Scene-transition ("door / exit") randomizer. Doors are the field-VM `0x3F` named-scene-change ops — **partition-2 MAN records** reached via the partition-2 record-offset table (see [`man-relocation.md`](../../docs/formats/man-relocation.md)). `SceneDoors::locate` enumerates a scene's door sites (`legaia_asset::man_edit::scene_change_sites`); `rebuild` applies destination rewrites through the **variable-length** `man_edit` relocation engine, recompresses, validates, and reports whether it fits the footprint. The only randomizer that resizes an asset. |
| `house_door` | Intra-town ("house / interior") door randomizer. Entering a house is a field-VM `0x23 MOVE_TO` to an interior tile within the **same** scene (intra-scene reposition, pinned via `probe.step.find_writer`; writer in `FUN_801de840` `case 0x23`). `SceneHouseDoors::locate` enumerates a scene's non-sentinel MOVE_TO sites (`legaia_asset::man_edit::move_to_sites`); `shuffle` does a per-scene, multiset-preserving shuffle of the target tiles (same-size 2-byte operand edit). Shuffle-only + experimental (the op is shared with NPC/cutscene movement). |
| `starting_items` | New-game starting-inventory randomizer. There is no static starting-inventory table — the new-game data-init `FUN_80034A6C` code-builds the bag, writing one slot (Healing Leaf `0x77` ×5) into the live consumable inventory (see [`legaia_asset::new_game::StartingInventory`]). So this rewrites the **seed code**: the reclaimable 40-byte region at `0x80034b04` (the original seed + a redundant inline zero-loop both callers already cover with their `SC`-block `memset`). `plan_starting_items` picks `n` distinct random consumables from `STARTING_ITEM_POOL` (`0x77..=0x8e`) with small random counts; `build_seed_patch` encodes them as one packed halfword store per slot (`addiu $v0,(count<<8)\|id; sh $v0,off($s0)`), capping at `MAX_STARTING_ITEMS` = 5. Same-size code patch (no executable growth), applied via `patch_named_file`. |
| `unused` | Curated "unused content" the opt-in toggles re-introduce. `UNUSED_ENEMY_IDS` = the Evil Bat clones 176/177/178 (no formation references them, but the battle loader streams a monster slot on demand by id, so adding one to a scene's encounter Random pool via `SceneEncounters::randomize_with_extra` is enough to spawn it); `UNUSED_ITEM_IDS` = Something Good `0x6B` + the unnamed accessory `0xFD`, unioned into the random-fill item pool by `extend_pool`. |
| `item_name` | `NameInjection`: name the otherwise-blank accessory `0xFD` "Seru Bell" so `--unused-items` hands out a presentable item. A same-size SCUS patch — write the string into preserved rodata padding (`SERU_BELL_STRING_VA = 0x8007AB40`, pinned for the US build inside a 1028-byte zero gap flanked by rodata constants proven preserved file→RAM; **not** the data-segment zero-fill tail, which is `.sbss` scratch the game clobbers, nor an arbitrary always-zero region, which can be boot-cleared) and repoint only `0xFD`'s `name_ptr_slot`, leaving the other empty-name ids blank. |
| `apply` | High-level orchestration the CLI drives: `current_drops` / `apply_drop_plan` / `randomize_drops` for drops (a `DropApplyReport` records any slot too tight to re-pack), `randomize_encounters` for per-scene formations (`EncounterApplyReport`; takes an `unused_enemies` id slice unioned into the Random pool), `randomize_chests` for treasure (global shuffle/random of chest item ids → `ChestApplyReport`), `current_steals` / `randomize_steals` for the steal table (`StealApplyReport`), `current_doors` / `randomize_doors` for scene transitions (`DoorApplyReport`; `DoorCoupling::Coupled` re-pairs doors into genuinely two-way connections, `Decoupled` reassigns each independently), `current_house_doors` / `randomize_house_doors` for intra-town house doors (`HouseDoorApplyReport`; per-scene MOVE_TO tile shuffle), `current_starting_items` / `randomize_starting_items` for the new game's starting inventory (`StartingItemsApplyReport`; rewrites the SCUS seed code with `n` random consumables), and `inject_seru_bell_name` (names the unnamed accessory).

**Coupling.** `Decoupled` uses the full variable-length relocation, so any destination can land in any door (a scene that overflows on rebuild is skipped). `Coupled` instead restricts itself to **length-preserving** swaps — it re-pairs only balanced connections (equal door counts each direction) whose names match in length, so the decompressed MAN size never changes and no scene (including the un-growable overworld hubs) can overflow. That keeps every reconnection genuinely two-way (walk through a door, turn around, return the way you came) and introduces **zero** new one-way edges; doors with no length-compatible reverse partner are left at their original destination and reported as `unpaired`. |
| `ppf` | PPF 3.0 patch writer. `diff_runs` reduces original-vs-patched to the changed byte runs, `write_ppf3` serializes them, `apply_ppf3` replays a patch (used by the round-trip test). The PPF is the redistributable deliverable — it ships only deltas the user already owns. |

## CLI (`legaia-rando`)

The top-level binary reads a user-supplied disc, plans a randomization from a
seed, and emits a portable **PPF 3.0** patch plus (optionally) a full patched
`.bin` for local play. The shareable artifacts are the patcher and the seed; a
patched `.bin` contains Sony bytes and must never be redistributed.

```bash
# Read-only: list every monster's current drop (with item names from the disc).
legaia-rando drops --input "Legend of Legaia (USA).bin"

# Read-only: list every treasure chest the randomizer would touch, grouped by
# scene, plus the item-multiset summary -- audit which items would change before
# committing (e.g. to spot quest items that should stay static).
legaia-rando chests --input "Legend of Legaia (USA).bin"

# Shuffle drops from a memorable seed -> a portable patch (default <input>.ppf).
legaia-rando randomize --input DISC.bin --seed myrun --drops shuffle

# Shuffle chests but keep quest / key items static (the default protected set).
# Override with --keep-static-items 0x9a,0x71,...  (or "" to randomize all).
legaia-rando randomize --input DISC.bin --seed myrun --chests shuffle

# Random drops + shuffled encounters + shuffled chests + shuffled steals + image.
legaia-rando randomize --input DISC.bin --seed 0xC0FFEE --drops random \
    --encounters shuffle --chests shuffle --steals shuffle --patch run.ppf \
    --output patched.bin --manifest run.toml

# Read-only: audit what the Evil God Icon steals from each monster.
legaia-rando steals --input DISC.bin

# Read-only: list every scene-transition door/exit (home scene -> destination).
legaia-rando doors --input DISC.bin

# Read-only: list the intra-town (house) MOVE_TO target tiles per scene.
legaia-rando house-doors --input DISC.bin

# Experimental: shuffle intra-town house doors (per-scene MOVE_TO tile shuffle).
legaia-rando randomize --input DISC.bin --seed myrun --house-doors shuffle

# Bidirectional door shuffle (walk back the way you came).
legaia-rando randomize --input DISC.bin --seed myrun --doors shuffle --door-coupling coupled

# Start the new game with 3 random items instead of the fixed Healing Leaf x5.
legaia-rando randomize --input DISC.bin --seed myrun --starting-items 3

# Read-only: show the new game's current starting bag.
legaia-rando starting-items --input DISC.bin

# Confirm a shared patch applies cleanly to your own disc before playing.
legaia-rando verify --input DISC.bin --patch run.ppf
```

`--drops` / `--encounters` / `--chests` / `--steals` / `--doors` each take
`shuffle` / `random` / `none`; `--door-coupling` is `coupled` (default,
bidirectional) or `decoupled` (one-way); `--starting-items N` seeds the new
game with `N` random consumables (`0` = vanilla Healing Leaf ×5; capped at 5).
`--unused-enemies` adds the unused Evil Bat to the Random encounter pool (needs
`--encounters random`); `--unused-items` adds Something Good + the "Seru Bell"
accessory to the Random fill pool (and names the accessory).
`--dry-run` plans + reports the run without writing any files. `--manifest`
writes a small TOML record of the seed + options + change counts (no game
bytes — safe to share). `verify` applies a PPF to a copy of your disc and
confirms the result still parses (records applied, PROT entry + drop counts).

`--seed` takes a number (decimal or `0x`-hex) or any string (hashed stably to a
number); the resolved numeric seed is always printed so a run reproduces
exactly. `--drops` and `--encounters` each take `shuffle` / `random` / `none`.
`--drops random` needs the SCUS item table off the disc for the valid item
pool; the others need no external table. `--encounters` reassigns each scene's
formation monster ids from that scene's own id set, so every swapped-in monster
is one the scene already loads (no missing model).

## How an edit reaches the disc

A PROT-entry-relative offset maps to the PROT.DAT-logical offset
`start_lba[entry] * 2048 + offset_in_entry`, which
`legaia_iso::write::patch_file_logical` turns into physical-sector writes plus
EDC/ECC re-encode. Every edit is same-size and in place — no LBA, TOC, or
directory record moves.

## Tests

- Synthetic (CI): planner determinism + shuffle-preserves-the-multiset, RNG
  stability, surgical `set_drop`, PPF diff/write/apply round-trip, and a patch
  round-trip through a hand-built Mode 2 disc with a real-format PROT TOC.
- Disc-gated: edit real monster drops and a real monster slot on a scratch copy
  of the disc, asserting the edit is surgical (drop applied; everything else
  byte-identical) and the patched sectors stay EDC/ECC-valid; a full-archive
  shuffle that plans, applies, diffs into a PPF, and confirms the PPF reproduces
  the patched image (and is deterministic for a fixed seed); a whole-disc
  encounter shuffle that re-decodes every patched scene MAN off the disc and
  asserts counts + id multiset preserved, ids in-pool, sectors valid, and
  deterministic; and a whole-disc chest shuffle asserting give-item site offsets
  unchanged, the chest-item multiset preserved, sectors valid, and deterministic.

```bash
cargo test -p legaia-rando                                   # synthetic only
LEGAIA_DISC_BIN="/path/to/Legend of Legaia (USA).bin" \
    cargo test -p legaia-rando                               # + disc-gated
```

The patched image is never written to disk by the tests; it lives only in
memory. A patched `.bin` contains Sony data and must never be committed.

## See also

- [`docs/tooling/randomizer.md`](../../docs/tooling/randomizer.md) — design + the patch chain.
- [`crates/lzs`](../lzs/README.md) — the LZS encoder (`compress`) re-packing relies on.
- [`crates/iso`](../iso/README.md) — Mode 2/2352 sector write-back (`write` module).
- [`crates/asset`](../asset/README.md) — the monster-archive + item-name parsers it edits.
