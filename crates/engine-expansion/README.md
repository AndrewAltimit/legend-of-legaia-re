# legaia-engine-expansion

The `World`-free runtime of an expansion pack
([`legaia-expansion-format`](../expansion-format/README.md)): kernels that
apply a pack's monster and formation overrides, level curve, rewards and
quests to plain state. `engine-core` holds one `ExpansionState` on the
`World` and calls the kernels from its scene-entry, encounter and
battle-spoils steps. With no pack loaded every kernel is the identity.

| Module | Scope |
|---|---|
| `battle` | `battle.toml` compiled to engine ids (`CompiledBattle`), derived-monster and scaling kernels, the per-fight catalog swap plan, the weighted encounter pick, `BattleRuntime`. |
| `progression` | XP curve tables, EXP / gold multipliers, enemy level scaling, level-up rewards, `ProgressionRuntime`. |
| `quest` | The deterministic quest tracker over `QuestEvent`s. |
| `quest_challenge` | Challenge rule checks and scores. |
| `flags` | The pack's own flags and counters. |
| `dialog` | Pack lines compiled for the field VM's inline dialogue runner. |
| `quest_save` | The save's expansion block. |
| `map` | The active pack map: zones, exits, trigger events, once flags, the save location. |
| `cards` | The trading-card plumbing: grant drain, battle reward sources, stakes, card notices and toasts, the card screen seat, the monster portrait cache. The cards themselves are `quest_save::ExpansionProgress::cards`. |
| `modes` | `PackModeSeat`, the game-mode seat (instance picker, the running `ActiveMode`, the quit prompt, the results card) and the pending card rewards. |

Format and engine wiring: [`expansion-pack-battle.md`](../../docs/formats/expansion-pack-battle.md), [`expansion-pack-quests.md`](../../docs/formats/expansion-pack-quests.md).
