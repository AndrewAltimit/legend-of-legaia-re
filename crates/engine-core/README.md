# legaia-engine-core

Engine-agnostic primitives for the clean-room engine reimplementation:
virtual filesystem, asset cache, frame timing.

No `wgpu` / windowing / audio dependencies — the asset crates (Track 1)
talk to this layer, and the render and audio crates read from it.

## What it provides

### `Vfs` trait

Source of asset bytes. Two backends:

- `DirVfs` — filesystem-backed, rooted at a directory. Used in
  development against the output of `legaia-extract`.
- (Planned) `DiscVfs` — reads directly from a disc image, so end users
  don't need to extract anything ahead of time.

Both yield raw bytes addressed by a logical name (e.g.
`"prot/0123_some_entry.bin"`). The asset crates above this layer turn
bytes into typed structures.

### Asset cache

A bounded in-memory cache keyed by Vfs name. Avoids re-decoding the same
TIM/TMD/VAB on every frame when an actor is referenced repeatedly.

### Frame timing

`FrameClock` — fixed-step driver that targets the PSX's nominal NTSC
60 Hz. Returns the number of logical ticks elapsed since the last call,
so the host can drive the script VMs deterministically regardless of
render rate.

### Composite `World`

`world::World` ties together the actor, move, effect, field, and battle
VMs. One actor table (default capacity 64) is shared across all four
script VMs; the `Host` traits are implemented by routing through this
struct. `World::tick` runs:

1. Effect pool tick (every frame, every mode).
2. Per-actor move-VM tick — only for active actors with bytecode loaded
   via `set_move_bytecode`.
3. Mode-specific top-level VM:
   - `SceneMode::Battle` → battle-action state machine step.
   - `SceneMode::Field` / `SceneMode::Cutscene` → field-VM step.
   - `SceneMode::Title` → no further VM.

Engines that want a different storage layout (ECS, custom parallelism)
implement the per-VM `Host` traits themselves; `World` is the default.

### Battle helpers

- `art_strike` — translates `ArtStrikeInfo` into an `ArtStrikeOutcome`
  (HP delta, status, scheduled SFX cues) the world drains into its
  battle event queue.
- `ap_gauge` — per-character Action-Point gauge driving Tactical Arts
  command input. Charges +5 on Spirit-press, refills per turn.
- `battle_stats` — equipment-aware stat aggregator (clean-room port of
  `FUN_80042558`). Sums per-item modifiers, ORs ability bits, folds
  status-effect modifiers (Burned -ATK, Confused halves accuracy,
  Asleep / Stunned / Petrified zero evasion, Silenced / Petrified
  block Magic).
- `items` — typed inventory item-effect catalog. `apply_effect`
  resolves an `ItemEffect` against a `TargetSnapshot` to produce an
  `ItemOutcome` engines fold into world state.

## See also

- [`docs/subsystems/engine.md`](../../docs/subsystems/engine.md) — the
  clean-room boundary and architecture for the engine track.
