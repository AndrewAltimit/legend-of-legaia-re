# Field walk film

The corpus scores one frame per state, so it cannot see how a walk unfolds: how
fast the follow camera closes, which way the pad turns the player, what a script
carrying the player does to the ease. The film holds a pad schedule on one retail
state on both sides and compares them pass by pass.

```bash
scripts/ci/walk-film.py kor_field_route_walk --pad 0-180:UP --frames 200 --run
scripts/ci/walk-film.py kor3_field_route_walk --pad 0-120:UP --no-retail   # engine side only
```

| Side | Piece | Writes |
|---|---|---|
| Retail | [`autorun_field_walk_film.lua`](../../scripts/pcsx-redux/autorun_field_walk_film.lua) under PCSX-Redux | `walk_film.csv` (player, camera, staging descriptor, `_DAT_8007BCAC`, one row a vsync), `walk_film_actors.csv` |
| Engine | `LEGAIA_RC_WALK_FILM` on `legaia-engine retail-compare` ([`walk_film.rs`](../../crates/parity/src/retail_compare/walk_film.rs)) | `engine.csv`, `engine.actors.csv` (one row a tick) |

Output goes under `captures/`: it reads retail RAM and stays out of the repository.

The engine side seeds as the corpus does, stands the camera on the state's pose,
and raises what both play hosts raise: the compass bias, the edge wall probes,
terrain follow, solid and living NPCs. `--run` sets the Field Move option to Run;
read it off the retail side's step (a run step is `24` at frame step `2`, a walk
step `16`), since the seed has no host to mirror it.

## Reading it

A retail pass is a row where the player or camera changed. The engine walks one
vsync at a time and eases on the pass grid, so each retail pass pairs with the last
engine row on the same spot, and the summary counts the camera fields that differ.
A low pairing count means the two walked different paths, which is the finding
to read first.

Both sides also write the player's clip: retail's bound record `+0x5E`, cursor
`+0x68` and rate `+0x6A`, the engine's bound clip, cursor and
`locomotion.player_clip_rate`. A still seats the pose; these columns say how the
pose moves. Retail's cursor steps once a pass, the engine's once a vsync at half
the step, so the two meet on every second row.

The engine side also seats every wanderer's ambient stream and the world seed
from the state's RAM (`retail_compare::retail_ambient_streams`,
`World::seed_ambient_stream`), and `engine.csv` carries the seed word in its
`rng` column against retail's `walk_film.csv` `rng`: the free-walk stretch agrees
draw for draw until an engage or a touch separates the two runs.

`LEGAIA_RC_WALK_PAD=<file>` (`tick,0xMASK` rows, one per change of the held word, a `LEGAIA_DIAG_TICK_DIGEST` run's engine pad) drives the engine side in place of the spans, the same rows `autorun_field_walk_film.lua` takes as `LEGAIA_PAD_FILE`; a press retail must see widens to two vsyncs on the retail side only. The engine also writes `engine.draws.csv` (`tick,flat,draws`, each ambient channel's `rand()` draws), against a retail breakpoint on `0x80056798`. The film runs the fog pool's render step as a play host does (`NativeSession::fog_render_tick`), since the live count it writes gates the next tick's fog spawns and their draws, and it stands on the state's fog records (`retail_fog_live`, the records with no step back to the displayed frame).

A retail pass reads the pad the probe set one vsync earlier, and a pass is on a game frame, so a pad file made from an engine digest replays on retail two vsyncs early (each row's `tick - 2`, one-vsync presses widened); `retail - 1` of a draw then pairs with the engine tick.

The first pass differs by construction: a state saved mid-walk has one more
pass of the old pad queued, as `kor3_field_route_walk`'s first row does.

The placement report keys on the flat MAN index. A wanderer's heading and seat
are `rand()` history, so one that parts from retail after a few dozen vsyncs is
not a finding by itself; a placement off retail from the first row is.

## What it has settled

- The follow ease runs once a game pass ([`encounter.md`](../formats/encounter.md#from-the-block-to-the-live-camera-compose-ease-snap)).
- A carried player reads as still, and its focus trails a pass (same page).
- The d-pad remap reads the [authored octant](../subsystems/field-locomotion.md#gp0x2d8-is-authored-not-computed).
- `kor3`'s placements, flats 15 and 20 among them, hold retail's headings for a whole film, with no spin.
- The placements that turn without moving under an idle pad turn on both sides. `rugi_field_card_boot`'s flats `40..=43` (`64` a pass) and `cave01_attached_light`'s flats `4..=8` hold retail's heading on every vsync of a 300-vsync film; `vell_fog_field`'s flat `12` turns at retail's rate (`42` or `43` a pass) one vsync ahead, the pass parity of the seed rather than a spin. An engine-side sweep of every field state (`LEGAIA_RC_WALK_PAD` with an empty pad, 600 ticks) finds no other placement that turns in place; the rest that turn often are wanderers.
- `kor3`'s stair warp lands the camera on the destination block. With the state's queued first pass replayed (`--pad "0-3:LEFT,3-120:UP" --run`) the engine walks retail's path into the kind-0 tile, lands a vsync apart, and stands on retail's shot from the landing on ([the timed warp](../subsystems/field-locomotion.md#the-timed-kind-0-warp)). The player turns on the landing too: the room selector's `4C 2x` runs under the landed sentinel and turns him by the octants it moves, to retail's `2048`. One thing still parts: the slide along the wall starts a step early, since the engine resolves the direction every vsync and retail once a pass.
- `geremi_field_door_hop`'s flat `53` is not a facing defect. The scene's system script (`P1[0]`) loops back to `CC 35 85 00 00 37` every pass, a zero-budget face-at whose terminal snap (`0x80037EF8..0x80037F04`) turns placement `53` onto the bearing of placement `55`, a wanderer; retail's heading is that bearing one pass late on every vsync of the film, and the engine's tracks its own `55` the same way. The `facing` channel's `.99` there is `55`'s `rand()` history read through `53`.
- An ambient stream's turns run at retail's rate and differ only in phase: `rikuroa_field_card_walk`'s flat `16` and `balden_field_route_walk`'s flat `26` repeat retail's ramps a fixed number of vsyncs early, the wait their streams were inside at the capture.
- `concnow_field_door_hop`'s `_DAT_8007BCAC` climbs on both sides: the door-hop record's `4C 49 0A 00 F0 00` is a 240-step ramp ([menuctrl](../subsystems/script-vm-menuctrl.md#sub-9s-tristate)), which the engine had posted at once. The two climb at different rates (about `1.0` and `0.7` a vsync) to different ends (`266`, `234`): the ends are the footing the op read on another phase of the floor bob, and a ramp's rate is its span over the same 240 steps.
- `kor_field_route_walk`'s displayed frame is its pass-two-back pose exactly, filmed over the passes before the save by `autorun_walk_route.lua` with `LEGAIA_FILM_SCENE` ([retail-compare](retail-compare.md#the-walk-film-before-the-save)).
- Town placements hold retail's headings and seats for a 300-vsync film in `garmel`, `vozz` and `retona` (`off retail 0`); `town01_field_card_boot`'s seven that part are wanderers, each first off after `40` vsyncs or more.
- A held walk's clip runs at retail's rate: `kor_field_route_walk` steps `16` a pass through the same ten frames on both sides.
- A carried player's height glides as retail's does. `tower_field_walk`'s scripted stair walk reads `-32, -8, 0` a pass on both sides once the walker legs leave `+0x16` to the [settle](../subsystems/field-locomotion.md#vertical-settle--ledge-hop---fun_801d1ba0--fun_801d1878); the step snap had landed it in one, and the camera fields off over its 61 passes went from pitch `33`, eye Y `35`, eye Z `34` to `1`, `4`, `1`. The leg itself now spends on the game-frame grid in a prop run as in a timeline (`8` a pass rather than `4` a vsync), so the follow focus trails it a whole pass as retail's does: focus Z off went from `54` to `7`, eye Y to `1`.

See also [`retail-compare.md`](retail-compare.md), [`cast-sequence-oracle.md`](cast-sequence-oracle.md).
