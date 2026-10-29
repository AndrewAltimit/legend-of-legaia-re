# legaia-expansion-format

The expansion-pack format: a modder-authored bundle of new content for the
port - custom maps, battles and bosses, a level curve, quests and
challenges, dialogue, trading cards and game modes - parsed into one
`ExpansionPack`.

A pack carries no Sony bytes. Disc content is named by coordinate
(`refs::DiscRef`: a monster id, a scene name, a PROT entry and object index)
and resolved against the player's own disc at load time, the rule a
shareable language pack follows. The directory layout and the `.lxpack`
single-file bundle are described in the crate docs; the format reference is
[`docs/formats/expansion-pack.md`](../../docs/formats/expansion-pack.md).

A format crate: it names no engine crate. The runtime is
[`legaia-engine-expansion`](../engine-expansion/README.md).
