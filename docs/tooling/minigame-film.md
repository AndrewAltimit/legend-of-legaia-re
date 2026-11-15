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

## Adding a minigame

Add the words to `SETS` in the driver (address, kind, a key per pre-play phase and
the columns that mark a play frame) and the same names to `columns` / `values` in
`minigame_film.rs`. A minigame whose play phase reads `rand()` needs the stream
seated at the aligned tick before the film can pair past its first roll.
