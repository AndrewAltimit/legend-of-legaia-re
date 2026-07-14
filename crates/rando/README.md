# legaia-rando

Randomizer / disc patcher for a user-supplied Legend of Legaia disc.

Edits gameplay data on the user's own `.bin` and writes it back: monster item
drops (plus an optional additive low-chance bonus equipment drop, injected into
the battle-end reward routine), random-encounter formations (plus an optional
experience reward on a successful escape, injected into the escape teardown),
treasure-chest contents, steal items, Tactical-Arts button combos, doors
(scene-transition, house-script and `.MAP` kind-0 map doors),
starting items,
starting level, equipment passive stat bonuses, weapon specialty (which class
each character favors), and a set of battle-tuning tables (monster combat stats,
special-attack power, the element-affinity matrix, spell MP costs). It is
Track-1-adjacent tooling - it does **not** touch the clean-room engine - and it
ships only code: no game bytes are embedded or committed, and every test that
needs real data is disc-gated.

See [`docs/tooling/randomizer.md`](../../docs/tooling/randomizer.md) for the
full design.

## Contents

- [Foundations](#foundations) - [`rng`](#rng), [`items`](#items), [`monster`](#monster), [`disc`](#disc)
- Randomizer features
  - [Drops](#drops)
  - [Bonus equipment drop](#bonus-equipment-drop)
  - [Encounters](#encounters)
  - [Run-away EXP](#run-away-exp)
  - [Enemy ally (charm)](#enemy-ally-charm)
  - [Shiny Seru](#shiny-seru)
  - [Seru trading](#seru-trading)
  - [Chests](#chests)
  - [Steals](#steals)
  - [Monster stats](#monster-stats)
  - [Move power](#move-power)
  - [Element affinity](#element-affinity)
  - [Spell cost](#spell-cost)
  - [Equipment bonuses](#equipment-bonuses)
  - [Weapon specialty](#weapon-specialty)
  - [Arts](#arts)
  - [Doors](#doors)
  - [House doors](#house-doors)
  - [Map doors](#map-doors)
  - [Shops](#shops)
  - [Casino](#casino)
  - [Starting items](#starting-items)
  - [Starting level](#starting-level)
  - [Item prices](#item-prices)
  - [Unused content](#unused-content)
  - [Name injection](#name-injection)
- [Translation packs](#translation-packs)
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

- `valid_item_pool` - the pool builder.
- `default_static_chest_items` - the chest randomizer's disc-derived default
  keep-static set: the data-driven quest/key/story items
  (`item_price::quest_item_ids` = named, price-0 items minus the chest-found
  equipment). Keeps every unsellable quest item out of chest randomization
  automatically, with no hand-list to maintain; buyable items (e.g. the Silver
  Compass accessory) stay randomizable.
- `DEFAULT_STATIC_CHEST_ITEMS` - the curated fallback (a subset of the
  disc-derived set) used only when the item table can't be read.

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

- `Shuffle` mode - redistribute the existing drops (preserves the economy).
- `Random` mode - draw from the item pool.

Deterministic in `(current drops, pool, seed, mode)`.

## Bonus equipment drop

A code hook that grants one *extra* random equipment piece on a low per-battle
chance, **on top of** the normal drop (`bonus_drop` + `equipment` modules).

A monster record has a single drop slot (`+0x48` item / `+0x49` chance), so no
data edit can make a monster drop two things - turning the slot into equipment
would destroy the normal drop. So this feature is **additive by code injection**,
not a data edit: it patches the executable's reward routine, like the
[starting bag](#beyond-the-direct-cap-starting_bag-module) splices a grant into
the opening scene.

- `equipment::equipment_pool` / `equipment_ids` classify which item ids are gear
  by matching the curated public weapon/armor/accessory names (`legaia_gamedata`)
  against the disc's own item-name table (no Sony bytes; ids come from the user's
  disc; the stray in-range consumable *Honey* is excluded by name).
- `bonus_drop::BonusDropInjection::plan` / `assemble_routine` build the injection.
  The battle-end reward routine `FUN_8004E568` tallies a battle's spoils once
  (gated on `actor+0x6ce == 0`). Right after the normal drop grant
  `FUN_800421d4(item, 1)` at `0x8004f608`, control joins at `0x8004f610`; the two
  instructions there (`lui v0,0x8008` / `lw v0,-0x4540(v0)`) are overwritten with
  `j <routine>` + `nop`. The injected routine rolls `rand() % 100 < chance`
  (default `DEFAULT_CHANCE_PCT` = 5, via the battle RNG `FUN_80056798`), then
  `rand() % table_len` to index an embedded equipment-id table, calls
  `FUN_800421d4(id, 1)` (an unguarded add, like the minigame reward
  `FUN_801C2748`), replays the two displaced instructions, and `j`s back. The
  join is reached once per battle, so the roll fires once per battle.
- The routine + id table live in the 1028-byte preserved rodata gap at
  `0x8007AB38` (the same loaded-and-preserved padding [name injection](#name-injection)
  uses, at a non-overlapping offset clear of the Seru-Bell string) - on PSX all
  resident RAM is executable. Every write is a same-size in-place `SCUS_942.54`
  edit; the planner guards on the detour-site words matching the known US build
  and the routine region being all-zero dead space, refusing a different layout.

The grant is silent (no victory-screen "received" line - the gear just appears in
the bag). `apply::inject_equipment_bonus_drop` performs the two edits.

## Encounters

Random-encounter randomizer (`encounter` module).

- `SceneEncounters::locate` finds a scene bundle's MAN inside a PROT entry and
  decompresses it.
- `randomize` rewrites the formation monster ids from the scene's own id pool (so
  every monster stays scene-loaded) in `Shuffle` / `Random` mode.
- `apply::randomize_encounters_scoped` widens that pool via `EncounterScope`:
  `Scene` (default), `Kingdom` (any monster in the same Drake/Sebucus/Karisto
  kingdom - partition derived from `CDNAME.TXT` in the `kingdom` module), or
  `World` (any monster on the disc, so late-game monsters can appear at the
  start). `Shuffle` conserves the scope-wide multiset via a lock-and-reshuffle
  fixpoint that survives re-pack skips.
- `repack` recompresses and reports whether it fits the original footprint.

**Only random formations are touched** - a formation is random iff a
`rate_increment > 0` region's `[base, +count)` range reaches it
(`random_formation_mask` / `is_random_formation`), so scripted/boss fights like
Tetsu (reached only by rate-0 regions) are left byte-identical.

**An explicit id guard backs that heuristic.** The region-rate test correctly
marks every story boss's formation scripted except the early **Gimard**
Seru-boss fight, whose formation sits at an index a rate>0 region's range spans -
so the heuristic alone would treat it as random and a roll could replace that
mandatory tutorial fight or donate the boss-tier enemy into an early encounter.
`PROTECTED_FORMATION_IDS` (Gimard) lists the ids that must never be a random
encounter; `locate` forces any formation holding one back to scripted and keeps
it out of every donor pool, so the fight ships exactly as authored. (The first
wild Piura are *not* listed - they are genuine random encounters.) Mirrors the
stat-side `monster_stats::PROTECTED_MONSTER_IDS`, which also pins Gimard.

`randomize_with_extra` unions extra ids (the unused enemies) into the Random pool
- see [Unused content](#unused-content).

### Solo strong fights

`randomize_encounters_full` adds an optional **solo-strong** pass (a
`SoloStrongConfig`): after the ids are assigned, any random formation whose
strongest monster is much stronger than the area's natives is forced down to that
lone enemy, so a wide-pool roll can't gang up 2+ over-strong monsters on the
party.

- Each monster is scored by `monster_stats::combat_power` (the sum of its combat
  stats - every field except MP), built once into a `MonsterPowerTable`.
- The baseline is each scene's **native** average power
  (`SceneEncounters::baseline_power`, read *before* randomizing - the area's
  authored difficulty, a stand-in for how strong the party is there).
- `SceneEncounters::enforce_solo_strong` collapses every multi-monster random
  formation whose strongest member clears `threshold_pct`% of that baseline
  (default `200` = twice as strong): it keeps the strongest monster in slot 0,
  zeroes the rest, and sets `count := 1` - a same-size edit inside the formation
  record's fixed stride.

The pass runs as a post-step over the already-randomized scenes, so it composes
with every scope (Scene / Kingdom / World) and mode (Shuffle / Random) without
disturbing their multiset bookkeeping; `solo == None` reproduces the prior output
byte-for-byte (the archive isn't even read). It only takes effect when encounters
are being randomized.

## Run-away EXP

A code hook that banks a slice of a fled fight's experience into the party on a
**successful escape** (`flee_exp` module). Vanilla awards nothing for running.

Like the [bonus equipment drop](#bonus-equipment-drop), the flee path never
reaches an experience grant, so there is no value to edit - it is **additive by
code injection**, not a data edit.

- `flee_exp::FleeExpInjection::plan` / `assemble_routine` build the injection. The
  per-actor battle state machine `FUN_801E295C` (battle-action overlay, base
  `0x801CE818` = **PROT entry 898**) handles "Run" across states `0x64..0x66`;
  state `0x66` is the **successful-escape teardown** (reached only when the run
  roll succeeds - a failed run goes `0x65 → 0x50`). Its handler entry at
  `0x801E5A10` (`lui v1,0x801d` / `addiu a0,v1,-0x6f90`) is overwritten with
  `j <routine>` + `nop` - a same-size **raw** edit of the overlay PROT entry,
  which maps linearly from its base (`file_off = va - 0x801CE818`).
- The injected routine sums the formation's listed experience (each live enemy
  record's EXP halfword at `+0x46`, via the table at `0x801C9348` for `actor[+1]`
  entries), scales it to `pct`% (default `DEFAULT_PCT` = 5), and adds that to
  **every** party member's cumulative-experience cell (`0x80084140 + (id-1)*0x414
  + 0x5C8`, the slot→id map at `0x8007BD10`), clamped to `9,999,999`, then replays
  the two displaced instructions and `j`s back. State `0x66` runs once per escape,
  and party HP was floored to `≥ 1` a state earlier, so every member is alive at
  the grant.
- The routine lives in the same preserved rodata gap as the bonus-equipment / name
  injections (`0x8007AB38`), at `0x8007AD00`, clear of the equipment routine + its
  id table so both battle hooks coexist. Same guards as the equipment drop (known
  build at the hook, all-zero dead space at the routine).

The grant is **banked**, not an immediate level-up: it only writes the experience
cell, so it shows in the status screen at once and applies as a level the next
time a won battle tallies it. `apply::inject_flee_exp` performs the two edits.

## Enemy ally (charm)

Gives a per-battle chance (`--enemy-ally`, `--enemy-ally-pct`%, default **20**)
that a random enemy fights on the player's side as an uncontrolled ally, in any
**multi-enemy** fight (`enemy_ally` module). Single-enemy fights are skipped (the
routine reads `DAT_8007BD0C[1]` and bails when there is no 2nd monster): charming
the lone enemy of an input-gated tutorial (the Tetsu sparring match) softlocks the
scripted fight, and solo bosses are likewise set-pieces. Retail can't host a genuine 4th
party combatant (battles are hard-wired to 3 party + 4 monster slots), so this
rides the stock **AI-delegated** flag: setting an actor's `+0x16E |= 0x380` makes
the action SM retarget it to the opposite side, so a flagged *monster* attacks the
other monsters. `apply::inject_enemy_ally` performs three same-size edits:

- a **setup detour** at `FUN_800513F0` `0x80051990` (after the monster loop) into
  a routine at `0x8007ACA0` - the gap window between the equipment-drop
  routine+table and the flee-EXP routine, so every gap feature coexists - that
  rolls the chance and OR's `0x380` into the frontmost enemy (actor slot 3),
- a **victory-mask widen** in battle-action overlay 0898 at `0x801E6638`
  (`andi v0,v0,0x4` -> `0x384`), so the charmed enemy counts as "down" in the
  monster-wipe gate and the player needn't defeat their own ally.

Same guards as the other hooks (known build at both sites, all-zero dead space at
the routine). On a solo-enemy boss the lone enemy turns on itself.

## Shiny Seru

Gives a per-battle chance (`--shiny-seru`, `--shiny-pct`%, default **2**) that the
frontmost **capturable** enemy spawns as a rare *shiny* variant: +35% stats at
battle load (and a translucent render), and the Seru you capture from it deals
+35% damage on every future cast, permanently (`shiny_seru` module; mirrors the
engine `seru_learning::SHINY_DAMAGE_BONUS_PCT`). Two cosmetics: the summoned
creature renders semi-transparent and a "+35% DMG!" banner replaces the spell name
on a shiny cast. "Capturable" is decided by indexing the first-monster id
(`DAT_8007BD0C`) into a 256-bit allowlist bitmap built **at patch time** from the
disc's monster names that match a player Seru-magic name (`capturable_monster_ids`
/ `SERU_NAMES`) - NOT the `actor+0x3e` byte (volatile, not a Seru flag).

The persistent bonus is stored in a **parallel per-spell-slot shiny byte at
`record+0x1C0`** (`+0x788` from the runtime base), **not** the spell-level byte's
`0x80` bit (which leaked into the shared spell-level-up+display fn
`FUN_800402f4`). The byte is inside the saved record so it survives a memory-card
save, and a grant-shift hook keeps it slot-aligned across the spell-list
insert-at-front shift. The "+35% DMG!" caption is dropped to Y `0x1E` so it sits
one line below the native "Magic effect:" box instead of overlapping it. Applies
to Seru captured **after** patching.

**Region placement - "zero is not dead" (three times).** A zero run is usable only
if **no code reads it**. Earlier layouts squatted in the zero padding of live
indexed tables three times, each passing `assert_zero` because the bytes *are*
zero: (1) the victory mouth-override table (`ART_MOUTH_VA 0x80077E80`, rows
`0x800781B0..`) -> **corrupted victory mouth**; (2) the move-power table
(`0x801F4FC4`) -> six move ids read garbage; (3) the **`0x80079xxx` SsAPI
sound/effect tables** - the item-use sound engine indexes `0x800794F0` into the
old bitmap, so a Healing Leaf read our bytes as garbage and the item banner never
dismissed (**the Tetsu-tutorial Healing-Leaf freeze**). The fix relocates
everything to regions verified all-zero **and** constant-zero across battle states
**and** outside every known table (a structural `assert_not_in_tables` guard over
`SCUS_TABLE_RANGES` / `OVERLAY_TABLE_RANGES`, now incl. the SsAPI sound ranges)
**and** - the part static checks can't prove - **read-watch-verified unreferenced
on a live battle** (item use, victory, summon cast).

`apply::inject_shiny_seru` performs **nine** same-size detours; all routines/data
are SCUS-resident, in four read-watch-verified-dead regions: gap 1 `0x80077728`
(scratch + setup + capture + capturable bitmap + cast flag + "+35% DMG!" string),
arena 1 `0x8007AE00` (damage / grant / grant-shift / battle-menu flag / field-menu
colour; usable to `0x8007AF00` where the SsAPI I/O table begins), arena 2
`0x8007AFF8` (+35% caption routine), slot 6 `0x80078A88` (summon-fade). Hook sites:
setup boost + capturable
check (`0x80051A20`), capture-copy (`0x801EE2E8`), grant clean-level + shiny-byte
(`0x801E93B4`), grant-shift (`0x801E9320`), damage `×135/100` (`0x801DDB08`),
menu-digit colour (`0x801D2FA0`, overlay 0899), battle-menu flag (`0x801D1B00`),
summon-fade (`0x8004AD0C`), "+35% DMG!" caption (`0x800321D4`). Every routine
honours the R3000 load-delay slot. Same all-zero / known-build guards as the other
hooks, plus the table-overlap guard. On by default in the web Balanced / Full
Chaos presets.

## Seru trading

Adds an **in-shop trading vendor** that runs on real hardware (`--seru-trade`;
`seru_overlay` + `apply::inject_trade_full`). Every merchant grows a fourth
**Buy / Sell / Trade / Quit** row; picking Trade slides a screen in where the
player swaps a party member's learned Seru-magic for a different one, at a level
shown up front. The offer is **time-bucketed** (rotates as play continues) and
fully **deterministic from the seed**.

- **Offer math (`legaia_asset::seru_trade`, shared with the engine).** Each of 64
  buckets holds one `(want, give, give_level)` preference (`give_level` in
  `4..=9`). `bucket_offers` derives the schedule from the seed;
  `bucket_table_to_bytes` serializes it (3 bytes/entry). `expand_offers` maps a
  bucket against the live party to **one line per member who owns the wanted
  seru**, *excluding* members who already own the give-back (a pointless trade).
  Id space is the player Seru-magic block `0x81..=0x95`.
- **Where it lives.** All of it - the picker edits, both stubs, the trade
  handler, the strings, the bucket table, and the runtime cells - is hosted in the
  **menu overlay (PROT 0899)**: two byte-verified edits add the Trade row + route
  a confirm into an unused picker sub-mode, and everything else sits in 0899's
  reference-free ~3.8 KB all-zero dead run (resident throughout the shop). Nothing
  touches the SCUS rodata gap, so seru trading **composes with the
  bonus-equipment-drop / flee-EXP / Seru-Bell-name gap features**.
  `apply::inject_trade_full` writes each piece via `patch_prot_entry(899, …)`,
  guarded as all-zero dead space.
- **The swap** rewrites the chosen owner's spell list in place (id at `+0x13D`,
  level at `+0x161` = the bucket's `give_level`), mirroring
  `engine_core::seru_trade::apply_trade`.
- Cadence: the `0x80084570` counter ticks ~per-frame, so the handler divides by
  `RESEED_PERIOD_FRAMES` (≈9 min/bucket; full cycle ≈9.6 h). The engine track also
  uses a `SeruTradeConfig` blob (`apply::enable_seru_trades`) for its own
  clean-room trade UI off the same kernel.

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
- `SceneChests::set_site` rewrites the operand **and** those tokens together - the
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
  only - the steal chance is preserved. No LZS re-pack, so nothing is ever skipped.

## Monster stats

Monster combat-stat randomizer (`monster_stats` module).

- Redistributes every enemy's HP / MP / ATK / UDF / LDF / INT / SPD across the
  `battle_data` archive (PROT 867), **column-wise**: `plan_stats` permutes each
  stat field across the populated roster (`Shuffle`, multiset-preserving) or
  draws each cell from the column pool (`Random`). The AGL action gauge (`+0x0E`) is left untouched.
- `set_stats` re-packs a monster slot through [`monster::repack_slot`]; the
  decoded length is unchanged, so each slot keeps its `0x14000` footprint (a slot
  too tight to re-pack is skipped, like drops).

## Move power

Special-attack power randomizer (`move_power` module).

- Redistributes the `+0x00` power halfword of the battle-action move-power table
  (`0x801F4F5C`, PROT 0898; enemy specials + Seru-magic, not party arts). Only
  populated records participate, so the index-0 sentinel + empty slots stay zero.
- `plan_powers` permutes the power column (`Shuffle`) or draws from it (`Random`);
  the apply path writes the halfwords back - a same-size raw PROT-0898 edit. Every
  other record byte (geometry, timing, effects, sound) is untouched.

## Element affinity

Element-affinity matrix randomizer (`element_affinity` module).

- Redistributes the 8×8 affinity matrix (`matrix[attacker][defender]`, PROT 0898;
  damage-scale percentages). `plan_matrix` permutes the 64 cells (`Shuffle`,
  multiset-preserving) or draws each (`Random`).
- The per-character element assignment + summon-power rows are left untouched; the
  edit is a same-size raw PROT-0898 write.

## Spell cost

Spell MP-cost randomizer (`spell_cost` module).

- Redistributes the `+3` MP-cost byte of the named, costed spells in the static
  `SCUS_942.54` spell table (`DAT_800754C8`). `plan_costs` permutes the cost
  column (`Shuffle`) or draws from it (`Random`); free / unnamed internal-tier
  spells never participate.
- The apply path emits a same-size in-place SCUS patch via `patch_named_file`
  (like steals). The public `spell_names::stats_file_offset` resolves the offset.

## Equipment bonuses

Equipment passive stat-bonus randomizer (`equip_bonus` module).

- Redistributes the `+0..+4` stat tuple (`INT/ATK/UDF/LDF/SPD`) of the static
  `SCUS_942.54` equipment bonus table (`DAT_80074F68`). `plan_bonus_shuffle`
  groups the rows by their `+7` slot category (body/head/weapon/footwear) and
  permutes the tuples within each category (`Shuffle`) or draws from the
  category pool (`Random`). The `+5/+6/+7` tail (accessory passive / equip mask /
  slot type) never moves, so a tuple never crosses a slot boundary.
- Operates on bonus **rows**, not item ids - several items can share a row, so a
  per-id rewrite would double-edit it (`equip_stats::items_for_rows` maps rows →
  the ids that reach them; only referenced rows participate, so an unused row
  can't hand a real item a junk tuple).
- The apply path emits a same-size in-place SCUS patch via `patch_named_file`.
  The public `equip_stats::bonus_table_file_offset` + `EquipStatTable::rows`
  resolve the table.

## Weapon specialty

Weapon-specialty randomizer (`weapon_specialty` module).

- Reassigns which weapon **class** each character specializes in. In retail,
  equipping a weapon outside a character's favored class (Vahn blades, Noa claws,
  Gala clubs/axes) makes that character's **arm** command (action-gauge command
  `0x0C`) cost more AP in an arts combo. The cost is a per-(character, weapon)
  byte inside each weapon's LZS-compressed section of the player battle file, at
  `decoded_section[+0x04]` (the swing-record offset) `+0x74` (favored `0x1E` /
  off-class `0x2A`). See [`docs/subsystems/arts-command-gauge.md`](../../docs/subsystems/arts-command-gauge.md).
- `plan_favored` permutes the three favored families (`{blade, claw, club}`)
  among the three characters (a seeded bijection - one specialist per class).
  `weapon_family` maps each equippable weapon id to its family; non-class weapons
  (the Astral Sword, armor) map to `None` and are never touched, so the Astral
  Sword stays always-wide.
- The apply path (`apply::randomize_weapon_specialty`) walks the three player
  files (`0863`/`0864`/`0865`), and for each weapon section decompresses it,
  rewrites the arm-cost byte for its new favored relationship, and re-compresses
  in place. A section whose re-compressed stream wouldn't fit its slot is skipped
  (counted in the report) rather than aborting - in practice every section
  re-packs.

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

Doors are the field-VM `0x3F` named-scene-change ops - **partition-2 MAN records**
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
door record** - record names pair fullwidth ＩＮ/ＯＵＴ, the 入口/出口 kanji,
or trailing Ａ/Ｂ endpoint letters. Plain `0x23` MOVE_TOs are actor (NPC /
prop / cutscene) positioning and are never touched.

- `SceneHouseDoors::locate` enumerates a scene's classified door warps
  (partition-0 record walk with the chest module's inline-dialogue skip).
- `shuffle` does a per-scene, **class-preserving** shuffle: interior landing
  tiles permute among ＩＮ sites, exterior doorsteps among ＯＵＴ sites
  (same-size 2-byte operand edits) - every exit still lands outside, so no
  interior-to-interior softlock is constructible.

Shuffle-only. Census + signature + the runtime-captured Mei's-house anchor are
pinned by the disc-gated `house_door_classifier_real` test. The same option
also runs the [map-door pass](#map-doors).

## Map doors

`.MAP` kind-0 intra-scene-teleport randomizer (`map_door` module) - the door
class most house **exits** belong to, and the largest door population on the
disc: `[tile_x][tile_z][dest_x][dest_z]` records in the per-scene `.MAP`
file's `+0x10000` trigger block, no script and no MAN record (retail arm
`FUN_801D1EC4` at `0x801d21c0`; destinations in half-tiles,
`world = (dest_x*64 + 64, (dest_z+1)*64)`).

- `SceneMapDoors::locate` parses a `.MAP` entry's kind-0 sub-table and
  attributes each record's trigger tile + destination to the scene's
  4-connected walk components (the spawn-resolver samplers: object-grid
  walk-visible floor minus collision wall bits).
- `plan_shuffle` permutes the attributable destinations per scene and accepts
  a permutation only when the resulting component graph preserves every
  retail reachability pair and creates no new one-way trap from the main
  component (bounded deterministic retries; unverifiable scenes stay
  vanilla).
- Edits are same-size 2-byte in-place writes into the raw (uncompressed)
  `.MAP` sectors; records past the `.MAP`'s own `0x12000` footprint (the
  fallback window is the next PROT entry) are never touched.

Shuffle-only, driven by the same `--house-doors` option. Round-tripped by the
disc-gated `map_door_patch_real` test and the engine-side
`map_door_randomizer_runtime_e2e` dispatch oracle.

## Shops

Town-merchant shop randomizer (what stores sell) (`shop` module).

A gold shop's stock is **inline in the scene's field-VM script** (the MAN), opened
by field-VM op `0x49` (`STATE_RESUME`, the `_DAT_8007B450` menu-register driver)
carrying `[u8 count][count× item_id][ASCII name\0]` (pinned from live Rim Elm +
Biron captures).

- `SceneShops` finds sites by **scanning** the MAN for the op-`0x49` sub-0
  signature - *not* an opcode walk, which desyncs on shops gated behind a Yes/No
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
overlay's data segment - PROT entry 899 (`0899_xxx_dat`), file offset `0x15D00`,
four `0x60`-byte blocks of 8-byte `[u16 id][u16 story-gate][u32 coin-price]` records
(it debits casino *coins*, `_DAT_800845A4`, not gold - which is how it's told apart
from a gold shop).

`CasinoExchange::parse`/`randomize`/`write_back` shuffle/random the whole records
(price + gate travel with the prize), same-size in place (no LZS).

## Starting items

New-game starting-inventory randomizer (`starting_items` module).

There is no static starting-inventory table - the new-game data-init `FUN_80034A6C`
code-builds the bag, writing one slot (Healing Leaf `0x77` ×5) into the live
consumable inventory (see [`legaia_asset::new_game::StartingInventory`]). So this
rewrites the **seed code**: the reclaimable 40-byte region at `0x80034b04` (the
original seed + a redundant inline zero-loop both callers already cover with their
`SC`-block `memset`).

- `plan_starting_items` picks `n` distinct random consumables from
  `STARTING_ITEM_POOL` (`0x77..=0x8e`) with small random counts.
- `build_seed_patch` encodes them as one packed halfword store per slot
  (`addiu $v0,(count<<8)|id; sh $v0,off($s0)`); the inventory region holds
  `INV_REGION_SLOTS` = 5 and the warp region two more (`MAX_STARTING_ITEMS` = 7).
  Same-size code patch (no executable growth), applied via `patch_named_file`.
  Items past that cap are granted by the `starting_bag` GIVE_ITEM path (below).

### Door-of-Wind / all-warps toggles

`StartingSeedOptions` / `plan_seed` extend it with starting-bag convenience
toggles:

- `door_of_wind` (a count, default 10, clamped to 99) forces that many Door of Wind
  (`0x89`) into a slot.
- `incense` (a count, default 10, clamped to 99) forces that many Incense (`0x8A`,
  the encounter-rate consumable) into a slot - same shape as `door_of_wind`; both
  are seeded first (surviving the five-slot clamp) and excluded from a reroll.
- `speed_chain` / `chicken_heart` / `good_luck_bell` (a count, default 1, clamped
  to 99) force those accessories (`0xD1` / `0xF4` / `0xFC`) into a slot. They are
  "Goods", but the owned-item list is one ordered `(id, count)` array shared by
  every category, so they seed exactly like a consumable.
- `extra_items` is an explicit `(id, count)` list (CLI `--start-with`). Unlike the
  random fill (consumable pool only), it takes **any** id - consumable, equipment,
  or accessory - and is seeded into the forced prefix after the toggles, excluded
  from the reroll, and de-duplicated (id/count `0` and already-seeded ids dropped).
- `all_warps` presets the all-towns visited bitmask (`0x8008575C = 0xF77F`,
  `0x8008575E = 0xF8FF`).

The warp preset shares a second reclaimable region (`0x80034adc`, four redundant
`sw $zero` stores; uses `$v1` to spare the live `$v0`) that does double duty: it
holds EITHER the all-towns bitmask (`build_warp_patch`) OR the two item slots that
overflow the inventory region's five (`build_warp_items_patch`). So the **direct**
seed holds seven items with all-warps off, five with it on, and the random fill
stays additive to the convenience items up to that cap (`plan_seed`); the slots
both regions write are contiguous, and `StartingInventory::from_scus` replays both.

### Beyond the direct cap (`starting_bag` module)

The reclaimable seed code can't grow, so anything past the seven-slot direct cap
(`overflow_bag`) is granted a different way: a run of **silent `GIVE_ITEM` field-VM
ops** (`0x39`) - the same op a treasure chest uses - spliced into the opening scene
`town01`'s entry script, wrapped in a **once-only guard** on a persistent SC
story flag (the `0x50` SET / `0x70` TEST bank at `0x80085758`). The block is emitted
by `starting_bag::guarded_grant_block` (round-tripped through `legaia_asset::field_disasm`)
and inserted **after the entry script's BGM op** via `man_edit::apply_insertions`
(variable-length MAN insert with partition / jump-delta fixups; injecting before the
BGM made the silent gives run before the sound bank loaded and flashed a stray
"WARNING VAB NO …"). `apply::apply_starting_bag` recompresses the MAN and bumps its
descriptor size word. So `direct + overflow` reconstructs the full bag (unit-tested)
and the player gets all the explicit convenience items **plus** the full requested
random count. Disc-gated oracle: `starting_bag_real`.

## Starting level

`apply_starting_level` begins a New Game with the starting party at a chosen level
instead of 1 (`starting_level` module). The **displayed level** is the byte at
`+0x130` (boot-confirmed - *not* derived from experience at a New Game; `+0x100` is
zero in retail), and the seed routine's **record-init loop stamps `+0x130` on every
roster slot**, so the level applies party-wide. Same-size SCUS edits make a level-`N`
start coherent:

- **Level** - the seed loop's level literal + stores set `+0x130 = N` for every party
  record (packed `addiu $v0, (1<<8)|N; sh $v0, 0x6f8($s0); nop`, keeping magic rank
  `+0x131` at 1).
- **Stats** - overwrite **each growth-capable slot's** eight `u16` template stats
  (`PARTY_TEMPLATE_VA`) with that character's level-`N` values, accumulated from the
  disc's deterministic per-level growth curves (`GrowthTables::level_gain_core`) on
  top of the level-1 template. The growth table covers `GROWTH_CHAR_COUNT` characters
  (Vahn/Noa/Gala); the 4th template slot (Terra) has no curve and keeps its base
  stats. This keeps the stats coherent with the level the loop stamps for every slot -
  fixing the prior bug where Noa/Gala showed level `N` with level-1 stats.
- **Experience + threshold** - **each growth-capable slot's** `+0x0` gets the midpoint
  of level `N`'s XP band (from the disc's own `xp_thresholds_from_scus`) and its `+0x4`
  gets `reach(N+1)`. The seed routine never writes `+0x0` natively and seeds `+0x4` by
  storing one shared `$v0` literal, so the edit feeds a single `$t0` preload into three
  `sw $t0, <+0x0>($s0)` stores (repurposing the slot-1/slot-2 threshold reloads and a
  redundant `lui`), and dropping those reloads leaves `$v0` = `reach(N+1)` intact for
  the routine's existing `+0x4` stores - so all three slots take the same threshold.
  The small per-slot `FUN_801E9504` correction is re-applied by the level-up applier on
  each character's first post-seed level-up. The preload is a single 16-bit immediate,
  which caps the level at `MAX_STARTING_LEVEL` (14). Fixes the prior bug where only the
  lead's XP was seeded, so Noa showed experience 0 and Gala a stale level-1 threshold.

The disc-gated `starting_level_real` oracle round-trips every level in range and
checks each leveled slot's stats off the patched image; a companion test runs a
MIPS-subset interpreter over the patched seed routine and asserts every growth
record's `+0x0`/`+0x4`/`+0x130` land correctly.

## Item prices

Item shop-price edits + the sellable pool (`item_price` module).

The shop price is the `u16` at item-record `+2` (table base `0x80074368`); price
`0` marks a quest/found-only item.

- `sellable_pool` = ids priced `> 0` (the shop `Random` pool - auto-excludes quest
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

`item_name` module - `NameInjection`: name the otherwise-blank accessory `0xFD`
"Seru Bell" so `--unused-items` hands out a presentable item.

A same-size SCUS patch - write the string into preserved rodata padding
(`SERU_BELL_STRING_VA = 0x8007AB40`, pinned for the US build inside a 1028-byte zero
gap flanked by rodata constants proven preserved file→RAM; **not** the data-segment
zero-fill tail, which is `.sbss` scratch the game clobbers, nor an arbitrary
always-zero region, which can be boot-cleared) and repoint only `0xFD`'s
`name_ptr_slot`, leaving the other empty-name ids blank.

## Translation packs

`translation` module + the `legaia-rando translate` subcommands
(`export` / `init` / `strip` / `merge` / `stats` / `import`): community
language packs. Exports every cataloged user-facing string into an editable YAML
pack - the SCUS name pools (items, item types, spells, Tactical Arts, accessory
passives, new-game party names) and the `0x1F`-segment dialog corpus
(scene-bundle MANs, LZS-decompressed; plus raw carriers - v12 event-script
prescripts and the streaming-MAN dungeon scenes). Import applies filled
`translation:` fields as same-size in-place patches (strings re-terminated -
budget reclaims the 4-byte-alignment zero padding; dialog segments space-padded
to their exact framing; a scene whose recompress overflows its LZS footprint
rolls back its longest lines one at a time), with per-character encodability
errors for anything outside the retail ASCII glyph set. Untranslated entries
stay byte-identical.

Two pack shapes: a **working** pack carries `source:` (the game's own text - the
translator's reference, gitignored, never committed) while a **distributable**
pack (`translate strip`) drops the source and keeps only `key -> translation`,
so it holds no original script and *is* committable - the shipped
`site/lang/*.yaml` packs are this shape, byte-budget-validated against the disc
at import (the in-pack budget is a hint only). `translate init --resume`
seeds a fresh working pack from a shipped one, `--chunk` splits for a parallel
bulk fill, `merge` recombines. Full workflow + schema:
[`docs/tooling/translation.md`](../../docs/tooling/translation.md).

## Orchestration (`apply`)

High-level orchestration the CLI drives. Each feature has a read-only collector and
a randomize entry that emits a per-feature `*ApplyReport`.

| Feature | Read-only | Randomize | Notes |
|---|---|---|---|
| Drops | `current_drops` | `apply_drop_plan` / `randomize_drops` | a `DropApplyReport` records any slot too tight to re-pack. |
| Equipment drops | - | `inject_equipment_bonus_drop` | injects a code hook into the battle-end reward routine that grants one extra random equipment piece on a low per-battle chance - additive, leaving the normal drop untouched (two same-size `SCUS_942.54` edits via `bonus_drop`). |
| Run-away EXP | - | `inject_flee_exp` | injects a code hook into the battle-action escape teardown that banks a slice of a fled fight's experience into the party on a successful escape - vanilla gives nothing for fleeing (a raw overlay-entry detour + a `SCUS_942.54` routine via `flee_exp`). |
| Enemy ally (charm) | - | `inject_enemy_ally` | injects a code hook into battle setup that, on a per-battle chance, sets the AI-delegated bits (`0x380`) on the frontmost enemy so it fights on the player's side, plus a one-word widen of the victory check so the ally isn't an enemy you must defeat (a `SCUS_942.54` detour + gap routine + an overlay-0898 edit via `enemy_ally`). |
| Shiny Seru | - | `inject_shiny_seru` | injects nine code hooks so that, on a per-battle chance, a capturable enemy spawns with +35% stats (translucent) and its captured Seru deals +35% damage forever, plus cosmetics (translucent summon + a "+35% DMG!" caption below the effect box); the persistent flag is a parallel per-spell shiny byte at `record+0x1C0` (not the spell-level byte), with a grant-shift hook keeping it slot-aligned; all routines/data live in six verified-dead SCUS arenas **outside every live table** (an earlier layout squatted in the victory mouth-override + move-power tables - corrupted mouth + 6 broken moves - now guarded by `assert_not_in_tables`) via `shiny_seru`. |
| Shops | `current_shops` | `randomize_shops` | `ShopApplyReport`; first `apply_item_price_edits` prices the chest-found equipment, then `Random` draws from the priced sellable pool so no quest item is sold. |
| Casino | `current_casino` | `randomize_casino` | the casino prize exchange. |
| Encounters | - | `randomize_encounters` / `randomize_encounters_scoped` / `randomize_encounters_full` | per-scene formations (`EncounterApplyReport`; takes an `unused_enemies` id slice unioned into the Random pool). `_full` adds the optional [solo-strong](#solo-strong-fights) pass (`SoloStrongConfig`) on top of any scope/mode. |
| Chests | - | `randomize_chests` | treasure (global shuffle/random of chest item ids → `ChestApplyReport`); honors a `keep_static` id set - kept items never move and never enter the shuffle/random pool. |
| Steals | `current_steals` | `randomize_steals` | the steal table (`StealApplyReport`). |
| Arts | `current_arts` | `randomize_arts` | Tactical-Arts button combos (`ArtsApplyReport`; same-size `+8` pointer reassignment, input count + within-character uniqueness preserved). |
| Doors | `current_doors` | `randomize_doors` | scene transitions (`DoorApplyReport`; takes a `DoorCoupling` = `Coupled` re-pairs doors into genuinely two-way connections / `Decoupled` reassigns each independently). |
| House doors | `current_house_doors` | `randomize_house_doors` | intra-town house doors (`HouseDoorApplyReport`; per-scene class-preserving shuffle of the player door warps, shuffle-only). |
| Starting items | `current_starting_items` | `randomize_starting_items` | the new game's starting inventory (`StartingItemsApplyReport`; rewrites the SCUS seed code with random consumables + convenience items, additively, across the inventory + warp-preset reclaimable regions). |
| Starting level | `current_starting_level` | `apply_starting_level` | the new game's starting level (`StartingLevelReport`; rewrites the seed routine's XP literal + recomputes slot 0's stat template to the level via the disc's growth curves). |
| Name injection | - | `inject_seru_bell_name` | names the unnamed accessory. |

## Door coupling

`Decoupled` uses the full variable-length relocation, so any destination can land
in any door (a scene that overflows on rebuild is skipped).

`Coupled` instead restricts itself to **length-preserving** swaps - it re-pairs
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

The PPF is the redistributable deliverable - it ships only deltas the user already
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

# Randomize encounters world-wide. The solo-strong pass is on by default, so an
# over-strong monster appears alone, never in a pack (opt out with
# --no-solo-strong-encounters).
legaia-rando randomize --input DISC.bin --seed myrun --encounters random \
    --encounter-scope world

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

# Start the new game at character level 10 instead of 1.
legaia-rando randomize --input DISC.bin --seed myrun --starting-level 10

# Read-only: show the new game's current starting bag + level.
legaia-rando starting-items --input DISC.bin

# Read-only: list what each town store sells / the casino prize exchange.
legaia-rando shops  --input DISC.bin
legaia-rando casino --input DISC.bin

# Shuffle drops, plus a low-chance bonus equipment drop on top of every battle.
legaia-rando randomize --input DISC.bin --seed gear --drops shuffle --equipment-drops

# Randomize what stores sell (quest items excluded) + the casino prizes.
legaia-rando randomize --input DISC.bin --seed mart --shops random --casino shuffle

# Confirm a shared patch applies cleanly to your own disc before playing.
legaia-rando verify --input DISC.bin --patch run.ppf
```

### Randomize flags

- `--drops` / `--encounters` / `--chests` / `--shops` / `--casino` / `--steals` /
  `--arts` / `--doors` each take `shuffle` / `random` / `none`.
- The battle-tuning + equipment-bonus passes - `--monster-stats` / `--move-power` /
  `--element-affinity` / `--spell-cost` / `--equip-bonus` - each also take
  `shuffle` / `random` / `none`.
- `--equipment-drops` injects a low-chance bonus equipment drop into the
  battle-end reward routine - granted on top of `--drops`, never disturbing it.
  `--equipment-drop-chance N` sets the per-battle percent (default 5).
- `--door-coupling` is `coupled` (default, bidirectional) or `decoupled` (one-way).
- `--starting-items N` seeds the new game with `N` random consumables
  (`0` = vanilla Healing Leaf ×5). The random fill shares a seven-slot capacity
  (five with `--all-warps`) with the convenience toggles below, additively.
- `--starting-level N` begins the new game at character level `N` instead of 1
  (`0`/`1` = vanilla; range `2..=14`). Rewrites the seed routine's XP literal and
  recomputes the starting stats to the level from the disc's growth curves.
- `--door-of-wind [N]` adds `N` Door of Wind (the warp consumable; default 10) to
  the starting bag.
- `--incense [N]` adds `N` Incense (the encounter-rate consumable; default 10) to
  the starting bag (same shape as `--door-of-wind`).
- `--speed-chain [N]` / `--chicken-heart [N]` / `--good-luck-bell [N]` add those
  accessories (default 1 each) to the starting bag, same shape as the above.
- `--all-warps` presets the visited-towns story-flag bitmask so Door of Wind can
  teleport to any town from the start (both ride the same reclaimable seed region as
  the starting items).
- The solo-strong pass is **on by default** whenever `--encounters` is set: any
  randomized formation holding a monster much stronger than the area's natives is
  forced down to that lone enemy (a strong monster appears solo, never in a pack
  of 2+). `--solo-strong-threshold N` sets the cut-off as a percent of the area's
  native average monster power (default 200 = twice as strong);
  `--no-solo-strong-encounters` opts out (keep the over-strong packs). Likewise on
  by default in the web Balanced / Full Chaos presets.
- `--unused-enemies` adds the unused "Comm" + Evil Bat enemies to the Random
  encounter pool (needs `--encounters random`).
- `--unused-items` adds Something Good + the "Seru Bell" accessory to the Random
  fill pool (and names the accessory).
- `--keep-static-items` overrides the protected chest set.

### Run-control flags

- `--dry-run` plans + reports the run without writing any files.
- `--manifest` writes a small TOML record of the seed + options + change counts (no
  game bytes - safe to share).
- `verify` applies a PPF to a copy of your disc and confirms the result still parses
  (records applied, PROT entry + drop counts).

### Seed + pool notes

- `--seed` takes a number (decimal or `0x`-hex) or any string (hashed stably to a
  number); the resolved numeric seed is always printed so a run reproduces exactly.
- `--drops` and `--encounters` each take `shuffle` / `random` / `none`.
- `--drops random` needs the SCUS item table off the disc for the valid item pool;
  the others need no external table.
- `--encounters` reassigns each scene's formation monster ids; `--encounter-scope`
  sets the pool it draws from: `scene` (default - the scene's own ids, every swap is
  one the scene already loads), `kingdom` (any monster in the scene's Drake / Sebucus
  / Karisto kingdom), or `world` (any monster on the disc). The wider pools rely on
  the battle loader streaming a monster by id, so an out-of-area enemy still loads.

### Read-only listings

`drops` / `chests` / `shops` / `casino` / `steals` / `arts` / `doors` /
`house-doors` / `starting-items` / `monster-stats` / `move-powers` / `affinity` /
`spell-costs` / `equip-bonuses` each list the current data for that feature.

## How an edit reaches the disc

A PROT-entry-relative offset maps to the PROT.DAT-logical offset
`start_lba[entry] * 2048 + offset_in_entry`, which
`legaia_iso::write::patch_file_logical` turns into physical-sector writes plus
EDC/ECC re-encode. Every edit is same-size and in place - no LBA, TOC, or
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
  byte-identical**; a whole-disc World-scope random pass that asserts the
  solo-strong option collapses every strong pack (a multi-monster formation with
  a monster ≥ 2× the area's native average) to a lone enemy - non-vacuous
  (strong packs exist without it), sector-valid, and deterministic; a whole-disc
  chest shuffle asserting give-item site offsets
  unchanged, the chest-item multiset preserved, sectors valid, and deterministic;
  the bonus-equipment-drop injection asserting the patched `SCUS_942.54` carries
  the `j routine` detour + the hand-assembled routine + the equipment-id table
  (replaying the two displaced instructions and returning), the edit is surgical,
  deterministic, and the build guard refuses a corrupted hook / non-dead routine
  region; the run-away-EXP injection asserting the real disc's escape-teardown
  hook site **is** the expected displaced pair, then that the overlay carries the
  `j routine` detour and `SCUS_942.54` the hand-assembled routine, each edit is
  surgical + EDC/ECC-valid + deterministic, and the build guard refuses an
  unknown layout; a town-shop + casino pass (Variety Store + its 10 ids
  enumerate, shuffle preserves the multiset/counts, casino preserves the
  prize set); and the item-price edits (the 13 chest-equipment items get their
  reviewed values, the sellable pool excludes quest ids, and a shop `Random`
  pass only stocks priced items); and an equipment-bonus shuffle asserting each
  slot category's stat-tuple multiset is preserved while every row's
  passive/mask/slot tail stays byte-identical.

```bash
cargo test -p legaia-rando                                   # synthetic only
LEGAIA_DISC_BIN="/path/to/Legend of Legaia (USA).bin" \
    cargo test -p legaia-rando                               # + disc-gated
```

The patched image is never written to disk by the tests; it lives only in
memory. A patched `.bin` contains Sony data and must never be committed.

## See also

- [`docs/tooling/randomizer.md`](../../docs/tooling/randomizer.md) - design + the patch chain.
- [`crates/lzs`](../lzs/README.md) - the LZS encoder (`compress`) re-packing relies on.
- [`crates/iso`](../iso/README.md) - Mode 2/2352 sector write-back (`write` module).
- [`crates/asset`](../asset/README.md) - the monster-archive + item-name parsers it edits.
