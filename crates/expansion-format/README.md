# legaia-expansion-format

The expansion-pack format: a modder-authored bundle of new content for the
port - custom maps, battles and bosses, a level curve, quests and
challenges, dialogue, trading cards and game modes - parsed into one
`ExpansionPack`.

A pack carries no Sony bytes. Disc content is named by coordinate
(`refs::DiscRef`: a monster id, a scene name, a PROT entry and object index)
and resolved against the player's own disc at load time, the rule a
shareable language pack follows. The format reference is
[`docs/formats/expansion-pack.md`](../../docs/formats/expansion-pack.md).

| Module | Scope |
|---|---|
| `lib` | `ExpansionPack`, the `PackFiles` tree, `from_files` / `to_files`, the directory and bundle loaders |
| `manifest` | `pack.toml`, id and semver syntax |
| `refs` | `DiscRef` coordinates, their disc-free range checks, the `DiscResolver` trait |
| `bundle` | The deterministic `.lxpack` single-file archive |
| `validate` | The validator, the `SectionRules` extension trait, the no-disc-bytes rule |
| `battle`, `progression`, `quest`, `dialog`, `cards`, `modes`, `map` | The content sections |

The core API takes and returns bytes, so it builds for `wasm32`; only
`from_dir` / `read_files` touch the file system. The command line is
`asset expansion` in [`legaia-asset`](../asset/README.md).

A format crate: it names no engine crate. The runtime is
[`legaia-engine-expansion`](../engine-expansion/README.md).
