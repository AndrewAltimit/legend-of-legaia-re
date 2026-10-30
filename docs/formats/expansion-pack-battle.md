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
| `drop`, `steal` | The victory drop and the Evil God Icon steal, item and percent. The item is a retail id or a pack item ([items](expansion-pack-items.md#battle-aliases)). |
| `element` | Element id `0..=7` (`7` neutral). |
| `spells` | Spell ids the monster AI may cast, at most three; replaces the base's. |
| `script` | The monster's own battle AI in place of the base's retail picks ([boss scripts](#boss-scripts)). |

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

A boss is a formation plus staging. `no_escape` marks its formation scripted. `music` replaces the battle track for this fight (when the host's battle-music swap is on): a retail cue or a pack track, with `audio.toml`'s `music.boss` standing in when it is unset ([audio](expansion-pack-audio.md#boss-music-phases-and-telegraphs)). `reward_items` and `reward_gold` pay on victory on top of the formation's spoils, and land in the spoils' drops and gold; a reward item may be a pack item, which lands in the pack bag with a toast ([items](expansion-pack-items.md)). A `once` boss refuses a second start after a victory (`World::expansion_boss_cleared`).

Quests and maps start fights through two `World` entry points: `start_expansion_battle(formation)` and `start_expansion_boss(boss)`. Both go through the ordinary encounter path - transition, battle music, `begin_encounter_battle` - as `World::force_encounter` does.

## Boss scripts

```toml
[monsters.cinder_warden.script]
target = "random"
immune = ["sleep", "confuse", "stone"]
last_stand = { line = "warden_last" }
enrage = { when = { round = 15 }, line = "warden_inferno", attack = { mul = 1.25 } }

[[monsters.cinder_warden.script.phases]]      # phase 1 opens the fight
moves = [
    { attack = 0, weight = 3 },               # the base monster's first swing
    { spell = 0x25, weight = 1, cooldown = 2 },
]

[[monsters.cinder_warden.script.phases]]
when = { hp_below_pct = 50 }
line = "warden_rage"
attack = { mul = 1.3 }
summon = ["pyre_wisp"]
moves = [
    { attack = 2, hits = 2, weight = 3, target = "lowest_hp" },
    { spell = 0x46, weight = 2, charge = true, charge_line = "warden_charge" },
]
```

A pack monster with a `script` table fights with the script's AI in place of its base monster's retail AI: on each of its turns the script, not the retail picker, chooses the action and the target. A pack monster without one keeps the base's AI, and a fight with no scripted monster in it never reaches the script runtime.

The parser is `legaia_expansion_format::boss_script`; the kernel `legaia_engine_expansion::boss_script`; the `World` hooks `crates/engine-core/src/world/expansion_boss.rs`.

### Moves are disc content

A move names retail content by index and never carries a number of its own:

- `attack = n` is the `n`th (0-based) swing of the base monster's attack list: its archive swing records, the entries the retail picker rolls over (tags `0x0C..=0x1F` with an AGL cost, listed by `asset monster-archive --id N` and by the workbench's swing picker). The move stages that entry through the ordinary attack band, so the clip, the hit events and the swing's power byte are the disc's. `hits` chains the swing up to eight times in one turn.
- `spell = id` is a spell-table id, cast through the retail cast path as a pack monster's `spells` list is: MP debit, cast band, the move-power record and impact status. A capture-class special (a streamed cast module) resolves off the disc table as the retail monster AI's own scripted casts do. A cast the monster cannot afford from its live MP falls back to a physical strike, as in retail.

A swing index past the base's list, or a spell id the disc does not hold, makes that turn fall back to the retail pick; the disc-gated test `expansion_boss_script_disc` checks every Rebirth move against the disc.

### Phases

`phases` is an ordered list. The first opens the fight and takes no `when`. Each later one begins when any condition its `when` sets holds:

| `when` | Holds when |
|---|---|
| `hp_below_pct` | The monster's HP is below this percent of its maximum. |
| `round` | The battle has reached this round (the first is `1`). |
| `allies_down` | At least this many monsters of the fight have fallen. |

Phases run forward only: a monster healed back above a threshold stays in its phase. The checks run as each round opens and at the top of each of the monster's turns, and one check may begin several phases in order. A phase applies its changes once, as it begins:

| Field | Effect |
|---|---|
| `line` | A pack dialog line shown as the battle banner. |
| `moves` | The weighted move list; a phase that lists none keeps the moves before it. |
| `target` | The phase's targeting rule. |
| `attack`, `defence`, `agility`, `speed`, `intelligence` | Set or scale (`{ mul = x }`) the monster's live battle stat and its base half. |
| `element` | The monster's element (`0..=7`); the affinity matrix reads it as attacker and defender, so it changes what the monster resists. |
| `heal_pct` | Restores this percent of maximum HP. |
| `summon` | Monsters seated into free monster seats at the next turn boundary. |
| `music`, `sound` | Music that cross-fades in as the phase begins, and a sound played then ([audio](expansion-pack-audio.md#boss-music-phases-and-telegraphs)). |

### Picks, targets and telegraphs

Each turn draws once from the battle RNG over the standing moves' `weight`s (no draw when only one move is ready), so a fight replays bit for bit. `cooldown` keeps a move out of the draw for that many of the monster's own turns; with every move cooling, the one nearest ready is used.

`target` picks among the living party: `random` (one draw), `lowest_hp`, `highest_attack`, and `healer` (a member who knows an ally-side spell, the most MP first; with none, the most MP). A move's own rule comes first, then the phase's, then the script's (`random` by default). A spell whose shape is not one enemy - the whole party, an ally - keeps its shape's targets.

A move with `charge = true` is telegraphed: the turn it is picked the monster only gathers power and `charge_line` (or a generic "gathering power" line) stands as the banner, and `charge_sound` plays; on its next turn the move lands with no draw.

A spell move casts through the retail cast path, which turns a cast the monster cannot afford into a swing. `ignore_mp = true` casts it whatever MP is left and spends none (the pick tops the monster's MP up by the spell's cost, which the cast band's debit takes back), so a scripted signature spell always lands; the validator refuses it on a swing.

### Enrage, immunities, last stand

- `enrage` is a phase keyed on `when.round` alone that applies over whatever phase stands, once; its `moves`, when set, replace the phases' for the rest of the fight. With no `line` its banner reads "<name> is enraged!".
- `immune` lists statuses that never take hold (`venom`, `toxic`, `numb`, `sleep`, `confuse`, `rot`, `curse`, `stone`): every battle frame strips them, tracker entry and status-word bits, before anything reads them.
- `last_stand` holds the first lethal blow of the fight at 1 HP, with its `line` (or "<name> refuses to fall!") as the banner. The next lethal blow kills.

### Banners

Every line shows on the battle's timed-message line, the one the wave banner uses, for its hold (about two seconds). A banner raised while another stands queues behind it, so a phase change and a charge on one turn read one after the other. A banner is one row: the line's first page, rows joined, and the validator warns past 40 characters.

The three stock banners - an enrage with no `line`, a charge with no `charge_line`, a last stand with no `line` - read the pack's own `dialog.toml` lines when it has them, so a pack written in another language words them its way; without them the engine's English wording stands. In each, `<name>` reads as the boss's name.

| Dialog line | Engine wording |
|---|---|
| `boss_enraged` | `<name> is enraged!` |
| `boss_charging` | `<name> is gathering power!` |
| `boss_last_stand` | `<name> refuses to fall!` |

```toml
[lines.boss_last_stand]
text = "<name> ne tombe pas !"
```

### Summons

A summon takes a free monster seat - one whose monster has fallen, or one the layout has not yet used - through the wave seating path, at the next safe point: a phase that changes as a round opens seats it before the initiative seed, so it acts in that round; a phase that changes on the boss's own turn mid-round seats it as that turn ends, before the next actor is picked (the seam a wipe's wave arrives on), and it first acts the round after. Either way its mesh binds on both hosts and it joins the fight's spoils. When the summons need more seats than the layout has, the monster block widens and the standing monsters move to the wider layout's seats. A summon with no free seat (four standing) is dropped.

Summons fight in the same battle, so they count toward the [one-version rule](#one-version-of-each-base-monster) with the formation's waves.

### Validation

`BattleSection::problems` refuses: a first phase with a `when` or no moves, a later phase with none, `hp_below_pct` thresholds that do not fall phase by phase, a move with both or neither of `attack` and `spell`, `hits` outside `1..=8` or without `attack`, a zero `weight`, a `charge_line` without `charge`, an `ignore_mp` without `spell`, an enrage keyed on anything but `round`, an unknown summon, more than four summons in one phase, more than eight phases or twelve moves, and a duplicated immunity. Every `line` and `charge_line` must name a dialog line of the pack.

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
| Boss script pick | `World::expansion_boss_pick`, the top of `World::pick_monster_action`; a swing stream through `World::arm_monster_strike_budget` |
| Boss phase checks, summons | `World::expansion_boss_round_start`, from `World::begin_battle_round`; a mid-round summon from `World::cycle_battle_turn` |
| Last stand | `World::expansion_survives_lethal`, from `apply_battle_hp_delta` and `apply_combo_total` |
| Immunities, banner queue | `World::expansion_boss_frame`, ahead of the battle frame's dead-marking sweep |

A boss's `once` record lives in the runtime (`BattleRuntime::cleared_bosses`) and is saved as tail record 1 of the save's expansion block ([`expansion-pack-quests.md`](expansion-pack-quests.md#saves)), so a beaten once-only boss stays beaten across a Load.
