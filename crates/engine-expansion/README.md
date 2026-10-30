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
| `cutscene` | The cutscene sequencer (`CutsceneRun`): step tracks, waits, branches, skip, its camera pose and screen layer; `preview` plays one against a stand-in world for the workbench timeline. |
| `quest_save` | The save's expansion block. |
| `map` | The active pack map: zones, exits, trigger events, once flags, the save location. |
| `cards` | The trading-card plumbing: grant drain, battle reward sources, stakes, card notices and toasts, the card screen seat, the monster portrait cache. The cards themselves are `quest_save::ExpansionProgress::cards`. |
| `items` | Pack items at runtime: the compiled catalog, `ItemProgress` (the pack bag and worn gear, saved as tail record 4), the worn-gear stat / passive / set arithmetic, the per-fight `BattleAliases` over the five unnamed retail ids. |
| `arts` | Pack arts at runtime: queue-byte allotment (`0x33..=0x7F` per hero), the bank-walk extension rows, segment expansion and the chain pass, `ArtsProgress` (saved in tail record 5), the clips in play. |
| `spells` | Pack Seru at runtime: menu ids from `0xC0`, `SpellProgress` (levels and XP, tail record 5), the cast in flight, capture lookup, magnitude scaling. |
| `item_screen` | The pack item screen's state machine (shop Buy / Sell, bag Items / Gear with the equip compare) over an `ItemScreenHost`, and the model it draws from. |
| `modes` | `PackModeSeat`, the game-mode seat (instance picker, the running `ActiveMode`, the quit prompt, the results card) and the pending card rewards. |
| `entrance` | Retail-scene entrances into the pack's maps (`EntranceWatch`). |
| `journal` | The pause menu's Journal: content (`journal_view`) and cursor (`JournalSession`). |
| `toast` | Quest notices as one-line toasts (`notice_text`, `ToastQueue`). |
| `audio` | The soundscape kernels: pack track music ids (`0xE000 + n`), the music a map or zone plays, `AudioRuntime` (applied music, script pick, raised sounds, ambient levels, keyed retail cues), the positional law `ambient_level`. Presentation only. |
| `mode_stage` | `WorldStage`: a golf round or race played on a pack map, mapped onto the field each frame (leader pose and clip, balls, aim arrow, bodies, boost trails, camera shot, banner). Presentation only. |

Format and engine wiring: [`expansion-pack-battle.md`](../../docs/formats/expansion-pack-battle.md), [`expansion-pack-quests.md`](../../docs/formats/expansion-pack-quests.md).
