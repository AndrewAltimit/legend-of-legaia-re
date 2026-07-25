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

---

# Second pass - the eight unowned field-band rows

**Seven of eight ported. `801d4a60` is not, and is handed back mapped.**

## Dump selection came first, and it mattered

All eight resolve cleanly only in the **live-RAM field captures**
(`overlay_cutscene_dialogue_*`, cross-checked against
`overlay_cutscene_mapview_*` / `overlay_world_map_*` / `overlay_dialog_*` -
five independent captures, identical sizes). The `overlay_0897_*` static-base
dumps are unusable for **four** of the eight, in three distinct ways:

| Addr | `overlay_0897` dump says | Reality |
|---|---|---|
| `801d9c3c` | `entry=801d9c0c`, 73 insn | name-mismatch, `-0x30` |
| `801de478` | `entry=801de468`, 35 insn | name-mismatch, `-0x10` |
| `801d84b4` | `entry=801d8308`, 267 insn | name-mismatch, `-0x1AC` |
| `801ddc20` | **0 instructions, 1 byte** | corpus gap |

Both name-mismatch deltas are negative, matching Lane 1's finding that all 142
are. So the warning was not theoretical for this set - it decided the dump of
record for half of it.

## Ported (7)

| Addr | What it is | Module |
|---|---|---|
| `801d7518` | Scene-transition **teardown sweep** - retires actors by handler address, stamps the transition bit, reallocates side buffers, reseeds CLUT-walk accumulators | `field_actor_kernels` |
| `801ddc20` | Per-actor **colour tween** - delay / ramp / hold on a packed RGB triple | `field_actor_kernels` |
| `801e6984` | op-`0x49` submode **list-panel** row layout + ink selection | `field_submode` |
| `801da390` | Field camera **yaw easing** | `camera_ease` |
| `801d9c3c` | Submode **open**: context reset + driver-actor spawn | `field_submode` |
| `801de478` | Fixed-template **scene-actor spawn** + state-byte seed | `field_submode` |
| `801d84b4` | **CARD mode request** leaf | `field_submode` |

All seven disclosed `NOT WIRED`; the missing input is the same for most of
them and is worth stating once: **the engine's actors carry no `+0x0C`
per-frame handler address.** `801d7518` retires by handler identity and
`801d9c3c` searches by it, so neither can run against typed actor kinds.
That is one plumbing change, and it would unblock both.

### Two cross-checks that came out of the bytes

- `801d7518`'s second retire test compares the handler against `0x801DDC20` -
  the entry of the colour tween in the same commit. Decoding either predicted
  a field of the other; neither reading was fitted to the other.
- `801d84b4` writes master game mode `0x16`, which is exactly the mode the
  field initialiser's BGM wait barrier bails out on
  (`mode_entry_init::FIELD_BGM_WAIT_ABORT_MODE`, ported in the first pass).
  The two are the same gate seen from each end, and there is a unit test
  asserting they agree.

### On `801d84b4` and the ignore list

It is a **7**-instruction leaf, not 6, and it has **no `jal`** - two stores and
`jr ra` with the second store in the delay slot. The port comment records why
the old "PADDING" ignore reason was wrong for field(897) even though it holds
for the minigame images at the same VA.

## Not ported: `801d4a60` - and why

It is **not** the "scripted actor-approach state machine" the docs describe. It
is a **38-state jump-table dispatcher** on the actor's `+0x54`:

- bound `sltiu v1,0x26`, table at `0x801CE960` (`0x801D0000 - 0x16A0`), one
  word per state, dispatched by `jr v0` - an out-of-range state falls to the
  epilogue;
- prologue snapshots the player transform (`+0x14`/`+0x18` and `+0x24`/`+0x28`)
  into two stack vectors and biases the snapshot Y by `-0x40`;
- early states set story flag `0x17` / clear `0x18`, swap the BGM slot to
  `0x7F3`, fire SFX `0x200`, and stage move-VM parts from `0x801F2658` via
  `FUN_80021B04` once per `_DAT_1F800393` tick, accumulating in `+0x9E`.

The existing description covers roughly one state. I corrected
`field-locomotion.md` to say what the function actually is and to warn that
`overlay_0897_801d4a60.txt` is **short** - 690 instructions where five field
captures agree on 756.

I stopped rather than port it because 38 arms is more than remained in this
sitting, and a partially-understood state machine ported as a skeleton would be
a paraphrase - the thing this wave is auditing for. A token
`STATE_COUNT = 38` constant would have moved a number without porting a
function. The state space is now mapped, so the next lane starts from the
table rather than from the address.
