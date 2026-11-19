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

The retail side records on PCSX-Redux's recompiler (`--fast`). The recompiler can wedge on a state: `rim_elm_queen_bee_battle` parks the CPU in a BIOS handler on its first vsync, and the film read as a game frozen for 700 vsyncs. A film whose frame-pacer ring cursor (`0x8007B758`) never moves over its last 60 vsyncs is re-recorded on the interpreter, which runs the state on into its fight; `--interp` forces the interpreter.

The engine side seeds as the corpus does, stands the camera on the state's pose,
and raises what both play hosts raise: the compass bias, the edge wall probes,
terrain follow, solid and living NPCs. `--run` sets the Field Move option to Run;
read it off the retail side's step (a run step is `24` at frame step `2`, a walk
step `16`), since the seed has no host to mirror it.

A state saved mid-departure seats the scene-change actor (`RetailObs::departure_countdown` / `departure_state`: the countdown `gp+0x710` while the screen byte `0x8007B648` reads `0x80`, and `+0x1A` of the `FUN_80021934` actor). The engine's parked hold takes the vsync retail hands off on (the first frame past zero on the state's frame grid) and, for an actor still in state 3, what is left of its chunk read. Left to a fresh actor, `s2_rimelm_town01` left `map01` at vsync `70` where retail leaves at `32`. A harness seat; `LEGAIA_RC_NO_DEPART_SEAT=1` keeps the engine's.

Under the retail pad pipeline the film also seats the pad the state's last frame latched (`RetailObs::pad_buffer`, libpad's buffer at `0x800840F8` inverted, `World::seat_retail_frame_pad`): the frame after the save reads it before the film's own pad. `kor3_field_route_walk` steps once more under the route walk's Left before turning under the film's Up, and so follows retail onto its warp pad; `tower_field_walk`'s `rng` agrees to its end (`52` to `150` of `150`). A harness seat; `LEGAIA_RC_NO_PAD_SEAT=1` keeps the pipeline empty.

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

Both sides write the region roll's step counter as `enc` (retail's `_DAT_8007B5FC`, the engine's `World::encounter_step_counter`), and the engine side seats it from the state's RAM: the steps left before a fight are walk history the card load does not carry, so a film from `karisto_sol_pre_encounter` (counter `14`) rolls its fight on retail's first tile crossing, and a `map02_field_door_hop` walk drains `42` a crossing on both sides. A harness seat; `LEGAIA_RC_NO_STEP_SEAT=1` keeps the seed's counter.

Each side writes its talk state too: retail's dialogue byte `*(0x801C6EA4) + 0x62` (`talk`), each placement's talk save `+0x5A` and record cursor `+0x9E`, and the engine's `World::dialogue_owns_input`. A placement's clip is on both sides as well: retail's `+0x5C` / `+0x68` / `+0x6A` in `walk_film_actors.csv`, the engine's bound cursor (`FieldNpcState::clip_cursors`) in `engine.actors.csv`.

The film seats every drawn actor's clip cursor where the state's RAM holds it (`RetailObs::object_clips_ram`, through `World::seed_object_prop_clip`), so the columns compare how each pose moves from retail's own phase; `LEGAIA_RC_NO_CLIP_SEAT=1` leaves the seed's phases.
The seat writes the clip's control word `+0x62` onto the channel's context as well as the cursor, since the advance reads it there every tick: seated on the cursor alone, a clip retail holds at its end (`+0x62 & 2`) clamped back a frame on the next tick (`chapter2_garmel_pre_songi` clip `23`, `239` to `224`). Kingdom maps seat too, their placements ticking through the same driver (`map01`'s parked flats `15..=17`). A harness seat. Over the idle set it takes the placements' cursor agreement from about a fifth of the rows to nine in ten; what still parts is a wanderer changing clips on its own stream, a cursor bound off the seat's record, and the clamped one-shots a scene entry plays to their end.

The engine side also seats every wanderer's ambient stream and the world seed
from the state's RAM (`retail_compare::retail_ambient_streams`,
`World::seed_ambient_stream`), and `engine.csv` carries the seed word in its
`rng` column against retail's `walk_film.csv` `rng`: the free-walk stretch agrees
draw for draw until an engage or a touch separates the two runs.

A span names any pad button on both sides (`UP`, `CROSS`, `L1`, `TRIANGLE`, `START`, ...); spans that overlap hold their buttons together, so `10-90:UP,10-90:RIGHT` is a diagonal.

`LEGAIA_RC_WALK_PAD=<file>` (`tick,0xMASK` rows, one per change of the held word, a `LEGAIA_DIAG_TICK_DIGEST` run's engine pad) drives the engine side in place of the spans, the same rows `autorun_field_walk_film.lua` takes as `LEGAIA_PAD_FILE`; a press retail must see widens to two vsyncs on the retail side only. The engine also writes `engine.draws.csv` (`tick,flat,draws`, each ambient channel's `rand()` draws), against a retail breakpoint on `0x80056798`. The film runs the fog pool's render step as a play host does (`NativeSession::fog_render_tick`), since the live count it writes gates the next tick's fog spawns and their draws, and it stands on the state's fog records (`retail_fog_live`, the records with no step back to the displayed frame).

It also stands on the live count the state's last render step wrote (`_DAT_8007BCA8`, `RetailObs::fog_live_count`), which is what the first pass's spawner reads: a state saved between a pass's spawns and its render step holds more records up than that count (`field_walled_collision_pin`: 73 against 66).

A state saved inside a frame, after its actor pass and before its render pass, holds that frame's spawns unwalked: records at age `0`, more of them up than the live count. Retail's first step past such a save is that frame's fog walk, so the film runs one walk at the frame step before its first tick (`pending_fog_walk`).

A state saved inside the walk itself owes only part of it. The walk zeroes the live word before it starts and adds each survivor as it passes it, in slot order (`0x8003F398`, `0x8003F3C8`), so a table that already holds fog sheets, under a word smaller than the records up, names how far the walk got: the first `live` records up have taken the frame's step and the rest have not. `vell_fog_field` holds `24` up under the word `14`, and its first vsync steps the ten records past the fourteenth and none before them; the film walks just those (`PendingFogWalk::From`). Walking all `24` had freed every aged-out record one pass early, so the spawner read a free slot retail's did not have (`rng` agreement `29` to `150` of `151`).

The pool alone cannot always tell, since a frame whose emitter spawned nothing leaves no age-`0` record, so the film also reads the table the frame is building: the frame opener `FUN_8001698C` points scratchpad `0x1F8003F4` at it and clears it, and a table that holds packets but no fog sheet (`retail_frame_table`) was saved before the render pass reached its fog stage (`bylon_mist_gala_goes_ahead`, which spawns nothing over its cap). Without the walk every record aged and freed a pass behind retail's, and the first spawn after a record faded out drew on a full pool where retail's had room. `LEGAIA_RC_NO_FOG_WALK=1` turns it off, and `LEGAIA_RC_WALK_FOG=1` writes `engine.fog.csv`, the live count and every record's `slot:age:rate:x:z:y` a tick.

It seats each live ambient part's program from the state too (`retail_part_vm`, `World::seat_ambient_part_vm`): the PC `+0x70`, the wait `+0x54`, the loop PCs and counters `+0x88..+0x8E` and `+0x5C`, paired with the engine's parts record by record in list order. A part's waits run on the game pass from its spawn, so which pass its next `rand()` op draws on is time since the scene entry, and the seed's settle had left `koin1`'s parts on other PCs and other waits than the state's (`0x5EE` at PC `20`, wait `96`, against retail's `38`, `24`). Positions, banks and render state stay the engine's. A harness seat; `LEGAIA_RC_NO_PART_VM=1` turns it off and `LEGAIA_RC_PART_DIAG=1` prints both sides' parts.

A retail pass reads the pad the probe set one vsync earlier, and a pass is on a game frame, so a pad file made from an engine digest replays on retail two vsyncs early (each row's `tick - 2`, one-vsync presses widened); `retail - 1` of a draw then pairs with the engine tick.

Both sides log the cue ring. Retail's `cues` column is the ring's four ids (`0x8007B6D8`, `-1` empty), and an id that newly fills a slot is a push (the ager empties a played slot the frame after, so a re-push shows as a new fill). The engine's is the ids its producers handed out that tick, read off a tap on `World::take_sfx_ring_ops` (`World::open_sfx_ring_tap`), since the session drains the ring for its director inside the tick. The report pairs each retail push with the engine's nearest push of the same id and prints the offset in vsyncs. Cues the hosts key on their own (the pause menu's blips, `AudioDirector::queue_ui_cue`) are not on the ring and not in the column.

The first pass differs by construction: a state saved mid-walk has one more
pass of the old pad queued, as `kor3_field_route_walk`'s first row does.

The placement report keys on the flat MAN index: the channel slot past the
partition-0 count of the scene the film stands in, re-read when a film crosses
a scene change. A wanderer's heading and seat
are `rand()` history, so one that parts from retail after a few dozen vsyncs is
not a finding by itself; a placement off retail from the first row is.

## What it has settled

- The follow ease runs once a game pass ([`encounter.md`](../formats/encounter.md#from-the-block-to-the-live-camera-compose-ease-snap)).
- A carried player reads as still, and its focus trails a pass (same page).
- The d-pad remap reads the [authored octant](../subsystems/field-locomotion.md#gp0x2d8-is-authored-not-computed).
- The `rng` column, read as draws per vsync (step the LCG from one row's word to the next), names a consumer on the wrong clock: every retail draw lands on a pass vsync, so an engine draw on a vsync between passes is a defect. Two were: the ambient move-VM parts ticking on a counter of their own ([field-ambient-fx](../subsystems/field-ambient-fx.md#the-part-pool-and-the-halted-part-free)), and the fog pool walked every vsync with `dt = 1`, freeing an aged-out record a vsync early so the next pass's spawner drew a burst retail had not ([renderer](../subsystems/renderer.md#field-fog-sheets-fun_8003f348)). An idle film of `deroa_field_door_hop` then agrees draw for draw over its whole 300 vsyncs.
- `kor3`'s placements, flats 15 and 20 among them, hold retail's headings for a whole film, with no spin.
- The placements that turn without moving under an idle pad turn on both sides. `rugi_field_card_boot`'s flats `40..=43` (`64` a pass) and `cave01_attached_light`'s flats `4..=8` hold retail's heading on every vsync of a 300-vsync film; `vell_fog_field`'s flat `12` turns at retail's rate (`42` or `43` a pass) one vsync ahead, the pass parity of the seed rather than a spin. An engine-side sweep of every field state (`LEGAIA_RC_WALK_PAD` with an empty pad, 600 ticks) finds no other placement that turns in place; the rest that turn often are wanderers.
- `kor3`'s stair warp lands the camera on the destination block. With the state's queued first pass replayed (`--pad "0-3:LEFT,3-120:UP" --run`) the engine walks retail's path into the kind-0 tile, lands a vsync apart, and stands on retail's shot from the landing on ([the timed warp](../subsystems/field-locomotion.md#the-timed-kind-0-warp)). The player turns on the landing too: the room selector's `4C 2x` runs under the landed sentinel and turns him by the octants it moves, to retail's `2048`. One thing still parts: the slide along the wall starts a step early, since the engine resolves the direction every vsync and retail once a pass.
- `geremi_field_door_hop`'s flat `53` is not a facing defect. The scene's system script (`P1[0]`) loops back to `CC 35 85 00 00 37` every pass, a zero-budget face-at whose terminal snap (`0x80037EF8..0x80037F04`) turns placement `53` onto the bearing of placement `55`, a wanderer; retail's heading is that bearing one pass late on every vsync of the film, and the engine's tracks its own `55` the same way. The `facing` channel's `.99` there is `55`'s `rand()` history read through `53`.
- An ambient stream's turns run at retail's rate and differ only in phase: `rikuroa_field_card_walk`'s flat `16` and `balden_field_route_walk`'s flat `26` repeat retail's ramps a fixed number of vsyncs early, the wait their streams were inside at the capture.
- `concnow_field_door_hop`'s `_DAT_8007BCAC` climbs on both sides: the door-hop record's `4C 49 0A 00 F0 00` is a 240-step ramp ([menuctrl](../subsystems/script-vm-menuctrl.md#sub-9s-tristate)), which the engine had posted at once. The two climb at different rates (about `1.0` and `0.7` a vsync) to different ends (`266`, `234`): the ends are the footing the op read on another phase of the floor bob, and a ramp's rate is its span over the same 240 steps.
- `kor_field_route_walk`'s displayed frame is its pass-two-back pose exactly, filmed over the passes before the save by `autorun_walk_route.lua` with `LEGAIA_FILM_SCENE` ([retail-compare](retail-compare.md#the-walk-film-before-the-save)).
- A talk turns and restores the addressed placement on both sides. `town01_field_card_boot` (`4-8:UP`, then Cross every `50` vsyncs) snaps flat `47` onto the player bearing as the box opens and writes its `+0x5A` back as the last page closes, on both talks of the film; the engine's edges land four vsyncs before retail's, the film's input latency on every press (the opening tap turns the player four vsyncs apart too).
- A wanderer's turn after a talk starts from the restored heading. A state saved inside a talk holds the talker's touch save in `+0x5A` (the one placement whose record runs, `+0x10 & 0x100`), and the film seats it (`retail_talk_facing_save`, `World::seed_talk_facing_save`) so the talk's end restores it; a harness seat. Talk films with Cross every `40` vsyncs over `town01_npc16_dialogue_first_page`, `koin3_gold_card_guard_talk`, `casino_ticket_counter_dialog`, `retock_innkeeper_talk_open` and `town01_tetsu_topic_prompt` then hold every talker on retail's heading through the talk and after it; what parts is the wanderers' resume after the last page, two or three vsyncs off retail's, the film's press latency.
- A placement with a motion stream and no talk or menu record stands on its header tile from the scene's start, as the spawner `FUN_8003A1E4` seats every partition-1 placement (`0x8003A328..0x8003A354`). The engine had started such a stream at the origin, so `koin1`'s floating flat `57` walked from `(0, 0)` and stood at `(4, 0)` where retail's floats along `(9408..9632, 8512)`, and its wander drew on the wrong box; now `koin1_field_route_walk`'s `rng` agrees to its end (`9` to `35` of `35`) and both `koin1` films read `off retail 0`.
- Prologue-poked placements idle in step. On retail every placement a prologue seats with `4C 51` or pokes with `22` starts its clip on the same pass ([motion-vm](../subsystems/motion-vm.md#the-draws-are-the-worlds-stream)); `town01_field_card_boot`, `vozz_field_card_boot` and `garmel_field_card_boot` show each such clip's placements on one cursor, and the engine's do too.
- Town placements hold retail's headings and seats for a 300-vsync film in `garmel`, `vozz` and `retona` (`off retail 0`); `town01_field_card_boot`'s seven that part are wanderers, each first off after `40` vsyncs or more.
- An idle 300-vsync film (`--pad 900-901:L2`, nothing pressed inside the film) of `retock`, `taiku`, `nilboa`, `jou`, `deene`, `dohaty`, `noaru`, `chitei2`, `korb2` and `jouind` reads `off retail 0`. In `conc_field_card_boot`, `conc2_field_route_walk` and `dolk2_market_noa` every placement that parted turned or stepped at retail's rate one vsync early, and in `koin1_field_route_walk` one vsync late: the seed's pass parity, not a turn law. The film now seats it ([below](#the-game-frames-phase)), and `conc`, `conc2` and `town01_field_card_boot` read `off retail 0`. What still parts is `dolk2` flat `70`, a vsync-one seat `64` units off retail's on every row, and wanderers (`doman` flats `21` / `23`, `koin1` flats `17` / `55` / `60`).
- `dolk2_market_noa`'s flat `70` is the state's history, not a spawn defect. The placement (`P1[41]`) heads on tile `(73, 119)`, and the spawner `FUN_8003A1E4` seats it at `tile * 128 + 0x40` (`0x8003A328..0x8003A354`) and stores the seat tile `(z - 0x40) >> 7` into `+0x8D` (`0x8003A514..0x8003A534`): `(9408, 15296)`, tile `119`, the engine's seat. The state's node holds `(9408, 15232)` with `+0x8D = 120`, a tile the spawner cannot write from that header; the scene's `P2[7]` runs the context (`CC 46 51 49 78 ..`, an NPC run to `(73, 120)`) and moves it with its move programs, so the seat is what that beat left, and a film seeded from the scene's spawn does not replay it. The state is also a resident-patch capture, outside the retail set.
- A held walk's clip runs at retail's rate: `kor_field_route_walk` steps `16` a pass through the same ten frames on both sides.
- A carried player's height glides as retail's does. `tower_field_walk`'s scripted stair walk reads `-32, -8, 0` a pass on both sides once the walker legs leave `+0x16` to the [settle](../subsystems/field-locomotion.md#vertical-settle--ledge-hop---fun_801d1ba0--fun_801d1878); the step snap had landed it in one, and the camera fields off over its 61 passes went from pitch `33`, eye Y `35`, eye Z `34` to `1`, `4`, `1`. The leg itself now spends on the game-frame grid in a prop run as in a timeline (`8` a pass rather than `4` a vsync), so the follow focus trails it a whole pass as retail's does: focus Z off went from `54` to `7`, eye Y to `1`.

- A beat's last turn holds when the engine plays the beat. `son` `P2[5]` turns Luctes (flat `10`) onto `0x600` at `(2432, 7936)` and the engine film from the mid-beat checkpoint ends on the same heading and seat; a checkpoint after the beat seeds through the card load, whose entry stands him on his record's `0xE00`, so its facing miss was the seed's, not the engine's; the comparison now seats the beat's turn ([retail-compare](retail-compare.md#the-seeding-model)).
- A cutscene glide steps once a game frame. The mover `FUN_801DC0BC` adds `DAT_1F800393` to its progress once a frame, from the actor-list walk that runs after the script arming it, so a glide's first frame already stands a whole step along and every pose holds until the next frame. The engine had credited one a vsync: `son_field_door_hop`'s glides stood between two of retail's poses on the vsyncs between, and the one armed inside the film trailed retail's by `step - 1` frames for its whole flight.
  Crediting the step on the frame's opening vsync (`FrameClock::count_credited_frame`) takes the camera fields off retail on `son_field_door_hop` from `200` vsyncs to `0`, `taiku2_field_door_hop` `141` to `0`, `vozz_genesis_tree_songi` `43` to `0`, and `concnow_field_door_hop` `76` to `3`, whose `rng` column then agrees to the end (`34` to `100` of `100`); the follow ease keeps banking vsyncs toward its passes.
- A record's wait restarts when a player leg it armed lands. `tower_field_walk`'s lift glides the player, fades and waits `4A 3C`; the engine had moved the player `60` vsyncs after the fade began and retail moves him `60` after the glide lands, the landing clearing the record's `+0x54` ([script-vm](../subsystems/script-vm.md#0x37--0x41--0x47-yield-family-motion-ops)). The move now lands on retail's vsync: player position off `128` vsyncs to `16`, camera `213` to `101`, `rng` `44` to `52` of `150`.
- A talk stands every deferring wanderer, as retail's does. The engine had held only the talker, so under an open box the rest walked and drew; with the player engaged for the whole run ([motion-vm](../subsystems/motion-vm.md#the-0x8a-byte-is-a-suppression-mask-not-an-enable)) `retock_innkeeper_talk_open`'s `rng` agrees `150` of `150` (from `1`) and `retock_inn_stay_prompt`'s `150` of `151` (from `10`), and over the idle set the placement rows off retail's heading fall from `13452` to `7449`, off its seat from `15950` to `8607`.

- Field placements stand still under the battle intro. Its init rebuilds the actor lists and stores game mode `9` (`FUN_801CE8CC`), so no placement's driver `FUN_8003BC08` runs until the fight. `rim_elm_queen_bee_battle` (interpreter film) holds the seed `0xE49D91B4` from the intro's seeding (vsync `92`) to the battle's first draw (`234`); the engine's wanderers drew twenty times through that window, so the fight opened on another seed. It now opens on retail's. What still parts is the opening: the engine's intro starts at the trigger, where retail spends six vsyncs in mode `8` loading the intro overlay, and its battle draws `14` at entry and one a pass after, where retail draws one before mode `21` and four some thirty vsyncs later.
- A record waiting on a player leg moves on the pass after the leg lands. The runner steps the record ahead of the walk kernel in the object's tick (`FUN_8003BC08`: runner at `0x8003BD34`, kernel at `0x8003BD50`), so on the landing pass it still finds the leg in flight. `tower_field_walk`'s lift record walks the player, lands and walks him on; resumed on the landing pass, each later leg started a pass ahead of retail's. The player now stands on retail's position on every vsync of the film.
- The camera reads the locked player's let-through (`0x1F800394 & 0x10000`) as the player's tick saw it. `FUN_801D1344` tests it at `0x801D1704` and pins the focus onto the carried player before the objects' records run, so the record's `2F 10` on the pass after the last leg lands (vsync `215`, recorded on retail) still leaves the focus on the landing. Read live after the slice, the focus stopped a pass short for good: `tower_field_walk`'s camera rows off retail went from `101` to `2`, the two the post-warp snap leaves (the record snaps on the player before the kernel steps him that pass).

## The game frame's phase

Retail's pass runs once every frame step of vsyncs, and a state is saved on one vsync of that frame. The frame pacer `FUN_80016B6C` waits each frame out with libetc `VSync(step)` (`jal 0x8005FB84` at `0x800171C0`), which stores the vblank count on return (`0x800797D4`, `0x8005FC8C..0x8005FCA0`), so the vblank counter `0x8007A894` less that store is the vsync of its frame the state holds (`RetailObs::vsync_phase`), and the next pass opens `step - phase` vsyncs into the film.
The engine side seats its game-tick accumulators to match (`actor_vsync_accum`, the CLUT bank's twin): `phase - 1`, and on a phase of `0` it holds the first vsync. `conc_field_card_boot` (phase `0`) read `12` placements off on the seed's own phase and reads `0`, `conc2_field_route_walk` (`0`) `14` to `0`, `dolk2_market_noa` (`1`) `9` to `1`, `koin1_field_route_walk` (phase `2` of a step-`3` frame) `5` to `3`, `town01_field_card_boot` `5` to `0`. A harness seat.

The step is not a constant of the scene either. `FUN_80016B6C` measures it: each frame's time in hblanks goes into a sixteen-entry ring (`DAT_80084098`, cursor `gp+0x440`), and the step is the longest entry against the `0xF0` / `0x1FE` / `0x2D0` rungs, raised to the floor `DAT_8007B9D8`, which the field init stores as `2` on every field scene and every world map (`0x801D6990`). So a state saved a few frames after a slow stretch runs on at the raised step until those samples leave the ring. `garmel_zeto_appears_rising` holds ten clamped samples (`700`) behind six of about `450`: retail runs ten more frames at `3` and then `2` for the rest of the film.

The film seats that ring (`RetailObs::frame_cost_seat`, `World::seed_frame_cost_history`): the history, its cursor, the step bytes `0x1F800392` / `0x1F800393` off the scratchpad, and the newest sample as the cost of every frame that follows. The engine then resolves each next step by the ported law and waits the step of the frame that just ran, as the pacer's `VSync(0x1F800392)` does before it stores the new one. A newest sample reading the clamp under a step of `4` is a frame past the `0x2D0` rung, and the seat costs it there (`map01_tree_revival_cutscene` holds `4`). A harness seat; `LEGAIA_RC_NO_FRAME_RING=1` holds the state's step for the whole film.

What the seat cannot know is a cost that changes after the save. `doman_field_card_boot` (`432..449` hblanks at the save) and `korout_field_door_hop` (`471..496`) sit under the `0x1FE` rung and cross it inside their films, retail stepping `3` from there on; `field_walled_collision_pin` (`700..718`) crosses `0x2D0` and steps `4`.

The engine prices a field frame off what it draws ([the field frame-cost model](../subsystems/engine.md#the-field-frame-cost-model)), and a film anchors that price on the state's measured cost (`World::anchor_field_frame_cost`): each frame costs the state's newest sample plus the change the model prices since the seat, so the model carries the film's changes and the ring its level. No film's step moves under the anchor: the model's change over `doman_field_card_boot` and `korout_field_door_hop` stays under the `0x1FE` rung that retail's crosses.
The anchor is taken once the camera and the fog pool stand on the state's, with the sheets a walk over a copy of the pool emits counted: an installed snapshot carries none, and anchored on an empty sheet list `retona_field_card_boot` read its `45` sheets as a cost rise on the first walk. A harness seat; `LEGAIA_RC_NO_COST_ANCHOR=1` runs the model unanchored, as play does. A still holds the state's measured step for its whole seed and settle, the image child too (`LEGAIA_FIELD_FRAME_COST=0`): the model is play's estimate where no capture says what a frame costs.

A held pad walks at the speed of the Field Move option the state was saved under (`0x800846CC`, Walk `0` / Run `1`, which the walk XORs with the run button, `World::field_run_active`). The film seats it (`RetailObs::field_move_run`); most of the library's states were saved under Run. Left on the engine's Walk default, a film walked `8` units a vsync where retail walked `12` (`stone_field_route_walk`, `balden_field_route_walk`, `tunnelc_field_route_walk`), which read as a speed defect and is an input mismatch.
Seated, the distance per vsync matches retail's on step-`2` and step-`3` scenes alike (`stone`, `kor3`, `koin6`, `balden`, `deroa`, `tunnelc` under Run; `first_town_interactive`, `bubu2`, `uru2`, `nilboa` under Walk). A harness seat; `--run` / `LEGAIA_RC_WALK_RUN=1` forces Run and `=0` Walk.

The retail recorder writes the pacer's side of each vsync (`ring_cur`, the slot the next frame stores to; `frame_cost`, the newest frame time; `step`, `0x1F800393`), and `LEGAIA_RC_WALK_COST=1` writes the engine's beside it: each tick's `World::field_frame_features`, step and price (`engine.cost.csv`) and the state's own ring (`engine.ring.txt`). Those two files are what the model is fitted on.

## Where a film parts

[`autorun_field_rand_callers.lua`](../../scripts/pcsx-redux/autorun_field_rand_callers.lua) counts the calls of the `rand()` thunk `FUN_80056798` by return address, a vsync at a time, and writes the fog pool beside them (`fog_pool.csv`, the rows `LEGAIA_RC_WALK_FOG=1` writes on the engine side). Run on a film's state up to the vsync its `rng` column parts on, it names the consumer that drew more or fewer times there. The field's callers:

| Return address | Consumer | Draws |
|---|---|---|
| `0x801D61CC`, `0x801D61E8`, `0x801D6200` | the ambient emitter `FUN_801D6058`, scene arm | `72` a pass: twenty-four candidates, three draws each |
| `0x801D6214`, `0x801D622C`, `0x801D6238` | the same arm's bursts | one for a burst's size, two for each of its points |
| `0x801D629C..0x801D6560` | the fog spawner `FUN_801D629C` | five for a record it seats, two for one the overworld depth test drops, none for one it rejects |
| `0x801D371C`, `0x801D4600` | the move-VM extension `FUN_801D362C`: an ambient part's random operand | one an op |
| `0x80038C74`, `0x80039080` | the scripted-motion VM `FUN_80038158`: a wanderer's stream | one a decision |

Every film that parts before its end parts on one of four causes, read off that probe at the first parted pass:

| Cause | Films | Kind |
|---|---|---|
| The ambient parts the state holds and the seed does not: a spawner part's children live longer than the seed's settle, so the engine stands fewer of them (`garmel`: nine of retail's twenty-nine), and their random operands go undrawn | the five `garmel` films, `dohaty_field_card_walk`, `jagaroom_field_door_hop`, `koin1_field_route_walk`, `map02_field_door_hop`, `uru2_field_door_hop` | harness seat |
| A wanderer's stream on another phase than the state's | `doman_field_card_boot` | harness seat |
| The fog pool: a burst point the spawner seats on one side and rejects on the other, the pools having parted earlier with no draw to show it | `tower_field_walk`, `concnow_field_door_hop`, `teien_field_run`, `son_arrival_from_doman` | engine (fog pool) or seat, film by film |
| The measured step changing inside the film | `korout_field_door_hop`, `field_walled_collision_pin` | retail's frame time |

`koin6_field_route_walk` and `balden_field_route_walk` parted on the tick order inside the pass: the ambient parts are walked before the placements ([field-ambient-fx](../subsystems/field-ambient-fx.md#the-part-pool-and-the-halted-part-free)), and with the parts ticked after them each wanderer drew the value retail hands a part. Both now agree to their ends (`13` to `91` of `91`, `58` to `150` of `150`), and every placement in both holds retail's heading and seat for the whole film.

Five more films parted on the fog pool until the emitter took its centre from the camera translation rather than the player ([field-ambient-fx](../subsystems/field-ambient-fx.md#mechanism-4---the-ambient-particle-emitter)): `taiku2_field_door_hop` (`51` to `150` of `151`), `rikuroa2_field_door_hop` (`46` to `150`), `rikuroa_field_cave_exit_arrival` (`71` to `150`), `map01_tree_revival_cutscene` (`2` to `76` of `76`) and `s2_rimelm_town01` (`5` to `17` of `18`). The probe's `spawner_calls.csv` (every spawner entry with its tile, the walk-region box and the emitter's span) named it: on `taiku2` the box and span matched on both sides, and the tiles did not.

The pass clock is the cause only in the last row: with the ring seated, every other film whose emitter draws each pass opens its passes on retail's vsyncs up to the pass it parts on.

## The rand ratchet

How far a film's `rng` column agrees with retail's is one number for "the field runs
retail's draws on retail's clock", and
[`walk-film-rand.py`](../../scripts/ci/walk-film-rand.py) holds it to
[`walk-film-rand-baseline.json`](../../scripts/ci/walk-film-rand-baseline.json), a
film at a time:

```bash
scripts/ci/walk-film-rand.py --check                     # every cached film, engine side only
scripts/ci/walk-film-rand.py --only 'kor*' --keep        # a few films, engine CSVs kept
scripts/ci/walk-film-rand.py --propose /tmp/rand.json    # a candidate baseline
```

The retail side is a cache of recordings, `captures/walk-film-retail/<state>/`:
`walk_film.csv`, `walk_film_actors.csv` and the `manifest.txt` that names the pad and
the length, one idle film (`--pad 900-901:L2 --frames 300`) for each field state. A
film is added by recording it into that directory (`walk-film.py <state> --pad ..
--frames .. --out captures/walk-film-retail/<state>`). The ratchet runs only the
engine side, several films at once (the child is headless, and a film is a fraction
of a second, so the whole cache runs in seconds), and skips when the cache is absent.
The count is the same from run to run and at any `--jobs`.

Each side's word column is reduced to its changes, one entry a draw burst. Up to
three leading entries are dropped from either side, which absorbs the vsync inside its
frame the state was saved on, and the entries are compared in order up to the first
that differs: `agree` is that run's length, `draws` the retail film's entries. A film
that agrees to its end reads `agree == draws`, or one short when retail's last vsync
drew.

A baselined film fails `--check` when its `agree` falls, or when it produces no
engine film. A cached state the engine writes no film for (one saved inside a scene
change, `doman_arrival_from_korb2`) has no baseline row and is listed as `[no film]`. One whose retail side holds another number of entries was re-recorded
and is listed, not scored. A state captured on a patched disc (`resident_patch` in
`scripts/scenarios.toml`) is run and reported and never baselined. `--propose` writes
a candidate, each film at the better of baseline and measurement; a lower value needs
a [`baseline-loosenings.toml`](../../scripts/ci/baseline-loosenings.toml) entry
([ratchet integrity](ratchet-integrity.md)). The [full gate](build-and-test-speed.md#the-full-gate)
runs it as `walk-film-rand`.

A film that parts early is not by itself a defect: an engage, a touch or a CD load
the engine answers at once separates the two runs for good, and the count stops
there ([where a film parts](#where-a-film-parts) names the cause film by film). The ratchet holds what agrees, so a consumer moved onto the wrong clock shows
as a fall on every film that draws through it.

## The frame-cost gates

The [field frame-cost model](../subsystems/engine.md#the-field-frame-cost-model) is
scored on the pacer columns of its own idle films, and
[`field-cost-films.py`](../../scripts/ci/field-cost-films.py) runs every gate:

```bash
scripts/ci/field-cost-films.py record            # film the states the cache lacks
scripts/ci/field-cost-films.py score             # engine side + every gate
scripts/ci/field-cost-films.py score --only 'doman*' -v
```

| Gate | What it scores |
|---|---|
| films | each retail game frame priced off the engine's counts at that vsync, run through the ring from the state's own, against retail's step; the `fit` and `extra` sets apart |
| unanchored step | the engine's own step a vsync, run as play runs it (`LEGAIA_RC_NO_COST_ANCHOR=1`), against retail's |
| states | the price of the engine's first frames on each state against the rung of the state's saved ring, and the rms of price against the ring's median |

The films are listed in
[`film-sets/field-cost.txt`](../../scripts/ci/film-sets/field-cost.txt) and the states in
[`film-sets/field-cost-states.txt`](../../scripts/ci/film-sets/field-cost-states.txt). A
film is `--pad 900-901:L2` for `300` vsyncs on the PCSX-Redux recompiler, or on the
interpreter when the recompiler wedges (`rim_elm_queen_bee_battle`); the `extra` set
is scored over its first `150`. States captured on a patched disc are not filmed. A
state saved inside a load (`doman_pre_entry_card_boot` and two more) parks the pacer
for the whole film and scores no frame. The price scored is the engine's own
(`engine.cost.csv`'s `cost`), cycler uploads included. Re-filmed, the retail side
reads the same step on every game frame and the same cost on all but a few, a hblank
or two apart.

The retail films live in `captures/film-retail/field-cost/<state>/` (`walk_film.csv`,
`walk_film_actors.csv`, `manifest.txt`), beside the cast-sequence films of the same
cache, and the rand ratchet's in `captures/walk-film-retail/`. Both are durable
references: never delete them in a disk cleanup, since the gates have no other retail
side, and a re-film costs a full `record` run. The engine side is re-run on every
`score` under `target/field-cost-films/`, which is disposable.

See also [`retail-compare.md`](retail-compare.md), [`cast-sequence-oracle.md`](cast-sequence-oracle.md).
