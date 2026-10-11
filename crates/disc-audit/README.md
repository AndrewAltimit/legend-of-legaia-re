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
`legaia-ui-formats`, `legaia-actor-formats`,
`legaia-game-tables` and `legaia-overlay-images`. None of those may depend on
it, and it must not depend on any engine or patcher crate.

## Module reference

Entry points, CLI subcommands (`asset ...` is the binary in
[`crates/asset`](../asset/README.md)) and the doc page each module is specified on.

| Module | Covers |
|---|---|
| `categorize` | Dispatcher - runs every detector and tags the entry's `Class`. |
| `tim_catalog` | Flat strict-validated TIM inventory over the whole `PROT.DAT` image (catches the unindexed-gap TIMs `tim_scan` can't); maps each to its owning entry + offset; reproduces an external reference decoder's TIM set item-for-item. |
| `tim_deep_catalog` | Separate tier: strict-validated TIMs recovered from inside LZS-compressed sections, keyed by `(entry, LZS section, offset-in-section)`. |
| `tim_labels` | Curated semantic labels for cataloged TIMs (raw + deep), keyed by content fingerprint: coarse visual categories + precise reverse-engineered pins for the boot/title/menu textures. Our own annotations, not asset bytes. |
| `battle_jump_tables` | PROT 0898's `switch` jump tables and head string pool, bound to the instructions that read them. |
| `field_probe_tables` | The field overlay's three probe-offset tables, bound to their readers. |
| `switch_tables` | `switch` jump tables in any overlay image, found from their dispatch. |
| `inherited_tail` | Where an overlay image stops being its own content (the donor bytes at the end of its extent). |
| `other3_roster` | The `OTHER3` dev module's selection roster (PROT 0974). |

### Byte accounting (`byte_account`)

Sibling of `categorize`, one entry at a time. `categorize` answers *what format
this entry is*; `byte_account` answers *how much of it any parser here actually
consumes*. It runs the walkers that apply to the entry's class, emits
`[start, end, owner]` claims, merges them, and classifies every uncovered run by
shape (zero padding, repeated fill, ASCII pool, pointer table, plausible MIPS,
low / high entropy). Compressed spans are decoded and accounted in a nested
pass, and claims found by a magic sweep over the residue are tagged `scan` so
they never inflate the structural figure.

For an overlay code image the "parser" is the Ghidra dump corpus. The module
re-implements `dump_header.py`'s header parse in Rust and resolves slot-A VA
aliasing from the bytes: it re-encodes a dump's first printed instructions and
compares them to the image at the mapped offset, so an extent that belongs to a
sibling overlay is dropped rather than credited.

CLI `asset account`; full reference
[`docs/tooling/byte-accounting.md`](../../docs/tooling/byte-accounting.md).

### Structural detectors (for `categorize`)

The dispatcher `categorize` runs every detector in the format crates and tags
each entry's `Class`. Detector coverage and provenance are tracked in
[`docs/formats/scene-bundles.md`](../../docs/formats/scene-bundles.md).

### `formation_census`

Disc-wide census of every scene MAN's formation rows (bundled LZS MANs and the
raw type-`0x03` streaming chunks): per monster id, rows whose header byte is
non-zero (the scripted / boss rows that raise `DAT_8007BD60` bit `0x80`), rows
with a zero header, and rows a `rate > 0` region can roll. `FightRows::met_scripted`
treats an id in **no** row as scripted (an inline-script install, which raises
the bit by construction). Drives the site's per-enemy fight class, boost profile
and side-effect verdicts. CLI `asset formation-census <PROT.DAT | dir>`.
