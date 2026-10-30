# Expansion pack: cutscenes

`cutscenes.toml` holds an expansion pack's scripted scenes: a camera rising up a hall, an NPC walking to meet the party, a line, a choice that branches, a fade, a fight. It is parsed by `legaia_expansion_format::cutscene`, run by the sequencer `legaia_engine_expansion::cutscene`, and wired to the `World` by `engine-core`'s `world::expansion_cutscene`. The camera and screen layer reach both play hosts through `legaia_engine_world_views::camera_view::pack_cutscene_view` and `legaia_engine_screens::screen_layers::expansion_cutscene_prims`.

Every hook is the identity with no pack active: no cutscene can start, nothing locks the player, and the camera, the screen layer and the save gates read as before.

## Why a step list, not field-VM bytecode

Retail's own cutscenes are field-VM records (MAN partition 2, [`cutscene.md`](../subsystems/cutscene.md#in-engine-3d-opening-the-five-scene-new-game-chain)): camera beats through op `0x45`, actor moves through op `0x23`, inline dialogue, flag ops. A pack could in principle carry such bytecode and run it on the ported VM, but a pack cutscene is a list of named steps run by a small sequencer of its own, for three reasons:

- **A pack never writes a retail story flag.** Field-VM bytecode reaches the flag bank, the scene control block and every placement's script state directly. A step list has no op for any of them; its flags are pack flags ([`expansion-pack-quests.md`](expansion-pack-quests.md#ids-and-namespaces)), exactly as a pack dialogue line's are.
- **It can be checked as a whole.** Every actor, line, label, track, flag, fight and tile a cutscene names is known before it runs, so the validator refuses a broken one ([Validation](#validation)). Field-VM records reference placements by slot and branch by byte offset; a mistake shows up only when the record runs.
- **It is written by hand and edited in the workbench.** A step reads as what it does (`move`, `say`, `camera`); the workbench's step-list editor is a form over the same shape.

Retail bytecode stays the faithful path for retail's own scenes; nothing here runs on it.

## `cutscenes.toml`

```toml
[cutscenes.tower_intro]
title = "The Tower Wakes"
map = "rebirth_tower_1f"
letterbox = true

[[cutscenes.tower_intro.steps]]
do = "camera"
focus = { tile = [7, 11] }
pitch = -14.0
zoom = 0.55

[[cutscenes.tower_intro.steps]]
do = "start_track"
track = "crane"

[[cutscenes.tower_intro.steps]]
do = "say"
line = "intro_hall"

[[cutscenes.tower_intro.tracks.crane.steps]]
do = "camera"
focus = { tile = [7, 1] }
zoom = 2.3
frames = 300
```

A step list can also be written inline, one step per line: `steps = [ { do = "wait", frames = 30 }, ... ]`. Both forms parse to the same thing.

| Key | Meaning |
|---|---|
| `title` | Display title (workbench, logs). |
| `map` | The pack map it plays on. Tiles are then that map's local tiles, actors that map's NPCs, and the engine refuses to start it elsewhere. Without it, tiles are the running scene's field-grid tiles and the actors are the player and `retail:<slot>` placements. |
| `skippable` | `true` (default): Start skips it once it has been seen. |
| `lock_player` | `true` (default): the player is held from the first frame to the last. |
| `letterbox` | Cinematic bars slide in while it runs. |
| `steps` | The main track. The cutscene ends when it does. |
| `tracks.<name>.steps` | Named tracks a `start_track` step runs beside the main one. |

## Steps

Each step is a table whose `do` key names it. Frames are display frames, 60 a second. A step that can take time (`move`, `camera`, `camera_reset`, `fade`) takes `wait = false` to start and let the next step run at once.

| `do` | Fields | What it does |
|---|---|---|
| `wait` | `frames` | Waits. |
| `say` | `line`, `on_choice` | Shows a `dialog.toml` line and waits for it to close. `on_choice` names a label per option (`""` goes on). |
| `move` | `actor`, `to` / `path`, `speed`, `run`, `clip`, `wait` | Walks an actor through the tiles in order. |
| `face` | `actor`, `facing` / `toward` | Turns an actor to a compass sector (`0..8`) or toward another actor. |
| `clip` | `actor`, `clip`, `frames` | Plays a clip; with `frames`, waits and puts the idle clip back. |
| `teleport` | `actor`, `tile`, `facing` | Puts an actor on a tile at once. |
| `show` / `hide` | `npc`, `tile` | Brings a pack NPC on (optionally on a tile) or takes it off. |
| `camera` | `focus`, `yaw`, `pitch`, `zoom`, `frames`, `wait` | Moves the shot ([Camera](#camera)). |
| `camera_reset` | `frames`, `wait` | Eases back to the follow camera. |
| `shake` | `amplitude`, `frames` | Shakes the shot, dying out. |
| `fade` | `to` (`black` / `white` / `clear`), `frames`, `wait` | Fades the screen. |
| `flash` | `color`, `frames` | An additive flash that dies out. |
| `tint` | `color`, `frames`, `mode` (`add` / `subtract`) | Holds a colour over the frame; `[0, 0, 0]` clears it. |
| `banner` | `line` | A title in the place-name banner's balloon: the line's first page, rows joined. Does not wait. |
| `set_flag` / `clear_flag` | `flag` | A pack flag. |
| `counter` | `counter`, `add` | Adds to a pack counter. |
| `label` | `name` | A jump target. |
| `goto` | `label` | Jumps. |
| `branch` | `requires`, `counter`, `at_least`, `goto`, `otherwise` | Jumps when the gate holds (the quest prerequisite shape, plus a counter test). |
| `start_track` / `join` | `track` | Starts a named track; waits for one to finish. |
| `lock` / `unlock` | - | Holds or frees the player midway. |
| `battle` / `boss` | `formation` / `boss` | Ends the cutscene on a `battle.toml` fight. |
| `give_item` / `take_item` | `item`, `count` | A retail item. |
| `gold` | `amount` | Gives (or takes) gold. |
| `give_card` | `card` | A `cards.toml` card. |
| `start_quest` | `quest` | Starts a quest (its start line and fight follow as usual). |
| `sfx` | `id` | A sound effect from the static table ([`sfx-table.md`](sfx-table.md)). |
| `bgm` | `bgm` | Switches the music (`{ kind = "bgm", id = N }`). |
| `sound` | `sound` | A retail cue or a pack sound ([`expansion-pack-audio.md`](expansion-pack-audio.md)). |
| `music` | `music`, `fade` (30) | Cross-fades to a retail cue, a pack track or silence, held over the map's music zones (`World::expansion_set_music`). |
| `music_release` | - | Hands the music back to the map and its zones. |
| `end` | - | Ends the cutscene. |

### Actors

An actor is `"player"` (the party leader the field draws), a pack NPC id of the cutscene's `map`, or `"retail:<slot>"` for a retail scene's placement. The player walks at the pad walk's pace (8 world units a frame, 12 with `run`) and plays its walk clip; an NPC walks each leg on the motion VM at its own glide speed.

A walk goes straight from stop to stop where that line keeps to open ground. Where it would cross a wall, the leg goes round on the collision grid instead (an A* search over the grid's open 64-unit sub-cells, pulled taut into the fewest straight legs: `World::field_walk_path`), so naming a tile across the room walks the actor round the table. A leg with no way round - its end on a wall, or a region the grid does not join - walks straight as before, so a `path` still steers a walk exactly where the author wants it.

A pack NPC's `present` gate ([`expansion-pack-maps.md`](expansion-pack-maps.md#npcs)) keeps it off the map until it holds; a `show` step brings it on for the visit. `hide` parks it on retail's own off-map hide box (`FIELD_OFFMAP_HIDE_XZ`), which keeps it out of every draw, the talk probe and the collision boxes on both hosts.

### Camera

The camera steps move the shot from the follow camera's own, so a cutscene that never moves the camera looks exactly like the field:

- `focus` replaces the follow focus: `{ tile = [c, r] }` a tile's centre, `{ actor = "id" }` an actor, followed while it moves.
- `yaw` and `pitch` are degrees added to the follow shot's yaw and pitch (positive pitch looks further down).
- `zoom` scales the eye-space translation, so the shot pulls straight back along its own line of sight (`> 1` is further out).

A step glides from the current pose to the new one over `frames` with a smoothstep ease (still at both ends); `frames = 0` cuts. When the cutscene ends the shot is the follow camera's again.

### Choices and branches

A `say` waits until the box closes. When its line ends on a menu, the option the player committed picks a label from `on_choice`; the option's own effects (`set_flags`, `next`, a fight) apply as they do for any pack line, before the branch runs. `branch` reads the same gates a quest's prerequisites do, so a cutscene can play differently once a boss has fallen.

## Hooks

| Hook | Where | Plays |
|---|---|---|
| `kind = "cutscene"` trigger | a map's `[[triggers]]` | when the player steps into the zone (`once` and `requires` as for any trigger). |
| `enter_cutscenes` | a map | on arrival: the first entry whose `requires` holds and which is unseen (or has `repeat = true`). A save resumed on the map plays none. |
| `cutscene` | a pack NPC | when the player talks to it, in place of its line. |
| `start_cutscene`, `complete_cutscene` | a quest | after the quest's start or completion line, once the player is back in the field. |
| `intro`, `outro` | a `battle.toml` boss | as its fight is called, the fight starting when it ends; after a won fight, back on the field ([battle](expansion-pack-battle.md#bosses)). |

A hook queues the cutscene; it starts on the first field frame with no dialogue box open, no fight waiting and no scene change in flight.

## Runtime

The sequencer (`CutsceneRun`) is a deterministic interpreter. Each frame it advances its clocks by the display frames the world ran, clears the waits that are over, and runs each track forward to its next wait; it reads the world only through `CutsceneEnv` (is this actor walking, is the box closed, which option was picked, does this gate hold) and writes it only through `Command`s, so the same answers give the same commands frame for frame. A track that runs `STEP_BUDGET` steps in one frame without waiting ends the cutscene rather than hang the frame.

What each command does to the world uses a path the world already has: a line is the faithful inline runner's (`World::expansion_say`), an NPC leg the motion VM's (`World::start_field_npc_motion`), the player's walk moves its anchor and binds its walk clip as the in-world mode stage does, a fight is the pack's queued fight. While a cutscene holds the player, `World::script_context_engages_player` answers `true`: the walk, the talk probe, the menu button and the encounter roll all already read it. The pack map's zones and exits fire nothing under a cutscene (`ActiveMap::observe`), its NPC patrols pause, and the tile a cutscene leaves the player on counts as already seen.

The camera pose, shake, fades, flash, tint and letterbox are the sequencer's own state. Both hosts read them through the shared frame resolver and screen-layer builder; nothing of the shot is host-side.

### Skipping

A cutscene seen to the end (or skipped) once raises the pack flag `cutscene.<id>` (`seen_flag`), saved with the pack's progress. From then on Start skips it, if `skippable`: every track runs to its end at once - a walk lands where it was going, a glide arrives, a line not yet shown is not shown, every flag, item and quest step still happens. A line that ends on a menu stops the skip there, since its choice is the player's; the cutscene plays on from it. The skipped run holds until Start is let go, so the same press does not open the pause menu.

### Saves

A cutscene is not a state a save resumes into. `World::host_save_allowed` refuses a host save while one runs, and the pause menu cannot open (the player is held), so no save is ever made mid-cutscene. A loaded save starts outside any cutscene; the seen flags record what was watched.

## Validation

Errors stop the pack loading; warnings do not.

| Check | Severity |
|---|---|
| An actor that is not `player`, a `retail:<slot>` (without `map`), or an NPC of the cutscene's map | error |
| A tile off the cutscene map's grid | error |
| A line, formation, boss, quest or card that does not exist | error |
| `on_choice` on a line with no menu, or more labels than options | error |
| A jump to a label the track does not define, or a label defined twice | error |
| A step no flow reaches (past a `goto`, `end` or fight with no label) | error |
| A loop that never waits a frame | error |
| `start_track` / `join` of a track that does not exist | error |
| A `face` with both or neither of `facing` / `toward`; a facing past `7` | error |
| A zoom outside `0.25..=4`, a pitch outside `-60..=60`, more than ten minutes in one step, a shake over 64 | error |
| A track no `start_track` starts | warning |
| A hook naming a cutscene that does not exist, or an arrival cutscene bound to another map | error |
| A boss `intro` or `outro` that starts a fight of its own | error |
| A `banner` line past 40 characters | warning |

## Gaps

- The field draws the party leader only, so the other party members cannot be moved.
- The path round a wall is the collision grid's: a prop with no wall bits under it (a pack prop the map gives no wall) is walked through.
- A cutscene plays on one scene: a scene change ends it, so there is no step that changes map midway.
- `clip` on the player names a clip base of the party bank (idle, walk, run, ...), not a free animation.
