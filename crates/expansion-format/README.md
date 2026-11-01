# legaia-expansion-format

The expansion-pack format: a modder-authored bundle of new content for the
port - custom maps, battles and bosses, a level curve, quests and
challenges, dialogue, trading cards, game modes, items, arts and Seru magic - parsed into one
`ExpansionPack`.

A pack carries no Sony bytes. Disc content is named by coordinate
(`refs::DiscRef`: a monster id, a scene name, a PROT entry and object index)
and resolved against the player's own disc at load time, the rule a
shareable language pack follows. The format reference is
[`docs/formats/expansion-pack.md`](../../docs/formats/expansion-pack.md).

| Module | Scope |
|---|---|
| `lib` | `ExpansionPack`, the `PackFiles` tree, `from_files` / `to_files`, the directory and bundle loaders |
| `manifest` | `pack.toml`, id and semver syntax, the pack-format and engine gates, dependency entries |
| `version` | Semantic versions and cargo-style version ranges |
| `refs` | `DiscRef` coordinates, their disc-free range checks, the `DiscResolver` trait |
| `bundle` | The deterministic `.lxpack` single-file archive |
| `validate` | The validator, the `SectionRules` extension trait, the no-disc-bytes rule, the import trust checks |
| `xref` | The reference model: every place a pack id is defined, written, read or used, by kind, with its holder and gates; the rename with references |
| `flow` | Reachability from the entry points, the flow lints the validator runs, and the flow graph the workbench draws |
| `share` | The import gates a shared `.lxpack` passes, the library summary, the sources zip |
| `diff` | Two versions of a pack compared entry by entry; the semver bump and changelog draft |
| `battle`, `progression`, `quest`, `dialog`, `cards`, `cutscene`, `modes`, `items`, `arts`, `spells`, `map`, `terrain` | The content sections (`items` also holds `ItemRef`, the retail-or-pack item reference every section names items by; `arts` resolves a segment's retail art against `legaia_art`'s per-character tables) |
| `npc_postures` | The committed table (`data/npc_postures.toml`) of each borrowed NPC placement's clips by measured pose, which the map validator reads to warn of a seated clip with no seat ([NPC clip postures](../../docs/formats/expansion-pack-maps.md#npc-clip-postures)) |
| `map_rest` | Resting placement: `on` resolved onto a support's measured top, and the floating / off-the-top / overhang check over a caller's mesh shapes ([Resting placement](../../docs/formats/expansion-pack-maps.md#resting-placement)) |
| `mesh_catalog` | The committed borrowed-mesh catalog (`data/mesh_catalog.toml`): per base-scene mesh a label, kind, measured size and suggested `solid`, for the workbench and modders ([The mesh catalog](../../docs/formats/expansion-pack-maps.md#the-mesh-catalog)) |
| `audio`, `audio_clip` | `audio.toml` and map `[audio]` (`MusicRef`, `SoundRef`, tracks, sounds, zones, ambient sources), and the one WAV / Ogg Vorbis decoder and probe (`lewton` for Vorbis) the validator and both hosts run ([`expansion-pack-audio.md`](../../docs/formats/expansion-pack-audio.md)) |

The core API takes and returns bytes, so it builds for `wasm32`; only
`from_dir` / `read_files` touch the file system. The command line is
`asset expansion` in [`legaia-asset`](../asset/README.md).

A format crate: it names no engine crate. The runtime is
[`legaia-engine-expansion`](../engine-expansion/README.md).
