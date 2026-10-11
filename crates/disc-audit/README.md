# legaia-disc-audit

Tooling that measures the disc rather than parsing one format. The `asset`
binary ([`crates/asset`](../asset/README.md)) is its command-line front.

| Module | Covers |
|---|---|
| `byte_account` | Per PROT entry, which bytes a parser consumes and what shape the rest has. See [`byte-accounting.md`](../../docs/tooling/byte-accounting.md). |
| `categorize` | Bulk classification of PROT entries by trying every detector. |
| `inherited_tail` | Where an overlay image stops being its own content. |
| `tim_catalog`, `tim_deep_catalog`, `tim_labels` | The catalog of every standard TIM on the disc (raw and behind LZS) and its semantic labels. |
| `switch_tables`, `battle_jump_tables`, `field_probe_tables`, `other3_roster` | Overlay-resident jump tables, probe tables and the dev roster, read so the accounting can claim them. |
| `formation_census` | Disc-wide count of the formations each monster id appears in. |

## Layering

The top of the format stack. `byte_account` and `categorize` call nearly
every parser there is, so this crate depends on all of
`legaia-container-formats`, `legaia-scene-formats`, `legaia-minigame-formats`,
`legaia-ui-formats`, `legaia-battle-models`,
`legaia-game-tables` and `legaia-overlay-images`. None of those may depend on
it, and it must not depend on any engine or patcher crate. `legaia-asset`
re-exports each module at its old path.
