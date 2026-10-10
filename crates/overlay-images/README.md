# legaia-overlay-images

The code-overlay image formats, split out of `legaia-asset`: the readers
whose input is a MIPS overlay image rather than an asset container.
`legaia-asset` depends on this crate, never the other way round, and
re-exports every module at its old path (`legaia_asset::static_overlay`,
`legaia_asset::move_power`, ...), so downstream crates name the same paths
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
| `move_power` | The battle overlay's per-move power + behaviour table and its id map. See [`move-power.md`](../../docs/formats/move-power.md). |
| `menu_windows` | The menu overlay's window descriptor table. See [`field-menu.md`](../../docs/subsystems/field-menu.md). |
| `widget_script` | The window-script VM's bytecode programs in the menu overlay's data segment. See [`window-script.md`](../../docs/formats/window-script.md). |

## The split line

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
- [`crates/game-tables`](../game-tables/README.md) - the static SCUS tables,
  including the spell table whose id space `move_power` shares.
