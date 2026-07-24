# Lane L2 (dance / fishing / slot machine) - handoff

## 1. URGENT: a shared-stash collision damaged another lane's work (recovered)

`git stash` is a **repo-global** stack shared by every worktree. Lane L2 ran
`git stash push` / `git stash pop` around a baseline test check; another lane
pushed a stash inside that window, so L2's `pop` restored the *other* lane's
changes into L2's worktree and left L2's own on the stack.

Both sides were recovered from the dangling stash commits:

- L2's own work: `0493a3b7aea3ddf5e694931ab8879cb4a98bd0ef` (re-applied, committed).
- The other lane's work (`worktree-agent-a06d4881a31fb277c`, on top of
  `92419e91 engine-vm: port the minigame-hub system-actor handler family ...`):
  commit `d47310232ee7f5974637dfc709d86061aef35ab4`, **put back on the stash
  stack** with the message `recovered: WIP on worktree-agent-a06d4881a31fb277c
  (popped into the wrong worktree by lane L2; restored)`.

That lane should `git stash list`, confirm the entry, and `git stash pop` it
back into *its own* worktree. If the entry is gone again, the content is still
reachable at `d4731023` (`git checkout d4731023 -- <paths>`; untracked files
are in `d4731023^3`).

**No lane should use `git stash` while the wave is running.**

## 2. Pre-existing test failure on the wave's base commit

`cargo test -p legaia-engine-core` fails at base `d9d0c0b7`:

```
world::battle::casting::capture_bypass_tests::the_bypass_wrapper_weights_the_defence_terms_more_heavily
```

L2 has **zero** diff under `crates/engine-core/src/world/`, and the test's file
was last touched by `d9d0c0b7` itself. Whoever owns the battle-casting slice
should look; the coordinator's consolidated gate will trip on it.

## 3. Wiring blockers L2 could not close (hosts live outside L2's path scope)

L2 owns `engine-core/{dance*,fishing*,slot_machine*}`, `asset/minigame_slot_scene.rs`,
`engine-ui/ui_fishing*.rs`. Every remaining orphan in that slice needs a *host*
edit in a file L2 does not own. Concretely:

| Orphan group | Host that would close it | Where |
|---|---|---|
| `minigame_slot_scene::reel_y` / `reel_z` / `reel_shade` | a reel vertex consumer | `crates/web-viewer/src/minigames.rs` + `site/_content/minigames.html`, or a native cabinet pass in `engine-render` |
| `minigame_slot_scene::compose_marquee` / `clear_dots` / `place_message` / `compose_marquee_frame` | the same cabinet host | same |
| `slot_machine::payline_prims` | a 3D projection + OT pass | `engine-render` / web-viewer |
| `dance::*` HUD glyph-U + effect-spawn kernels | a dance sprite page + widget-quad emitter | `crates/web-viewer/src/minigames_dance.rs` (it already loads the overlay's HUD widget table + face rigs) |
| `fishing_chrome::centred_panel` | a fishing sub-screen builder | `crates/engine-ui/src/ui_fishing.rs` - but the kernel must **move** there, since `engine-ui` cannot depend on `engine-core` |
| `fishing_chrome::splash_burst` / `ripple_spawn`, `dance::step_mark_effect_spawn` | a minigame effect-part pool | `engine-vm::effect_vm` + a host that maps overlay sprite ids into it |

One blocker L2 *did* remove from its own side:
`legaia_asset::minigame_slot_scene::parse_paylines` now takes the five payline
segments off the raw overlay with no decoded art plane, so
`slot_machine::payline_prims` no longer needs a caller to build the page-3
plane. Only the render sink is left.

## 4. A cross-lane RE fact worth propagating

The per-scene floor buffer byte at `*(_DAT_1F8003EC) + 0x4000` (row pitch
`0x80`) is **two fields in one nibble pair**:

- **low** nibble = index into a 16-entry terrain height ramp
  (`ramp[i] = i * 0x20`, installed by `FUN_801D2A10` at scratchpad
  `0x1F80035C`), read by `FUN_801D6028` / `FUN_801D3A2C` / `FUN_801D6BBC` /
  `FUN_801D2A10`;
- **high** nibble = the four sub-cell wall bits, read by the field collision
  probe `FUN_801CFE4C` (`>> 4 & quadrant`) and by `FUN_801D7030`.

`docs/subsystems/field-locomotion.md` describes `+0x4000` only as "4 sub-cell
wall bits per 128-unit tile", which is true of the high nibble and silent about
the low one. The owner of that page may want to add the height half; L2 did not
edit it (out of scope) but has documented the split on
`docs/subsystems/minigame-fishing.md` and `minigame-dance.md`.
