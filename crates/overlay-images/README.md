# legaia-overlay-images

Code images: how a PROT entry is recognised as MIPS code, which entry each
loader picks and where it loads, and how a slot-B cast / summon module image
is laid out.

## The rule

This crate describes **the image**; [`legaia-game-tables`](../game-tables/README.md)
decodes **the data tables found in an image**, whether that image is
`SCUS_942.54` or an overlay. The dependency runs one way: `legaia-game-tables`
takes an overlay's PROT index, load base and window bounds from here, and
nothing here names a table. A band format that is part of the slot-B module
layout stays here even though it yields records, because the layout and the
band reader call each other.

## What lives here

| Module | Covers |
|---|---|
| `mips_overlay` | Per-PROT MIPS-code-likelihood detection. See [`mips-overlay.md`](../../docs/formats/mips-overlay.md). |
| `overlay_ptr_table` | The sister detector for pointer-table-led overlay entries. See [`overlay-ptr-table.md`](../../docs/formats/overlay-ptr-table.md). |
| `static_overlay` | The static-overlay map (which PROT entry loads at which base), the named slot-A images, base recovery, and extraction of each clean-copy overlay. See [`static-overlay-pipeline.md`](../../docs/tooling/static-overlay-pipeline.md). |
| `boot_overlay` | The loaders' index arithmetic: overlay parameter to PROT entry, the slot-B default, the effect side-band choice. See [`boot.md`](../../docs/subsystems/boot.md). |
| `slot_b_module` | File layout of the slot-B cast / summon images: head table, code, spawn-record band, inherited tail. See [`slot-b-module-layout.md`](../../docs/formats/slot-b-module-layout.md). |
| `summon_overlay` | The spawn-record band of a slot-B module: the spawn-call scan and the part records it names. See [`cast-module.md`](../../docs/subsystems/cast-module.md). |
| `cast_effect_pool` | The slot-B band indexed by PROT entry (action id or capture sub-id to module), each image resolved to its spawn records. |

## Layering

Depends on `legaia-bytes` and `legaia-lzs` only (its disc-gated tests also
read through `legaia-prot` and `legaia-container-formats`). It must not depend on
`legaia-game-tables`, the container layer, or any scene, UI, minigame or
actor-model format crate. Above it, [`legaia-disc-audit`](../disc-audit/README.md)
holds `inherited_tail`, `switch_tables` and `byte_account`, which read
overlay images too but lean on the byte-accounting walk.

The static-overlay map this crate embeds stays at
`crates/asset/data/static-overlays.toml`, the path the analysis scripts and
the docs open by name.
