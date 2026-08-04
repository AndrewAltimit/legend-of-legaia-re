# Lane 1 handoff - Muscle Dome intermission cadence

Out-of-scope items found while fixing "the intermission fires per turn". None
of these were touched; each is a one-line edit for whoever owns the file.

## 1. `CLAUDE.md` - the map row for the dome contradicts the page it links to

`CLAUDE.md`'s `minigame-muscle-dome.md` row reads:

> Muscle Dome **card-battle arena**: match SM (`FUN_801d0748`, phase byte
> `ctx+6`), 4-slot hand deal/commit under a point budget into the actor
> `+0x1df` action queue, resolution via the shared battle-action path. **Own
> overlay, not the hub family.**

Two of those are falsified on the page itself:

- "card-battle arena" - the doc's third line is "It is **not a card battle**";
  the four slots are the four direction commands `0xC..=0xF`.
- "Own overlay" - the match SM is resident in the **battle-action** overlay
  (PROT 0898) and the contest hub in PROT 0977. Neither is a dome-only overlay.

Suggested replacement cell: *Muscle Dome arena ladder: match SM
(`FUN_801d0748`, phase `ctx+6`) in the battle overlay, contest hub
(`FUN_801cf870`) in PROT 0977; four direction commands under an AP budget into
the actor `+0x1df` queue. Not a card battle.*

## 2. `site/_content/tooling/overlay-capture.html` - stale "score loop"

Line ~125 says `FUN_801D0748` is "pad read, phase dispatch on `ctx+6`,
pick/commit/resolve/**score loop**" and calls `FUN_801D388C` the "card
/presentation driver".

The score loop is the **arena hub's** (`FUN_801CF074` / `FUN_801D1184`, PROT
0977), not the match SM's - that mis-attribution is exactly what the browser
dome page acted on. The same sentence in `docs/subsystems/minigame-muscle-dome.md`
is corrected in this lane's commit; the site copy still carries it.

## 3. `crates/engine-core/src/world/frame_tick.rs` - `TurnOver` waits on Cross

`tick_muscle_dome`'s `MusclePhase::TurnOver` arm requires a `Cross` press
before `next_turn()`. Retail's turn boundary is automatic: `FUN_801E295C` writes
`ctx[6] = 0x14` at `0x801E67F0` and the round driver re-enters its own command
cluster `0x28` with no confirm. The native window draws no screen there (this
lane checked and left it correct), so it is a silent one-press stall rather than
a visible intermission - but it is the same cadence defect one layer down, and
the browser page now auto-advances. Suggested: drop the `confirm &&` guard so
the arm reads `if let Some(s) = self.muscle_dome.as_mut() { s.next_turn(); }`.

`report_muscle_leg` in the same file drains hub states `0x0A`..`0x0C` inline, so
the state a host can observe afterwards is `Fight` / `Settle`. That is what
`muscle_dome::leg_boundary_raises_interval` is written against; if the drain is
ever moved out, that predicate's doc comment needs re-checking.
