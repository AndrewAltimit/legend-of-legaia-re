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
- A held walk's clip runs at retail's rate: `kor_field_route_walk` steps `16` a pass through the same ten frames on both sides.

See also [`retail-compare.md`](retail-compare.md), [`cast-sequence-oracle.md`](cast-sequence-oracle.md).
