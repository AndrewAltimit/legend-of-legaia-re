# legaia-overlay-images

The code-overlay image formats, split out of `legaia-asset`: the readers
whose input is a MIPS overlay image rather than an asset container.
`legaia-asset` depends on this crate, never the other way round, and
re-exports every module at its old path (`legaia_asset::static_overlay`,
`legaia_asset::slot_b_module`, ...), so downstream crates name the same paths
they always did.

## What lives here

| Module | Covers |
|---|---|
| `mips_overlay` | Per-PROT MIPS-code-likelihood detection. See [`mips-overlay.md`](../../docs/formats/mips-overlay.md). |
| `overlay_ptr_table` | The sister detector for pointer-table-led overlay entries. See [`overlay-ptr-table.md`](../../docs/formats/overlay-ptr-table.md). |
| `static_overlay` | The static-overlay map and the extraction of each clean-copy runtime overlay at its statically-recovered base. See [`static-overlay-pipeline.md`](../../docs/tooling/static-overlay-pipeline.md). |
| `slot_b_module` | File layout of the slot-B cast / summon images: head table, code, spawn-record band, inherited tail. See [`slot-b-module-layout.md`](../../docs/formats/slot-b-module-layout.md). |
| `summon_overlay` | The spawn-call scan and part records of a slot-B module. See [`cast-module.md`](../../docs/subsystems/cast-module.md). |
| `cast_effect_pool` | The cast-module band indexed by PROT entry, each image resolved to its effect parts. |

## The split line

This crate holds the image-level formats: what an overlay image is, where it
loads, and how a slot-B module is laid out band by band (`summon_overlay` and
`cast_effect_pool` are that layout's spawn-record and effect-part bands, and
`slot_b_module` and `summon_overlay` call each other). A data table that
merely happens to be resident in an overlay - move power, the window
descriptors, the window scripts - lives in
[`legaia-game-tables`](../game-tables/README.md), which depends on this crate
for the overlay window bounds and the part-record reader.

Each of these takes an overlay image as a byte slice and names only its
siblings plus `legaia-bytes` and `legaia-lzs`. None reaches the asset
dispatcher, the DATA_FIELD stream, the pack and bundle formats or the scene
formats, which is what lets the cluster sit below `legaia-asset`.

What stays in `legaia-asset`: `inherited_tail` and `switch_tables`, which
read overlay images too but lean on the byte-accounting walk, and
`byte_account` itself, which consumes every reader here.

The static-overlay map this crate embeds stays at
`crates/asset/data/static-overlays.toml` - the path the analysis scripts and
the docs open by name.

## See also

- [`crates/asset`](../asset/README.md) - the format hub this was split from.
- [`crates/game-tables`](../game-tables/README.md) - the static data tables,
  the overlay-resident ones (`move_power`, `menu_windows`, `widget_script`)
  among them.
