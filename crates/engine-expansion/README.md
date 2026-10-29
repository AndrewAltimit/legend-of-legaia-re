# legaia-engine-expansion

The `World`-free runtime of an expansion pack
([`legaia-expansion-format`](../expansion-format/README.md)): kernels that
apply a pack's monster and formation overrides, level curve, rewards and
quests to plain state. `engine-core` holds one `ExpansionState` on the
`World` and calls the kernels from its scene-entry, encounter and
battle-spoils steps. With no pack loaded every kernel is the identity.

<<<<<<< HEAD
| Module | Scope |
|---|---|
| `battle` | `battle.toml` compiled to engine ids (`CompiledBattle`), derived-monster and scaling kernels, the per-fight catalog swap plan, the weighted encounter pick, `BattleRuntime`. |
| `progression` | XP curve tables, EXP / gold multipliers, enemy level scaling, level-up rewards, `ProgressionRuntime`. |

Format and engine wiring: [`expansion-pack-battle.md`](../../docs/formats/expansion-pack-battle.md).
=======
Quests, challenges and pack dialogue: `quest` (the deterministic quest
tracker over `QuestEvent`s), `quest_challenge` (challenge rule checks and
scores), `flags` (the pack's own flags and counters), `dialog` (pack lines
compiled for the field VM's inline dialogue runner) and `quest_save` (the
save's expansion block). Reference:
[`expansion-pack-quests.md`](../../docs/formats/expansion-pack-quests.md).
>>>>>>> cba62b2b1 (docs: expansion-pack quests, challenges, dialogue and the save block)
