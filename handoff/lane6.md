# Lane 6 handoff — the overworld walk

Branch work: `crates/engine-shell/tests/critical_path_replay.rs`,
`crates/engine-core/tests/world_map_axis_convention.rs`,
`scripts/replays/critical_path_baseline.toml`,
`docs/subsystems/world-map.md`, `site/_content/subsystems/world-map.html`.

## Headline: there was no axis inversion

The brief's premise — "X moves the right way and overshoots, Z moves the wrong
way" — was an artifact of the harness, not a defect in the engine.

At `map01` arrival the player walks **exactly** the direction the harness asks
for: 120 straight frames of world X− at 8 units/frame from a pad of `0x0010`
(engine `PadButton::Up`), which is what `world_map_camera_relative_bits(0,0,1)`
selects and what `advance_with_collision(0x8000)` performs. The remap
round-trips.

At frame ~121 a **region-keyed random encounter** fires and the world enters
`SceneMode::Battle`. In battle the player actor's `move_state` is the battle
*arena* transform, so `player_world()` began reporting `(0, -825)` — a
coordinate in a different space entirely. Every downstream number then lied in
the same direction at once: no progress → stall detector trips at 240 frames
(the battle takes ~570), the stall site reports an out-of-grid tile with all
four walls set, and the whole thing reads exactly like an inverted movement
axis. It is a mode confusion.

Fixed in the harness (`drain_battle`). Nothing in `engine-core` needed changing
for it.

## Retail's overworld axis convention (disassembly)

Now recorded in `docs/subsystems/world-map.md#overworld-axis-convention`.
Summary, so nobody re-derives it:

- `FUN_800467E8` (SCUS) is an **octant ring rotation**, not trigonometry:
  `pad & 0xF000` is looked up in the 8-entry table `DAT_800766FC`, the integer
  octant count at `gp + 0x2D8` is added, `& 7`, entry written back.
  Ring bytes read off the disc image:
  `1000, 3000, 2000, 6000, 4000, C000, 8000, 9000` — a compass turning from
  `+Z` toward `+X`. Count `0` is the identity.
- `FUN_801D01B0` step arms: `0x1000`→`actor[+0x18] += 2` (Z+),
  `0x4000`→Z−, `0x2000`→`actor[+0x14] += 2` (X+), `0x8000`→X−. Those are the
  raw PSX d-pad bit positions, so retail's unrotated overworld d-pad is
  Up = world Z+, Right = world X+.
- Same routine writes facing `actor[+0x26] = ((ring_index + 4) & 7) * 0x200`,
  i.e. retail heading `0` = world **Z−**. The engine's `render_26` puts `0` at
  Z+ and compensates at the animation-sector lookup
  (`(render_26 + 0x800) & 0xFFF`). Consistent, not a bug — but if anyone
  "fixes" the engine bearing to match retail, that `+0x800` bias has to go with
  it.

**The port's overworld frame is rotated relative to retail's, and that is
load-bearing.** Retail's yaw-0 walk camera looks down `+Z`;
`engine-render::window::world_map_camera_mvp` puts the eye on `+X` at azimuth
0. `world_map_camera_relative_bits` carries the compensating rotation. Player
experience matches retail; the world-axis assignment does not. Do not rotate
one half of the remap/camera pair alone.

## What now stops rung 4 (`map01` → `keikoku`)

Ratchet moved 2 → 3. Rung 4 does not clear, and this is the open item.

The overworld leg now genuinely walks: **~79 tiles** Manhattan from the
`(96, 25)` arrival, **10 random encounters** fought and survived, replanning
throughout. It stops at tile `(54, 62)` / world `(7012, 8098)`, beside the
`suimon` cluster. Two independent facts:

1. **A solid `.MAP` placed-object collider blocks the last step.**
   `FieldPropCollider { anchor: Some((54, 64)), center: (6976, 8240),
   moving_box: false, interact: false, solid: true }`. `field_dir_blocked`
   reports **no** wall in any direction there; `advance_with_collision`'s
   unconditional prop arm is what refuses. Installed by
   `SceneHost::install_field_prop_colliders` (`scene/host/scene_entry.rs`) from
   `Scene::field_object_placements`, with `solid = cflags & 3 == 0` and a ±80
   box (`FIELD_PROP_BOX_HALF`).
2. **No `keikoku` mouth is reachable at all.** A walkability flood from the
   arrival, walls only (no props, no portal hazards), gets these residuals in
   32-unit sub-cells to `map01`'s six `keikoku` portals:

   | portal tile | (64,68) | (77,69) | (81,82) | (81,83) | (53,93) | (53,94) |
   |---|---|---|---|---|---|---|
   | residual | **54** | 105 | 168 | 172 | 119 | 123 |

   54 sub-cells ≈ 13.5 tiles. An ASCII dump of the grid over tiles
   `x 40..104, z 20..100` is a **plausible landscape** — mountain ranges with
   corridors, including an open north–south corridor at `x = 64` running rows
   61→70 straight into the `(64, 68)` mouth. The flood reaches `(55, 63)` and
   is separated from that corridor by a wall band at `x = 56..57` around rows
   60..62.

So the two regions really are disconnected in the port's `map01` collision
inputs, and the question is which input is wrong. Ruled out already:

- **Not the mist walls.** `map01` `P2[34..36]` (`C1 = [0x482]`) do fire — the
  unseeded run reports 4 scripted sequences on the way. Seeding `0x482` (the
  ladder now does, alongside the town-gate pair) removes them and changes the
  stall **not at all**.
- **Not the encounter path** — fixed, 10 fought and survived.
- **Not the pad remap** — see above.

Candidates for whoever picks this up, roughly in order:

- The **prop colliders on an overworld scene**. Retail's `FUN_801CF754` does
  put placed objects in the collision candidate list, so their presence is not
  by itself wrong — but `solid` defaults to true for every placement with no
  `31 00`, and a ±80 box on overworld-scale scenery may be sealing corridors
  retail leaves open. Cheapest experiment: rerun the ladder with
  `field_prop_colliders` cleared on world-map scenes and see whether the flood
  connects.
- The **walkability-grid decode for a `map\d\d` scene**. `field_tile_is_wall`
  indexes `z >> 6` into the `0x12000`-byte `.MAP` block's `+0x4000..+0x8000`
  region. If an overworld map's grid has a different stride or origin from a
  town's, the landscape would still *look* plausible while the corridors landed
  in the wrong place.
- The possibility that the route legitimately goes **through `suimon`** (Sui
  Mon = floodgate; its four markers at `(55..58, 61..62)` carry two different
  entry coordinates, `dir 5` → `(68,44)` and `dir 2` → `(32,87)`, i.e. two
  sides of a pass-through). If so the spine leg is `map01 → suimon → map01 →
  keikoku` and rung 4 needs re-shaping, not the collision fixing.

Repro:

```
export LEGAIA_DISC_BIN=".../Legend of Legaia (USA).bin"
LEGAIA_CPR_DEBUG=1 cargo test -p legaia-engine-shell --release \
  --test critical_path_replay -- --nocapture critical_path_score
```

`LEGAIA_CPR_DEBUG` prints the per-candidate portal residuals and dumps every
prop collider within 400 units of a stall.

## Out-of-scope observations for other lanes

- **`crates/engine-core/src/world/field_movement.rs`** — nothing needed
  changing; `advance_with_collision`'s bit→axis table matches `FUN_801D01B0`
  exactly and is now pinned by
  `crates/engine-core/tests/world_map_axis_convention.rs`. Two mutation checks
  confirm the new test is non-vacuous (inverting the remap's world Z, and
  inverting the mover's `0x1000` arm, each fail it).
- **Collision-flag drift across hosts.** `play-window`
  (`window/run.rs:400`) and the browser play page (`web-viewer/runtime.rs:538`)
  both set `leading_edge_wall_probes = true` **and**
  `solid_field_npcs = true`; a bare `World` defaults both false, so every test
  that builds one is scoring a third collision model no player meets. The
  ladder now sets both, so it scores the shipped model. Worth a sweep of the
  other `World::default()`-based locomotion oracles for the same gap —
  `grep -rn "leading_edge_wall_probes\|solid_field_npcs" crates/*/tests
  crates/*/src/world/tests` shows which ones opt in and which are silently on
  the third model.

  Note the ordering constraint if anyone touches this: solid NPCs are only
  survivable because `plan_path` now consults `field_actor_dir_blocked` too.
  With solid NPCs and a wall-only planner, a Rim Elm townsperson parks in the
  route and rung 2 stalls at tile `(25, 22)` with `actors X+`. Turning the
  flag on without the planner change re-breaks the ladder at rung 2.
