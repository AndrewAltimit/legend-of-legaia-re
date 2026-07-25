# Lane 4 - field / scene-band port worklist rows

Six `REAL` rows, all ported into `crates/engine-core`. One wired, five disclosed
`NOT WIRED` with a specific missing input each.

## Per-address

| Addr | What it is | Port | Wiring |
|---|---|---|---|
| `801d6704` | Field / town scene init ("MAIN_INIT") | `mode_entry_init` | **wired** - `SceneHost::enter_field_scene` seats the player through `field_spawn` |
| `801d9d3c` | Enemy target-selection-menu builder (battle overlay) | `target_picker::enemy_menu_rows` / `layout_enemy_menu_rows` | **wired** (label half) - `BattleSession::open_target_picker{,_mut}` rebuild the rows |
| `801dea50` | Battle action effect-script stepper | `action_effect_script` | disclosed |
| `801cfa48` | Effect-ribbon geometry emitter (render mode 4, `0x2000` arm) | `effect_ribbon` | disclosed |
| `801cf00c` | Baka Fighter duel-arena overlay init | `mode_entry_init::duel_overlay_init` | disclosed |
| `801d7b50` | Sub-area window-rebuild placed-object sweep | `field_regions::window_rebuild_spawns` | disclosed |

## What a sibling lane would have to edit to close the disclosures

Each is a one-caller change outside this file scope.

- **`801cfa48` (`effect_ribbon`)** needs `engine-render` (or `engine-ui`) to ask
  for ribbon geometry when it draws a battle effect. `build_ribbon` is pure and
  takes its RNG and its two `i16` LUTs as parameters precisely so the consumer
  can live in the render crate. The retail caller is the render dispatcher
  `FUN_8001ADA4` case 4, selected off `actor[+0x9E] & 0x2000`, so the engine
  needs an actor render-mode channel first.
- **`801dea50` (`action_effect_script`)** needs the battle-action path to carry
  the disc effect-script block for the active move. The block is reachable -
  `legaia_asset::move_power` already parses the record the terminator installs -
  but `engine-core`'s action path drives typed art strikes and has no
  `ctx[+0x1014]` slot, no per-target `+0x1144` homing block and no actor
  `+0x1F5` cursor.
- **`801cf00c` (`duel_overlay_init`)** needs a duel **overlay-entry** host.
  `baka_fighter.rs` (Lane 5's file) starts from a match state, not an overlay
  load, so there is nowhere in scope to call an initialiser from. Its two values
  that already have engine mirrors (`round_win_target = 2`, two fighter slots)
  agree with the rules engine's best-of-three.
- **`801d7b50` (`window_rebuild_spawns`)** needs the field host to hold the
  `.MAP` image resident for the scene's lifetime and to re-plan on every
  region-box change. This is the same missing buffer that leaves
  `field_regions::refresh_object_grid_marks` unwired. `field_env` already
  honours the sweep's `0x400` ownership gate, so nothing is *visually* missing
  today - what is missing is the windowed re-plan.
- **`801d9d3c` layout half** needs a per-monster **projected screen X** (the
  battle actor's `+0x34`). `engine-core` is renderer-free, so the wired
  `BattleSession::enemy_menu_rows` leaves the accumulator at `0` and the caller
  supplies positions to `layout_enemy_menu_rows`.

## Doc corrections made (all disassembly-grounded)

1. **`field-locomotion.md` § Spawn position** - the sub-tile remainders of the
   saved transition coords were described as producing the warp landing. They
   are computed under `_DAT_8007B8B8 == 2` and consumed under
   `_DAT_8007B8B8 == 0`; the gates are mutually exclusive, so they never reach
   an actor write. Cold spawn is exactly `(0xA40, 0, 0xA40)`; the warp landing
   comes from the `_DAT_8007BACC` window branch. The two entry arms also differ
   structurally (allocate vs. seven-list sweep), which the page did not say.
2. **`overlay_0897_801d6704.txt` is an incomplete dump.** Correctly based
   (`0x801CE818 + 0x7EEC`) but it jumps `0x801d71b4 -> 0x801d72d4`, dropping the
   two-part-BGM arm, and has no `jr ra`. The base-`0x801C0000` live-RAM captures
   agree across all 901 instructions. `field-locomotion.md` cited the incomplete
   one as provenance; both it and `asset-loader.md` now point at the captures.
3. **`script-vm.md`** - `0x801D9D3C` is two functions. The row there is the
   field(897) fragment; the battle(898) occupant is the real 388-instruction
   entry (four dumps agree byte for byte). Added the two structural facts that
   invite wrong readings: the dedup is *positional* (`A A B A` gives three rows,
   not two) and the row suffix **overwrites** the label's last character rather
   than appending, so labels never grow.
4. **`field-locomotion.md` § field-VM actor handlers** - `FUN_801dfb10`
   reattributed to `FUN_801EE328` per the coordinator's Lane 2 finding, verified
   independently against the bytes: identical prologues under the `+0xE818`
   re-key, the mis-based slice truncating at 133 instructions where the based
   dump runs 171 to `jr ra`, and `+0x1FB10` already recorded as `FUN_801EE328`'s
   file offset in `functions/world-map.md`. The behaviour survives but is
   reframed: it is the dev-menu "ON RULA," MAP CHANGE warp applier's rise-up
   animation, not a player-turn cutscene, and the threshold spawns a flash quad
   rather than starting a fade. Full 5-state table added.

5. **`seru_stats.rs` / `levelup/observation.rs`** - the battle-actor `+0x74`
   stamp is `0x00808080`, not `0x80808080`. Both comments quoted the correct
   instruction pair (`lui v0,0x80` + `ori v0,v0,0x8080`) and then stated a value
   that pair cannot produce - `0x80808080` needs `lui v0,0x8080`. Re-read
   `ghidra/scripts/funcs/800480d8.txt` directly: two sites (`0x80048238`,
   `0x800482d4`) both feeding the single `sw v0,0x74(s0)` at `0x800482dc`, gated
   on the record byte `+0x21C == 2`. Reframed as what it is - the actor's
   **colour** word getting a 24-bit RGB mid-grey stamp - which also explains the
   zero top byte: `+0x74`'s high byte is the flag half (the placed-object window
   sweep ORs `0x40000000` / `0x10000000` into exactly that word).

   **Not a live defect.** A repo-wide grep of `crates/engine-core` for
   `80808080` returns only those two comment sites; no code compares against or
   writes the constant, so nothing depended on the wrong width.

6. **A false claim of my own, caught by its own test.** The first draft of
   `effect_ribbon::damp_wander`'s doc said the wander accumulator "never damps
   all the way out from the negative side in one roll". It does: both shifts are
   arithmetic (floor), so `1..=3` from above and `-12..=-1` from below fold to
   exactly `0`. The *implementation* was faithful to `0x801cfeec..0x801cff44`
   throughout - only the prose and the assertion derived from it were wrong.
   Recorded here because it is the same failure mode this wave is auditing for,
   just caught before it reached a committed doc.

## Note back to the coordinator

- `801d4a60` is **not** one of this lane's rows (the six are listed above), so
  the `801d4a80` correction was not acted on here. FWIW every `801D4A80`
  reference now in the repo (`docs/reference/functions/menus.md`,
  `crates/engine-ui/src/ui_menu_window_painters.rs`) is keyed to the **menu**
  overlay's window-34 content renderer and is already ported there. That is
  consistent with the field(897) occupant being interior to `801d4a60` - the two
  are a VA alias across overlays, not a contradiction. Whoever owns
  `phantom-print-index.md` should make the image explicit on its
  `0x801C6268 -> 0x801D4A80` row.
- `lib.rs` gained exactly one contiguous block under `// --- lane 4 ---`:
  `action_effect_script`, `effect_ribbon`, `mode_entry_init`.
