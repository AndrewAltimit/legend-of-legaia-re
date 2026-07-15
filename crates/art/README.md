# legaia-art

Tactical Arts data system: Action Constants, per-character art tables, Miracle Art / Super Art trigger logic, and a best-effort `ArtRecord` parser.

## Scope

- `ActionConstant` - battle action queue values `0x00–0x32` (Nothing, Item, Magic, Attack, Spirit, Escape, directional inputs, starters, per-character art constants).
- Per-character art name tables - `0x1B–0x32` resolves to a different art per character (Vahn / Noa / Gala). Slot ordering matches the on-disc Learned Art Constant table.
- `MiracleMatcher` - command-string → full action queue replacement. The 4 leading bytes of each replacement carry the on-disc MSB-set quirk, normalised here.
- `SuperMatcher` - find/replace pattern matcher applied to the **tail** of the action queue. Returns the longest match per character. `try_trigger_at_tail` matches the byte-exact queue; `trigger_by_art_sequence` matches a recognized art *ordering* against `SuperArt::art_sequence()` (the Find pattern projected to its art constants only), for the connector-abstracted live-submenu path.
- `recognize_art_sequence` - tokenizes a flat directional command string into the ordered named arts it performs (each identified by its own `ArtRecord::commands`, greedy longest-match, skipping unrecognized connector directions). The recognizer the live Arts submenu uses to detect a Super-Art chain.
- `ArtRecord` / `parse_record` - schema for the 40-field art binary record. The strict parser reads the leading command sequence + action constant + animation index; the rest is variable-width and surfaced via the `tail` bytes for downstream tooling.
- `arts_table::parse_from_scus` - decodes the SCUS arts-name table (`DAT_80075EC4`): per-character name + AP cost + command-input direction sequence, recovered from the menu's arrow-glyph display string. An independent, byte-exact source for each art's command (validates the best-effort PROT `0x05C4` parse and the curated gamedata AP column).
- `ArtsOracle` - queryable view over the decoded table (`by_name` / `by_command` / `by_character_index`). The ground-truth oracle the best-effort `parse_record` command-decode is contract-tested against, and the source the curated `legaia-gamedata` `directions` / `ap` columns are cross-validated against (disc-gated tests in `crates/art/tests/` and `crates/gamedata/tests/`).
- `arts_voice::ArtsVoiceTable::parse_from_scus` - decodes the arts-voice cue tables (`FUN_8004C140`): the per-character shout file (`clip_file` = `XA2`/`XA4`/`XA6.XA` for Vahn/Noa/Gala) and, per art **action constant**, the candidate voice-channel pool the retail cue picks from (range table `0x800781A4`, first/second-half candidate tables, `dur` table `0x80077A8C`). Capture-verified against a live PCSX-Redux trace; consumed by the site's arts viewer (`legaia-web-viewer::arts_view`). Distinct from the ordinary directional-attack grunt (`XA30.XA`) and the stereo Miracle fanfares (`XA3`/`XA5`).

The data tables (Action Constants, art names, Miracle/Super patterns) come from external reverse-engineering of RAM addresses `0x80160EFC` (Vahn), `0x80176998` (Noa), `0x8018BA54` (Gala) and PROT entry `0x05C4`.

See [`docs/formats/art-data.md`](../../docs/formats/art-data.md) for the byte-level layout, the multiplier encoding for power bytes, and full citations.

## CLI

```bash
art constants                                  # full ActionConstant table
art tables --character vahn                    # per-character art slots
art miracle vahn rdlulurdl                     # → Vahn's Craze trigger
art super vahn 1927 0F19 1F0E 1927             # → Tri-Somersault
art miracle-arts                               # list all 3 Miracle Arts
art super-arts vahn                            # list a character's Super Arts
```

`art parse <PATH>` runs the best-effort record decoder over a binary blob.
`art arts-table` decodes the SCUS arts-name table (name + AP + command
directions, e.g. `Burning Flare  RDLDL`); defaults to `extracted/SCUS_942.54`.

## Cross-crate integration

`engine-vm/src/battle_action.rs` imports the matchers and applies them via the `BattleActionHost::art_record` callback during action-queue resolution. See [`docs/subsystems/battle-action.md`](../../docs/subsystems/battle-action.md) for the resolution-order contract.
