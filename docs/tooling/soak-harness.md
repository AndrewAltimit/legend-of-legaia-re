# Soak / softlock harness

[`crates/engine-shell/tests/soak_harness.rs`](../../crates/engine-shell/tests/soak_harness.rs)
drives the headless engine with seeded, game-shaped pseudo-random pad input
across every playable scene and every minigame, and watches for the engine
falling over. It is the bug-finding sibling of the route ladders in
[`determinism-replay.md`](determinism-replay.md): those walk a path someone
chose and score how far it gets; this one produces the input a player
produces by accident - a confirm pressed during a door fade, the pause menu
opened on the frame a battle starts, a direction held into a picker - and
reports whatever breaks.

Every finding is saved as a `j-replay-v1` pad script that reproduces it
deterministically, reduced to the input that matters.

## The driver

One run is `(scene, seed, frames)`. Each run opens a fresh
[`BootSession`](../../crates/engine-session/src/boot.rs), seeds the retail
New Game party, mounts an empty two-port card rack, seeds the world RNG from
the run seed, and enters the scene live (`enter_scene_live`) with the random
encounter loop and player-driven battles armed. It then ticks `frames`
frames. The only per-frame actuator is `World::set_pad`.

`BootSession::tick` is not a whole host: both play hosts do several things
around it every frame, and a driver that skipped them would measure its own
omissions. The harness does what the hosts do:

| host duty | what the harness does |
|---|---|
| presentation queues | drains the field-event, battle-event, hit, SFX, shout, XA, CLUT-stage and minigame-cue queues and routes battle effect spawns after every tick |
| FMV playback | skips the movie (`finish_cutscene`) and runs the shared post-play hand-off, as the headless `play` subcommand does |
| name entry | while the overlay is up, routes the pad edge into `step_name_entry` and skips the field tick, as both hosts' modal arms do |
| shop / prize counter | a shop or prize counter the tick staged opens in a `MenuRuntime`; while it is up the field gets a neutral pad (or is frozen whole, for a session that suspends it), the session steps on the frame's edges, and a closed one unparks the field script (`finish_field_shop` / `finish_prize_exchange`) - the play window's `tick_menu_runtime_session` |
| save screen | a Save pick writes `save_full` into an in-memory card and refreshes the grid's snapshot; a Load pick resumes that block's file through `resume_save` - the play window's `apply_save_commit`, with the card held in memory |

A minigame run is a pseudo-scene `<venue>+mg<sub_id>`: the venue is entered,
then `World::request_minigame_warp` arms the same mode-24 door warp the
cabinet's op-`0x3E` makes, so the next tick loads the minigame overlay
through the retail path. The venue's coin compare is satisfied by giving the
party casino coins first - the one non-pad precondition the harness sets.

A round-trip run is a pseudo-scene `<scene>+rt`. Every `ROUND_TRIP_EVERY`
frames, on a frame a player could save on (free field roam: no menu,
timeline, dialogue, name entry, shop or FMV), it takes `save_full`, resumes
that file through `resume_save` exactly as a Load does, and takes `save_full`
again; the two must agree field for field. The run also starts with one save
on the card, so the pause menu's Load row - open in every scene, where Save
is per-scene - has a file to resume, and random menu browsing reaches the
card flow end to end.

Any run label can end in `@<save>` (`town01+rt@PRO-14`): the run lifts that
save from the library card (`LEGAIA_SOAK_CARD`, default the playthrough card)
and plays the scene with its party, bag and story flags instead of the New
Game party's, so late-game scenes are soaked with the party and flags they are
reached with. `LEGAIA_SOAK_SAVE=<save>` applies it to the whole scene set.
Whatever value check the seeded state already fails (the playthrough card
holds a 255-count stack) belongs to the save and is not reported. A save on
another library card is named `@<card>:<save>`
(`bubu2@playthrough-ladder-pro00-14.mcr:PRO-07`); `LEGAIA_SOAK_SAVE` with a
non-default `LEGAIA_SOAK_CARD` labels its runs that way, so a replay written
from one carries its card. The card does not enter the run's seed.

An `@<save>` run still enters its scene by name, not through a door the
save's story opens, so a finding only such a run raises needs that door
before it is an engine defect. Two of the shape are known. From `PRO-04` on,
`map01`'s entry script (`P1[0]`, behind system flag `0x3BA`) walls in Rim
Elm's footprint (tiles `95..97 x 24..27` and row 25 from 94 to 98, leaving
`(96, 27)`); the `town0b` and `town0c` exits land on `(96, 25)`, inside
those walls, so `town0c@PRO-04` leaves the party boxed in on the overworld. That Rim Elm is
the chapter-1 town: in this stretch of the story the party's Rim Elm is
`town0d`, which the spine reaches through `concend`, not the overworld. And `korb3`'s default entry seat is walled
on all four sides once the arrival cutscene that moves the party off it is
gated off by the save's flags. Both are listed in the harness's `SAVE_ENTRY_ARTIFACTS`: when every
hit of such a signature comes from an `@<save>` run, the report names it on a
"known entry artifact" line instead of the findings table.

`opurud+rt` once failed its save / load round trip on the party records
(records 1 and 2 read zero at their first byte after the resume). That one
was an engine defect, not an entry artifact: a field save folded actor slots
1 and 2 back into the records, and in a field those slots are the scene's -
`opurud`'s scripts re-stamp them from zeroed nodes - so the save zeroed both
pools and the load re-seeded the "unjoined" members from the New Game
template. `World::save_party` now folds actor mirrors back only in a battle.
Its repro is `fixed/opurud_field_save_zeroes_party_records`; it needs the
library card the `@PRO-00` label names.

The soak reaches the round trip only where a random walk takes it. Its
library-wide sibling is `engine-shell/tests/save_roundtrip_library.rs`: every
Legaia save on the library cards is loaded through the card path both hosts
take (`MountedCard::save_at`, then `BootSession::resume_save`), checked field
for field against the block it came from, played on, and saved through a
blank card (`card_write::write_save_into_card`), the LGSF codec and a second
resume; each card's latest save repeats the round trip in a spread of
scenes. The field VM's scratchpad word (`ext.story_flags`) is outside that
comparison for the card path - no retail block carries it.

A shop run is a pseudo-scene `<scene>+shop`, one per scene whose MAN carries
a priced gold shop. Every `SHOP_VISIT_EVERY` free field frames it hands one of
the scene's shops (`World::scene_shop_session`) to the menu runtime, as the
merchant's op `0x49` would. Random walking almost never reaches a merchant
and picks Buy, so without it the buy / sell / quantity screens go unsoaked.

### The scene set

Every CDNAME label that resolves to a playable scene (a kingdom overworld, or
a scene whose field MAN resolves), unioned with the decoded `0x3F`
destinations of those scenes, plus the five minigame pseudo-scenes, a `+rt`
round-trip twin of every scene, and a `+shop` twin of every scene with a
shop. Nothing is hand-listed.

## Policies

The input source is a pure function of its seed and the observed session, so
`(scene, seed, frames)` alone regenerates a run. What it presses depends on
what holds the frame:

| state | policy |
|---|---|
| field / overworld | segments of: held walks (one direction or a diagonal, sometimes with Cross taps), Cross/Circle mashing, a Start tap, idle, and chaos (random masks of every button but Start) |
| dialogue owns input | mostly Cross, some directions (pickers), some Circle |
| pause menu | random taps for a random budget, then Circle until it closes |
| battle | mostly the route ladder's fighter (Begin, Attack, Auto, confirm the target, Cross any message box), otherwise random taps |
| minigame / other | random face buttons and held directions, and rarely Start - the minigames' escape |

Every UI surface reads `just_pressed`, so a press is a tap: the wanted mask
on one frame and neutral on the next.

## Detectors

| detector | fires when |
|---|---|
| `panic` | a tick (or scene entry, or the FMV hand-off) panics; the location and the first engine frames of the backtrace are kept |
| `tick_error` | `tick()` returns an error |
| `hang` | one tick runs past `LEGAIA_SOAK_HANG_SECS` of wall time; the watchdog writes `HANG.txt` and exits, since a hung tick cannot be interrupted |
| `softlock` | the progress digest is unchanged for a whole window while the pad took at least three distinct masks |
| `script_stall` | the modal timeline (or first helper context) sits at one PC for a whole window of field frames, even while something else moves - except that a `0xC7` walk whose walked bodies are still stepping is the op progressing |
| `battle_loop` | three battles open from one parked timeline PC |
| `menu_stuck` | the pause menu stays open through the Circle back-out |
| `battle_endless` | one battle outlasts `LEGAIA_SOAK_BATTLE_FRAMES` |
| `dropped_to_title` | the world mode becomes `Title` without a game over |
| `unknown_scene` | a transition names a map id with no scene, or a label outside the scene set |
| `non_finite` | a camera float or the caption alpha is NaN or infinite |
| `value` | roster HP / MP over max, a level outside `1..=99`, battle HP over max, money out of range, a bag stack over 99, a count on a free slot, or two stacks of one id inside the active window |
| `unbounded_growth` | an engine queue grows past a cap |
| `effect_residue` | on the first field or world-map frame after a battle exit or a scene load, `World::battle_effect_residue` names a battle effect still live (the `efect.dat` pool, a move-FX / effect-script / summon scene-graph, the streak block, a cast-band request) - retail resets the whole actor pool at both points ([`effect-vm.md`](../subsystems/effect-vm.md#battle-effects-die-with-the-battle)) |
| `save_roundtrip` | in a `+rt` run, the save taken after a resume disagrees with the save it resumed; the location names the first differing field (`party[i]+offset`, `ext.money`, `ext_v2.field_position`, ...) |

The **progress digest** hashes everything a player could see move: mode and
scene, the player's position and heading, every script context's PC, the
dialogue and menu state, shop and name-entry state, scripted NPC glides, the
battle command session and every actor's HP, the running minigame's session,
money and bag size. It is deliberately broad - a false "frozen" costs a
triage, a false "moving" only a missed finding.

A softlock is **located** by what holds the frame: `pause-menu:<row>:<phase>`,
`battle:command:<phase>`, `battle:state:<action state>`, `timeline@pc:op`,
`helper@pc:op`, `dialogue`, `shop`, `name-entry`, `mode:<minigame>`, or
`free-roam:immobile@tile(x,z)`. The pond names its sub-screen
(`mode:Fishing:<phase>`, `:hub-<screen>`, `:exchange`), so a reduction cannot
trade a park on one screen for an idle shore that merely waits for a cast. A battle park whose HP-bar pair is absorbing
(`hp != hp_display` with a zero accumulator - the shape that holds the
`0x51` bar-drain gate for good, see `BattleActor::set_hp_synced`) carries a
`:hp-bar-absorbing` suffix.

`script_stall` does not count frames spent in battle, the pause menu or a
minigame (they suspend the field VM by design), nor frames under a
player-owned modal (name entry, a shop).

## Findings, signatures and reports

A finding's **signature** is `detector|scene|location`. Findings are
deduplicated by signature and ranked: panics, hangs and tick errors first;
softlocks, battle loops, stuck menus, endless battles, drops to title and
unknown scenes next; free-roam immobility and script stalls after; value,
non-finite, queue-growth and save round-trip checks last.

A soak writes `target/soak/<tag>/` (gitignored with `target/`):

- `report.md` - volume, mode coverage, and the ranked signature table;
- `findings.json` - the same rows, machine-readable;
- `replays/<slug>.replay.toml` - one `j-replay-v1` file per signature, named
  by the signature with every non-alphanumeric character turned to `_` (cut
  at 96 characters).

For each signature the first occurrence is **confirmed** (replayed from its
pads, truncated to the finding frame, and checked to fire again) and then
**reduced**: chunks of the pad stream are zeroed while the signature still
fires, a bounded delta-debugging pass (`LEGAIA_SOAK_MINIMIZE` attempts). A
softlock is reduced under the neutral-pad control (`min_distinct = 1`), so the
input that only fed the frozen window drops out; its replay records that in a
`# soak-min-distinct` header.

The replay format carries no scene field, so the start scene and seed ride in
header comments (`# soak-scene`, `# soak-seed`, `# soak-signature`) that
TOML ignores.

## Running

```bash
# the fixed-budget gate: a few scenes + the five minigames, asserts no panic
cargo test -p legaia-engine-shell --profile release-test --test integration soak_harness::soak_smoke

# a long soak (report-only unless LEGAIA_SOAK_STRICT=1)
LEGAIA_SOAK_SEEDS=30 LEGAIA_SOAK_FRAMES=18000 LEGAIA_SOAK_JOBS=8 LEGAIA_SOAK_TAG=big \
  cargo test -p legaia-engine-shell --profile release-test --test integration soak_harness::soak_long -- --nocapture
```

| variable | meaning |
|---|---|
| `LEGAIA_DISC_BIN` | required; every disc-gated test skips and passes without it |
| `LEGAIA_EXTRACTED_DIR` | extracted tree to read (falls back to `<repo>/extracted`, then to the disc image itself) |
| `LEGAIA_SOAK_SEEDS` / `LEGAIA_SOAK_SEED_BASE` | seed count and first seed |
| `LEGAIA_SOAK_FRAMES` | frames per run |
| `LEGAIA_SOAK_SCENES` | comma list of scenes / pseudo-scenes instead of the full set |
| `LEGAIA_SOAK_SHARD` | `i/n` - run every n-th scene, to chunk a long soak across invocations |
| `LEGAIA_SOAK_JOBS` | worker threads |
| `LEGAIA_SOAK_TAG` / `LEGAIA_SOAK_OUT` | report directory name / base |
| `LEGAIA_SOAK_SOFTLOCK_FRAMES` | softlock and script-stall window |
| `LEGAIA_SOAK_BATTLE_FRAMES` | endless-battle threshold |
| `LEGAIA_SOAK_HANG_SECS` | single-tick watchdog |
| `LEGAIA_SOAK_MINIMIZE` / `LEGAIA_SOAK_CONFIRM_MAX` / `LEGAIA_SOAK_NO_CONFIRM` | reduction budget per signature / signatures confirmed / skip confirmation |
| `LEGAIA_SOAK_SAVE` / `LEGAIA_SOAK_CARD` | play every run from that card save / read saves from that library card |
| `LEGAIA_SOAK_NO_ENCOUNTERS` | disarm the random-encounter roll (a triage control: a battle that still starts is scripted) |

Runs are cheap - the headless engine ticks far faster than real time - so a
soak is bounded by the minimiser, not the runs. Chunk long soaks with
`LEGAIA_SOAK_SHARD` to keep each invocation short.

Overflow and debug-assertion coverage comes from building the same test with
the checks on, into its own target directory:

```bash
CARGO_TARGET_DIR=target/soak-checked \
CARGO_PROFILE_RELEASE_TEST_OVERFLOW_CHECKS=true \
CARGO_PROFILE_RELEASE_TEST_DEBUG_ASSERTIONS=true \
  cargo test -p legaia-engine-shell --profile release-test --test integration soak_harness::soak_long -- --nocapture
```

## Reproducing and triaging a finding

```bash
LEGAIA_SOAK_REPLAY=target/soak/big/replays/<slug>.replay.toml LEGAIA_SOAK_TRACE=60 \
  cargo test -p legaia-engine-shell --profile release-test --test integration soak_harness::soak_replay -- --nocapture
```

`LEGAIA_SOAK_REPLAY` takes a file or a directory of them. `LEGAIA_SOAK_TRACE=N`
prints a state line every `N` frames - mode, scene, holder, player position,
the bytes at every live script context's PC, scripted NPC glides with their
targets, and in battle the action state, frame timer and every actor's HP /
display / accumulator - plus each distinct script body a timeline or helper
runs, once, as hex (the first `LEGAIA_SOAK_TRACE_BYTES` bytes, `0x400` by
default). `LEGAIA_SOAK_TRACE_NPC=<slot>` adds that placement's position to
every line, walking or not, which is how to find where an actor stood before a
walk started. The trace goes to the terminal only; it is never written
into a report.

A useful first question for any `softlock` or `script_stall` is whether the
input matters at all: `LEGAIA_SOAK_MIN_DISTINCT=1` with an all-neutral replay
is the control.

## Fixtures

[`scripts/replays/soak/`](../../scripts/replays/soak/) holds reduced replays
of open findings - pad input only. `soak_fixtures` parses every one without a
disc and, with one, replays each and reports whether it **still reproduces**.
It never fails on a fixture that stopped reproducing: that is the signal a fix
landed, and the fixture then moves to `fixed/`.

An open finding's replay stays here until its fix moves it
to `fixed/`.

[`scripts/replays/soak/fixed/`](../../scripts/replays/soak/fixed/) holds the
closed findings as regressions. `soak_fixed_fixtures` replays each and fails
if its recorded signature fires again; a `# soak-expect-max-battles = N`
header also bounds the battles the replay may open, since a loop that stops
signing as `battle_loop` could still fight twice. A fixed replay's `frames`
may run past the finding frame, so the check covers what happens after it -
the town0c bee beat's replay runs through the lost fight and the field
return. Only the recorded signature is asserted: a replay reduced under the
neutral-pad control (`# soak-min-distinct = 1`) that now idles somewhere
new still reads as a softlock there, which is the control working, not a
regression.

| fixed replay | finding | what closed it |
|---|---|---|
| `town0c_scripted_battle_loop` | the Rim Elm bee fight re-fired on every return | the touch-resumed beat ends at its `0x21`; the system script sits out a held player and the battle intro |
| `town0d_dual_player_walk` | two first-talk records walked the player at once | a playing timeline steps only the placements it addresses |
| `rugi_npc_walk_from_hide_box` | a placed NPC walked in from the off-map hide box | a spawned record's `4C 51` seat is published before a same-slice `C7` walk |
| `urudre3_npc_walk_from_hide_box` | the same, for a seat poked in an earlier op of the slice | any cross-context poke that moves an actor is published at once |
| `bylon_party_actor_from_hide_box` | a party actor walked in from the hide box | `4C 37` copies the player onto the actor (the host hook answered "no player") |
| `urudre1_player_seat_dropped` | a scripted player walk ran 8000 units, then free roam resumed on a camera spot | a record's `A3 F8` / `CC F8 51` seat takes the player arm |
| `town0d_tetsu_picker_loop` | a conversation looping on its first picker option read as parked | harness: the digest counts timeline slices and the panel position |
| `vell_attack_short_step_park` | an attack approach parked a short step from its target | `+0x3C` is the per-frame body pair |
| `taiku2_hp_bar_absorbing_park` | the `0x51` bar-drain gate parked on an absorbing HP-bar pair | the restaged action clip replays |
| `station3_door_of_light_head_define` / `conc3_door_of_light_head_define` | a Door of Light used where the region record stores an all-zero return triple warped to `init_data` and failed the scene entry | a travel word inside the TOC header rows is a miss (`World::drain_staged_menu_warp`) |
| `jouinb_long_scripted_walk` | a scripted player walk across most of the map outlasted the stall window | harness: a walk op still stepping is progress |
| `ropeway_player_parked_on_gondola` | Octam's first-arrival cutscene (`ropeway` `P2[6]`) ended with the player seated on the gondola (`A3 F8 24 1F`), unable to step off in any direction | only the scene-init sweep's placements collide: the walk controller's candidate gather `FUN_801CF754` walks the `+0x0C` actor list, and the window sweep's placements (the gondola among them) live on `+0x24` |
| `edlast_ending_soft_reset` | after the credits' press the party stood in `edlast` with no way out | `49 0C` runs slot `0x33`, the return-to-title soft reset, instead of closing at once |
| `koin2_fatal_decision_stone_party` | a fight sat in Fatal Decision's capture band for 18000 frames | the `0x5A` wipe scan reads the packed `+0x16E` (`status_word`), so a petrified party wipes - the caster had been re-seeded forever |
| `map01_overworld_beat_walk_never_steps` | `urudre2`'s hand-off beat on `map01` walked two placements with `C7` and parked on `B3 12 0A` for good | the world-map arm steps cross-context walk legs, as the field arm does |
| `jouine_camera_glide_pan` | `jouine` `P2[16]` parked on `4C CD` for longer than the softlock window | harness: the camera glide countdown is in both digests - the pan is about 2200 frames and is the shot moving |
| `opurud_field_save_zeroes_party_records` | `opurud+rt@PRO-00` lost records 1 and 2 on its save / load round trip | a field save keeps the records; actor mirrors fold back only in a battle |
| `conc3_ambient_walker_seat_snapback` | a cutscene walk ran from the walker's off-stage wander box across the whole map | an ambient walker adopts the live seat a script's `0x23` writes; it had re-published its own stale coordinates on its next step |
| `balden_fishing_exchange_pad_dead` | the prize list opened from the pond's hub menu answered no pad input | the engine steps the list off the pad edge (state `0x78`'s keys) |
| `bubu2_bag_stack_past_99` | `bubu2@...:PRO-07`: a battle grant raised a held stack of 99 to 100 | every bag grant (drop, steal, refund, prize, shop, unequip) goes through the retail add, which caps a merged stack at 99 |
| `jouina_final_heal_readout_overshoot` | `jouina@...:PRO-01`: a member downed and Final-Healed by one enemy hit parked the `0x51` bar-drain gate | the Final Heal sweep re-syncs the readout before its revive seed; the killing hit's undrained remainder had stacked above max HP |
| `tower_floor_door_glides_ignored` | `tower@PRO-10`: a floor door set the player down inside the far door's doorway, walled on every side | a touched object's record plays its cross-context compass walks on the player (`B7 F8` / `C1 F8`) as legs it runs on past, and the out leg carries the player clear |

The field-side fixes are described with their retail evidence in
[`script-vm.md`](../subsystems/script-vm.md#engagement-and-the-system-script).

## What it cannot detect

- **Visual or audio wrongness.** No frame is rendered and no sample mixed; a
  scene that draws garbage or plays the wrong track while its state advances
  reads as healthy.
- **Retail divergence.** Every detector is an internal-consistency check. A
  battle that resolves with the wrong damage, a door that lands on the wrong
  tile, a script branch the port takes and retail does not - all look like
  progress. The parity oracles own that question.
- **Anything past the first finding in a run.** A run stops at its first
  fatal finding (panic, tick error, softlock, stuck menu, endless battle,
  drop to title), and at a game over.
- **Deep progression.** A plain run starts from a New Game party in the
  scene it names, and a New Game party loses most late-game encounters; the
  harness does not cheat past that (the HP-bar pair makes a bare HP top-up a
  false softlock of its own). An `@<save>` run brings the save's party and
  flags instead, but only the states the library card holds.
- **Host drift.** The harness mirrors the host duties listed above; a duty a
  real host performs that the list does not name is invisible here, and one
  the harness performs that a host has dropped is masked.
