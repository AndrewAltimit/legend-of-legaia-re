# Minigame film

The corpus scores a minigame state on one frame, which cannot see a rhythm game's
timing: when the beat clock starts against the music, which game frame judges a
press, how the camera track steps. The film holds a pad schedule on one retail
minigame state on both sides and compares a named set of the minigame's words
game frame by game frame.

```bash
scripts/ci/minigame-film.py minigame_dance_retail_ready --set dance --frames 3300
scripts/ci/minigame-film.py minigame_dance_retail_ready --set dance --frames 2000 --shift 3 \
    --pad 130-133:SQUARE,144-147:CIRCLE,407-410:TRIANGLE
scripts/ci/minigame-film.py minigame_dance_retail_ready --set dance --no-retail   # engine side only
```

| Side | Piece | Writes |
|---|---|---|
| Retail | [`autorun_minigame_film.lua`](../../scripts/pcsx-redux/autorun_minigame_film.lua) under PCSX-Redux | `minigame_film.csv`, one row a vsync of the words `LEGAIA_FILM_WORDS` names (`name=ADDR:KIND`, or `name=*PTR+OFF:KIND` through a pointer) |
| Engine | `LEGAIA_RC_MINIGAME_FILM_OUT` on `legaia-engine retail-compare` ([`minigame_film.rs`](../../crates/parity/src/retail_compare/minigame_film.rs)) | `engine.csv`, the same names off the session, one row a tick |

Output goes under `captures/`: it reads retail RAM and stays out of the repository.
The driver writes `film-score.tsv` beside the two films: each column's agreement and
its first diverging frame.

The engine side takes the seeded state's warp ([minigame states](retail-compare.md#minigame-states))
and starts writing on the tick the session stands where the capture does (the
dance: the READY counter at the state's own value, with no display lag, since the
film pairs RAM words). It runs the dance's venue step after every tick, as both
play hosts do, so the camera track moves.

## Reading it

Both sides are cut into game frames and paired on the minigame's own key: for the
dance, the READY counter through state 3, the `GO!` accumulator through states 4 and
5, the lead-in count through states 6 and 7, then the song's game frames by their
ordinal from the first. A vsync of entry latency on either side therefore does not
read as a divergence; the driver prints the vsync offset of every pair instead,
and a constant offset is the save state's own vsync phase.

A save state's RAM is the vsync before the film's first row, so the engine's row 1
stands a few vsyncs ahead of retail's. A hands-off run prints that offset; pass it
as `--shift` on a pressed run, so the engine holds each press on the vsync that
matches retail's.

## What the dance film pins

Hands off, from `minigame_dance_retail_ready` to the results: the count-in states,
the beat phase and song timer, both rivals' scores, gauges, triangle stock, chain
cursors and banked combo slots, and the camera key, segment timer, angles and eye,
on every game frame of the song. With a press schedule, the human's score, gauge,
stock, chain, latch and miss counter too. The words the engine does not keep past
their phase (the READY counter after state 3, the song timer past its limit) are
the columns that stay apart.

## What the hub film pins

`--set hub` from `minigame_muscle_dome_retail_welcome`, hands off: the contest
hub's arm byte `0x801D1A78` and its counter family (`0x801D1A70`, `0x801D1A7C..0x801D1A8C`)
agree on every game frame from the Welcome hold through the ROUND card's fade-out,
and the hub hands the leg over on the same vsync. The engine's own scene mode rides
along as an engine-only `emode` column: retail spends the vsyncs after the hand-over
in game mode `0x14` before the fight's `0x15`, while the engine enters the fight on the
hand-over tick.

## The fishing replay

`--set fishing --replay` records a catch from `minigame_fishing_retail_shore` with
Cross driven closed-loop on the fishing globals (`LEGAIA_FILM_POLICY=fishing_catch`:
tap on the shore, lock the meter at `0xF00`, pump the cadence, then hold under the
tension band), and the engine side ticks the pond once per retail game frame on
that frame's step and pad words instead of running its own clock
(`LEGAIA_RC_MINIGAME_FILM_REPLAY`), then runs the venue's actor frame on the same
step (`tick_fishing_venue_on_host`), whose re-target roll and catch trail draw from
the pond's `rand()` stream. A game frame is a move of the frame driver's
counter `_DAT_8007B798`; the step it runs on is read a vsync after the move, its
pad words on the move, and its results on the next move. Two harness seats stand
the pond on the capture: its `rand()` stream on the state's BIOS seed and the pad
pump's free-running auto-repeat clock `_DAT_8007B940` on the capture's phase. The
replay compares rules, not pace: the pace question is the [venue's game
tick](../subsystems/minigame-fishing.md#the-venues-game-tick). Over a catch the
`rand()` word and the point total agree on every game frame through the close-up
and the plate; the state column keeps `0x1E` past the close-up, since the port
holds the showcase states `0x20..0x22` in the venue camera, not the session.

Without `--replay` the engine runs the pond on its own clock, and the pace is
the comparison: the engine side writes `edt` (the step the next game frame runs
on), `ran` (the step this vsync's frame ran on, `0` between frames) and `cost`
(what the last frame stored in the pond pacer's ring), beside retail's `dt` and
frame counter `gframe`. The names are the engine's own, so the rules table
leaves them out. Add the ring (`0x80084098`, sixteen `s16`) and its cursor
(`0x8007B758`) to the retail words to read retail's cost per frame.

## The Baka Fighter film

`--set baka` from `minigame_baka_fighter_retail_attract`, START at vsync `30` and
Cross every `20` vsyncs: the cabinet's state, its clocks, both fighters' HP, wins,
chosen attacks and combo counters, and the `rand()` word (the cabinet's stream
seated on the capture's, harness). Map each side's `rand()` words to their index
in the BIOS stream to read which exchange took which draw; the two sides then
agree exchange for exchange through both rounds. The cabinet's opponent install
(state `0x1E`) blocks on its CD load for `26` vsyncs in the film; the port's
retail-faithful mode, which the film runs, holds it as long, and the default
loads at once.

The engine side also writes `dt`, the step the next game frame runs on, which
the driver pairs with retail's step byte, and `cost`, what the last frame stored
in the cabinet's frame ring ([frame pacing](../subsystems/minigame-baka-fighter.md#frame-pacing)).
Two PCSX-Redux recordings of the same state and press train can differ by one
Cross of the train (`20` vsyncs) at an exchange whose press lands in a frame
paced at step `3`: the press-to-commit edge there is marginal on retail's side,
so a residual of one press cycle at such an exchange is not by itself a port
defect.

## Adding a minigame

Add the words to `SETS` in the driver (address, kind, a key per pre-play phase and
the columns that mark a play frame) and the same names to `columns` / `values` in
`minigame_film.rs`. A minigame whose play phase reads `rand()` needs the stream
seated at the aligned tick before the film can pair past its first roll.
