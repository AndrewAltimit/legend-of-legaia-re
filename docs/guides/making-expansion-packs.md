# Making an expansion pack

A pack is a folder of TOML files, so any text editor makes one. The
expansion workbench (the site page `tooling/expansion-workbench.html`,
reference [`expansion-workbench.md`](../tooling/expansion-workbench.md))
makes it easier: it reads your disc for the pickers, re-checks the pack
after every edit, paints maps tile by tile and exports the shareable
`.lxpack` - all in the browser, with nothing uploaded. Either way the result
is the same files, and the same checks decide whether the engine will load
it. The format is [`expansion-pack.md`](../formats/expansion-pack.md).

The site page `guides/making-expansion-packs.html` mirrors this guide, with
screenshots of the workbench.

## Your first expansion pack

A walk through the workbench from nothing to a shared `.lxpack`: a small
harbour map with a character who talks, a quest, a monster and a boss, a
trading card, the checks, a play test and the export. It takes a disc image
and a browser; nothing is installed.

1. **Start from the template.** Pick your disc, then **New pack ...**: give
   the pack an id (`harbor_tales`) and a name, and start from the **minimal
   template**. It holds one of everything a first pack wires together - a
   monster made from a retail one, a fight of it, a line of dialogue, a
   quest to win the fight, and an 11 x 6 map whose two trigger zones show
   the line and start the fight. The checks panel on the right lists the
   pack's three warnings (no description, authors or licence); the
   Manifest tab fixes them.
2. **Paint the map and place a character.** On the **Maps** tab, open *Map
   settings* and pick a base scene: the map borrows its textures, meshes
   and characters, and the floor stays yours. Paint with the legend's
   characters (`#` wall, `.` floor), then pick the **NPC** tool and click a
   tile. The NPC's form picks its model from the base scene's characters,
   its line (`welcome`) and its id (`harbor_master`, the key a quest's talk
   objective names). The **Dialog** tab edits the line, with a check under
   it that shows each text-box row as the game draws it.
3. **Write a quest.** The **Quests** tab holds the template's quest: an
   objective (win `first_fight`), a reward (100 gold) and whether it starts
   by itself. Add a talk objective on `harbor_master` and the quest asks
   the player to speak to your NPC first.
4. **Make a monster and a boss.** On the **Battle** tab a monster is a
   retail one with new numbers: each stat is the base's, a value or a
   multiplier, with the retail number beside it. **+ Boss** turns a
   formation into a once-only boss fight with its own music, an intro and
   outro cutscene and rewards; **Boss script** on its monster gives it
   phases. **Fight in 3D** starts the fight in the preview.
5. **Add a card.** The **Cards** tab's **+ Card** makes a trading card:
   its four sides, element, rarity and portrait. Boosters, drops and a
   dealer make it collectable.
6. **Read the checks.** Rename the fight on the Battle tab and the quest's
   objective and the map's trigger name a formation that is not there: two
   errors. Every edit re-runs the validator with your disc
   resolving each coordinate. A check names its file and field; clicking it
   opens the tab, selects the item and focuses the field. A pack with an
   error does not export. `F8` steps through the checks.
7. **Play it.** **Play here** opens the play page on the map the Maps tab
   is on; while that tab is open, the next Play here (or **Live**, every
   edit) reloads the pack in place with the player where they stand.
   **Open 3D preview** draws the map the way the engine builds it, above
   the painter.
8. **Export and share.** **Download .lxpack** writes the bundle (`Ctrl+S`).
   Anyone with their own disc opens it on the play page (*Load expansion*)
   or in the native port with `--expansion`. **Share** sets a cover,
   compares against an older `.lxpack` for a changelog and a version bump,
   downloads the editable sources as a zip and adds the pack to the play
   page's library. The **Files** tab gives each TOML file on its own, for a
   pack kept as a folder.

Every tab opens with a **?** to the format reference for the file it
edits, and the section headings carry one for their section.

## On the workbench

- **Pick your disc.** The workbench reads its monsters, items, scenes and
  music for the pickers. Nothing is uploaded, and no name from the disc is
  written into the pack: a picker writes the coordinate
  (`{ kind = "monster", id = 109 }`), never the name beside it.
- **Start from something.** Load the Rebirth example (the project's own
  flagship pack), a `.lxpack`, a pack folder of your own, or start a new
  pack: empty, from the minimal template, or as a clone of Rebirth.
- **Edit section by section.** One tab per file. Undo and redo cover the
  whole pack; **Find** lists every place an id or a retail coordinate is
  used.
- **Lay out a map.** Paint tiles, place spawns, exits, trigger and
  encounter zones, props and NPCs, and turn on **Heights** to see which
  cells the floor slopes across. The detail layer under the painter dresses
  the floor - decorations, scatter brushes, edge runs, decals, animated
  materials, lights, fog and particles - each a list form whose tile fields
  pick on the canvas. For an outdoor map, the **Terrain** box and the
  **Sculpt** tool shape hills, ridges and a shoreline
  ([terrain](../formats/expansion-pack-maps.md#terrain)).
- **See it in 3D.** The 3D preview rebuilds a moment after each edit, with
  walls, zones, triggers, spawns and NPCs as see-through volumes; a click in
  3D selects the tile in the painter.

The working pack autosaves in the browser, so closing the tab loses
nothing. A file you did not edit exports as the bytes it was loaded from,
comments included.

## By hand

The smallest pack is a manifest (`pack.toml`: `id`, `name`, `version`,
`authors`, `license`). Each section is a file of its own, and everything in
it is optional:

| To add | Write | Reference |
|---|---|---|
| Monsters, fights, bosses | `battle.toml` | [battles](../formats/expansion-pack-battle.md) |
| A level curve, rewards, enemy scaling | `progression.toml` | [progression](../formats/expansion-pack-battle.md#progression) |
| Quests and challenges | `quests.toml` | [quests](../formats/expansion-pack-quests.md) |
| Things people say | `dialog.toml` | [dialogue](../formats/expansion-pack-quests.md#dialogtoml) |
| Scripted scenes | `cutscenes.toml` | [cutscenes](../formats/expansion-pack-cutscenes.md) |
| Trading cards and dealers | `cards.toml` | [cards](../formats/expansion-pack.md#cardstoml) |
| Poker, duels, golf, races, prop hunt | `modes.toml` | [modes](../formats/expansion-pack.md#modestoml) |
| A new place | `maps/<id>.toml` | [maps](../formats/expansion-pack-maps.md) |

Ids are lowercase `[a-z0-9_]` (cards and mode instances may use `-`),
unique within their kind across the pack. A file under `assets/` must be a
PNG, glTF, Ogg, WAV or text file - nothing taken from the disc
([the rule](../formats/expansion-pack.md#the-no-disc-bytes-rule)).

## Check and bundle from the command line

```
asset expansion validate my-pack            # every issue, with file and field
asset expansion validate my-pack --strict   # warnings fail too
asset expansion bundle my-pack -o my-pack.lxpack
asset expansion info my-pack.lxpack [--json]
asset expansion unbundle my-pack.lxpack -o my-pack-copy/
```

None of these reads a disc, so they range-check coordinates without
confirming them; the engine and the workbench check them against a real
disc. Play it with `legaia-engine play-window --disc <image> --expansion
my-pack`, adding `--map <id>` to walk straight onto a map.

## Things that catch people out

- **One version of each base monster per fight.** A pack monster fights
  under its base monster's id, so a formation cannot hold a pack variant
  beside the retail monster it is built on.
- **Four monsters at once.** A battle has four monster seats; more enemies
  come in [waves](../formats/expansion-pack-battle.md#waves).
- **Heights are corner heights.** A raised block of tiles slopes down along
  its high-X and high-Z edges; the *Heights* overlay shows where
  ([tiers](../formats/expansion-pack-maps.md#tiers-are-corner-heights)).
- **An NPC needs a base scene with characters.** Its model is one of the
  base scene's placements; a base scene with none is a check on the NPC.
- **An entrance needs a free tile.** Pick a rectangle the retail walk
  reaches and no retail door shares
  ([entrances](../formats/expansion-pack-maps.md#entrances)).
- **Dialogue rows are short.** No retail text box wraps; keep a row within
  36 characters.
