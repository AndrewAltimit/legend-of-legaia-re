# legaia-rando

Randomizer / disc patcher for a user-supplied Legend of Legaia disc.

Edits gameplay data on the user's own `.bin` and writes it back: monster item
drops (optionally as rare equipment), random-encounter formations, treasure-chest
contents, steal items, Tactical-Arts button combos, doors, and starting items. It is
Track-1-adjacent tooling — it does **not** touch the clean-room engine — and it
ships only code: no game bytes are embedded or committed, and every test that
needs real data is disc-gated.

See [`docs/tooling/randomizer.md`](../../docs/tooling/randomizer.md) for the
full design.

## Contents

- [Foundations](#foundations) — [`rng`](#rng), [`items`](#items), [`monster`](#monster), [`disc`](#disc)
- Randomizer features
  - [Drops](#drops)
  - [Equipment-as-drops](#equipment-as-drops)
  - [Encounters](#encounters)
  - [Chests](#chests)
  - [Steals](#steals)
  - [Arts](#arts)
  - [Doors](#doors)
  - [House doors](#house-doors)
  - [Shops](#shops)
  - [Casino](#casino)
  - [Starting items](#starting-items)
  - [Item prices](#item-prices)
  - [Unused content](#unused-content)
  - [Name injection](#name-injection)
- [Orchestration (`apply`)](#orchestration-apply)
- [Door coupling](#door-coupling)
- [PPF output (`ppf`)](#ppf-output-ppf)
- [CLI (`legaia-rando`)](#cli-legaia-rando)
- [How an edit reaches the disc](#how-an-edit-reaches-the-disc)
- [Tests](#tests)
- [See also](#see-also)

## Foundations

These modules underpin every feature: the deterministic PRNG, the valid item
pool, the monster-archive re-packer, and the disc write-back.

### `rng`

Version-stable `SplitMix64` PRNG. A published seed always reproduces a run,
independent of any external generator's algorithm (the first output for seed 0 is
pinned by a test).

### `items`

The valid item-id pool from the SCUS item-name table (`legaia_asset::item_names`),
so a randomized drop is always an item the game has a name and handler for.

- `valid_item_pool` — the pool builder.
- `DEFAULT_STATIC_CHEST_ITEMS` — curated quest/key items kept out of chest
  randomization.

### `monster`

Re-pack a monster slot in the `battle_data` archive (PROT 867).

- `repack_slot` decompresses the `0x14000`-byte slot, hands the decoded record to
  an in-place mutator, recompresses with `legaia_lzs::compress`, and zero-pads back
  to the original slot size so no offset moves.
- `set_drop` is the drop-id + chance wrapper.

### `disc`

`DiscPatcher`: own a mutable disc image, locate PROT.DAT + read its TOC, and apply
same-size PROT-entry edits via the Mode 2/2352 sector write-back in
`legaia_iso::write`.

- `patch_monster_slot` / `monster_slot` are the `battle_data` helpers.
- `patch_prot_entry` is the generic form.
- `patch_named_file` handles non-PROT files (like `SCUS_942.54`).

## Drops

The drop-table planner (`drops` module).

`plan_drops` reassigns the monsters that currently drop something, in:

- `Shuffle` mode — redistribute the existing drops (preserves the economy).
- `Random` mode — draw from the item pool.

Deterministic in `(current drops, pool, seed, mode)`.

## Equipment-as-drops

Equipment-as-enemy-drops (`equipment` module).

- `equipment_pool` classifies which item ids are gear by matching the curated
  public weapon/armor/accessory names (`legaia_gamedata`) against the disc's own
  item-name table (no Sony bytes; ids come from the user's disc).
- `plan_equipment_drops` turns **every** monster's drop slot into a rare random
  equipment piece. The chance is "both combined" — the lower of the item's price
  tier and the enemy's EXP tier (early 3 % / mid 2 % / late 1 %). The retail roll
  is integer `rand() % 100`, so the requested late-game 0.5 % is floored to the
  1 % minimum.

## Encounters

Random-encounter randomizer (`encounter` module).

- `SceneEncounters::locate` finds a scene bundle's MAN inside a PROT entry and
  decompresses it.
- `randomize` rewrites the formation monster ids from the scene's own id pool (so
  every monster stays scene-loaded) in `Shuffle` / `Random` mode.
- `repack` recompresses and reports whether it fits the original footprint.

**Only random formations are touched** — a formation is random iff a
`rate_increment > 0` region's `[base, +count)` range reaches it
(`random_formation_mask` / `is_random_formation`), so scripted/boss fights like
Tetsu (reached only by rate-0 regions) are left byte-identical.

`randomize_with_extra` unions extra ids (the unused enemies) into the Random pool
— see [Unused content](#unused-content).

## Chests

Treasure-chest / scripted item-gift randomizer (`chest` module).

Gives go through field-VM op `0x39` (`[0x39, item_id]`, inline operand) in the MAN
partition-1 interaction scripts, usually **after** the inline dialogue that
announces the item.

- `give_item_sites` walks each record's script with the Track-1 field-VM
  disassembler, **skipping `0x1F` dialogue segments** so it reaches the
  post-announcement give, and bounds each walk to the record's extent so it never
  mis-reads a `0x39` data byte.
- `SceneChests::locate` bundles the sites with the decoded MAN for rewriting (275
  sites / 50 scenes on the retail disc).

A chest also names its item in a **separate** dialogue token `0xC2 <id>` (the
"There is a {item}…" announcement) distinct from the `0x39` grant.

- `give_sites_and_display_tokens` recovers each site's `0xC2` tokens.
- `SceneChests::set_site` rewrites the operand **and** those tokens together — the
  flavor text tracks what the chest actually grants.

## Steals

Steal-item randomizer (the Evil God Icon) (`steal` module).

- `StealEdits::locate` reads the static `SCUS_942.54` steal table (`DAT_80077828`,
  per-monster `[chance, item]`, see
  [`steal-table.md`](../../docs/formats/steal-table.md)).
- `plan` reuses the drop planner to reassign the item for every stealable monster
  (`Shuffle` redistributes the existing steal-item multiset, `Random` draws from
  the item pool).
- `item_patches` emits same-size single-byte SCUS edits that touch the **item**
  only — the steal chance is preserved. No LZS re-pack, so nothing is ever skipped.

## Arts

Tactical-Arts button-combo randomizer (`arts` module).

Each combo lives in **two** files:

- The **matcher** (what fires the art) reads the `1=L,2=R,3=D,4=U` combo at record
  `+0` (fixed `0xD0` stride) in each character's player-data file `record0` (Vahn
  `PROT 0861`, Noa `0864`, Gala `0865`).
- The **display** is the SCUS `DAT_80075EC4` `+8` glyph string.

Editing only the SCUS copy changes the menu but not the trigger (two emulator
playtests proved it), so `randomize_arts` patches both:

- `patch_player_record0` decompresses `record0`, rewrites each art's combo bytes in
  place (clean-start search filtered to the `0xD0` grid; multi-record arts get all
  their records), recompresses to fit.
- The SCUS glyph string is rewritten to the same combo.

The assignment permutes the distinct display strings' contents within each length
class (display strings are deduplicated across characters), so **input count is
preserved** and each character's combos stay unique by construction.

- `Shuffle` reassigns existing same-length combos.
- `Random` writes fresh same-length combos.
- The Miracle Art (`0xFF09`) is left untouched.

## Doors

Scene-transition ("door / exit") randomizer (`door` module).

Doors are the field-VM `0x3F` named-scene-change ops — **partition-2 MAN records**
reached via the partition-2 record-offset table (see
[`man-relocation.md`](../../docs/formats/man-relocation.md)).

- `SceneDoors::locate` enumerates a scene's door sites
  (`legaia_asset::man_edit::scene_change_sites`).
- `rebuild` applies destination rewrites through the **variable-length** `man_edit`
  relocation engine, recompresses, validates, and reports whether it fits the
  footprint.

The only randomizer that resizes an asset. See [Door coupling](#door-coupling)
for the `Coupled` / `Decoupled` semantics.

## House doors

Intra-town ("house / interior") door randomizer (`house_door` module).

Entering a house is a field-VM MOVE_TO to an interior tile within the
**same** scene (intra-scene reposition, pinned via `probe.step.find_writer`; writer
in `FUN_801de840` `case 0x23`). The door warp has a clean structural signature:
the **cross-context player form `0xA3 0xF8 xb zb`** (opcode `0x23 | 0x80`
dispatched into the system/player channel `0xF8`) inside a **named partition-0
door record** — record names pair fullwidth ＩＮ/ＯＵＴ, the 入口/出口 kanji,
or trailing Ａ/Ｂ endpoint letters. Plain `0x23` MOVE_TOs are actor (NPC /
prop / cutscene) positioning and are never touched.

- `SceneHouseDoors::locate` enumerates a scene's classified door warps
  (partition-0 record walk with the chest module's inline-dialogue skip).
- `shuffle` does a per-scene, **class-preserving** shuffle: interior landing
  tiles permute among ＩＮ sites, exterior doorsteps among ＯＵＴ sites
  (same-size 2-byte operand edits) — every exit still lands outside, so no
  interior-to-interior softlock is constructible.

Shuffle-only. Census + signature + the runtime-captured Mei's-house anchor are
pinned by the disc-gated `house_door_classifier_real` test.

## Shops

Town-merchant shop randomizer (what stores sell) (`shop` module).

A gold shop's stock is **inline in the scene's field-VM script** (the MAN), opened
by field-VM op `0x49` (`STATE_RESUME`, the `_DAT_8007B450` menu-register driver)
carrying `[u8 count][count× item_id][ASCII name\0]` (pinned from live Rim Elm +
Biron captures).

- `SceneShops` finds sites by **scanning** the MAN for the op-`0x49` sub-0
  signature — *not* an opcode walk, which desyncs on shops gated behind a Yes/No
  "Buy them?" picker (Biron's Corey) and silently misses them. Strict validation
  (sub-op byte `0`, small non-zero count, all ids non-zero + SCUS-named via
  `locate_with_items`, printable letter-initial name) rules out false positives.
- `set_id` rewrites an item-id byte (same-size).
- `repack` recompresses the MAN.

Global shuffle/random in `apply`; `Random` draws from the priced sellable pool (see
[Item prices](#item-prices)) so no quest items are sold.

## Casino

Casino prize-exchange randomizer (`casino` module).

Unlike town shops, the casino prizes are a **static raw table** in the menu
overlay's data segment — PROT entry 899 (`0899_xxx_dat`), file offset `0x15D00`,
four `0x60`-byte blocks of 8-byte `[u16 id][u16 story-gate][u32 coin-price]` records
(it debits casino *coins*, `_DAT_800845A4`, not gold — which is how it's told apart
from a gold shop).

`CasinoExchange::parse`/`randomize`/`write_back` shuffle/random the whole records
(price + gate travel with the prize), same-size in place (no LZS).

## Starting items

New-game starting-inventory randomizer (`starting_items` module).

There is no static starting-inventory table — the new-game data-init `FUN_80034A6C`
code-builds the bag, writing one slot (Healing Leaf `0x77` ×5) into the live
consumable inventory (see [`legaia_asset::new_game::StartingInventory`]). So this
rewrites the **seed code**: the reclaimable 40-byte region at `0x80034b04` (the
original seed + a redundant inline zero-loop both callers already cover with their
`SC`-block `memset`).

- `plan_starting_items` picks `n` distinct random consumables from
  `STARTING_ITEM_POOL` (`0x77..=0x8e`) with small random counts.
- `build_seed_patch` encodes them as one packed halfword store per slot
  (`addiu $v0,(count<<8)|id; sh $v0,off($s0)`), capping at `MAX_STARTING_ITEMS` = 5.
  Same-size code patch (no executable growth), applied via `patch_named_file`.

### Door-of-Wind / all-warps toggles

`StartingSeedOptions` / `plan_seed` extend it with two Door-of-Wind convenience
toggles:

- `door_of_wind` (a count, default 10, clamped to 99) forces that many Door of Wind
  (`0x89`) into a slot.
- `all_warps` presets the all-towns visited bitmask (`0x8008575C = 0xF77F`,
  `0x8008575E = 0xF8FF`).

The warp preset gets its OWN reclaimable region (`build_warp_patch` at
`0x80034adc`, four redundant `sw $zero` stores; uses `$v1` to spare the live `$v0`),
so it does NOT reduce the item budget — items keep all five slots.

## Item prices

Item shop-price edits + the sellable pool (`item_price` module).

The shop price is the `u16` at item-record `+2` (table base `0x80074368`); price
`0` marks a quest/found-only item.

- `sellable_pool` = ids priced `> 0` (the shop `Random` pool — auto-excludes quest
  items).
- `CHEST_EQUIPMENT_PRICES` gives the 13 chest-found Ra-Seru/Astral gear (which ship
  free) reviewed values (~28800–55000), and `price_patches` emits the same-size SCUS
  edits so they aren't free + join the pool.

## Unused content

Curated "unused content" the opt-in toggles re-introduce (`unused` module).

- `UNUSED_ENEMY_IDS` = "Comm" (id 78, a standalone unused enemy) + the Evil Bat
  clones 176/177/178 (no formation references them, but the battle loader streams a
  monster slot on demand by id, so adding one to a scene's encounter Random pool via
  `SceneEncounters::randomize_with_extra` is enough to spawn it).
- `UNUSED_ITEM_IDS` = Something Good `0x6B` + the unnamed accessory `0xFD`, unioned
  into the random-fill item pool by `extend_pool`.

## Name injection

`item_name` module — `NameInjection`: name the otherwise-blank accessory `0xFD`
"Seru Bell" so `--unused-items` hands out a presentable item.

A same-size SCUS patch — write the string into preserved rodata padding
(`SERU_BELL_STRING_VA = 0x8007AB40`, pinned for the US build inside a 1028-byte zero
gap flanked by rodata constants proven preserved file→RAM; **not** the data-segment
zero-fill tail, which is `.sbss` scratch the game clobbers, nor an arbitrary
always-zero region, which can be boot-cleared) and repoint only `0xFD`'s
`name_ptr_slot`, leaving the other empty-name ids blank.

## Orchestration (`apply`)

High-level orchestration the CLI drives. Each feature has a read-only collector and
a randomize entry that emits a per-feature `*ApplyReport`.

| Feature | Read-only | Randomize | Notes |
|---|---|---|---|
| Drops | `current_drops` | `apply_drop_plan` / `randomize_drops` | a `DropApplyReport` records any slot too tight to re-pack. |
| Equipment drops | `current_monster_exp` | `randomize_equipment_drops` | every monster's slot → a rare tiered equipment drop, reusing `apply_drop_plan`. |
| Shops | `current_shops` | `randomize_shops` | `ShopApplyReport`; first `apply_item_price_edits` prices the chest-found equipment, then `Random` draws from the priced sellable pool so no quest item is sold. |
| Casino | `current_casino` | `randomize_casino` | the casino prize exchange. |
| Encounters | — | `randomize_encounters` | per-scene formations (`EncounterApplyReport`; takes an `unused_enemies` id slice unioned into the Random pool). |
| Chests | — | `randomize_chests` | treasure (global shuffle/random of chest item ids → `ChestApplyReport`); honors a `keep_static` id set — kept items never move and never enter the shuffle/random pool. |
| Steals | `current_steals` | `randomize_steals` | the steal table (`StealApplyReport`). |
| Arts | `current_arts` | `randomize_arts` | Tactical-Arts button combos (`ArtsApplyReport`; same-size `+8` pointer reassignment, input count + within-character uniqueness preserved). |
| Doors | `current_doors` | `randomize_doors` | scene transitions (`DoorApplyReport`; takes a `DoorCoupling` = `Coupled` re-pairs doors into genuinely two-way connections / `Decoupled` reassigns each independently). |
| House doors | `current_house_doors` | `randomize_house_doors` | intra-town house doors (`HouseDoorApplyReport`; per-scene class-preserving shuffle of the player door warps, shuffle-only). |
| Starting items | `current_starting_items` | `randomize_starting_items` | the new game's starting inventory (`StartingItemsApplyReport`; rewrites the SCUS seed code with `n` random consumables). |
| Name injection | — | `inject_seru_bell_name` | names the unnamed accessory. |

## Door coupling

`Decoupled` uses the full variable-length relocation, so any destination can land
in any door (a scene that overflows on rebuild is skipped).

`Coupled` instead restricts itself to **length-preserving** swaps — it re-pairs
only balanced connections (equal door counts each direction) whose names match in
length, so the decompressed MAN size never changes and no scene (including the
un-growable overworld hubs) can overflow. That keeps every reconnection genuinely
two-way (walk through a door, turn around, return the way you came) and introduces
**zero** new one-way edges; doors with no length-compatible reverse partner are left
at their original destination and reported as `unpaired`.

## PPF output (`ppf`)

PPF 3.0 patch writer.

- `diff_runs` reduces original-vs-patched to the changed byte runs.
- `write_ppf3` serializes them.
- `apply_ppf3` replays a patch (used by the round-trip test).

The PPF is the redistributable deliverable — it ships only deltas the user already
owns.

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

# Read-only: list the intra-town (house) door-warp target tiles per scene.
legaia-rando house-doors --input DISC.bin

# Shuffle intra-town house doors (per-scene class-preserving warp shuffle).
legaia-rando randomize --input DISC.bin --seed myrun --house-doors shuffle

# Bidirectional door shuffle (walk back the way you came).
legaia-rando randomize --input DISC.bin --seed myrun --doors shuffle --door-coupling coupled

# Start the new game with 3 random items instead of the fixed Healing Leaf x5.
legaia-rando randomize --input DISC.bin --seed myrun --starting-items 3

# Read-only: show the new game's current starting bag.
legaia-rando starting-items --input DISC.bin

# Read-only: list what each town store sells / the casino prize exchange.
legaia-rando shops  --input DISC.bin
legaia-rando casino --input DISC.bin

# Every monster drops rare tiered equipment instead of its normal drop.
legaia-rando randomize --input DISC.bin --seed gear --equipment-drops

# Randomize what stores sell (quest items excluded) + the casino prizes.
legaia-rando randomize --input DISC.bin --seed mart --shops random --casino shuffle

# Confirm a shared patch applies cleanly to your own disc before playing.
legaia-rando verify --input DISC.bin --patch run.ppf
```

### Randomize flags

- `--drops` / `--encounters` / `--chests` / `--shops` / `--casino` / `--steals` /
  `--arts` / `--doors` each take `shuffle` / `random` / `none`.
- `--equipment-drops` instead turns every monster's drop into rare tiered equipment
  (overrides `--drops`).
- `--door-coupling` is `coupled` (default, bidirectional) or `decoupled` (one-way).
- `--starting-items N` seeds the new game with `N` random consumables
  (`0` = vanilla Healing Leaf ×5; capped at 5).
- `--door-of-wind [N]` adds `N` Door of Wind (the warp consumable; default 10) to
  the starting bag.
- `--all-warps` presets the visited-towns story-flag bitmask so Door of Wind can
  teleport to any town from the start (both ride the same reclaimable seed region as
  the starting items).
- `--unused-enemies` adds the unused "Comm" + Evil Bat enemies to the Random
  encounter pool (needs `--encounters random`).
- `--unused-items` adds Something Good + the "Seru Bell" accessory to the Random
  fill pool (and names the accessory).
- `--keep-static-items` overrides the protected chest set.

### Run-control flags

- `--dry-run` plans + reports the run without writing any files.
- `--manifest` writes a small TOML record of the seed + options + change counts (no
  game bytes — safe to share).
- `verify` applies a PPF to a copy of your disc and confirms the result still parses
  (records applied, PROT entry + drop counts).

### Seed + pool notes

- `--seed` takes a number (decimal or `0x`-hex) or any string (hashed stably to a
  number); the resolved numeric seed is always printed so a run reproduces exactly.
- `--drops` and `--encounters` each take `shuffle` / `random` / `none`.
- `--drops random` needs the SCUS item table off the disc for the valid item pool;
  the others need no external table.
- `--encounters` reassigns each scene's formation monster ids from that scene's own
  id set, so every swapped-in monster is one the scene already loads (no missing
  model).

### Read-only listings

`drops` / `chests` / `shops` / `casino` / `steals` / `arts` / `doors` /
`house-doors` / `starting-items` each list the current data for that feature.

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
  asserts counts + id multiset preserved, ids in-pool, sectors valid,
  deterministic, **and that scripted/boss formations (Tetsu, …) stay
  byte-identical**; a whole-disc chest shuffle asserting give-item site offsets
  unchanged, the chest-item multiset preserved, sectors valid, and deterministic;
  an equipment-drop pass asserting every monster drops a pool equipment id at a
  tiered chance; a town-shop + casino pass (Variety Store + its 10 ids
  enumerate, shuffle preserves the multiset/counts, casino preserves the
  prize set); and the item-price edits (the 13 chest-equipment items get their
  reviewed values, the sellable pool excludes quest ids, and a shop `Random`
  pass only stocks priced items).

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
