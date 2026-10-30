# Legend of Legaia: Rebirth - Expansion Pack

The flagship expansion pack for the port, and the worked example of the
[expansion-pack format](../../docs/formats/expansion-pack.md). Everything in
it is written with the files, rules and tooling any modder has - nothing in
the engine knows this pack by name - so anything Rebirth does, another pack
can do.

## The story

When the Mist was torn away it did not simply vanish. Everything it had
swallowed - the places it smothered, the beasts it twisted, the prayers it
drowned - gathered in one spot and rose as a tower that was never there
before: the **Tower of Rebirth**.

Its first hall holds the **Warden's Stone**, the last of Warden Ysolde, who
kept the tower's seal before the Mist and now speaks only through the stone.
She opens the stair to a party strong enough to climb it. Each floor above is
a memory of a place the Mist held, each kept by something that would rather
not let go, and at the top sits the **Hollow Sovereign**, made of everything
the Mist took.

The retail game ends with nothing left worth fighting past level 60 or so.
Rebirth is built for the climb from there to 99.

## What's inside

| File | Contents |
|---|---|
| `pack.toml` | The manifest. |
| `maps/rebirth_tower_1f.toml` ... `_8f.toml` | The eight floors of the tower. |
| `battle.toml` | 30 derived monsters, 23 formations (five in waves), seven tower encounter tables and one retail-scene override, 6 bosses. |
| `progression.toml` | The level curve past 60, EXP and gold multipliers, enemy scaling, level-up rewards. |
| `quests.toml` | A ten-quest main chain, ten side quests, five challenges. |
| `dialog.toml` | Every line the pack says. |
| `modes.toml` | Two poker tables, two card duels, three golf courses, three race tracks (one each on the 1F floor), two prop-hunt arenas. |
| `cards.toml` | The trading cards, boosters, dealer and drop rules. |
| `items.toml` | 20 pack items (consumables, the Warden and Rebirth gear, accessories), the Rebirth set and the 5F Tower Armory. |

### The tower

| Floor | Map | Base scene | Party level | What's there |
|---|---|---|---|---|
| 1F Atrium | `rebirth_tower_1f` | `tower` (Sky Gardens Tower), built from scratch | 40-50 | The Warden's Stone, the training gauntlet bell, the card dealer, the stair down to Rim Elm |
| 2F Ember Stair | `rebirth_tower_2f` | `rugi` (Rogue's Tower), kept whole | 50-60 | Boss: the Cinder Warden |
| 3F Hanging Garden | `rebirth_tower_3f` | `teien` (Sky Gardens), kept whole | 55-65 | The Garden Gauntlet (four waves), the Gilded Herd |
| 4F Verdant Vault | `rebirth_tower_4f` | `concnow` (Conkram), kept whole | 60-70 | Boss: the Verdant Colossus |
| 5F Gambler's Rest | `rebirth_tower_5f` | `koin1` (Sol Tower rooms), kept whole | - | No monsters; Keeper Odalric and his five tables |
| 6F Engine Deep | `rebirth_tower_6f` | `chitei2` (Jette's Fortress), kept whole | 70-85 | Boss: the Iron Regent; the Engine Gauntlet (twelve refills) |
| 7F Sanctum of Echoes | `rebirth_tower_7f` | `korb3` (Sol Tower underground), kept whole | 85-95 | Boss: the Azure Seraph |
| 8F Hollow Throne | `rebirth_tower_8f` | `taiku2` (Zora's Floating Castle), kept whole | 95-99 | Superboss: the Hollow Sovereign, in two waves |

1F and 2F carry the detail layer
([`expansion-pack-maps.md`](../../docs/formats/expansion-pack-maps.md#decorations)):
the Atrium's walls stand (the tower's stone blocks and a low south parapet),
Rogue's Tower torches line them and light the hall under enhanced lighting,
vines, grass, rubble and Rim Elm's barrels and crates dress the floor (props
from three scenes, their textures copied into free VRAM), four ember
crystals - the pack's own glTF model, `assets/models/ember_crystal.glb` from
`scripts/models/generate_ember_crystal.py` - float, turn and glow round the
dais, a rune ring turns before the Warden's Stone, the dais flagstones
drift, and mist curls over the outer ring. The Ember Stair keeps its retail
hall and adds the brazier's turning fire ring, two orbiting pyre wisps,
embers on the middle pillars, stalagmites, rubble, red lamps and smoke down
the aisles.

Each floor's stairs chain to the next (`exits` with `to = { kind = "map" }`);
the 1F stair down leads to Rim Elm.

### Bosses

| Boss | Floor | Built on | Music | Formation |
|---|---|---|---|---|
| Cinder Warden | 2F | Caruban (Lv3) | 28 "Attack!" | the Warden and two Pyre Wisps |
| Verdant Colossus | 4F | Berserker (Lv3) | 32 "Apostles of the Mist" | the Colossus and a Moss Grude |
| Iron Regent | 6F | Jette | 31 "Cort's War" | the Regent and two Brass Sentinels |
| Azure Seraph | 7F | Lapis | 29 "The Cold Koru" | the Seraph alone |
| Hollow Sovereign | 8F | Dohati, then Songi | 61 "Apocalypse" | wave 1: the Sovereign and two Throne Heralds; wave 2: the Sovereign Unbound |
| Echo of the Throne | 8F | Songi | 61 "Apocalypse" | repeatable rematch: the Unbound and a herald, four more heralds refilling |

The first five are `once = true`: their gold and item rewards pay out on the
first victory and the boss refuses a second start. The Echo is the
repeatable endgame fight.

Every boss fights with a boss script (`[monsters.<key>.script]` in
`battle.toml`) instead of its base monster's retail AI. Each move is the base
monster's own swing or a spell-table id, so damage and animation are the
disc's:

| Boss | Script |
|---|---|
| Cinder Warden | Below half its armour splits: attack x1.3, speed x1.25, Lava Breath and a double swing at the weakest member. Enrages at round 15. |
| Verdant Colossus | Below 60% it heals 15%, hardens, calls a Moss Grude back and telegraphs Earthquake; below 25% it turns to earth and rages. |
| Iron Regent | Strikes whoever hits hardest; at two thirds and one third it summons two Brass Sentinels into empty seats, and its last phase telegraphs Hyper Crush. |
| Azure Seraph | Below half turns to thunder and telegraphs Water Hazard; from round 14 the sky opens: Thunder Storm and triple swings. |
| Hollow Sovereign | Three phases: at two thirds it calls a herald, at one third it is unmade and telegraphs Grand Beam. A last stand, and an enrage at round 20. |
| Sovereign Unbound | Chaos Flare from the start; below half a telegraphed Thunder Burst. A last stand of its own. |

Their banner lines are the `warden_*`, `colossus_*`, `regent_*`, `seraph_*`,
`sovereign_*` and `unbound_*` lines in `dialog.toml`.

### Quests

The main chain climbs the tower: **A Tower Wakes** (starts on its own at
level 30) -> **The Warden's Trial** -> **The Ember Stair** -> **The Hanging
Garden** -> **The Verdant Vault** -> **The Gambler's Rest** -> **The Engine
Deep** -> **The Sanctum of Echoes** -> **The Hollow Throne** -> **Rebirth**
(reach level 99 and return to the stone). Each boss quest has a minimum
level, so the chain paces the climb.

Side quests: **Every Floor** (stand on all eight floors), two repeatable
bounties (**Ironhide Ogres**, **Brass Sentinels**), **A Remedy for Stone**
(hand in five Healing Fruit), **The Gilded Herd**, **Seasoned Hunter** (a
hundred wins), **Echo of the Throne** (repeatable superboss rematch),
**The Keeper's Tables** (win each of the Rest's five tables), **High
Roller** and **Trial Master** (clear three challenges).

Challenges: **Bare Hands** (ten rounds, no items, no magic), **Garden
Sprint** (the Garden Gauntlet in twelve rounds), **Lone Fist** (Vahn alone
against the Engine Gauntlet), **Humble Before the Seraph** (nobody above level
85), **Echo Storm** (the Echo at x1.5 strength within fifteen rounds).
Bare Hands is armed at the 1F trial stone (`kind = "challenge"` through a
dialogue choice) once the Warden's Trial is done; the other four are a
choice on their fight's own offer (`challenge = ...` beside `battle` /
`boss`), so the challenge applies to that fight.

Rest points (`kind = "rest"`, with a save) stand on the 1F stair landing,
by the 5F hearth and in a 7F alcove.

Quest and boss rewards hand out cards by id from `cards.toml`, among them
the four legendaries.

### Modes

The Gambler's Rest's five tables are `modes.toml` instances, and the
keeper's quests count wins at each by id:

- **Tower High Table** (`tower-high-table`): six-seat no-limit Hold'em for
  gold, 5,000 buy-in, expert opponents, blinds doubling every eight hands.
- **The Warden's Duel** (`wardens-duel`): a Grid Duel against the tower's
  own breed, with elemental cells, chain captures and stakes.
- **The Rebirth Links** (`rebirth-links`): six holes - bunkers guarding a
  green, a dogleg, an island green behind a bridge, stepped terraces
  (height grid), conveyor ramps round a wall (slope tiles), and a long par
  5 to a green that slopes away.
- **The Spiral Circuit** (`spiral-circuit`): a twelve-checkpoint circuit for
  six racers with three boost pads.
- **The Old Vault** (`old-vault`): six-player prop hunt, two hunters, among
  urns, crates, chests and braziers between pillar rows.

Once the keeper has welcomed the party, the 5F card room and games room
offer every table through dialogue choices (`mode = ...`), and the High
Table's empty seat deals in on a step (`kind = "mode"`). Two more instances
play on the tower itself, in the 3D field: **Tower Green** (`tower-green`),
one hole across the 1F dais, and **Sol Ring** (`sol-ring`), three laps of
the 1F floor round the dais; both open from the mode picker.

The earlier village instances (`mist-lantern-table`, `rim-elm-duel`,
`windmill-links`, `drake-circuit`, `storehouse`) stay as the gentler set.

### Gear

`items.toml` is the gear worth chasing between 60 and 99 (the format is
[`expansion-pack-items.md`](../../docs/formats/expansion-pack-items.md)).
Retail's best is the Ra-Seru set; the Rebirth set sits about 12-15% above
it a piece and rewards wearing it together.

| Where | What |
|---|---|
| 5F Tower Armory, from the start | Tower Draught (2,500 HP), Mist Tonic (300 MP), Phoenix Plume (party revive at 50%), Healing Berry, the Warden Mail / Robe / Plate (body armour between Triumph and Ra-Seru) |
| Armory, after the Verdant Colossus | Sovereign Nectar (full HP, party), Ysolde's Seal (Master Guard + HP Boost 1) |
| Armory, after the Engine Deep | Tower Sigil (AP Boost 2 + HP After) |
| Armory, after the Iron Regent | Rebirth Mail / Robe / Plate (UDF/LDF 86-96 against Ra-Seru's 75-84) and the Rebirth Crown (UDF 71, INT 70) |
| Armory, after the Azure Seraph | Rebirth Greaves (ATK 98, LDF 70, SPD 35 against Ra-Seru Thongs 88/64/32) |
| Boss rewards | Cinder Warden: Ember Heart (Fire Guard, +15 ATK). Verdant Colossus: Rebirth Maul (Gala, ATK 114 against the Ra-Seru Club's 100). Iron Regent: Rebirth Blade (Vahn, 112 against 98). Azure Seraph: Rebirth Talon (Noa, 110 against 96). Hollow Sovereign: Sovereign's Eye (Attack x2 + Arts Power). |
| Quests and drops | Every Floor: the Mist Lantern (EXP Boost + Items Up, for the climb to 99). A Remedy for Stone: two Phoenix Plumes. Throne Heralds drop Phoenix Plumes; Storm Giliums can be robbed of Mist Tonics. |

The Rebirth set (each hero's weapon and body armour, the crown, the greaves)
adds +10 ATK at two pieces, +12 UDF and LDF at three and All Guard at four.
Open the bag from the Journal's Items page; the Gear tab shows each stat
before and after a piece goes on.

## Levelling to 99: the numbers

**The curve.** Retail asks for 9.65 million XP to reach 99, 7.5 million of
it past level 60. `progression.toml` keeps retail's steps through 59 and
scales each later step to 35% (60-79) and 30% (80-99): 60 to 99 costs
**2,392,683 XP** per character.

**A fight's EXP.** A won battle sums its monsters' EXP; the pack's enemy
scaling multiplies it by `1 + 1% x (party level - 50)` (x1.49 at level 99,
under the x1.5 cap), `xp_mul` adds half again, and the retail victory share gives
each of three living members a quarter of the pool (3/4 split three ways):

```text
share = sum(monster exp) x scale(level) x 1.5 x 3/4 / 3
```

| Table | Played at | Average pool | Share per member | Next level costs | Fights per level |
|---|---|---|---|---|---|
| `tower_1f` | 45 | 18,666 | 7,000 | 61,347 | 8.8 |
| `tower_2f` | 55 | 23,000 | 9,056 | 91,597 (still retail) | 10.1 |
| `tower_3f` | 62 | 26,514 | 11,136 | 40,740 | 3.7 |
| `tower_4f` | 68 | 32,760 | 14,496 | 48,998 | 3.4 |
| `tower_6f` | 78 | 42,160 | 20,236 | 64,456 | 3.2 |
| `tower_7f` | 90 | 54,640 | 28,686 | 73,543 | 2.6 |

Grinding the matching floor's table, **60 to 99 takes about 110 battles** -
two or three full climbs of the tower. The gauntlets are the fast lane: the
Garden Gauntlet's fourteen monsters pay about 59,000 per member at level 90,
the Engine Gauntlet's fifteen about 98,000, and the Echo of the Throne about
87,000, each worth a level or more in one battle. Boss quests add 20,000 to
50,000 quest EXP on top.

**The stat budget.** A level-99 character's base stats reach their growth
ceilings (`DAT_80076918`; Vahn: HP 5,000, ATK 465, UDF/LDF 425). With endgame
gear a party member lands Tactical Art hits of roughly 1,300-1,600 Offense
([battle-formulas.md](../../docs/subsystems/battle-formulas.md#offense-value)).
Monster stats here are **record** values: the battle loader installs a random
encounter's defences x7/4 and a boss formation's ATK x5/4 and defences x2,
and the pack's scaling multiplies everything by up to x1.49. The tuning
targets, at the floor's level:

- 7F field monsters: record DEF 250-270 -> about 620 installed at 90, so an
  Art hit lands for roughly 700; record ATK 560-610 -> about 1,100 Offense
  against a level-90 party's ~650 defence, about 450 a swing.
- The Hollow Sovereign: record HP 44,000 per phase, which scaling holds at the
  65,535 cap by level 99, so the two waves are about 131,000 HP together; record
  DEF 260-280 -> about 800 installed, roughly 600 an Art hit, so a full
  level-99 party needs around ten rounds. Its second phase's record ATK 850
  lands about 1,500 a swing on a 9,999-HP party member - a fight that needs
  healing and Spirit, but one a prepared level-99 party wins.

These figures are worked from the formulas, not measured over a full fight;
retuning is a number edit in `battle.toml`.

## Playing it

The pack needs your own Legend of Legaia (NA) disc; it carries no game data.

In play, the tower is entered from the retail game: once the party reaches
level 30, the tiles behind the woodpile by Rim Elm's south gate (`town01`,
field-grid tiles 17-18 x 43) lead up into the Atrium - the 1F map's
`[[entrances]]` table - and the Atrium's stair down returns beside them. The
Journal's Travel page enters the tower from anywhere once it is open.

```bash
# Validate (no disc needed)
asset expansion validate packs/rebirth

# Native window: enter the tower's first floor directly
legaia-engine play-window --disc "Legend of Legaia (USA).bin" \
    --expansion packs/rebirth --map rebirth_tower_1f

# Any floor at a named spawn (floors 2F-7F have from_below and from_above)
legaia-engine play-window --disc ... --expansion packs/rebirth \
    --map rebirth_tower_6f --spawn from_below

# Share it as one file, and unpack one
asset expansion bundle packs/rebirth -o rebirth.lxpack
asset expansion unbundle rebirth.lxpack -o rebirth-copy
```

In the browser, the **Expansion workbench** page loads Rebirth as its
example pack; **Play in browser** hands the bundle to the play page.

Every table also opens from the mode picker (`` ` `` on the field in the
native window, the Game mode select on the play page).

## How it was built

Rebirth is meant to be read as the example of the workbench and format, so
here is how each part came together.

**Choosing base scenes.** Each kept floor is a retail room re-purposed: the
modder screenshots candidate scenes (`play-window --scene <name>
--screenshot`), picks one that reads as a tower floor, and lays the map over
it with `geometry = "keep"`. The base scene's meshes, textures and ground
draw as on the disc; the pack supplies the walls, zones and exits.

**Re-tracing the walls.** A kept room only walks the way it looks if the
pack's wall quarters match the room. The workbench's Maps tab paints them
over the base scene's own layout; for Rebirth the rows were traced from each
room's retail collision grid (wall quarters and floor tiers), flood-filled
from the floor's centre so only the room itself is floor, and every tile
outside it painted as a solid wall that keeps the retail floor tier. That last
part matters on a terraced scene: the ground's corner heights read the tiers
of the tiles around them ([the corner rule](../../docs/formats/expansion-pack-maps.md#tiers-are-corner-heights)),
so a terrace at tier 8 next to a tier-0 wall drops into a spike. 4F keeps
every surrounding tier for that reason.

**Built from scratch.** The 1F Atrium is the other style: `geometry = "none"`,
floor textures named by atlas cell (`ground = { kind = "texture" }`) or
borrowed from a base cell, and props from the base scene's geometry pack.
The workbench's `scene_objects` view lists each mesh's footprint; mesh 20 is
an octagonal pillar about two tiles across (placed at `scale = 0.6`), mesh 19
a carved face pillar, mesh 37 a stone altar block (the Warden's Stone).

**NPCs without models.** A pack NPC is a talk zone: stepping onto the tiles in
front of the Warden's Stone raises `talk tower_guide`, which is what a quest's
`giver` and `talk` objective listen for. Most bosses are a quest whose
`start_dialog` and `active_dialog` is a line with a fight choice, so the
player can step back from a fight and return to it. The Iron Regent's quest
also asks for the Engine Gauntlet, so his offer is a trigger of its own,
gated on him still standing (`requires = { bosses_unbeaten = [...] }`); a
second trigger on the same tiles says an after line once he has fallen
(`requires = { bosses = [...] }`), as on every boss floor.

**Monsters.** Every monster is a retail one renumbered: the workbench's
Battle tab picks the base from the disc catalog and shows its retail stats
beside each field. Bases were checked in a battle screenshot before use
(`play-window --battle cell:s:<hex id>`): the Juggernaut-form Cort (181)
draws nothing on an ordinary stage, so the superboss's second phase is built
on Songi (179) instead.

## No game data

The pack ships no bytes from the disc. Every monster, mesh, texture, scene or
music cue it uses is a coordinate the engine resolves against the player's
own copy, so the pack is safe to share. The validator enforces it.

## Gaps

The gaps found while building this pack - arming a challenge, entering a
mode from a map, gated exits, triggers and NPC lines, rest points - are
format features now ([`expansion-pack-maps.md`](../../docs/formats/expansion-pack-maps.md#gates)).
The stairs stay open by choice: the quest chain paces the climb by level.
