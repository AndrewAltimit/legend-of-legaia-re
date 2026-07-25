# Lane 7 handoff - stale `NOT WIRED` rows

All 18 rows of the `--live-audit` first section are closed. Nothing is left
pending a sibling lane's file, and no `scripts/ci/port-catalog.py` change is
proposed or needed.

Measured from the worktree (`load_rust_sources` is base-independent; only the
`dumped` column needs `ghidra/scripts/funcs/`):

```
before:  tagged NOT WIRED but analysed live : 18   disclosure gap : 0
after:   tagged NOT WIRED but analysed live :  0   disclosure gap : 0
```

## No tool change proposed

The task allowed for a row closable only by a tool edit. None was. Both residual
mechanisms - a coarse anchor, and a name collision the receiver gate cannot see -
are properties of *source*, and the source-side fix is strictly better than the
tool-side one:

- Moving a `//! PORT:` onto the function that implements the address makes the
  anchor precise for every consumer of the catalog, not just for this one audit
  question. A tool-side "read a module tag at item granularity" rule would need
  the tag to carry item information anyway, which is the same edit.
- The two collisions (`Rect12::to_le_bytes`, the duplicate free
  `countdown_frame`) were closed by renaming. Tightening the receiver gate to
  catch them would mean gating **unambiguous** names, which the gate deliberately
  does not do - `live-audit-triage.md` records that applying the gate
  unconditionally lost `801d7ea0`, the one genuine stale tag on the page. That
  is the failure direction the strict graph must not have.

If a future lane still wants the tool-side rule, the shape is: in
`compute_live.verdict`, for `kind == "module"`, restrict `module_scope` to the
`fn`s whose own tag block names the same address. That requires `collect_port_anchors`
to record, per file, the address set of each item tag - a real change, and it
buys nothing that the source edit does not already give.

## Two things a later refactor can silently undo

Both are named in-source, but they are exactly the edits a tidy-up would make:

1. `FootstepCadence::tick_cadence` (`crates/engine-audio/src/footstep.rs`) must
   not be renamed back to `tick`. `crates/engine-audio/src/lib.rs` re-exports
   the type *and* calls `spu.tick()`, so the receiver gate passes and the
   disclosure reads stale again.
2. `tutorial_countdown_frame` (`crates/engine-core/src/dance_tutorial.rs`) must
   not be renamed back to `countdown_frame`. The Baka round chrome has a live
   free function of that name, and free-function edges are never receiver-gated.

## One real gap this surfaced, not mine to close

`80053cb8` is now correctly reported wired - but only through the **browser**
Muscle Dome page (`crates/web-viewer/src/minigames_muscle.rs`). The native
battle path still seeds party battle stats in `engine-core`'s own
`seed_party_battle_stats` rather than delegating to
`battle_formulas::stat_init::init_party_battle_stats`, so the two can drift.
That is a `WIRE` candidate for whoever owns `engine-core`'s battle session; the
disclosure text on `equip_stat_bonuses` now says so instead of claiming the
kernel has no caller at all.

## Re-run the drift checker from the main checkout

`scripts/ci/check-port-tags.py` reads `ghidra/scripts/funcs/`, which a worktree
does not have, so it reported zero files here. Re-run
`python3 scripts/ci/check-port-tags.py --scan-all` from the main checkout after
this branch merges; the tag moves in `title_prim.rs` / `vram_rect_copy.rs` /
`world_map_overlay.rs` are the ones worth looking at.
