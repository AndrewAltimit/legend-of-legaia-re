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
[`BootSession`](../../crates/engine-shell/src/boot.rs), seeds the retail
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

A minigame run is a pseudo-scene `<venue>+mg<sub_id>`: the venue is entered,
then `World::request_minigame_warp` arms the same mode-24 door warp the
cabinet's op-`0x3E` makes, so the next tick loads the minigame overlay
through the retail path. The venue's coin compare is satisfied by giving the
party casino coins first - the one non-pad precondition the harness sets.

### The scene set

Every CDNAME label that resolves to a playable scene (a kingdom overworld, or
a scene whose field MAN resolves), unioned with the decoded `0x3F`
destinations of those scenes, plus the five minigame pseudo-scenes. Nothing
is hand-listed.

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
| `script_stall` | the modal timeline (or first helper context) sits at one PC for a whole window of field frames, even while something else moves |
| `battle_loop` | three battles open from one parked timeline PC |
| `menu_stuck` | the pause menu stays open through the Circle back-out |
| `battle_endless` | one battle outlasts `LEGAIA_SOAK_BATTLE_FRAMES` |
| `dropped_to_title` | the world mode becomes `Title` without a game over |
| `unknown_scene` | a transition names a map id with no scene, or a label outside the scene set |
| `non_finite` | a camera float or the caption alpha is NaN or infinite |
| `value` | roster HP / MP over max, a level outside `1..=99`, battle HP over max, money out of range, a bag stack over 99, a count on a free slot, or two stacks of one id inside the active window |
| `unbounded_growth` | an engine queue grows past a cap |

The **progress digest** hashes everything a player could see move: mode and
scene, the player's position and heading, every script context's PC, the
dialogue and menu state, shop and name-entry state, scripted NPC glides, the
battle command session and every actor's HP, the running minigame's session,
money and bag size. It is deliberately broad - a false "frozen" costs a
triage, a false "moving" only a missed finding.

A softlock is **located** by what holds the frame: `pause-menu:<row>:<phase>`,
`battle:command:<phase>`, `battle:state:<action state>`, `timeline@pc:op`,
`helper@pc:op`, `dialogue`, `shop`, `name-entry`, `mode:<minigame>`, or
`free-roam:immobile@tile(x,z)`. A battle park whose HP-bar pair is absorbing
(`hp != hp_display` with a zero accumulator - the shape that holds the
`0x51` bar-drain gate for good, see `BattleActor::set_hp_synced`) carries a
`:hp-bar-absorbing` suffix.

`script_stall` does not count frames spent in battle, the pause menu or a
minigame (they suspend the field VM by design), nor frames under a
player-owned modal (name entry, a shop).

## Findings, signatures and reports

A finding's **signature** is `detector|scene|location`. Findings are
deduplicated by signature and ranked: panics, hangs and tick errors first;
softlocks, battle loops and stuck menus next; free-roam immobility and
script stalls after; value checks last.

A soak writes `target/soak/<tag>/` (gitignored with `target/`):

- `report.md` - volume, mode coverage, and the ranked signature table;
- `findings.json` - the same rows, machine-readable;
- `replays/<signature>.replay.toml` - one `j-replay-v1` file per signature.

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
cargo test -p legaia-engine-shell --profile release-test --test soak_harness soak_smoke

# a long soak (report-only unless LEGAIA_SOAK_STRICT=1)
LEGAIA_SOAK_SEEDS=30 LEGAIA_SOAK_FRAMES=18000 LEGAIA_SOAK_JOBS=8 LEGAIA_SOAK_TAG=big \
  cargo test -p legaia-engine-shell --profile release-test --test soak_harness soak_long -- --nocapture
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
  cargo test -p legaia-engine-shell --profile release-test --test soak_harness soak_long -- --nocapture
```

## Reproducing and triaging a finding

```bash
LEGAIA_SOAK_REPLAY=target/soak/big/replays/<signature>.replay.toml LEGAIA_SOAK_TRACE=60 \
  cargo test -p legaia-engine-shell --profile release-test --test soak_harness soak_replay -- --nocapture
```

`LEGAIA_SOAK_REPLAY` takes a file or a directory of them. `LEGAIA_SOAK_TRACE=N`
prints a state line every `N` frames - mode, scene, holder, player position,
the bytes at every live script context's PC, scripted NPC glides with their
targets, and in battle the action state, frame timer and every actor's HP /
display / accumulator - plus each distinct script body a timeline or helper
runs, once, as hex. The trace goes to the terminal only; it is never written
into a report.

A useful first question for any `softlock` or `script_stall` is whether the
input matters at all: `LEGAIA_SOAK_MIN_DISTINCT=1` with an all-neutral replay
is the control.

## Fixtures

[`scripts/replays/soak/`](../../scripts/replays/soak/) holds reduced replays
of open findings - pad input only. `soak_fixtures` parses every one without a
disc and, with one, replays each and reports whether it **still reproduces**.
It never fails on a fixture that stopped reproducing: that is the signal a fix
landed, and the fixture is then deleted.

## What it cannot detect

- **Visual or audio wrongness.** No frame is rendered and no sample mixed; a
  scene that draws garbage or plays the wrong track while its state advances
  reads as healthy.
- **Retail divergence.** Every detector is an internal-consistency check. A
  battle that resolves with the wrong damage, a door that lands on the wrong
  tile, a script branch the port takes and retail does not - all look like
  progress. The parity oracles own that question.
- **Anything past the first finding in a run.** A run stops at its first
  fatal finding (panic, softlock, stuck menu, endless battle).
- **Deep progression.** Every run starts from a New Game party in the scene
  it names, and a New Game party loses most late-game encounters; the harness
  does not cheat past that (the HP-bar pair makes a bare HP top-up a false
  softlock of its own), so late-game battles are reached only in their first
  rounds.
- **Host drift.** The harness mirrors the host duties listed above; a duty a
  real host performs that the list does not name is invisible here, and one
  the harness performs that a host has dropped is masked.
