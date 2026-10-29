# Expansion packs: battles and progression

Two files of an expansion pack shape its fights: `battle.toml` (monsters, formations, encounter overrides, bosses) and `progression.toml` (the XP curve, EXP and gold multipliers, enemy level scaling, level-up rewards). Both are optional and every field in them is optional, so a pack writes only what it changes. The parser is `legaia_expansion_format::{battle, progression}`; the runtime kernels are `legaia_engine_expansion::{battle, progression}`; the `World` hooks are `crates/engine-core/src/world/expansion.rs`.

With no pack active every hook is the identity, and the retail game plays bit for bit (`engine-core` test `expansion_battle::no_pack_and_a_dropped_pack_play_the_retail_fight`).

## Ids

Every key in these files is local to the pack. The engine qualifies it with the pack id: key `ash_court` in pack `rebirth` is `rebirth:ash_court`. A reference may use either form; a reference into another pack is refused.

Disc content is named only by coordinate ([`DiscRef`](../../crates/expansion-format/src/refs.rs)): a retail monster by its 1-based monster-archive id (`{ kind = "monster", id = 73 }`), an item by its item id (`{ kind = "item", id = 12 }`), a music cue by its sound-test index (`{ kind = "bgm", id = 30 }`).

## Monsters

```toml
[monsters.ashen_caruban]
base = { kind = "monster", id = 73 }
name = "Ashen Caruban"
hp = 30000                 # a bare number sets the stat
attack = { mul = 1.8 }     # { mul = x } scales the base monster's
exp = { mul = 12.0 }
drop = { item = { kind = "item", id = 12 }, chance_pct = 50 }
steal = { item = { kind = "item", id = 9 }, chance_pct = 25 }
element = 3
spells = [0x81]
```

A pack monster is always **derived** from a retail one. It keeps the base monster's mesh, animations, attack set, sounds and Seru, which the engine reads off the player's disc; the pack changes its numbers.

| Field | Effect |
|---|---|
| `base` | The retail monster it is built on (required). |
| `name` | The battle HUD name. |
| `hp`, `mp`, `exp`, `gold` | Set or scaled; held to `0..=65535`. |
| `attack`, `defence`, `agility`, `speed`, `intelligence` | The record stats ATK, UDF + LDF, AGL, SPD, INT. |
| `drop`, `steal` | The victory drop and the Evil God Icon steal, item and percent. |
| `element` | Element id `0..=7` (`7` neutral). |
| `spells` | Spell ids the monster AI may cast, at most three; replaces the base's. |

The battle stats are **record** values, not what the fight installs. The battle loader boosts the record by the fight's class exactly as it does a retail one: a scripted fight installs ATK `x5/4` and both defences `x2`, a random encounter defences `x7/4` and INT `x5/4` (`MonsterDef::installed_stats`, [`battle.md`](../subsystems/battle.md)). Record stats clamp to `0x3FFF` so the scripted doubling cannot wrap 16 bits.

### Pack monsters fight under their base monster's id

The monster id a battle seat carries keys every disc read for that seat - mesh, action clips, the AI's swing entries, the battle heap layout - in both play hosts. A pack monster therefore keeps its base monster's id. For the length of a fight the engine swaps the pack monster's numbers into the catalog under that id and restores the scene's entry when the fight ends (`World::expansion_prepare_battle` / `expansion_end_battle`).

The consequence for authors: one formation holds one version of each base monster. A formation of two `ashen_caruban` is fine; `ashen_caruban` beside a retail Caruban, or beside a second Caruban variant, is a validation error.

## Formations

```toml
[formations.ash_court]
label = "Ash Court"
monsters = ["ashen_caruban", { kind = "monster", id = 12 }]
scripted = false
```

A slot is a pack monster key or a retail monster ref. A formation may continue in [waves](#waves). `scripted = true` makes the formation a retail-style boss row: no escape, the boss battle intro, the alternate seat layout and the scripted stat boost (the formation header byte `record[+0]`, [`encounter.md`](encounter.md)).

Pack formations register in the formation table at ids `0xE000` and up, in key order, beside the scene's own MAN rows. They are re-registered before every pack fight, since a scene's MAN install fills the table.

### Formation size

A formation holds at most **four** monsters at once. The battle actor table has eight seats: the party takes seats `0..=2`, and seat `7` is the summon seat every Seru cast poses its creature on, which leaves four monster seats beside a full party. Retail's largest formations are four monsters, so four is both the retail and the engine limit; a fifth monster would share the summon seat with the first summon cast. A fight with more enemies than that brings them in [waves](#waves): later monsters take the seats earlier ones vacate.

## Waves

```toml
[formations.tower_gauntlet]
label = "Tower Gauntlet"
monsters = ["ironhide_ogre", "ironhide_ogre"]        # wave 1

[[formations.tower_gauntlet.waves]]                  # wave 2
monsters = ["ember_lizard", "ember_lizard", "ember_lizard"]

[[formations.tower_gauntlet.waves]]                  # wave 3
monsters = ["runed_golem", "ironhide_ogre"]
arrival = "refill"
count = 6
max_standing = 3
```

`monsters` is the first wave, the monsters the battle opens on. Each `[[formations.<key>.waves]]` table after it is one more wave, in order. The battle never returns to the field between waves: a wave's monsters take the monster seats of the fight in progress, a "Wave N / M" banner stands for the battle timed message's hold (about two seconds) as a wave begins, and when the last wave is down the ordinary victory runs once.

| Field | Effect |
|---|---|
| `monsters` | The wave's monsters, pack keys or retail refs. |
| `arrival` | `wipe` (the default): the whole wave seats together once every monster standing is down, at most four. `refill`: one monster per freed seat, at the start of a round. |
| `count` | `refill` only: arrivals in all, cycling through `monsters` (default one per listed monster, at most 64). |
| `max_standing` | `refill` only: no arrival while this many monsters stand (default four). |

A formation lists at most 16 waves after its first.

### When a wave arrives

A `wipe` wave arrives at the moment the action that downed the last standing monster resolves: the battle action SM's end-of-action wipe scan asks the `World` for more monsters before it signals the victory, and seats them before any frame sees an empty monster side. The round then runs on: party members still owed a turn act against the new arrivals, and the next round opens with every seat's initiative rolled afresh.

A `refill` wave tops up freed seats as each round opens, ahead of the round's initiative seed, so its arrivals act in that round; a wipe also refills every free seat at once. When a wave starts that stands more monsters than the seats laid out, the seat layout widens to it and the monsters still standing move to the wider layout's seats. The next wave, of either kind, takes over once a `refill` wave has sent its `count`.

### Spoils

Every monster a wave seats joins the fight's formation, so the one victory pays all of them together: the EXP of every wave summed (then the pack's `xp_mul`, the retail `3/4` share), the gold accumulated over every monster (then the retail halving and the pack's `gold_mul`), and one drop roll over every seat. A fight the party flees or loses pays nothing, as any fight.

### One version of each base monster

A pack monster fights under its base monster's id, and one catalog swap covers the whole battle, so the one-variant rule holds across all of a formation's waves together: an `ashen_caruban` in wave 1 and a retail Caruban in wave 3 is a validation error.

### Escape and bosses

Escape stays open between and during waves unless the formation is scripted (`scripted = true`, or a `no_escape` boss's formation). A boss whose formation has waves pays its `reward_items` and `reward_gold` once, after the last wave.

## Encounter overrides

```toml
[[encounters]]
scene = "chitei2"
retail_weight = 3
formations = [{ formation = "ogre_pack", weight = 2 }, { formation = "forge_guard" }]
steps = [120, 240]
```

An override changes what one scene (by CDNAME name) rolls.

- **Without `steps`**, retail's own rate decides *when* a fight comes - the region roll or the mean-rate stand-in, every Incense and accessory modifier included - and the override decides *which*: one `rand()` picks among the listed formations by weight, with the retail roll's own formation keeping `retail_weight` (default `0`, which replaces every retail fight in the scene).
- **With `steps = [min, max]`**, the pack owns the rate: a counter drawn in `min..=max` counts field steps and fights when it runs out, so a scene retail never fights in can hold encounters. The session's transition and post-battle grace, the Incense window and the engaged hold still gate it.

## Bosses

```toml
[bosses.ash_king]
formation = "ash_court"
no_escape = true                     # the default
music = { kind = "bgm", id = 30 }
reward_items = [{ item = { kind = "item", id = 233 }, count = 1 }]
reward_gold = 50000
once = true
```

A boss is a formation plus staging. `no_escape` marks its formation scripted. `music` replaces the battle track for this fight (when the host's battle-music swap is on). `reward_items` and `reward_gold` pay on victory on top of the formation's spoils, and land in the spoils' drops and gold. A `once` boss refuses a second start after a victory (`World::expansion_boss_cleared`).

Quests and maps start fights through two `World` entry points: `start_expansion_battle(formation)` and `start_expansion_boss(boss)`. Both go through the ordinary encounter path - transition, battle music, `begin_encounter_battle` - as `World::force_encounter` does.

## Progression

```toml
xp_mul = 1.5
gold_mul = 1.25

[xp_curve]
kind = "banded"
bands = [{ from_level = 60, mul = 0.35 }, { from_level = 80, mul = 0.3 }]

[enemy_scaling]
from_level = 50
per_level = 0.02
max_mul = 1.8
scope = "pack"
rewards = true

[[level_rewards]]
level = 99
items = [{ item = { kind = "item", id = 237 } }]
character = 0
```

| Field | Effect |
|---|---|
| `xp_mul`, `gold_mul` | Scale every won battle's summed EXP (before the retail `3/4` share) and credited gold. |
| `xp_curve` | `retail`; `scaled { mul }`; `banded { bands }`, which scales retail's per-level steps from each band's `from_level`; `explicit { levels }`, 98 cumulative values for levels 2 to 99. |
| `enemy_scaling` | Past `from_level`, each level of the strongest present member adds `per_level` to a stat multiplier capped at `max_mul`. |
| `level_rewards` | Items granted when a character first reaches `level`; `character` (roster slot) restricts it to one. |

Every curve is held rising and within the record's `9,999,999` ceiling. The pack's curve replaces the tracker's table at activation and the previous one returns at deactivation; the Noa / Gala threshold corrections still apply on top of it.

Enemy scaling multiplies HP, MP, ATK, both defences, INT and SPD, and EXP and gold when `rewards` is set. It leaves AGL alone, since AGL is the action gauge a monster's swings spend and scaling it changes how many attacks a turn holds. `scope = "pack"` reaches the pack's formations only; `scope = "all"` reaches retail fights too.

## Engine wiring

| Hook | Where |
|---|---|
| Activation, formation install, XP curve swap | `World::activate_expansion` / `deactivate_expansion` |
| Base monsters merged from the disc's monster archive at each scene entry | `SceneHost::merge_expansion_monsters` |
| Catalog swap at seat time (every wave's monsters), restore after the spoils | `World::enter_battle_from_formation`, `finish_battle`, `abandon_live_battle` |
| Wave arrivals | `World::expansion_seat_wave`, from the action SM's wipe hold (`BattleActionHost::reinforce_monster_wipe`) and `World::begin_battle_round` |
| Wave meshes | `battle_stage::stage_wave_monsters` (world half), the play window's `bind_wave_monsters`, the browser page's `bind_wave_monsters_web` |
| Encounter override | `World::on_field_step` |
| Spoils scaling, boss reward | `World::apply_battle_loot` |
| Level-up rewards | `World::apply_battle_xp` |
| Steal override | the death-spoils steal in `battle/steal_attack.rs` |
| Boss track | `World::swap_to_battle_bgm` |

A boss's `once` record lives in the runtime (`BattleRuntime::cleared_bosses`) and is saved as tail record 1 of the save's expansion block ([`expansion-pack-quests.md`](expansion-pack-quests.md#saves)), so a beaten once-only boss stays beaten across a Load.
