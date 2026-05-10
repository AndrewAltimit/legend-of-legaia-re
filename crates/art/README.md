# legaia-art

Tactical Arts data system: Action Constants, per-character art tables, Miracle Art / Super Art trigger logic, and a best-effort `ArtRecord` parser.

## Scope

- `ActionConstant` - battle action queue values `0x00–0x32` (Nothing, Item, Magic, Attack, Spirit, Escape, directional inputs, starters, per-character art constants).
- Per-character art name tables - `0x1B–0x32` resolves to a different art per character (Vahn / Noa / Gala). Slot ordering matches the on-disc Learned Art Constant table.
- `MiracleMatcher` - command-string → full action queue replacement. The 4 leading bytes of each replacement carry the on-disc MSB-set quirk, normalised here.
- `SuperMatcher` - find/replace pattern matcher applied to the **tail** of the action queue. Returns the longest match per character.
- `ArtRecord` / `parse_record` - schema for the 40-field art binary record. The strict parser reads the leading command sequence + action constant + animation index; the rest is variable-width and surfaced via the `tail` bytes for downstream tooling.

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

## Cross-crate integration

`engine-vm/src/battle_action.rs` imports the matchers and applies them via the `BattleActionHost::art_record` callback during action-queue resolution. See [`docs/subsystems/battle-action.md`](../../docs/subsystems/battle-action.md) for the resolution-order contract.
