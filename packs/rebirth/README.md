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
| `battle.toml` | The derived monsters, formations (five in waves), the tower and Isles encounter tables, one retail-scene override, eight bosses (every one refuses Run). |
| `progression.toml` | The level curve past 40, EXP and gold multipliers, enemy scaling, level-up rewards at 55, 60, 70, 80, 90 and 99. |
| `quests.toml` | An eleven-quest main chain ending in the First Breath, ten side quests, four Isles quests, five challenges. |
| `dialog.toml` | Every line the pack says. |
| `cutscenes.toml` | The tower intro on the first arrival in the Atrium, the voice on the throne speaking at the Gambler's Rest, the Sovereign rising before the last fight and the throne breaking after it, and each floor keeper's intro and fall. |
| `modes.toml` | Two poker tables, two card duels, three golf courses, three race tracks (one each on the 1F floor), two prop-hunt arenas. |
| `cards.toml` | The trading cards, boosters, dealer and drop rules. |
| `items.toml` | 20 pack items (consumables, the Warden and Rebirth gear, accessories), the Rebirth set and the 5F Tower Armory. |
| `audio.toml` | The pack's own drone track, its ambient loops (fire, water, wind, crowd, engine, drips, surf, birdsong) and boss stings, the tower bosses' victory music, the mode music at the Rest's tables. |
| `assets/audio/` | WAV files synthesised by `scripts/models/generate_rebirth_audio.py`. |
| `assets/models/` | glTF models: the ember crystal (`scripts/models/generate_ember_crystal.py`) and the floors' set pieces - sundial, fountain, armillary ring, lanterns, cogs, banners, obelisks, braziers (`scripts/models/generate_rebirth_props.py`). |
| `arts.toml` | One Tactical Art per hero learned on the climb (level 55, after the Cinder Warden) and one Rebirth finisher each (level 80, after the Iron Regent), chained from that art into a retail one; all on the heroes' own retail clips. |
| `spells.toml` | Four tower Seru - Hearthfire, Verdant Bloom, Grove Fang, Tempest Dive - each cast as the retail Seru it grew from and taught by capturing its pack monster once that floor's keeper falls. |

### The tower

| Floor | Map | Base scene | Party level | What's there |
|---|---|---|---|---|
| 1F Atrium | `rebirth_tower_1f` | `tower` (Sky Gardens Tower), built from scratch | 40-48 | The Warden's Stone, the training gauntlet bell, the card dealer, the stair down to Rim Elm |
| 2F Ember Stair | `rebirth_tower_2f` | `rugi` (Rogue's Tower), kept whole | 50-55 | Boss: the Cinder Warden |
| 3F Hanging Garden | `rebirth_tower_3f` | `teien` (Sky Gardens), kept whole | 55-60 | The Garden Gauntlet (four waves), the Gilded Herd |
| 4F Verdant Vault | `rebirth_tower_4f` | `concnow` (Conkram), kept whole | 60-68 | Boss: the Verdant Colossus |
| 5F Gambler's Rest | `rebirth_tower_5f` | `koin1` (Sol Tower rooms), kept whole | - | No monsters; Keeper Odalric and his five tables |
| 6F Engine Deep | `rebirth_tower_6f` | `chitei2` (Jette's Fortress), kept whole | 70-78 | Boss: the Iron Regent; the Engine Gauntlet (six refills) |
| 7F Sanctum of Echoes | `rebirth_tower_7f` | `korb3` (Sol Tower underground), kept whole | 80-88 | Boss: the Azure Seraph |
| 8F Hollow Throne | `rebirth_tower_8f` | `taiku2` (Zora's Floating Castle), kept whole | 88-99 | Superboss: the Hollow Sovereign, in two waves; the Herald Watch on the long carpet |

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

The Atrium's pillars are decorations wearing octagonal stone decals over
the retail mesh's black caps, moss and worn-stone decals break up its floor,
embers rise from the crystals and sparks hang round the Warden's Stone. The
floors above keep their retail rooms and play the rooms' own animated
scenery (`base.scenery`), with a theme each: falling leaves, fireflies,
flower beds and a turning cloud ring in the Hanging Garden; drips, spores
and a green ring of light in the Verdant Vault; hearth smoke and sparks in
the Gambler's Rest; spark showers, steam and two flywheels in the Engine
Deep; three orbiting echo crystals and blue motes in the Sanctum; dust,
violet motes and a low haze in the Hollow Throne.

Each floor's stairs chain to the next (`exits` with `to = { kind = "map" }`);
the 1F stair down leads to Rim Elm. The 1F stair up is the seal Ysolde
lifts: it stays shut (`requires = { flags = ["tower_open"] }`, with a locked
line) until the Warden's Trial is done.

### The Rebirth Isles

Past a sea gate in the Atrium's west wall (open once Ysolde has first
spoken, so a level-30 party grows here into the Warden's Trial) lies an
outdoor region of four maps, each laid out from scratch at
the field grid's larger sizes and dressed with the detail layer. The
four stand on a sculpted `[terrain]` (sloping shores, cliffs, dunes, a
hill, a forest of wooded banks, a rock floor and a pool under animated
water with rolling surf; `docs/formats/expansion-pack-maps.md`,
"Terrain"). The grotto's rock and the Thornwood's lane and turf are
borrowed from other retail scenes' VRAM (`from`), and each map has its own
ambience (`[audio]`: surf, wind, drips, birdsong, the village's murmur):

| Map | Base | Size | What is there |
|---|---|---|---|
| `isles_beach` Saltwind Beach | `town01` | 64 x 36 | The sea-stair landing, dunes and palms, surf, a ruined jetty, the ferrywoman; shore encounters |
| `isles_village` Tolva's Haven | `town01` | 52 x 42 | Huts, the well and windmill, the lodge (`rest`), Pell's Trading Post (`shop` `isles_trader`), villagers; a dusk mood |
| `isles_thornwood` The Thornwood | `vell` | 88 x 34 | A forest path walled by trees, three clearings, the ruined court, the hermit's camp and the Thornwood Queen (`isles_hive_queen`) in the gate |
| `isles_grotto` The Singing Grotto | `cave01` | 40 x 36 | A cavern under the ruins, its pool and the Pearl |

The region's monsters, formations, encounter tables and mini-boss are
`isles_*` entries in `battle.toml`; its four quests (`isles_landfall`,
`isles_nets`, `isles_thornwood`, `isles_pearl`) and their lines are in
`quests.toml` and `dialog.toml`.

### Bosses

| Boss | Floor | Built on | Music | Formation |
|---|---|---|---|---|
| Cinder Warden | 2F | Caruban (Lv3) | 28 "Attack!" | the Warden and two Pyre Wisps |
| Verdant Colossus | 4F | Berserker (Lv3) | 32 "Apostles of the Mist" | the Colossus and a Moss Grude |
| Iron Regent | 6F | Jette | 31 "Cort's War" | the Regent and two Brass Sentinels |
| Azure Seraph | 7F | Lapis | 29 "The Cold Koru" | the Seraph alone |
| Hollow Sovereign | 8F | Dohati, then Songi | 61 "Apocalypse" | wave 1: the Sovereign and two Throne Heralds; wave 2: the Sovereign Unbound |
| Echo of the Throne | 8F | Songi | 73 (Wild Arms battle) | repeatable rematch: the Unbound and a herald, two more heralds refilling |
| The First Breath | 1F, past 99 | Rogue | 61 "Apocalypse" | the Breath alone, calling two heralds at 70% and two more at 40% |
| Thornwood Queen | the Isles | Queen Bee | 28 "Attack!" | the Queen and two Ruin Wasps |

All but the Echo are `once = true`: their gold and item rewards pay out on
the first victory and the boss refuses a second start. The Echo is the
repeatable endgame fight. Every boss is `no_escape = true`, which also seats
it on the battle loader's scripted profile (ATK x5/4, both defences x2).

Each keeper's fight opens on its intro cutscene (`battle.toml` `intro`):
the shot finds the keeper's place, the entrance sting hits, the screen
flashes the keeper's colour and its title banner rises. Each phase change
rings the phase shimmer under the script's banner line, and a won fight
closes back on the floor (`outro`) on the falling knell and a banner of
what is left.

Every boss fights with a boss script (`[monsters.<key>.script]` in
`battle.toml`) instead of its base monster's retail AI. Each move is the base
monster's own swing or a spell-table id, so damage and animation are the
disc's:

| Boss | Script |
|---|---|
| Cinder Warden | Below half its armour splits: attack x1.3, speed x1.25, Lava Breath and a double swing at the weakest member. Enrages at round 15. |
| Verdant Colossus | Below 60% it heals 15%, hardens, calls a Moss Grude back and telegraphs Earthquake; below 25% it turns to earth and rages. |
| Iron Regent | Strikes whoever hits hardest; at two thirds and one third it summons a Brass Sentinel into an empty seat, and its last phase telegraphs Hyper Crush. |
| Azure Seraph | Below half turns to thunder, calls two Fallen Lanterns (the drowned choir) and telegraphs Water Hazard; from round 14 the sky opens: Thunder Storm and triple swings. |
| Hollow Sovereign | Three phases: at two thirds it calls a herald, at one third it is unmade and telegraphs Grand Beam. A last stand, and an enrage at round 20. |
| Sovereign Unbound | Chaos Flare from the start; below half a telegraphed Thunder Burst. A last stand of its own. |
| The First Breath | Fire, then wind (70%: double swings at the weakest, Chaos Flare, two heralds), then thunder (40%: heals 10%, two more heralds, a telegraphed Grand Beam with the chime). A last stand, and an enrage at round 22. |

Their banner lines are the `warden_*`, `colossus_*`, `regent_*`, `seraph_*`,
`sovereign_*`, `unbound_*` and `breath_*` lines in `dialog.toml`.
Every telegraphed spell is `ignore_mp`: once a boss has charged it, it lands
whatever MP the boss has left.

### Quests

The main chain climbs the tower: **A Tower Wakes** (starts on its own at
level 30) -> **The Warden's Trial** -> **The Ember Stair** -> **The Hanging
Garden** -> **The Verdant Vault** -> **The Gambler's Rest** -> **The Engine
Deep** -> **The Sanctum of Echoes** -> **The Hollow Throne** -> **Rebirth**
(reach level 99 and return to the stone) -> **The First Breath** (what
Ysolde's seal held under the stone all along). Each boss quest has a minimum
level (50, 60, 70, 80, 88), so the chain paces the climb, and every keeper's
payoff line - and each floor's arrival line - names the level the next one
asks for, so the player always knows where to go and when.

The Isles quests open as soon as Ysolde has spoken (**Landfall**, **Torn
Nets**, **The Thornwood Queen** from level 35, **The Pearl Below**): the
region is the bridge from the tower's opening at 30 to the trial at 40. A
few NPCs answer progress through `line_after`: the Warden's familiar once the
throne is empty, the 5F card room once the engine stops and the throne
falls.

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

Tower Game Night ends on a podium in the Rest's hall: the top three's
leaders on 2nd / 1st / 3rd blocks, the rivals played by the Rest's
regulars, confetti and a slow shot round them (`modes.toml` `podium`).

- **Tower High Table** (`tower-high-table`): six-seat no-limit Hold'em for
  gold, 5,000 buy-in, expert opponents, blinds doubling every eight hands.
- **The Warden's Duel** (`wardens-duel`): a Grid Duel against the tower's
  own breed, with elemental cells, chain captures and stakes, played at the
  card room's table on the 5F floor: the Warden's second across the board,
  two regulars watching, the tower's card dealer beside them (talk to the
  second to sit down).
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
it past level 60. `progression.toml` keeps retail's steps through 39 (the
story) and cuts each later step in bands (x0.75 at 40 down to x0.3 at 90),
so every level from 40 to 99 costs between about 36,000 and 90,000 XP and
the cost rises steadily: 60 to 99 is **2,492,187 XP** per character.

**A fight's EXP.** A won battle sums its monsters' EXP; the pack's enemy
scaling multiplies it (and the monsters' stats) by `1 + 0.5% x (party level
- 50)`, at most x1.25 - slower than the party itself grows, so a level gained
is a level that counts. `xp_mul` adds half again, and the retail victory
share gives each of three living members a quarter of the pool:

```text
share = sum(monster exp) x scale(level) x 1.5 x 3/4 / 3
```

**The curve, checked.** `crates/engine-expansion/tests/rebirth_balance.rs`
seats every pack formation the way the engine does (`derive_monster`, the
level scaling, the battle loader's install profile), runs both sides' hits
through the melee kernel (`physical_predamage`) and holds each fight to a
target at Normal, Hard and Nightmare; the model's assumptions are in the
file's header. `rebirth_chain.rs` plays the whole quest chain through the
tracker. At Normal, at the level each area is first met:

| Area | Fight | Level | Rounds to win | Rounds survived unhealed | Worst move | Fights per level |
|---|---|---|---|---|---|---|
| Isles | `isles_shore` | 30 | 3.4 | 9.7 | 122 | 6.1 |
| Isles | Thornwood Queen | 35 | 7.5 | 4.4 | 980 | 3.4 |
| 1F | `forge_guard` | 40 | 4.1 | 9.0 | 145 | 4.3 |
| 1F | Tower Gauntlet | 45 | 9.1 | 13.3 | 145 | 1.8 |
| 2F | Cinder Warden | 50 | 9.0 | 6.5 | 1,114 | 1.9 |
| 3F | `mire_bloom` | 55 | 4.6 | 10.6 | 170 | 3.2 |
| 3F | Garden Gauntlet | 60 | 12.7 | 14.5 | 167 | 1.2 |
| 4F | `fountain_court` | 60 | 4.7 | 10.2 | 187 | 2.8 |
| 4F | Verdant Colossus | 65 | 9.8 | 6.4 | 785 | 1.7 |
| 6F | `storm_engine` | 70 | 4.9 | 7.4 | 286 | 2.5 |
| 6F | Iron Regent | 75 | 11.8 | 4.3 | 1,314 | 1.3 |
| 7F | `prism_storm` | 80 | 3.9 | 7.6 | 461 | 3.0 |
| 7F | Azure Seraph | 85 | 11.1 | 6.0 | 1,874 | 1.9 |
| 8F | `herald_watch` | 90 | 4.5 | 7.2 | 470 | 2.9 |
| 8F | Hollow Sovereign | 90 | 14.7 | 3.5 | 1,514 | 1.0 |
| 8F | Echo of the Throne | 95 | 10.2 | 6.5 | 734 | 1.6 |
| 8F | Echo Storm (x1.5) | 99 | 14.4 | 3.5 | 1,411 | - |
| 1F | The First Breath | 99 | 15.3 | 4.7 | 2,620 | - |

Field fights last three to five rounds and move the bar every three to six
of them; a keeper or gauntlet is worth a level or more. Hard (enemy stats
x1.35, attacks x1.25) stretches the keepers to 12-20 rounds; Nightmare
(x1.8, x1.5) to 16-23, with the party lasting about one round unhealed
against the superbosses - winnable with Spirit and healing, and no single
blow at any preset lands for a full-HP member's whole HP.

**Rewards on the way.** Level 55 brings the tower's arts and a Power and
Guardian Water, 80 the Rebirth finishers, 99 the Warden's last request and
the First Breath; the gear chase runs from the Ra-Seru set through the
Rebirth set (see Gear).

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

**Sound.** Every floor keeps a retail track by its sound-test id; the
map `[audio]` tables add the rest
([`expansion-pack-audio.md`](../../docs/formats/expansion-pack-audio.md)).
5F's games room is a music zone on the Sol casino's "Wild Declaration", so
the bar tune cross-fades into it at the door; 7F's inner sanctum hands the
music to the pack's own drone, a six-second loop of whole-cycle sine partials
that carries its loop in its WAV `smpl` chunk. The ember crystals (1F), the
braziers and the fire at their middle (2F), the Rest's hearth (5F) and the
throne's standing lamps (8F) crackle, the Warden's Stone hums and
water runs in the garden's channels (3F), each a positional source heard by
distance and panned across the view. The Hollow Sovereign opens on "Apostles
of the Mist", turns to "Apocalypse" as its shell cracks, rings a chime as it
is unmade and on every Grand Beam telegraph, and its unbound form arrives to
the Wild Arms battle theme. The Rest's crowd murmurs, loudest by the
tables (5F); the engine hums at both flywheels (6F); the sunken court drips
(4F); a fountain plays in the garden (3F); and the wind howls through the
open top (8F). The WAV files are synthesised from sine
partials and seeded noise by `scripts/models/generate_rebirth_audio.py`; no
disc audio is copied.

## No game data

The pack ships no bytes from the disc. Every monster, mesh, texture, scene or
music cue it uses is a coordinate the engine resolves against the player's
own copy, so the pack is safe to share. The validator enforces it.

## Gaps

The gaps found while building this pack - arming a challenge, entering a
mode from a map, gated exits, triggers and NPC lines, rest points - are
format features now ([`expansion-pack-maps.md`](../../docs/formats/expansion-pack-maps.md#gates)).
Above 1F the stairs stay open by choice: the quest chain paces the climb by level.

Every pack flag and counter the pack writes is read somewhere (the
validator's flow lints hold it to that): the trial's `tower_open` lifts the
stair seal and arms the dais guardian, `rest_open` opens the 5F tables,
`hollow_heard`, `tables_beaten`, `sovereign_fallen` and `breath_released`
turn NPC lines, `throne_broken` and `reborn` gate the last two quests, the
Isles flags gate the next Isles quest and Pell's late stock, and the
`bosses_felled`, `bounties` and `echoes_broken` counters are objectives of
**Rebirth**, **Tower Bounty Hunter** and **Echo Breaker**.
