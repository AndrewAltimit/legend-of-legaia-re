# Randomizer / disc patcher

Track-1-adjacent tooling that edits gameplay data on a **user-supplied** retail
disc image: it shuffles monster item drops (optionally turning them into rare
equipment), random-encounter formations, treasure-chest contents, what town
stores sell (and the casino prize exchange), per-monster steal items,
scene-transition doors/exits, intra-town (house / interior) doors, the new
game's starting items, equipment passive stat bonuses, and a set of
**battle-tuning** tables - monster combat stats, special-attack power, the
element-affinity matrix, and spell MP costs - and writes the result back into the
`.bin`. It does not touch the clean-room engine.

Crate: [`crates/rando`](../../crates/rando/README.md) (`legaia-rando`). It ships
only code - no game bytes - and every test that needs real data is disc-gated,
so CI runs without a disc.

## Contents

- [Why this needs three new capabilities](#why-this-needs-three-new-capabilities)
- [Editing model: same-size in place, except doors](#editing-model-same-size-in-place-except-doors)
- [In the browser](#in-the-browser)
- [CLI: `legaia-rando`](#cli-legaia-rando)
- Per-randomizer mechanics:
  - [Keep-static items](#keep-static-items)
  - [Equipment drops](#equipment-drops)
  - [Random encounters](#random-encounters)
  - [Seru trading](#seru-trading)
  - [Treasure chests](#treasure-chests)
  - [Town shops (what stores sell)](#town-shops-what-stores-sell)
  - [Casino prize exchange](#casino-prize-exchange)
  - [Steal items (Evil God Icon)](#steal-items-evil-god-icon)
  - [Monster combat stats](#monster-combat-stats)
  - [Special-attack power](#special-attack-power)
  - [Element-affinity matrix](#element-affinity-matrix)
  - [Spell MP costs](#spell-mp-costs)
  - [Equipment stat bonuses](#equipment-stat-bonuses)
  - [Arts button combos](#arts-button-combos)
  - [Doors (scene transitions)](#doors-scene-transitions)
  - [House doors (intra-town)](#house-doors-intra-town)
  - [Starting items](#starting-items)
  - [Starting-bag convenience toggles](#starting-bag-convenience-toggles)
  - [Unused content](#unused-content)
  - [Re-pack slack](#re-pack-slack)
- [The patch chain](#the-patch-chain)
- [EDC/ECC: not game-specific](#edcecc-not-game-specific)
- [Tests](#tests)
- [No-Sony-bytes hygiene](#no-sony-bytes-hygiene)
- [See also](#see-also)

## Why this needs three new capabilities

Most editable values live *inside* a Legaia LZS stream that the asset
dispatcher decompresses at load. Changing one is therefore
decompress → mutate → recompress → write-back, which needed three pieces the
preservation track never had (it only ever *read* the disc):

1. **An LZS encoder** - `legaia_lzs::compress`. The retail game ships only a
   decoder (`FUN_8001A55C`); there was no way to produce a stream it accepts.
   See [LZS compression](../formats/lzs.md).
2. **Mode 2/2352 sector write-back** - `legaia_iso::write`. Overwriting the
   2048-byte user payload of a CD sector also requires recomputing its 4-byte
   EDC and 276-byte P/Q ECC, or the sector reads as corrupt. See
   [PSX disc geometry](../formats/disc.md).
3. **A disc bridge** - `legaia_rando::disc::DiscPatcher`, which ties the editing
   primitives to the sector write-back through the PROT.DAT TOC.

## Editing model: same-size in place, except doors

Drops / encounters / chests / steals / **house doors** (the player door-warp
tile shuffle) overwrite bytes **in place** and never change a byte count, so no LBA,
PROT TOC, or ISO 9660 directory record ever moves. **Scene-transition doors are
the one exception**: a scene-transition destination carries its
target scene's name inline, so re-pointing a door at a differently-named scene
changes the record's byte length. That is made safe by the
[MAN relocation engine](../formats/man-relocation.md), which rebuilds the
decompressed MAN, fixes every internal offset the resize disturbs, and keeps the
*recompressed* stream within the asset's on-disc footprint (or skips the scene).
The disc image's total size never changes either way. For the same-size edits:

Every same-size edit overwrites bytes **in place** and never changes a byte
count, so no LBA, PROT TOC, or ISO 9660 directory record ever moves. That keeps the patch a
pure byte-overwrite (plus EDC/ECC recompute) with no risk of cascading offset
shifts. It works because the edit targets fit a fixed slot with slack:

- The monster `battle_data` archive (PROT entry 867) gives each monster a fixed
  `0x14000`-byte slot laid out `[u32 decompressed_size][LZS stream]`. The
  decoded record length is unchanged by a drop edit, and the original stream was
  produced by Sony's packer; our greedy packer is weaker but still fits the slot
  comfortably (`repack_slot` rejects the rare case where it would not). The slot
  is re-emitted zero-padded back to `0x14000`.

## In the browser

The same randomizer is also exposed client-side: `legaia_web_viewer::rom_patcher`
(`patch_rom`) compiles the crate to WASM, and the static site's
`tooling/rom-patcher.html` page lets a user supply their own disc, toggle every
setting (drops / equipment-drops / encounters / chests / town shops / casino /
steals / doors / house doors / starting items / unused content), and download a
patched image - the disc bytes never leave the browser. The CLI below is the
scriptable / shareable-PPF path.

## CLI: `legaia-rando`

The top-level binary turns a disc + seed into a portable patch:

```bash
legaia-rando drops     --input DISC.bin                       # read-only: monster drops
legaia-rando chests    --input DISC.bin                       # read-only: chest contents
legaia-rando steals    --input DISC.bin                       # read-only: steal items
legaia-rando doors     --input DISC.bin                       # read-only: scene transitions
legaia-rando house-doors --input DISC.bin                     # read-only: intra-town door warps
legaia-rando starting-items --input DISC.bin                  # read-only: new-game starting bag
legaia-rando shops     --input DISC.bin                       # read-only: what town stores sell
legaia-rando casino    --input DISC.bin                       # read-only: casino prize exchange
legaia-rando monster-stats --input DISC.bin                   # read-only: monster HP/MP/ATK/DEF/AGL/SPD
legaia-rando move-powers   --input DISC.bin                   # read-only: special-attack power table
legaia-rando affinity      --input DISC.bin                   # read-only: element-affinity matrix
legaia-rando spell-costs   --input DISC.bin                   # read-only: spell MP costs
legaia-rando equip-bonuses --input DISC.bin                   # read-only: equipment stat-bonus table
legaia-rando randomize --input DISC.bin --seed myrun --drops shuffle
legaia-rando randomize --input DISC.bin --seed brutal --monster-stats shuffle \
    --move-power shuffle --element-affinity shuffle --spell-cost shuffle    # battle-tuning shuffle
legaia-rando randomize --input DISC.bin --seed gear --drops shuffle --equipment-drops   # +low-chance bonus gear drop
legaia-rando randomize --input DISC.bin --seed flee --encounters shuffle --flee-exp     # +5% experience on a successful escape
legaia-rando randomize --input DISC.bin --seed pal --enemy-ally                         # 20% chance an enemy fights on your side
legaia-rando randomize --input DISC.bin --seed pal --shiny-seru                         # 2% chance a capturable enemy is shiny (+35% stats / captured-Seru damage)
legaia-rando randomize --input DISC.bin --seed swap --seru-trade                        # vendors trade seru-for-seru (clean-room engine UI)
legaia-rando randomize --input DISC.bin --seed mart --shops shuffle --casino shuffle
legaia-rando randomize --input DISC.bin --seed 0xC0FFEE --drops random \
    --encounters shuffle --steals shuffle --arts shuffle --doors shuffle --door-coupling coupled \
    --starting-items 3 --patch run.ppf --output patched.bin --manifest run.toml
legaia-rando randomize --input DISC.bin --seed wild --encounters random \
    --unused-enemies --chests random --unused-items                  # bring back unused content
legaia-rando randomize --input DISC.bin --seed chaos --encounters random \
    --encounter-scope world                                          # late-game monsters anywhere; over-strong fights go solo by default
legaia-rando verify    --input DISC.bin --patch run.ppf       # apply + sanity-check
```

`randomize` plans the run, applies it to an in-memory copy of the disc, diffs
the result against the original, and writes the changes as a **PPF 3.0** patch
(default `<input>.ppf`). `--output` also writes a full patched `.bin` for local
play, plus a matching single-track Mode 2/2352 `.cue` beside it (so emulators
that reject a bare BIN - e.g. mednafen on a >64 MiB image - can open it
directly). The seed is resolved from a number or a hashed string and always printed,
so a run reproduces exactly; the same seed yields a byte-identical patched image
and PPF. `--drops`, `--encounters`, `--chests`, `--shops`, `--casino`,
`--steals`, `--arts`, `--doors`, `--monster-stats`, `--move-power`,
`--element-affinity`, `--spell-cost`, and `--equip-bonus` each take `shuffle` / `random` / `none`
(`--arts` reassigns Tactical-Arts button combos - see [Arts button combos](#arts-button-combos);
the four battle-tuning passes are described under
[Monster combat stats](#monster-combat-stats) and the three sections after it);
`--equipment-drops` injects a code hook into the battle-end reward routine that
grants one extra random equipment piece on a low per-battle chance
(`--equipment-drop-chance N`, default 5) on top of `--drops`, never disturbing it
(see [Equipment drops](#equipment-drops)); `--flee-exp` injects a code hook into
the battle-action escape teardown so a successful run banks `--flee-exp-pct`%
(default 5) of the fled fight's experience into the party (see
[Run-away EXP](#run-away-exp)); `--enemy-ally` injects a code hook into battle
setup so that, with `--enemy-ally-pct`% chance (default 20), a random enemy is
charmed onto the party's side as an uncontrolled ally - bosses included (see
[Enemy ally (charm)](#enemy-ally-charm)); `--shiny-seru` injects code hooks so
that, with `--shiny-pct`% chance (default 2), a capturable enemy spawns as a rare
shiny variant (+35% stats) whose captured Seru deals +35% damage forever (see
[Shiny Seru](#shiny-seru)); `--seru-trade` embeds a config so the clean-room
engine lets vendors swap one of a character's seru for another, reseeding every
two in-game hours (`--seru-trade-offers N` caps offers per vendor; see
[Seru trading](#seru-trading));
`--door-coupling` is `coupled` (default, bidirectional) or `decoupled`
(one-way); `--encounter-scope` widens the monster pool an encounter roll draws
from to `scene` (default), `kingdom`, or `world`; the **solo-strong** pass
(cut-off `--solo-strong-threshold N`, default 200%) forces an over-strong
randomized fight down to a lone enemy and is **on by default whenever
`--encounters` is set** (`--no-solo-strong-encounters` opts out - see
[Random encounters](#random-encounters)); `--starting-items N` seeds the new game with `N` random consumables
(0 = vanilla; the random fill shares a seven-slot capacity - five with
`--all-warps` - with the convenience toggles, additively). `--door-of-wind [N]` adds
`N` Door of Wind (the warp consumable; default 10) to the starting bag,
`--incense [N]` adds `N` Incense (the encounter-rate consumable; default 10)
likewise, `--speed-chain [N]` / `--chicken-heart [N]` / `--good-luck-bell [N]`
add those accessories (default 1 each), `--start-with id[:count],…` seeds
explicit item(s) on top (any id - consumable, equipment, or accessory), and
`--all-warps` unlocks every Door-of-Wind destination from the start (see
[Starting-bag convenience toggles](#starting-bag-convenience-toggles)).
`--starting-level N` begins the new game with the starting party at level `N`
instead of 1 (0/1 = vanilla; range 2..=14, see [Starting level](#starting-level)).
`--unused-enemies` and `--unused-items` re-introduce
content the game ships but never surfaces (see
[Unused content](#unused-content) below).
`--weapon-specialty` (a toggle) reassigns which weapon class each character
favors (see [Weapon specialty](#weapon-specialty)).
`--dry-run` reports the plan without writing; `--manifest` writes a small TOML
record of the seed + options + change counts (no game bytes, safe to share). The
`verify` subcommand applies a PPF to a copy of the user's disc and confirms the
result still parses end to end - a recipient's check that a shared patch + seed
match their own disc.

The read-only `drops`, `chests`, `shops`, `casino`, `steals`, `arts`, `doors`,
`starting-items`, `monster-stats`, `move-powers`, `affinity`, `spell-costs`,
`equip-bonuses`, and `weapon-specialty` subcommands write nothing
- they decode the randomizable populations off the user's disc and print them
(item ids + names resolved from the disc's own SCUS table; chests + doors grouped
by scene via CDNAME). `chests` lists the exact 275-site treasure population the
chest randomizer reassigns, which is the natural place to audit for quest / key
items a run might want to keep static. `doors` lists every scene-transition exit
(home scene → destination + entry tile) - the 160-site door population.

### Keep-static items

Progression / quest / key items are things the player needs in a predictable
place - door keys, garden-quest tools, letters, story books, one-off plot items.
The chest randomizer keeps the **full quest-item set** static by default, derived
from the disc rather than a short hand-list (`items::default_static_chest_items`
→ `item_price::quest_item_ids`): every **named, unsellable** item - the item
table prices quest/key/story items at `0`, the game's own "a shop never trades
this" marker - **minus** the handful of chest-found *equipment* pieces (the
Ra-Seru gear + Astral Sword) that ship price-0 only because they're never sold
but are real, randomizable gear.

This automatically covers every door/dungeon key, the egg/talisman/book
collectibles, the fishing rods, the casino cards, and the internal Ra-Seru
weapon-state template entries - no manual list to keep in sync with the game.
Buyable items (priced > 0, e.g. the Silver Compass accessory) are intentionally
left randomizable. A chest whose original item is in the set keeps that item, the id
is excluded from the shuffle multiset (so it can never move to another chest),
and it is dropped from the `random` fill pool (so it can't be placed into an
unrelated chest). If the item table can't be read, the randomizer falls back to
the curated `items::DEFAULT_STATIC_CHEST_ITEMS` subset. Override with
`--keep-static-items 0x9a,0x71,…` (decimal or `0xHH`), or pass an empty value
(`--keep-static-items ""`) to randomize every chest. The resolved set is recorded
in the run manifest.

Because an edit changes bytes *inside* an LZS stream, the whole touched stream
is re-packed, so the changed-byte count (and the PPF) is dominated by
re-compression churn, not by the gameplay delta - this is inherent to editing
compressed data, and every edit stays same-size. `--drops random` reads the SCUS
item table off the disc for the valid item pool; the other modes need no
external table.

### Equipment drops

`--equipment-drops` is genuinely **additive**: it grants one *extra* piece of
equipment on a low per-battle chance, **on top of** the normal drop, which it
never touches. A monster record has a single drop slot (`+0x48` item id /
`+0x49` chance), so no data edit can make a monster drop two things - turning the
slot into equipment would destroy the normal drop. So instead of editing data,
this feature **patches the executable's reward routine** the same way the
starting-bag feature splices a grant into the opening scene: a small routine is
injected that rolls the game's own RNG and, on success, calls the inventory-add
helper for a random equipment id. This is why every gameplay preset of the
in-browser patcher enables it; only "Vanilla" leaves it off.

**The hook (`bonus_drop` module).** The battle-end reward routine `FUN_8004E568`
tallies a battle's spoils exactly once (gated on the per-battle state byte
`actor+0x6ce == 0`, which it then sets to `1`). Right after it grants the
formation's normal drop via `FUN_800421d4(item, 1)` at `0x8004f608`, control
joins at `0x8004f610` (`lui v0,0x8008` / `lw v0,-0x4540(v0)`). The randomizer
overwrites those two instructions with `j <routine>` + `nop` (a detour), and the
injected routine:

1. rolls `rand() % 100 < chance` (the low-chance gate, default 5 %, reusing the
   battle RNG `FUN_80056798`);
2. rolls `rand() % table_len` to index an embedded equipment-id table;
3. calls `FUN_800421d4(id, 1)` to add the gear - the same helper the normal
   drop, shops, and minigame rewards use (an unguarded add, like the minigame
   completion reward `FUN_801C2748`);
4. replays the two displaced instructions and `j`s back to `0x8004f618`.

The join is reached once per battle, so the roll fires once per battle. The
routine + id table are written into the 1028-byte preserved rodata gap at
`0x8007AB38` (the same loaded-and-preserved padding the [name injection](#name-injection)
uses, at a non-overlapping offset clear of the Seru-Bell string) - on PSX all
resident RAM is executable, so a routine placed there runs when jumped to.
Everything is a same-size, in-place `SCUS_942.54` edit; the planner guards on the
two detour-site words matching the known US build and on the routine region being
all-zero dead space, refusing a differently-laid-out image rather than corrupting
it.

The grant is silent (no victory-screen "received" line); the gear simply appears
in the bag after the battle. The chance is `--equipment-drop-chance N` (percent,
default 5).

**The id table** is the equipment pool: the retail item id space is one flat
table shared by consumables, key items, and equipment, with nothing that flags
"this id is a weapon" in a single byte, so the equipment ids are recovered by
**name** - every weapon / armor / accessory in the curated public
[gamedata tables](../reference/gamedata.md) is matched case-insensitively against
the disc's own item-name table to find its id (`legaia_rando::equipment::equipment_pool`).
The names ship in the repo; the ids come from the user's disc - no Sony bytes are
embedded (the injected routine is the randomizer's own code), and the join
doubles as a cross-check of the curated tables against the real executable. About
150 of the ~155 curated equipment names resolve; the stray in-range consumable
*Honey* is correctly excluded.

> The clean-room engine can't execute injected MIPS, so - unlike the data-edit
> randomizers - this feature has no engine runtime oracle. It is verified by the
> byte/disassembly checks in `equipment_drops_real` (the detour + routine + table
> decode as the hand-assembled code, the edit is surgical, the build guard
> refuses an unknown layout) plus an emulator playtest.

### Random encounters

Formations live in the per-scene MAN asset (type `0x03`, descriptor index 2 of a
scene bundle), inside an LZS stream; each formation record is
`[3 reserved][u8 count 0..4][u8 ids...]` (see
[encounter records](../formats/encounter.md)). `apply::randomize_encounters`
walks every PROT entry, and for each scene bundle it locates the MAN
(`SceneEncounters::locate`, straight from the entry bytes - no engine
dependency), decompresses it, rewrites the formation monster ids
(`Shuffle` redistributes the existing ids, `Random` draws from the pool),
recompresses, and writes the stream back over the original (the LZS decoder
stops at the descriptor's decompressed size, so a same-or-shorter re-pack is
safe). The id pool is **per scene** - only ids the scene already uses - so every
swapped-in monster is one the scene loads; no missing model, no crash.

**Bosses are protected.** A scene's formation array mixes random encounters with
*scripted* fights the field VM engages by explicit index - boss battles (the Rim
Elm Tetsu tutorial, Cort, Songi, …) and story encounters. Only the genuinely
random formations are touched: the encounter section's region records each name
a `[formation_range_base, +count)` slice **and** a `rate_increment` (the per-step
amount added to the encounter counter inside that region's AABB), and a region
with `rate_increment == 0` never triggers an encounter, so it can reference a
formation without ever rolling it. `SceneEncounters` marks a formation random iff
some **`rate_increment > 0`** region reaches it (the retail position-aware roll
`FUN_801D9E1C`); formations reached only by rate-0 regions (or no region) are
left byte-identical. town01 is the canonical case - its rate-0 regions cover
formations 2..=4, but the only rate>0 regions reach 0..=2, so Tetsu at index 4 is
correctly left alone. The candidate pool for `Random` is likewise the random
formations' ids only, so a roll never drops a boss into an ordinary encounter.

**An explicit id guard backs the heuristic.** The region-rate test classifies
every story boss's formation as scripted with one exception: the early **Gimard**
Seru-boss fight sits at a formation index a rate>0 region's range happens to span,
so the heuristic alone would treat it as random - and a roll could then replace
that mandatory tutorial fight (stranding a fresh save) or donate Gimard, a
boss-tier enemy, into an ordinary early encounter. `encounter::PROTECTED_FORMATION_IDS`
lists the ids that must never be a random encounter (Gimard, id 10); `locate`
forces any formation holding one back to scripted, and such ids never enter a
donor pool, so the fight ships exactly as authored regardless of the region
layout. (The first wild Piura are deliberately not listed - they are genuine
random encounters.) This mirrors the stat-side guard
(`monster_stats::PROTECTED_MONSTER_IDS`, which also pins Gimard) and is validated
on a real disc by `tests/encounter_patch_real.rs`
(`protected_formations_survive_every_encounter_mode`).

**Pool scope (`--encounter-scope`).** By default the pool is per scene, but
`apply::randomize_encounters_scoped` widens it to one of three
[`EncounterScope`] settings:

| Scope | Pool a scene draws from | Effect |
|---|---|---|
| `scene` (default) | the scene's own monsters | classic; difficulty stays local. |
| `kingdom` | every monster in the scene's **kingdom** (Drake / Sebucus / Karisto) | "within a region": late Drake monsters can appear in early Drake, but nothing crosses a kingdom boundary. |
| `world` | every monster on the disc | "across regions": a late-game Karisto monster can appear in the opening Drake caves. |

The kingdom partition is derived from the disc's own `CDNAME.TXT`, never a
hardcoded scene list: the three overworlds (`map01` / `map02` / `map03`) are
pinned world-map bundles, so **Sebucus begins at the first CDNAME block after
`map01`, Karisto at the first after `map02`** (Karisto absorbs the dungeons
listed after `map03`). See [`kingdom`](../../crates/rando/src/kingdom.rs).
`kingdom` scope needs `CDNAME.TXT`; `world` does not. The wider pools rely on the
battle loader streaming a monster's archive slot on demand by id, so an
out-of-area enemy still loads and renders.

`Random` fills each scene's slots independently from its scope pool. `Shuffle`
conserves the scope-wide id **multiset** - it pools every random-encounter id in
the scope, permutes it once, and redistributes it across the scope's scenes, so
monsters move between scenes (and, for `world`, between kingdoms) while the
overall monster census is unchanged. Because a cross-scene shuffle is only
multiset-preserving if every shuffled scene is actually written back, any scene
whose recompressed MAN overflows its footprint is *locked to its original* and
the rest are reshuffled (a fixpoint), so a re-pack skip never duplicates or drops
a monster. Every scope×mode combination is byte-deterministic for a fixed seed
and validated on a real disc by `tests/encounter_scope_real.rs` (kingdom
confinement, cross-kingdom mixing under `world`, per-scope multiset conservation,
boss survival, EDC/ECC validity).

**Solo strong fights (on by default; `--no-solo-strong-encounters` opts out).**
The wider scopes can drop a late-game heavy hitter into an early area; left as a
pack of 2+ that is a soft-lock. `apply::randomize_encounters_full` adds a
`SoloStrongConfig` pass - applied to **every** CLI encounter run unless opted out
- that forces any such fight to a **single** enemy. It runs as a post-step over
the already-randomized scenes, so it composes with every scope×mode without
touching their multiset bookkeeping (and `solo == None` reproduces the prior
output byte-for-byte - the archive isn't even read):

- Each monster is scored by its **combat-stat budget**
  (`monster_stats::combat_power` - the sum of every combat stat except MP, which
  gates the AI spell economy rather than raw danger), built once into a
  `MonsterPowerTable` keyed by the formation byte (the 1-based `battle_data` id).
- Each scene's **native baseline** is the mean power of its *original* random
  monsters (`SceneEncounters::baseline_power`, captured before randomizing) - the
  area's authored difficulty, the stand-in for "how strong the party is here".
- `SceneEncounters::enforce_solo_strong` collapses every multi-monster random
  formation whose strongest member clears `threshold_pct`% of that baseline
  (default `200` = twice the area's norm): keep the strongest monster in slot 0,
  zero the rest, set `count := 1`. The count byte and dropped id bytes all live
  inside the formation record's fixed stride, so it stays a same-size in-place
  edit; a scene whose collapsed MAN no longer re-packs is skipped, like the rest.

Scripted/boss formations are never eligible (same `is_random_formation` gate), so
this only ever thins a *random* pack. Validated on a real disc by
`tests/solo_strong_encounter_real.rs`: a World-scope random pass produces strong
packs without the option and **zero** with it, non-vacuously, deterministically,
and EDC/ECC-valid. On by default in the web Balanced / Full Chaos presets.

### Run-away EXP

`--flee-exp` banks a slice of a fight's experience into the party whenever they
**successfully run away** - vanilla awards nothing for fleeing. Like the
[equipment drop](#equipment-drops), this is a runtime behaviour with no value to
edit (the flee path never reaches an EXP grant), so it **patches the executable**
rather than a table.

**The hook (`flee_exp` module).** The per-actor battle state machine
`FUN_801E295C` (battle-action overlay, base VA `0x801CE818` = **PROT entry 898**)
handles "Run" across states `0x64..0x66`. State `0x66` is the
**successful-escape teardown**, reached only when the run roll succeeds (a failed
run goes `0x65 -> 0x50` and the battle continues; see
[`battle-action.md`](../subsystems/battle-action.md)). Its handler begins at VA
`0x801E5A10` (`lui v1,0x801d` / `addiu a0,v1,-0x6f90`, the fade-template setup).
The randomizer overwrites those two instructions with `j <routine>` + `nop` (a
detour) - a same-size **raw** edit of the overlay PROT entry, which maps linearly
from its base (`file_off = va - 0x801CE818`). State `0x66` advances itself to the
terminal `0x67`, so it runs once per escape; the party HP was already floored to
`>= 1` in state `0x64` (the "escape restores a downed member" mechanism), so every
member is alive at the grant. The injected routine:

1. sums the formation's experience: it walks the live enemy record-pointer table
   at `0x801C9348` for `actor[+1]` (`*0x8007BD24`) entries and accumulates each
   record's EXP halfword (`+0x46` - the same field the victory-spoils routine
   `FUN_8004E568` reads);
2. scales the total to `--flee-exp-pct`% (default **5**);
3. adds the scaled amount to **every** party member's cumulative-XP cell - the
   slot→record-id map is at `0x8007BD10`, the record array is based at
   `0x80084140` (stride `0x414`), and cumulative XP lives at `+0x5C8` (where
   `FUN_8004E568` accumulates a win's EXP and `FUN_801E9504` reads it to apply
   levels), each clamped to the `9,999,999` cap;
4. replays the two displaced instructions and `j`s back to `0x801E5A18`.

The grant is **banked**, not applied as an immediate level-up: it only writes the
cumulative-XP cell (it never calls the level processor), so the experience shows
in the status screen at once and the character levels up the next time a won
battle tallies the accumulated total - small and side-effect-free during the
escape fade (no stray level-up screen). The routine lives in the same preserved
rodata gap as the [equipment-drop](#equipment-drops) and [name](#name-injection)
injections (`0x8007AB38`), at `0x8007AD00` - clear of the equipment routine + its
id table, so both battle hooks coexist. The planner guards on the detour-site
words matching the known US build and on the routine region being all-zero dead
space, refusing a differently-laid-out image rather than corrupting it. On by
default in the web Balanced / Full Chaos presets.

> The clean-room engine can't execute injected MIPS, so - like the equipment drop
> - this has no engine runtime oracle. It is verified by the byte/disassembly
> checks in `flee_exp_real` (the real disc's hook site **is** the expected
> displaced pair; the detour + routine decode as the hand-assembled code; each
> edit is surgical and EDC/ECC-valid; the build guard refuses an unknown layout)
> plus an emulator playtest.

### Enemy ally (charm)

`--enemy-ally` gives a per-battle chance (`--enemy-ally-pct`%, default **20**)
that a random enemy fights on the **player's** side as an uncontrolled ally - a
guest-character-style helper that can appear in any fight, bosses included
(`enemy_ally` module).

A genuine 4th player-side combatant is infeasible: retail battles are hard-wired
to 3 party slots + up to 4 monster slots (`FUN_800513F0`; party meshes/CLUTs/HUD
exist only for slots 0..2). So instead this rides a mechanic the game already
implements - the **"AI-delegated" flag**. Setting an actor's `+0x16E |= 0x380`
makes the action SM `FUN_801E295C` call the retarget helper `FUN_801E7320` at
ActionSeed, which **flips that actor's target to the opposite side**; for a
*monster*, the flip means it attacks the *other monsters*. The monster AI picker
`FUN_801E9FD4` already honours `0x380` (plain attacks, no scripted specials), so
"an enemy assists you" is just "set `0x380` on one monster at battle setup".

Two same-size SCUS edits plus a one-word overlay edit (`apply::inject_enemy_ally`):

1. a **setup detour** at `FUN_800513F0` `0x80051990` (right after the monster
   loop, so the actor table + enemy count are populated) into a routine in the
   preserved rodata gap at `0x8007ACA0` - the free window between the
   equipment-drop routine+table (`0x8007AB80`..`0x8007ACA0`) and the flee-EXP
   routine (`0x8007AD00`), so every gap feature coexists. The routine rolls the
   chance and OR's `0x380` into the frontmost enemy (actor slot 3, `0x801C937C`,
   always present), then replays the displaced pair and returns;
2. a **victory-mask widen** in battle-action overlay 0898 at `0x801E6638`
   (`andi v0,v0,0x4` -> `andi v0,v0,0x384`), so a `0x380`-charmed monster counts
   as "down" in the monster-wipe gate (state `0x5A`) and the player doesn't have
   to defeat their own ally to win.

The planner guards on the SCUS hook words, the routine landing zone being all-zero
dead space, and the overlay victory word matching the known `andi v0,v0,0x4` -
refusing a differently-laid-out image rather than corrupting it. On a solo-enemy
boss the lone enemy turns on itself. (Side effect: while on, a vanilla
*confuse*-on-an-enemy - which also sets `0x380` - likewise stops counting toward
"enemies remaining".) On by default in the web Balanced / Full Chaos presets.

> Like the other code hooks, the clean-room engine can't execute injected MIPS, so
> this has no engine runtime oracle. It is verified by the byte/disassembly checks
> in `enemy_ally_real` (the real disc's hook site **is** `lui v1,0x8008` /
> `lbu v1,-0x42f4(v1)` and the victory site **is** `andi v0,v0,0x4`; the detour +
> routine decode as the hand-assembled code; it composes with flee-EXP in the same
> gap; each edit is surgical and EDC/ECC-valid) plus an emulator playtest.

### Shiny Seru

`--shiny-seru` gives a per-battle chance (`--shiny-pct`%, default **2**) that the
frontmost **capturable** enemy spawns as a rare *shiny* variant: +35% combat
stats at battle load, and the Seru you capture from it deals **+35% damage** on
every future cast (on top of its normal abilities), permanently (`shiny_seru`
module). This mirrors the clean-room engine implementation
(`legaia_engine_core::seru_learning`'s shiny set + `SHINY_DAMAGE_BONUS_PCT`).

"Capturable" is decided by indexing the **first-monster id global**
(`DAT_8007BD0C`, reliably set before the setup hook - the game's own `0xB5` check
reads it) into a 256-bit **allowlist bitmap** built *at patch time* from the
disc's monster names that match a player Seru-magic name (`capturable_monster_ids`
/ `SERU_NAMES`: Gimard / Theeder / Vera / Gizam / Nighto / Zenoir / Viguro /
Swordie / Orb / Freed / Nova + variants = 33 ids). The earlier `actor+0x3e` idea
was wrong - that byte is volatile (reads 0x55 for gobu) and isn't a Seru flag.
The persistent +35% rides the **free high bit `0x80` of the captured spell's level
byte** (`record+0x161`; max legit level is 9, so the bit is spare) - which means
it survives a memory-card save and the spell-list insertion shift when more Seru
are captured. (Every injected routine honours the R3000 load-delay slot - a
just-loaded register is never used by the next instruction, else the value isn't
ready yet; the boost loop in particular cascades into garbage without this.)

`apply::inject_shiny_seru` performs **eight** same-size detours into routines
across three reference-free regions: a *new* preserved SCUS rodata gap at
`0x80077728` (the padding before the steal table `DAT_80077828`, distinct from the
`0x8007AB38` gap so it composes), the battle-action overlay 0898's
move-power-table padding at `0x801F4FC4`, and a second SCUS gap at `0x800783C4`
(hosting the capturable bitmap + the level-up read-masks):

1. **setup** (`FUN_800513F0` `0x80051A20`) - roll the chance; if the frontmost
   enemy's monster id is set in the capturable bitmap, boost its stat block
   `×135/100` and stamp the free per-actor byte `+0x226` as a shiny marker;
2. **capture-success** (`0x801EE2E8`) - stash the captured enemy's `+0x226`
   marker into a scratch word (the captured-enemy actor isn't reachable at the
   grant site, so the link is carried here);
3. **grant** (`FUN_801E92DC` `0x801E93B4`) - when the scratch says shiny, OR
   `0x80` into the just-granted spell-level byte;
4. **damage** (`FUN_801dd864` `0x801DDB08`) - when `0x80` is set, multiply the
   summon-damage roll `×135/100`, then strip the bit for the normal
   `(level-1)/8` math;
5-7. **level-up gate / working read / write-back** (`FUN_801E70BC`
   `0x801E71C8` / `0x801E71DC` / `0x801E7224`) - mask `0x80` so a shiny Seru
   still levels up and the increment re-applies the flag;
8. **menu** (`FUN_801d2e74` `0x801D2FA0`, overlay 0899) - mask `0x80` so the
   spell-list level digit renders correctly.

The planner guards every hook's fingerprint word and all three routine regions
being all-zero dead space - refusing a differently-laid-out image rather than
corrupting it. On by default in the web Full Chaos / Balanced presets.

> Like the other code hooks, the clean-room engine can't execute injected MIPS,
> so the disc path has no engine runtime oracle - it's verified by the
> byte/disassembly checks in `shiny_seru_real` (all eight hooks match the known
> US build, every detour becomes `j routine` + nop, the injection is surgical and
> EDC/ECC-valid, it composes with enemy-ally, byte-deterministic, and the build
> guards refuse a corrupted hook / non-dead region) plus an emulator playtest. The
> *behaviour* is covered on the engine side by `legaia-engine-core`'s `shiny_*`
> tests (roll/boost, capture marking, +35% damage, LGSF v4 persistence).

### Seru trading

`--seru-trade` adds an **in-shop Seru-trading vendor** that runs **on real
hardware**: every merchant grows a fourth **Buy / Sell / Trade / Quit** row, and
picking Trade opens a screen where the player swaps a party member's learned
Seru-magic for a different one. The offer is **time-bucketed** - it rotates as
play continues - and fully **deterministic from the run's seed**, so a preview
and the game always agree.

**What an offer is (`legaia_asset::seru_trade`, the shared kernel).** Each time
bucket has one `(want, give, give_level)` preference: the vendor wants a seru
*type* and hands back a different one at a fixed level (`4..=9`, part of the
trade's value, shown before you trade). The randomizer precomputes the whole
64-bucket schedule from the seed (`bucket_offers` → `bucket_table_to_bytes`, 3
bytes/entry) and embeds it. At runtime the handler indexes it by
`(play_time / period) & 63`. Against the live party the bucket expands
(`expand_offers`) to **one selectable line per member who owns the wanted seru** -
so the same type held by two members lists once each - **excluding** any member
who already owns the give-back (a pointless trade). The seru id space is the
player Seru-magic block `0x81..=0x95`.

**The retail build (`seru_overlay` + `apply::inject_trade_full`).** This is a
hand-assembled MIPS feature, not a value edit. Two byte-verified edits to the
menu overlay (PROT **0899**) turn the picker into Buy / Sell / Trade / Quit and
route a confirmed Trade into an unused picker sub-mode; the trade screen itself -
the per-owner render, the native window-slide in/out, the cursor, the explicit
"Trade?" confirm, and the swap - is a routine hosted **entirely in 0899's own
reference-free dead region** (a ~3.8 KB all-zero run inside the resident overlay
image), reached by `j` from the in-overlay detours. Because nothing lands in the
SCUS rodata gap, seru trading **composes with every gap-based feature**
([equipment drops](#equipment-drops), [flee-EXP](#run-away-exp), the Seru-Bell
[name](#name-injection)). The injector writes the handler + stubs + strings + the
seed-derived bucket table via `patch_prot_entry(899, …)`, each guarded as
all-zero dead space. The swap rewrites the chosen owner's spell list in place
(id at `+0x13D`, level at `+0x161`), mirroring `engine_core::seru_trade::apply_trade`.

> Cadence note: the play counter at `0x80084570` advances ~per-frame (≈60/s), not
> per-second, so the retail handler divides by `RESEED_PERIOD_FRAMES` (≈9 minutes)
> and the full schedule cycles in ~9.6 h. The kernel's seconds-based
> `SECONDS_PER_RESEED` is the engine-facing constant.

**Engine mirror (clean-room track).** The same kernel feeds the engine's own
trade UI: `World::install_seru_trade_config` reads a 24-byte
[`SeruTradeConfig`] blob (enabled + seed) that `apply::enable_seru_trades` can
write, and `World::open_seru_trade` / `apply_seru_trade` render + apply the swap
through `MenuState::ShopMenu`/`ShopTrade`/`ShopTradeConfirm`. (Engine and retail
share the offer math; the engine UI's migration to the bucket+`give_level`
schedule is in progress.)

> Verified by the rando `seru_trade_real` disc oracle (every piece lands in 0899,
> the schedule round-trips to the kernel offers, the SCUS gap is left untouched,
> byte-deterministic) plus the kernel unit tests; the retail screen is
> hardware-confirmed (render → slide → cursor → confirm → swap).

### Treasure chests

A chest gives its item via the field-VM **`GIVE_ITEM` opcode `0x39`**, encoded
`[0x39, item_id]` - the item id is a **single inline operand byte** in the
per-scene field-VM script bytecode, not a per-scene table. (Pinned in the
dispatcher `FUN_801DE840` case `0x39` at `0x801E0448`: inventory-window setup
`FUN_8004313C` then add-by-id `FUN_800421D4(item_id, 1)`, PC += 2. The standalone
`FUN_801D71F0` add-item copy is dead/uncalled. See
[script-vm.md](../subsystems/script-vm.md).) The give sites live in the MAN
partition-1 per-actor interaction scripts (a chest is an interactable actor).

`chest::give_item_sites` finds them with a **dialogue-skipping opcode-aware
walk** - it walks each partition-1 record's interaction script from its true
entry PC with the field-VM disassembler ([`legaia_asset::field_disasm`], moved
into Track 1 for exactly this reuse). A chest's give op almost always sits
**after** the inline dialogue that announces it ("There is a {item} in the
treasure chest!" → give → "{name} now has the {item}!"). That dialogue is a
stream of `0x1F`-lead glyph segments, not bytecode, so a decode error **at a
`0x1F` byte** is treated as a segment to skip (advance past `0x1F`, consume
glyphs to the terminating `0x00`, with `0xC?` top-nibble bytes as 2-byte
escapes per the dialog box-pack format), and decoding resumes - the
inter-segment control bytes (`0x24`/`0x25`/`0x48` Nop, `0x26` `JMP_REL`, `0x36`
`SCENE_FADE`, …) are genuine ops that stay in sync, so the walk reaches the
post-dialogue `0x39`. Any **other** decode error stops the walk, and each
record's walk is bounded to the next record's start offset, so it can never run
off into unrelated data and mis-read a `0x39` data byte as an op - never a naive
`0x39` byte scan. (An earlier walk stopped at the *first* `0x1F` instead of
skipping it, which silently missed the post-announcement give in roughly 85% of
sites - including every chest in a scene whose first interactable record opens
with dialogue, such as `keikoku`.) Multi-`0x39` runs are genuine multi-item
gifts (a 10× consumable chest, the fishing starter kit of a rod + several lures,
the Genesis-Tree Ra-Seru equipment sets), each `0x39 <id>` its own op.

**Display vs grant - the announcement names the item from a different byte.** A
chest's flavor text ("There is a {item} in the treasure chest!" / "{name} now has
the {item}!") renders the item *name* from a dialogue **item-name token** `0xC2
<id>`, which is a **separate byte** from the `0x39` give operand that actually
adds the item to the bag. Patching only the give operand grants the new item but
leaves the message reading the old one - verified in-game: the inventory receives
the new item while the chest still *says* the original (an `0xC2 <old_id>` token
sits resident in the loaded MAN right beside the patched `0x39 <new_id>`). Pinned
across the corpus: of every `0xC?` 2-byte dialogue escape in chest records, only
`0xC2`'s argument matches the give operand (the other escapes are character-name /
glyph controls), and 241 of 275 sites carry one (announcement + "now has"). So
`give_sites_and_display_tokens` recovers, per give site, the `0xC2` token offsets
in the same record whose id equals that site's give operand (routed to the
*nearest* give so multi-item-gift records map each token correctly), and
`SceneChests::set_site` rewrites the operand **and** those tokens together - flavor
text stays in sync with the grant. Sites whose dialogue doesn't name the item
(~34) simply have no token to sync.

Chest item ids are global inventory ids, so `apply::randomize_chests` reassigns
them **globally** across every site (`Shuffle` redistributes the existing
multiset, `Random` draws from the valid item pool), then recompresses each
touched MAN like the encounter path. A scene whose recompressed MAN overflows is
excluded from the shuffle pool entirely (determined iteratively), so its items
neither leave nor enter circulation and `Shuffle` preserves the global multiset
exactly. On the retail disc this is 275 give sites across 50 scenes (one scene,
too tight to re-pack, is skipped).

### Town shops (what stores sell)

A gold merchant's stock is **inline in the scene's field-VM script** (the MAN),
the same place chests and doors live - *not* a global table. Opening a shop is
field-VM **op `0x49` (`STATE_RESUME`)**, the multi-frame state machine that
drives the menu-request register `_DAT_8007B450`. Its sub-op-`0` inline payload,
for a shop, is `[u8 count][count× u8 item_id][ASCII name\0]` followed by the
shop's `0x1F` dialogue ("Welcome!", "Thank you!"). This was pinned from a live
PCSX-Redux capture standing in the Rim Elm Variety Store - its 10 item ids match
the curated [shop table](../reference/gamedata.md).

`shop::SceneShops` finds sites by **scanning** the decompressed MAN for the
op-`0x49` sub-op-`0` shop signature - *not* by an opcode walk. A shop's `0x49` is
often gated behind a dialogue confirm-picker ("Buy them?") whose option-jump
table desyncs a linear disassembler before it reaches the op (Biron Monastery's
Corey vendor is the case that exposed this), so a walk silently misses those
shops. The scan doesn't care how the script reaches the op; false positives are
ruled out by strict record validation: the byte after the opcode must be `0x00`
(sub-op 0 - this alone rejects almost every stray `0x49`), the count is small and
non-zero, every id is non-zero, and the trailing shop name is a printable,
letter-initial, `0x00`-terminated string. The apply layer additionally passes a
SCUS "id names a real item" mask (`locate_with_items`), so an id that names
nothing can't anchor a false shop. `apply::randomize_shops` then reassigns the
item-id bytes **globally** across every town shop (`Shuffle` redistributes the
existing shop-item multiset, `Random` draws from the **sellable pool**),
same-size, and recompresses each touched MAN like the chest path.

**No quest items; chest gear gets a price.** The sellable pool is "items the game
prices `> 0`" (`item_price::sellable_pool`, read from the item table's per-record
price - `u16` at record `+2`, base `0x80074368`; see
[item-table.md](../formats/item-table.md)). Quest / key / story items all ship at
price `0`, so this automatically keeps them out of shops - no hand-maintained
exclusion list. The flip side is that a handful of genuinely-equippable items are
normally *only found in chests* and so also ship at price `0` (the Ra-Seru
weapon/armor/shoe set + Astral Sword); `randomize_shops` first prices those
(`item_price::CHEST_EQUIPMENT_PRICES`, ~28800–55000 gold, approximated from the
nearest priced gear of the same type) with a same-size SCUS edit, so they're
non-free and part of the sellable pool. On the retail disc this is
34 shops (the picker-gated vendors a walk used to miss, plus duplicate scene
clusters and per-story-phase shop records). `--shops shuffle|random`; read-only
`legaia-rando shops` lists every shop's stock.

### Casino prize exchange

The **casino** prize list (redeem coins for prizes) is a different mechanism from
the gold town shops: it is a **static table** in the menu overlay's data segment
(`DAT_801e4518`), and it debits the casino **coin** bank (`_DAT_800845A4`), not
gold - which is how it's told apart from a gold merchant. It lives in **PROT
entry 899** (`0899_xxx_dat`, stored raw), file offset `0x15D00` (VA `0x801E4518`
under the overlay data-segment load base `0x801CE818`), as four `0x60`-byte
blocks of 8-byte `[u16 item_id][u16 story-gate][u32 coin-price]` records (the
high-value prizes carry a non-zero gate that locks them behind casino
progression). `casino::CasinoExchange` shuffles / randoms the whole records (so a
prize keeps its coin price and progression gate wherever it lands), a same-size
raw edit with no LZS. `--casino shuffle|random`; read-only `legaia-rando casino`.
At runtime the prize-exchange UI is a menu-overlay session: it runs at
`game_mode 0x17` (the CARD/menu pair) with PROT 0899 resident in slot A, the
same hosting as the pause menu and the gold shop (see
[`subsystems/shop.md`](../subsystems/shop.md)).

### Steal items (Evil God Icon)

What the player steals from a monster (Evil God Icon equipped) is a per-monster
entry in a **static `SCUS_942.54` table** at `DAT_80077828` - `[steal_chance_pct,
steal_item_id]` per 1-based monster id, item at `+id*2+1` (see
[steal-table.md](../formats/steal-table.md)). It is **not** in the PROT 867
record. Because it's a plain executable table, an edit is the simplest of the
four: a single same-size byte overwrite of the item, applied straight to the
SCUS file via `DiscPatcher::patch_named_file` (the non-PROT sibling of
`patch_prot_entry`, built on `legaia_iso::write::patch_file_logical`). No LZS
re-pack, no overflow, so nothing is ever skipped. `apply::randomize_steals`
reassigns the item for every stealable monster (`Shuffle` redistributes the
existing steal-item multiset, `Random` draws from the valid item pool) and
**preserves each monster's steal chance** - the item changes, the rate doesn't.
On the retail disc 189 monsters are stealable. `legaia-rando steals` lists the
current table (the audit surface).

### Monster combat stats

`--monster-stats` redistributes every enemy's combat stats across the
`battle_data` archive (PROT 867). Each monster's record carries its stats as
`u16` halfwords at fixed offsets in the decoded block (HP `+0x0C`, MP `+0x10`,
then ATK / UDF / LDF / AGL / SPD; see
[battle-data-pack.md](../formats/battle-data-pack.md) and the
[monster stat-record archive](../formats/battle-data-pack.md) docs). The
randomizer works **column-wise**: it collects each stat field across the whole
populated roster, then `Shuffle` permutes that column (a 1:1 reassignment, so
the multiset of, say, every monster's HP is exactly preserved - the overall
difficulty budget stays put, only *which* monster is tanky changes) while
`Random` draws each cell from the column pool. Spirit/SP (`+0x0E`) is left
alone, since it gates the AI's spell economy rather than player-facing
difficulty. Each edit re-packs the monster's slot through the same
decompress → edit → recompress path as the drop randomizer
(`monster::repack_slot`); the decoded length is unchanged, so every slot keeps
its `0x14000`-byte footprint (a slot too tight to re-pack is skipped, as with
drops). `legaia-rando monster-stats` lists the current stats.

A set of scripted enemies (`monster_stats::PROTECTED_MONSTER_IDS`) is excluded
from the pass entirely - both as a source and a target - so each keeps its
original stats and never donates them to another monster. Two kinds qualify. The
**early tutorial enemies** (the Rim Elm sparring partner and the first wild
Piura): the sparring fight is unwinnable by design and has no game-over branch,
so a hard-hitting attack could one-shot the party and soft-lock a fresh game, and
the early wild enemies are fragile by design. The **story bosses** (Gimard,
Caruban, Zeto, Songi, Berserker, Tetsu, Dohati, Xain, the three Delilas, Gaza,
Zora, Jette, Cort - every version of each): their set-piece fights are tuned around
scripted HP/phase triggers, so scrambling their stats can make a mandatory fight
unwinnable, and leaking a boss's extreme stats onto a trash mob would wreck
balance. This is the stat-side companion to the encounter randomizer already
leaving those formations scripted. Under `Shuffle` the column multisets are still
exactly preserved (the pinned values are conserved in place).

### Special-attack power

`--move-power` redistributes the per-move power values in the battle-action
overlay's move-power table (`0x801F4F5C`, PROT 0898; see
[move-power.md](../formats/move-power.md)). The damage kernel reads each 26-byte
record's `+0x00` halfword as the move's power roll modulus - this is the
**special-attack** power space (enemy specials + Seru-magic), not party Tactical
Arts, which take power from the per-strike art-record byte. Only the `+0x00`
halfword moves, and only among **populated** records - empty records, including
the index-0 sentinel the table self-identifies by, stay all-zero, so a power is
never handed to an unused slot. The other 24 bytes of each record (strike
geometry, phase timing, impact-effect / trail / sound cue, contact + launch
effect lists) are untouched, so every move keeps its own animation and effects;
only how hard it hits changes. PROT 0898 is stored raw, so the write is a
same-size raw-entry edit. `legaia-rando move-powers` lists the table, each entry
tagged with the spell-table name of a move that resolves to it.

### Element-affinity matrix

`--element-affinity` scrambles which element beats which. The battle-action
overlay carries an 8×8 affinity matrix (`matrix[attacker][defender]`, PROT 0898;
see [battle-formulas.md](../subsystems/battle-formulas.md) and
[move-power.md](../formats/move-power.md)) whose cells are
damage-scale percentages (`100` neutral, `> 100` weak, `< 100` resist, `0`
immune). `Shuffle` permutes the 64 cells (the multiset of scale percentages is
preserved - the same number of weaknesses / resistances exists, just between
different element pairs); `Random` draws each cell from that pool. Only the
matrix moves; the per-character element assignment and the summon-power rows are
left untouched, so the change is purely *which element pairs interact*. PROT
0898 is raw, so the write is same-size in place. `legaia-rando affinity` prints
the labeled grid.

### Spell MP costs

`--spell-cost` redistributes MP costs across the named, costed spells in the
static `SCUS_942.54` spell table (`DAT_800754C8`, cost at record `+3`; see
[spell-table.md](../formats/spell-table.md)). `Shuffle` permutes the cost column
(the MP multiset is preserved - every cost still exists, on a different spell);
`Random` draws each from that pool. Only the `+3` byte moves, and only **named,
non-zero-cost** spells participate, so free / internal enemy-tier entries never
gain a cost and names / target shapes are untouched. The table is in
`SCUS_942.54`, so the edit is a same-size in-place SCUS patch via
`patch_named_file` (like steals). `legaia-rando spell-costs` lists the table.

### Equipment stat bonuses

`--equip-bonus` redistributes the passive stat tuples on the static
`SCUS_942.54` equipment bonus table (`DAT_80074F68`, 8-byte rows; see
[equipment-table.md](../formats/equipment-table.md)). Each row's `+0..+4` is the
five-stat bonus `[INT, ATK, UDF, LDF, SPD]`; `+5` the accessory passive, `+6` the
equip-character mask, `+7` the slot type (body / head / weapon / footwear, plus a
Ra-Seru bit). The pass moves only the `+0..+4` tuple, and only **within a slot
category** - a weapon's stats only ever land on another weapon, armor on armor -
so the mask, passive, and slot type stay welded to their row and the per-category
power budget is kept. `Shuffle` permutes each category's tuples (the per-category
multiset is preserved); `Random` draws each row's tuple from its category pool.

It edits bonus **rows**, not item ids: several items can share one record, so a
per-id rewrite would double-edit a shared row and corrupt its bonuses
(`equip_stats::items_for_rows` maps rows → the ids that reach them). Rows no
equippable item references are left untouched, so an unused/garbage row can never
hand a real item a junk tuple. The table is in `SCUS_942.54`, so the edit is a
same-size in-place SCUS patch. `legaia-rando equip-bonuses` lists the table,
grouped by slot category, with the items that reference each row.

### Weapon specialty

`--weapon-specialty` (a toggle, not a mode) reassigns which weapon **class** each
character favors. In retail, equipping a weapon outside a character's favored class
(Vahn blades, Noa claws, Gala clubs/axes) makes that character's **arm** command
cost more AP in an arts combo, so fewer commands fit. As the
[arts command gauge](../subsystems/arts-command-gauge.md) doc traces, the cost is
not a runtime class comparison - it is a per-(character, weapon) byte baked into
the player battle file, at the weapon section's `decoded_section[+0x04]` (swing
record) `+0x74` (favored `0x1E` / off-class `0x2A`).

The pass permutes the three favored families (`{blade, claw, club}`) among the
three characters - a seeded bijection, so each class keeps exactly one specialist -
then walks each player file (`0863` Vahn / `0864` Noa / `0865` Gala) and, for every
weapon section, decompresses it, rewrites the arm-cost byte for the character's new
favored relationship, and re-compresses in place. The byte lives inside an LZS
stream, so this is the one feature that decompresses + re-compresses a section per
edit (a section too tight to re-pack is skipped and reported; in practice every one
re-packs). The Astral Sword and non-class gear carry no family and are never
touched, so the Astral Sword stays always-wide. `legaia-rando weapon-specialty`
shows each character's current favored class.

### Arts button combos

Each art's combo lives in **two** files, and both must change together
(emulator playtests proved editing only the menu copy leaves the trigger on the
old combo - see [art-data.md](../formats/art-data.md)):

- **The matcher** (what fires the art) reads the per-character art records at RAM
  `0x80160EFC`/`0x80176998`/`0x8018BA54`, where the combo is the `1=L,2=R,3=D,4=U`
  byte run at record `+0`, on a fixed `0xD0` stride. They load from each
  character's player-data file `record0` - Vahn `PROT 0861`, Noa `0864`, Gala
  `0865`. `randomize_arts` decompresses `record0`, rewrites each art's combo
  bytes in place (located by clean-start search filtered to the `0xD0` grid;
  multi-record arts like Noa's 3-level Hurricane Kick get all their records),
  and recompresses to fit the original footprint.
- **The display** is the SCUS `DAT_80075EC4` arts-name table `+8` glyph string
  (the menu arrows), rewritten in place to the same combo.

`apply::randomize_arts` (`--arts shuffle|random`) assigns each art a new combo
and writes it to both copies. Because the display glyph strings are
**deduplicated across characters** (Vahn's Cyclone and Noa's Swan Driver share
one `D U U U` string), the assignment is a permutation of the *distinct combo
strings'* contents within each length class - so each art keeps its **input
count** (a 4-input art stays 4 inputs) and each character's combos stay unique
(every character's arts map to distinct strings; a bijection keeps them
distinct). `Shuffle` reassigns existing same-length combos (no new input
ambiguity); `Random` writes fresh same-length combos. The per-character
**Miracle Art** (`0xFF09` marker) is left untouched. `legaia-rando arts` lists
the current combos.

### Doors (scene transitions)

A field scene reaches another scene through the field-VM **`0x3F`
named-scene-change op**, which carries its destination inline: `[i16 index]
[u8 name_len][name][entry_x][entry_z][dir]`. These ops are **partition-2 MAN
records**, addressed at runtime through the partition-2 record-offset table (the
controller sets the VM bytecode base to `man_base + data_region +
partition2[slot]` and runs the record - pinned by a PCSX-Redux dispatch trace;
see [MAN relocation](../formats/man-relocation.md)). On the retail disc there are
160 doors across 48 scenes; the overworld scenes (`map01`/`map02`/`map03`) are
the hubs.

Because the destination name is variable length, `apply::randomize_doors` is the
only randomizer that **resizes** an asset: it rewrites the `0x3F` op through the
relocation engine, recompresses the MAN, and rewrites the descriptor's
decompressed-size word. The whole destination descriptor (scene + entry tile +
facing) moves as one unit, so a re-pointed door always lands you somewhere valid.

`--door-coupling` picks the connectivity:

- **`coupled` (bidirectional, default)** re-pairs doors into two-way connections
  via a random involution over the sites - for matched doors `A` and `B`, `A` is
  sent to where `B` is reached from and vice versa, so walking through a door and
  turning around returns you the way you came. To guarantee that this never
  half-applies, coupled mode restricts itself to **length-preserving** swaps: it
  re-pairs only *balanced* connections (equal door counts in each direction)
  whose destination names match in length, so the decompressed MAN size is
  unchanged and **no scene - including the un-growable overworld hubs - can
  overflow**. The result introduces zero new one-way edges (a whole-graph
  symmetry invariant, asserted by `door_patch_real`). Doors with no
  length-compatible reverse partner (dead-end / one-way story warps, or doors
  orphaned by an unequal-direction connection) are left at their original
  destination - never given a one-way reassignment - and reported as `unpaired`.
- **`decoupled` (one-way)** reassigns every door's destination independently
  (`shuffle` permutes the existing destinations, `random` draws from the global
  pool), so going back through the destination's own doors is not guaranteed to
  return you. This is the variable-length path: a destination of any name length
  can land in any door.

In **decoupled** mode a scene whose rebuilt MAN can't grow within its on-disc
footprint (the big overworld hubs, whose next asset sits flush after the MAN) is
**skipped** - it keeps its original doors - and reported, rather than relocating
the whole bundle. (Coupled mode is same-size, so this doesn't arise; should a
recompress ever overflow anyway, the revert is a transitive closure over both
the new and original pairings, so a whole connection cycle reverts together
rather than half-applying.)

### House doors (intra-town)

Entering a house/interior within a town is **not** a scene change - it's an
**intra-scene reposition**: the field VM runs a `MOVE_TO` that teleports the
player to an interior sub-area tile in the *same* scene (pinned at the
instruction level by `probe.step.find_writer`; the writer is `FUN_801de840`
`case 0x23` - see [pcsx-redux-automation.md](pcsx-redux-automation.md)).

**The door warp has a clean structural signature.** A house-door reposition is
not a plain `0x23 xb zb` (that form moves the *executing actor* - it's how NPC
/ prop / cutscene scripts position things). It is the **cross-context form
`0xA3 0xF8 xb zb`**: opcode `0x23 | 0x80` dispatched into the system/player
script channel `0xF8`, i.e. "make the *player* MOVE_TO this tile"
(`tile = byte & 0x7F`). These ops live in the scene MAN's **partition-0
interaction records** - whose header is `[u8 n][n*2 SJIS name][u8 attr]`, *not*
partition 1's `[n][n*2][4-byte header]` shape - and the records carry an
explicit door-pairing convention in their SJIS names: fullwidth `ＩＮ`/`ＯＵＴ`
(optionally digit-suffixed - the Ratayu inn is one `ＩＮ` with three numbered
`ＯＵＴ`s), the 入口/出口 entrance/exit kanji (the Sol city gates), or trailing
`Ａ`/`Ｂ` endpoint letters (the tower elevators). The runtime-pinned Mei's-house
entry (`town01`) is exactly the `0xA3 0xF8 0x61 0x36` (interior tile `(97, 54)`)
in the record named `…ＩＮ`.

`legaia_rando::house_door::SceneHouseDoors` enumerates these classified door
warps (the record walk skips inline-dialogue `0x1F` segments, the same
ground-truthed rule as the chest walk) and `--house-doors shuffle` does a
**per-scene, class-preserving shuffle**: `ＩＮ`-class targets (interior landing
tiles) permute among the scene's entry warps, `ＯＵＴ`-class targets (exterior
doorsteps) among its exit warps. Every target stays a tile the scene's door
system already uses, NPC / prop / cutscene positions never move (plain
actor-context `MOVE_TO`s are untouched), and every exit still lands outside -
no interior-to-interior cycle (no softlock) is constructible. Each edit is a
same-size 2-byte operand swap recompressed in place (no relocation). On retail:
56 classified door warps (27 entries + 29 exits) across 12 scenes; a handful of
class-less partition-0 story warps (e.g. the town01 intro "inside the house"
reposition) are detected but deliberately left vanilla.

The feature is opt-in and `shuffle`-only (a `random` draw would place the
player off-map). The read-only `house-doors` listing shows the population per
scene. The disc-gated `house_door_classifier_real` test pins the per-scene
ＩＮ/ＯＵＴ census, the `0xA3 0xF8` signature of every site, and the captured
Mei's-house anchor; `house_door_patch_real` round-trips the shuffle off a
patched image and asserts the per-scene, per-class target multisets, EDC/ECC
validity, and seed determinism; and the engine-side
`house_door_randomizer_runtime_e2e` drives the patched warp op through the real
field VM, asserting the runtime lands on the patched interior tile (baseline =
the live-captured Mei's-house world coords).

Towns whose interiors are separate scenes (e.g. `retock` → `retockin`) reach
them through `0x3F` scene-change doors - those are the [door
randomizer](#doors-scene-transitions)'s population, not this one's.

### Starting items

A vanilla New Game begins with one inventory slot - Healing Leaf (item `0x77`)
×5 - and there is **no static starting-inventory table** to edit: the new-game
data-init `FUN_80034A6C` builds it in code, writing `inventory[0] = (0x77, 5)`
into the live consumable bag at `0x80085958` (`SC + 0x1818`) with an
`addiu`/`sb` pair (see [new-game-table.md](../formats/new-game-table.md)). So
this randomizer rewrites the **seed code** itself. The 40-byte region at
`0x80034b04` is reclaimable: it holds that seed plus a 6-instruction loop that
zeroes the 512 bytes *below* the inventory - redundant, because **both** callers
of `FUN_80034A6C` `memset` the whole `SC[0..0x1a18)` block (which contains the
inventory) right before the call.

`apply::randomize_starting_items` plans `n` distinct random consumables (each a
small random count) and writes one **packed halfword store** per item into that
region - an inventory slot is two contiguous bytes `[id][count]`, so
`addiu $v0, (count<<8)|id; sh $v0, (0x1818 + 2k)($s0)` seeds a slot in two
instructions. Ten instructions / two per item gives the inventory region **five**
slots. The adjacent warp-preset region (below) carries **two more** slots when
the all-warps preset is not using it, for a combined cap of **seven** starting
items (five with `--all-warps`); the slots the two regions write are contiguous
in the inventory array, so a decode replays both regions as one run. The patch is
the same size as the original code (no executable growth or relocation), applied
like the steal table via `patch_named_file`. Because the write lands directly in
the consumable page (bypassing the engine's id-routing add primitive), the pool
is the contiguous consumable block `0x77..=0x8e` (Healing Leaf … Wonder Elixir).
`--starting-items N` (0 = leave vanilla); the read-only `starting-items` listing
shows the current bag.

#### Beyond the seven-slot cap - opening-scene `GIVE_ITEM` injection

The direct seed is hard-capped at seven slots (five with `--all-warps`): the
reclaimable executable region is that small, there is no safe code cave, and the
file can't grow within the same-size-sector / PPF patch model. So when the bag
(convenience items **plus** the requested random fill) exceeds the cap, the
overflow is granted a different way - the way a treasure chest grants an item: a
run of **silent `GIVE_ITEM` field-VM ops** (`0x39`, `[0x39, id]`; the "found X!"
text is a separate `0xC2` token, so a bare `0x39` is a silent add) spliced into the
**opening scene `town01`'s entry script**. That script runs on every scene load, so
the block is wrapped in a once-only guard on a persistent SC story flag (the
`0x50` SET / `0x70` TEST bank at `0x80085758`, where `--all-warps` writes): test the
flag, skip the block if set, else grant the bag and set it. `apply::apply_starting_bag`
emits the guarded block (`starting_bag::guarded_grant_block`), inserts it at the
entry script's first opcode via `man_edit::apply_insertions` (the same partition /
jump-delta relocation the door randomizer uses), recompresses the MAN in place, and
bumps the descriptor size word. `starting_items::overflow_bag` computes the items
past the direct cap; the direct seed still writes the prefix, so `direct + overflow`
is exactly the full bag (unit-tested - no duplicate, no gap). The disc-gated
`starting_bag_real` oracle round-trips the injected bytecode; the runtime grant
needs a boot test (the guard bit `0xD70` is chosen from the high, retail-unused end
of the saved bitfield but isn't proven free at runtime - it's a tunable constant).

### Starting-bag convenience toggles

Opt-in flags that ride the same reclaimable seed region as the starting items,
built for fast-travel and pacing testing. Door of Wind (item `0x89`) is the warp
consumable: using one opens a menu to teleport to any town you have already
visited. Incense (item `0x8A`) lowers the random-encounter rate for a while.

**`--door-of-wind [N]`** seeds Door of Wind into the new game's starting bag -
`N` of them (1..=99; the default when the flag is given bare is 10). It is
*additive*: with no `--starting-items` reroll the vanilla Healing Leaf ×5 is kept
alongside it; with a reroll the random consumables replace the Healing Leaf and
Door of Wind is forced on top.

**`--incense [N]`** seeds Incense into the starting bag the same way (`N` of them,
1..=99, default 10 when given bare). It is additive on the same terms as Door of
Wind, and the two stack.

**`--speed-chain [N]`**, **`--chicken-heart [N]`**, and **`--good-luck-bell [N]`**
seed those *accessories* ("Goods") into the starting bag (`N` 1..=99, default **1**
when given bare). Although accessories are a different in-game category, the owned-
item list is a single ordered `(id, count)` array the menu only *filters* into its
Items / Goods / Key tabs - verified against a real end-game save, where Speed Chain
(`0xD1`), Chicken Heart (`0xF4`), and Good Luck Bell (`0xFC`) all sit in that one
list as plain `(id, count)` pairs - so an accessory seeds exactly like a consumable.

All five item toggles are *additive* (the vanilla Healing Leaf ×5 is kept unless a
`--starting-items` reroll replaces it) and stack. Forced items are seeded first so
they survive the capacity clamp, and a `--starting-items` reroll takes whatever
capacity they leave (excluding every forced id so it never deals a duplicate) -
so a random fill adds *on top of* the convenience items instead of being crowded
out by them, up to the seven-slot cap (five with `--all-warps`).

**`--start-with ID[:COUNT],…`** seeds *explicit* items into the starting bag -
comma-separated `id[:count]` entries (id decimal or `0xHH`, count defaulting to 1,
clamped to 99), e.g. `--start-with 0x89:10,0xd1,154:3`. Unlike `--starting-items`
(whose random fill is restricted to the consumable block so a *random* start stays
sensible), the explicit list takes **any** item id - consumable, weapon, armor, or
accessory - because they all share the one owned-item array. The picks are treated
like the convenience toggles: seeded into the forced prefix (after the toggles, in
the order given), excluded from the random reroll, and de-duplicated (an id already
seeded by a toggle or an earlier pick is skipped, an id-`0` or count-`0` entry is
dropped). Picks past the direct cap overflow into the opening-scene `GIVE_ITEM`
grant just like the random fill, so an arbitrarily long explicit bag still lands.
The options carrier is `StartingSeedOptions::extra_items`.

**`--all-warps`** presets the "visited towns" bitmask so Door of Wind can warp
*anywhere* from the start. That bitmask is a 32-bit story flag at `0x8008575C`
(`SC + 0x161C`), split into the two halfwords the well-known "Access All Towns"
GameShark code writes (`0x8008575C = 0xF77F`, `0x8008575E = 0xF8FF`). It lives in
the story-flag block (`SC + 0x14C0..0x16C0`), which the New-Game seed `memset`
covers, so the seed code can preset it the same way it presets the inventory. The
preset lives in a **second** reclaimable region in `FUN_80034A6C` -
`0x80034adc..0x80034aeb`, four redundant `sw $zero` stores into `SC` words the
caller already zeroed. This region does double duty: it holds **either** the
all-warps bitmask **or** the two item slots that overflow the inventory region
(slots 6–7), so `--all-warps` and a full seven-item bag are mutually exclusive -
turning all-warps on lowers the item cap to the inventory region's five. Both
forms use `$v1` (not `$v0`, which carries a live `0x2dc0` constant into
`DAT_80073ef8` just below). The bitmask form survives because the inventory
seed's zero-loop, which would otherwise re-clear `SC+0x161C`, is always
overwritten when the seed is rewritten; the overflow-item form writes inventory
offsets above that loop's range, so it survives regardless.
`region_unlocks_all_warps` / `scus_unlocks_all_warps` read the bitmask back, and
`StartingInventory::from_scus` replays both regions to recover the full bag.

The clean-room engine seeds every forced item (Door of Wind, Incense, and the
accessories) through the same `World::seed_starting_inventory` path as any other
starting item (covered by the runtime oracle); the Incense and accessory paths
have their own disc round-trip oracles (`incense_round_trips_on_disc`,
`accessories_round_trip_on_disc`). The all-warps preset has no engine consumer yet
- there is no Door-of-Wind warp menu in the port - so it is validated at the
disc-round-trip level (`door_of_wind_and_all_warps_round_trip_on_disc`) and
matches the user-verified GameShark write byte-for-byte.

### Starting level

**`--starting-level N`** (web: a dropdown) begins a New Game with the starting
party already at level `N` instead of 1 (`0`/`1` = vanilla; range `2..=14`). A New
Game seeds these live-record cells (see
[save-record.md](../formats/save-record.md) / [new-game-table.md](../formats/new-game-table.md)):
the **displayed level** at `+0x130` (what "LV" shows - boot-confirmed; *not* derived
from experience at a New Game), the **cumulative experience** at `+0x0`, the
**next-level threshold** at `+0x4`, and the stats from the party template. Crucially
the seed routine's **record-init loop stamps `+0x130` on every roster slot**, so the
displayed level applies to the whole starting party, not just the lead. Vanilla seeds
level 1 / experience 0; a coherent level-`N` start takes same-size in-place edits to
`SCUS_942.54`, applied by `apply::apply_starting_level`:

1. **Level** - the seed loop's level literal + stores set `+0x130 = N` (packed
   `addiu $v0, (1<<8)|N; sh $v0, 0x6f8($s0); nop`, keeping the magic-rank byte
   `+0x131` at 1) for **every** party record. This is what makes the status screen
   read **LV N**. (An earlier version stamped the level on all slots but only seeded
   the lead's stats, so Noa/Gala read **LV N** with level-1 stats - the bug step 4
   fixes.)
2. **Experience** - seed **each growth-capable slot's** `+0x0` to the **midpoint of
   level `N`'s XP band** (between the disc's own thresholds to reach `N` and `N+1`,
   `legaia_asset::level_up_tables::xp_thresholds_from_scus`), so every character's
   "Experience" readout and the level-up applier's progression are coherent - not just
   the lead's. The seed routine does not write `+0x0` natively, so the randomizer feeds
   one `addiu $t0, midpoint` preload (at `0x800560FC`, the old Terra threshold store)
   into three `sw $t0, <+0x0>($s0)` stores at `0x80056100` / `0x80056108` / `0x80056118`
   (the old Noa + Gala threshold literals and a redundant `lui $at`), targeting Vahn
   `0x5c8` / Noa `0x9dc` / Gala `0xdf0`. The preload is a single 16-bit immediate, so
   the value must fit a positive `imm16` (`<= 0x7FFF`), which caps the level at **14**.
   (An earlier version seeded only the lead, leaving Noa with experience `0` and Gala
   with a stale level-1 threshold of `140` - which dings her almost immediately.)
3. **Next threshold** - set **each growth slot's** `+0x4` cell (the "next" readout) to
   `reach(N+1)`. The literal at `STARTING_XP_SEED_VA` (`0x800560F0`, vanilla
   `addiu $v0, $zero, 0x79` = 121) loads it into `$v0`; dropping the per-character
   reloads in step 2 leaves `$v0` intact through the routine's three existing
   `sw $v0, <+0x4>($s0)` stores, so all three slots take the same `reach(N+1)`. The
   per-slot `FUN_801E9504` correction (Noa −, Gala +; the reason the vanilla
   thresholds are `121`/`102`/`140`, ≤2 % near these levels) is re-applied by the
   applier on each character's first post-seed level-up.
4. **Stats** - the level-1 starting-party template (`PARTY_TEMPLATE_VA`) feeds each
   live record, so the randomizer overwrites **every growth-capable slot's** eight
   `u16` stats with that character's level-`N` values, computed by accumulating the
   deterministic (jitter-free) per-level growth gains (`GrowthTables::level_gain_core`,
   the `FUN_801E9504` curve arithmetic) on top of the level-1 template - so a level-10
   start gives Vahn level-10 HP/ATK/… (e.g. HP 584 vs the vanilla 180) *and* the same
   for Noa and Gala, matching the level the loop stamps. The growth table covers the
   three main characters (`GROWTH_CHAR_COUNT`); the 4th template slot (Terra) has no
   growth curve, so it keeps its base stats (she is a scripted guest who re-scales on
   her late join). Each 10-byte name is left untouched.

The disc-gated `starting_level_real` test round-trips the edit off the patched
image - the seeded experience decodes back to the requested level, the
level/experience/threshold instructions carry the planned values, **each** leveled
slot's template stats are the growth-curve values and strictly above that
character's vanilla stats (with its name preserved), and the surrounding
seed-routine code stays byte-identical and EDC/ECC-valid. A companion test runs a
tiny MIPS-subset interpreter over the *patched* seed routine and asserts every
growth slot's live record lands with the right `+0x0` / `+0x4` / `+0x130` - proving
the whole party, not just the lead, ends up coherent. The randomizer is enabled
at level 10 in the web "Balanced" and "Full Chaos" presets and off in
"Vanilla" / "Item Shuffle".

### Unused content

The game ships fully-formed content it never surfaces in normal play; two opt-in
toggles bring it back. They are *additive* - a normal run never places them, so
the disc stays vanilla unless you ask. Both are pinned by the disc-gated
`unused_content_real` test.

**`--unused-enemies`** re-introduces two cut enemies that no scene's encounter
formation references: **"Comm"** (id 78, a complete standalone record - HP 2520,
casts magic, exp 945) and the **Evil Bat** (monster ids 176/177/178, byte-identical
clones of each other and of the in-use Evil Bat at id 140). The battle loader
streams a monster's
`0x14000` archive slot on demand keyed by its id - there is **no per-scene
monster preload list** - so injecting one of these ids into a formation byte is
sufficient to make it spawn and render; nothing else needs patching. The toggle
adds the curated ids ([`unused::UNUSED_ENEMY_IDS`]) to each scene's encounter
candidate pool. It only takes effect with `--encounters random`: a
multiset-preserving `shuffle` can't introduce a new monster, by construction.

**`--unused-items`** adds two items to the random-fill pool used by the `random`
drop / chest / steal modes:

- **"Something Good" (`0x6B`)** - a 50,000 G sell item the shipped game never
  hands out. It is *named* in the item table, so the valid pool already accepts
  it; the toggle includes it explicitly for clarity.
- **the unnamed accessory (`0xFD`)** - an accessory-class slot whose name string
  is *empty*, so the valid pool excludes it. The toggle is what makes it
  obtainable. Because a blank name would read as an empty line in chests / menus,
  the toggle also **names it "Seru Bell"**: it writes the string into a reserved,
  runtime-**constant** region of `SCUS_942.54` and repoints *only* `0xFD`'s name
  pointer at it (a same-size patch, like the starting-item seed; the other ids
  that share the empty-string slot - `0x12`/`0x1A`/`0x52`/`0xB9` - are left
  blank). Picking the target is the subtle part: the data segment's *trailing*
  zero-fill is **not** usable - it is zero in the file but is `.sbss`/`.bss`-class
  scratch the game overwrites with variables at runtime (a string there renders
  as a glyph that changes every frame). Worse, a region that is zero in the file
  *and* zero at runtime is still not automatically safe - it can be boot-cleared
  scratch, which wipes the written string to zero (the name then renders empty).
  The reliable test is the *flanking* bytes: the string goes to
  `item_name::SERU_BELL_STRING_VA` (`0x8007AB40`), inside a 1028-byte zero gap at
  `0x8007AB38` whose adjacent rodata constants are preserved byte-for-byte across
  the file + diverse runtime states - proving it is read-only padding the loader
  keeps, not scratch. The injection guards on the target bytes being zero, so a
  differently-laid-out image is skipped rather than corrupted.

  The accessory's documented effect is to make only Seru-class enemies appear in
  random encounters. Because it is unobtainable in retail that effect is never
  exercised by the shipped game, so treat it as experimental.

### Re-pack slack

A scene MAN is packed with **no compressed slack** (the next asset starts right
after it), so the re-packed stream must be no larger than the original. The
[LZS re-packer's lazy matching](../formats/lzs.md#encoding-re-packing) makes that
hold for every scene MAN but one (and for every monster-archive slot). The rare
stream that still overflows is **skipped** (its scene / slot left unchanged) and
recorded in the apply report rather than aborting the run; the CLI prints the
skipped entries.

The "no larger than the original" budget is measured from the scene asset-table
**boundary** (the MAN's allotted span up to the next descriptor's offset), *not*
from the current compressed length. That matters when several passes (encounter,
chest, shop) edit the same scene MAN in one run: our re-packer is often a touch
tighter than Sony's, so reading the budget back from the just-written shorter
stream would shrink it on every pass and make a later pass needlessly overflow
and skip a scene - which is what left some shops (e.g. Biron Monastery's) vanilla
when run alongside encounters/chests. The boundary is fixed (all edits are
same-size in place), so every pass gets the same full budget.

## The patch chain

A PROT-entry-relative edit maps to a disc byte range like this:

```text
disc image (2352-byte sectors)
  -> ISO 9660: PROT.DAT lives at disc sector prot_lba
    -> PROT TOC: entry N starts at start_lba[N] * 2048 bytes into PROT.DAT
      -> asset: an edit at offset_in_entry bytes into the entry
```

so a PROT-entry-relative offset becomes the PROT.DAT-logical offset
`start_lba[N] * 2048 + offset_in_entry`, which
`legaia_iso::write::patch_file_logical` turns into physical-sector writes plus
EDC/ECC re-encode. `DiscPatcher::patch_prot_entry` is the generic entry point;
`patch_monster_slot` / `monster_slot` are the `battle_data` helpers.

## EDC/ECC: not game-specific

The error-correction math is the generic CD-ROM scheme from ECMA-130 / the
Yellow Book - the same EDC (CRC, reversed polynomial `0xD8018001`) and
Reed-Solomon P/Q ECC (over GF(2⁸), generator `0x11D`) every PSX disc and every
mastering tool uses. The header (`0x00C..0x010`) is treated as zero per the
Form 1 convention, so parity is independent of the sector's MSF address. It
embeds no game bytes. The decisive correctness check is the disc-gated test that
re-encodes real PROT.DAT sectors and reproduces their stored EDC/ECC
bit-for-bit.

## Tests

| Test | Gate | What it proves |
|---|---|---|
| `crates/lzs` unit tests | CI | `decompress(compress(x)) == x` across literals, RLE, repeats, pseudorandom, >4 KB-window input; structured data actually compresses |
| `crates/asset` `lzs_compress_roundtrip_real` | disc-gated | the encoder round-trips real monster records + LZS-container sections, and compresses them |
| `crates/iso` `write` unit tests | CI | encode is idempotent / self-consistent; corrupting user data invalidates until re-encoded; ECC is address-independent; a seam-straddling patch keeps both sectors valid |
| `crates/iso` `ecc_real` | disc-gated | the encoder reproduces real PROT.DAT sectors' EDC/ECC bit-for-bit; a one-byte patch + restore round-trips a real sector exactly |
| `crates/rando` unit tests | CI | seeded planner determinism; shuffle preserves the drop multiset; surgical `set_drop`; PPF diff/write/apply round-trip; a synthetic-disc patch round-trips through the disc → ISO → PROT chain |
| `crates/rando` `disc_patch_real` | disc-gated | patch a real monster's drop onto a scratch copy of the disc; it re-decodes off the patched image with neighbours untouched and sectors valid |
| `crates/rando` `rando_cli_real` | disc-gated | full-archive shuffle: plan from a seed → apply → each monster reads its planned drop (skipped slots unchanged) → diff into a PPF that reproduces the patched image; deterministic for a fixed seed |
| `crates/rando` `encounter_patch_real` | disc-gated | whole-disc encounter shuffle: re-decode every patched scene MAN off the disc and assert counts + id multiset preserved, ids in-pool, sectors EDC/ECC-valid, deterministic; **plus** every scripted/boss formation (Tetsu id `0x4F` among them) is byte-identical after the shuffle |
| `crates/rando` `chest_patch_real` | disc-gated | whole-disc chest shuffle: re-decode every patched scene MAN, assert give-item site offsets unchanged + chest-item multiset preserved + sectors valid + deterministic |
| `crates/rando` `steal_patch_real` | disc-gated | whole-disc steal shuffle: re-read the patched `SCUS_942.54` steal table, assert the steal-item multiset preserved + every steal chance byte untouched + the table sector EDC/ECC-valid + deterministic |
| `crates/rando` `arts_patch_real` | disc-gated | arts-combo shuffle + random: re-decode the patched combos, assert every art keeps its input count + each character's combos stay unique + the Miracle Arts untouched + (shuffle) the global per-length set of distinct combos preserved + sector EDC/ECC-valid + deterministic; **plus the MATCHER GUARD** - decompress each character's player-file `record0` and assert every art's display combo is present as a matcher record and the records actually changed (the desync the feature tripped over) |
| `crates/asset` `man_edit` unit tests | CI | the MAN relocation engine: grow / shrink a destination name relocates the section + later-record offsets, a spanning relative jump's delta is fixed (a non-spanning one isn't), the rebuilt MAN re-parses |
| `crates/rando` `door_enumerate_real` | disc-gated | whole-disc door census: 160 doors across 48 scenes, every destination a clean CDNAME label, the pinned town01 → map01 exit present, the overworld hubs fan out |
| `crates/rando` `door_patch_real` | disc-gated | whole-disc door shuffle (one-way + coupled): re-decode every patched scene MAN, assert the destination multiset preserved (clean shuffle) / names valid (with skips), sectors EDC/ECC-valid, image size unchanged, deterministic |
| `crates/rando` `house_door_classifier_real` | disc-gated | house-door warp census: every classified site carries the `0xA3 0xF8` cross-context player-MOVE_TO signature, the per-scene ＩＮ/ＯＵＴ class counts match the audited population (12 scenes, 27 + 29 sites), targets non-sentinel, and the runtime-captured Mei's-house interior `(97, 54)` is among town01's ＩＮ targets |
| `crates/rando` `house_door_patch_real` | disc-gated | whole-disc intra-town (house) door shuffle: re-decode every patched scene MAN, assert the per-scene ＩＮ-class and ＯＵＴ-class door-warp target multisets each preserved, sectors EDC/ECC-valid, image size unchanged, deterministic |
| `crates/rando` `starting_items_patch_real` | disc-gated | starting-item randomize: re-decode the rewritten `FUN_80034A6C` seed off the patched `SCUS_942.54`, assert the seeded items match the plan + are in-pool consumables + the surrounding function bytes are untouched + image size unchanged + sector EDC/ECC-valid + deterministic |
| `crates/rando` `equipment_drops_real` | disc-gated | inject the bonus equipment drop into a scratch `SCUS_942.54`; assert off the patched image that the hook site holds `j routine` + nop, the routine + id table decode as the hand-assembled bytes (replaying the two displaced instructions and returning), the table holds pool equipment ids, the edit is surgical (only the hook + routine regions change) and the disc still parses; byte-deterministic; the build guard refuses a corrupted hook site / non-dead routine region |
| `crates/rando` `flee_exp_real` | disc-gated | inject the run-away EXP hook: assert the real disc's escape-teardown site (PROT 898, VA `0x801E5A10`) **is** the expected displaced pair, then off the patched image that the overlay detour is `j routine` + nop, the SCUS routine decodes as the hand-assembled bytes (replaying the displaced pair + returning), each edit is surgical (only the 8-byte hook / the routine region change), the patched overlay + image still parse and stay EDC/ECC-valid; byte-deterministic; the build guard refuses a corrupted hook site / non-dead routine region |
| `crates/rando` `enemy_ally_real` | disc-gated | inject the enemy-ally charm: assert the real disc's setup hook (SCUS, VA `0x80051990`) **is** `lui v1,0x8008` / `lbu v1,-0x42f4(v1)` and the victory site (PROT 898, VA `0x801E6638`) **is** `andi v0,v0,0x4`, then off the patched image that the SCUS detour is `j routine` + nop, the routine decodes as the hand-assembled bytes (sets `0x380`, replays the displaced pair, returns), the victory word is widened to `andi v0,v0,0x384`, each edit is surgical, it composes with flee-EXP in the same gap, the image stays EDC/ECC-valid; byte-deterministic; the build guard refuses a corrupted hook / non-dead routine region / unexpected victory word |
| `crates/rando` `shiny_seru_real` | disc-gated | inject shiny Seru: assert all eight hook sites match the known US build (setup `0x80051A20`, capture `0x801EE2E8`, grant `0x801E93B4`, damage `0x801DDB08`, level-up `0x801E71C8`/`71DC`/`7224`, menu `0x801D2FA0`) and all three routine regions (`0x80077728` / `0x801F4FC4` / `0x800783C4`) are dead space, then off the patched image that every detour became `j routine` + nop, the capturable bitmap has Gimard (10) set and gobu (4) clear, every byte outside the planned edits is untouched (across SCUS + overlays 0898/0899), the disc still parses + stays EDC/ECC-valid, it composes with enemy-ally (disjoint gaps), byte-deterministic, and the build guards refuse a corrupted hook / non-dead region |
| `crates/rando` `shop_patch_real` | disc-gated | enumerate every town shop (assert the Rim Elm Variety Store + its 10 ids, names printable, ids named); a town-shop shuffle preserves the global multiset + per-shop counts/names + is deterministic; a casino shuffle preserves the (item, coin-price) prize multiset + block counts + is deterministic |
| `crates/rando` `item_price_real` | disc-gated | the 13 chest-found equipment items ship at price 0 and get the reviewed shop values (idempotent), the sellable pool (item price > 0) includes them + excludes known quest/key ids, and a shop `Random` pass only stocks priced (non-quest) items |
| `crates/rando` `unused_content_real` | disc-gated | the unused-content facts: Evil Bat ids 176/177/178 are byte-identical clones of id 140, "Comm" (id 78) is a populated standalone record (not a clone); item `0x6B` is named vs `0xFD` unnamed (so the pool widens by exactly one); the `--unused-enemies` toggle injects an unused id only when enabled (deterministic); and the "Seru Bell" injection names only `0xFD` (others stay blank), same-size, sector EDC/ECC-valid, idempotent |
| `crates/rando` `monster_stats_real` | disc-gated | whole-archive monster-stat shuffle: re-decode every patched `battle_data` record off the disc, assert each stat column's multiset is preserved, every non-randomized field (spirit, drop, exp, gold, name, element) byte-identical, every protected monster's (tutorial enemies + story bosses) combat stats unchanged, slot footprints fixed, deterministic |
| `crates/rando` `move_power_real` | disc-gated | special-attack power shuffle: re-parse the patched PROT 0898 move-power table, assert the power multiset preserved + every non-power record byte byte-identical (only `+0x00` moves) + deterministic |
| `crates/rando` `element_affinity_real` | disc-gated | element-affinity shuffle: re-parse the patched PROT 0898 matrix, assert the scale-percent multiset preserved + the per-character element + summon-power sibling tables untouched + deterministic |
| `crates/rando` `spell_cost_real` | disc-gated | spell MP-cost shuffle: re-read the patched `SCUS_942.54` spell table, assert the MP-cost multiset + the named/costed-spell id set preserved + the table sector EDC/ECC-valid + deterministic |
| `crates/rando` `equip_bonuses_real` | disc-gated | equipment stat-bonus shuffle: re-read the patched `SCUS_942.54` bonus table, assert each slot category's `+0..+4` stat-tuple multiset preserved (no tuple crosses categories) + every row's `+5/+6/+7` tail (passive/mask/slot) byte-identical + the table sectors EDC/ECC-valid + deterministic |
| `crates/rando` `seru_trade_real` | disc-gated | seru-trade config write: assert an unpatched disc reports no config, then off the patched image the embedded blob decodes back to the written `(enabled, seed, offer cap)`, the write is same-size + a tiny localized edit, re-running with a new seed overwrites the prior blob, and a fixed seed is byte-deterministic |
| `crates/engine-core` `seru_trade_randomizer_runtime_e2e` | disc-gated | runtime oracle: patch the seru-trade config onto the disc, re-decode it from the patched SCUS, install it into a `World` holding a known party, open a vendor session, confirm the first offer, assert the owner's spell list swaps give→receive, and that advancing past a two-in-game-hour boundary reseeds the offers (baseline: an unpatched disc reports trading disabled) |
| `crates/engine-core` `chest_randomizer_runtime_e2e` | disc-gated | runtime oracle: patch one chest, re-decode the MAN off the patched image, drive its inline interaction script through the real field VM, assert the runtime grants the patched id (not the original) |
| `crates/engine-core` `monster_drop_randomizer_runtime_e2e` | disc-gated | runtime oracle: patch one monster's drop item, re-decode the record off the patched archive, build the engine catalog, drive a one-monster formation through the victory-spoils path (`apply_battle_loot`), assert the runtime grants the patched drop (not the original) |
| `crates/engine-core` `encounter_randomizer_runtime_e2e` | disc-gated | runtime oracle: patch one scene formation's slot-0 monster id, re-decode the MAN off the patched image, build the encounter table + per-row formation defs from those bytes, force that row into a battle through the live-loop encounter path, assert the spawned enemy actor carries the patched id (not the original) |
| `crates/engine-core` `steal_randomizer_runtime_e2e` | disc-gated | runtime oracle: patch one monster's steal item byte in `SCUS_942.54`, re-decode the steal table off the patched image, drive the engine steal-grant kernel (`World::apply_steal`), assert the runtime steals the patched id (not the original); chance preserved |
| `crates/engine-core` `arts_randomizer_runtime_e2e` | disc-gated | runtime oracle: shuffle the arts combos (in-place glyph-byte edits), re-decode them off the patched image, and drive the real combo-recognition kernel (`battle_arts::chain_matches_record`) - assert every changed art fires on the new combo bytes and no longer on the old one (baseline: each art fires on its original combo) |
| `crates/engine-core` `door_randomizer_runtime_e2e` | disc-gated | runtime oracle: patch Rim Elm's exit (the `0x3F` op → map01) to a differently-named scene, re-decode the patched MAN off the patched image, drive the patched op through the real field VM (`World::load_field_script` + `tick`), assert the runtime warps to the patched destination (not the original) |
| `crates/engine-core` `house_door_randomizer_runtime_e2e` | disc-gated | runtime oracle: baseline town01's Mei's-house entry warp (`0xA3 0xF8`) through the real field VM at the live-captured world coords `(0x30C0, 0x1B40)`, shuffle the house doors on a scratch copy, re-decode the patched MAN, drive the same op offset and assert the runtime warps to the patched interior tile (not Mei's) |
| `crates/engine-core` `starting_items_randomizer_runtime_e2e` | disc-gated | runtime oracle: confirm a New Game off the unpatched disc seeds Healing Leaf ×5 (baseline), randomize the seed on a scratch copy, re-decode it off the patched image, seed a fresh world via `World::seed_starting_inventory`, assert the bag holds exactly the patched items (not the vanilla Healing Leaf ×5) |
| `crates/engine-core` `unused_enemy_randomizer_runtime_e2e` | disc-gated | runtime oracle: run the `--unused-enemies` toggle path until it places an unused Evil Bat id at a formation slot, re-decode off the patched image, force that row into a battle, assert the spawned enemy actor carries an unused-enemy id (baseline spawns the vanilla monster) |
| `crates/engine-core` `unused_item_randomizer_runtime_e2e` | disc-gated | runtime oracle: apply the "Seru Bell" name injection and assert the item table resolves `0xFD` to it (others stay blank), then patch a monster's drop to `0xFD` and drive `apply_battle_loot`, asserting the bag receives the unused accessory (baseline grants the original) |
| `crates/engine-core` `shop_randomizer_runtime_e2e` | disc-gated | runtime oracle: patch a town-shop slot (scene MAN op `0x49`) and a casino prize (PROT 899 table), re-decode the patched stock, drive `World::buy_from_shop` (shared with the menu `ShopConfirm` commit), assert the runtime sells/grants the patched id (not the original) |

Disc-gated tests read `LEGAIA_DISC_BIN`; with it unset they skip and pass.

The `engine-core` runtime oracles answer a question the `crates/rando`
patch tests don't: not just that the patched byte is *written* faithfully, but
that a runtime actually *reads it and acts on it* - grants the new item, spawns
the new monster, or warps to the new scene. A savestate can't prove this - the
scene MAN / `battle_data` archive / steal table is resident in RAM the moment
you're in the room / battle (or as soon as the executable loads), so a state
captured on a patched disc still serves the original from the cached RAM copy;
the patched value is only seen after a fresh scene / battle / executable load
re-streams it off disc. The clean-room engine sidesteps that cache by decoding
straight from disc bytes and running the actual grant / spawn / warp path, so it
observes the patch a savestate would mask.

## No-Sony-bytes hygiene

The crate never embeds, commits, or redistributes game bytes. A patched `.bin`
contains Sony data and is never committed; the intended distribution form is a
patcher tool + seed, and/or the **PPF patch** the CLI emits. A PPF carries only
the deltas between the user's original disc and the patched one - it is
meaningless without the original image the user already owns, so it is safe to
share where a patched `.bin` is not.

## See also

- [`crates/rando`](../../crates/rando/README.md) - the crate.
- [LZS compression](../formats/lzs.md) - the encoder this builds on.
- [PSX disc geometry](../formats/disc.md) - the Mode 2/2352 sector layout.
- [PROT.DAT TOC](../formats/prot.md) - entry → LBA addressing.
- [Monster animation](../formats/monster-animation.md) and
  [encounter records](../formats/encounter.md) - the data the randomizer edits.
