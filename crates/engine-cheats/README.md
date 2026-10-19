# legaia-engine-cheats

Cheats applied to a running `World`. Two halves, both host-facing:

- **`cheats`** - the one player cheat list both play hosts expose
  (`PlayerCheat`, `PLAYER_CHEAT_LIST`): the native window's `--cheat-*` flags
  and `F6` / `F7`, the browser play page's Cheats panel. Each cheat is a
  method of the `WorldCheats` extension trait on `World`, and each goes
  through the engine's own rules rather than poking a record: levels through
  the party's level-up tracker (lowering rebuilds from the New Game
  template), Seru magic through retail's learn edge, arts through the ordered
  learned-art insert, items through the bag's add helper, gold and coins to
  the purses' ceilings. `apply_cheat` returns the one-line outcome both hosts
  show.
- **`cheat_applier`** - GameShark / Pro-Action-Replay codes (parsed by
  [`legaia-cheats`](../cheats/README.md)) resolved through the
  `engine-system::ram_map` registry onto the engine fields they target, for
  `play-window --cheat-file`; codes that land in no known cell are reported
  as unmapped.

## Place in the layering

The crate sits directly above `legaia-engine-core`. It writes a `World` the
host already owns and nothing in the simulation calls it, so a cheat change
rebuilds only the hosts. The `World` steps the cheats reuse
(`rebuild_live_hp_mp`, `seed_party_battle_stats`, `party_roster_slot`) are
`pub` on `World`.

Call sites bring the trait into scope:

```rust
use legaia_engine_cheats::cheats::{PlayerCheat, WorldCheats};
let line = world.apply_cheat(&PlayerCheat::MaxAp, None);
```

The pinned RAM offsets the code applier relies on are documented in
[`docs/reference/cheats.md`](../../docs/reference/cheats.md).
