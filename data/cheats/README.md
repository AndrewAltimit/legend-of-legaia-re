# Cheat databases

Third-party cheat code dumps for *Legend of Legaia* (NTSC-U).
These are GameShark / Pro-Action-Replay codes published in public
cheat databases over the past 25 years and reproduced here as a
reference fixture for `crates/cheats`. The codes are not derived
from the retail executable and contain no Sony-owned bytes - they
are byte-pair edits keyed by RAM address, not extracted asset data.

| File | Format | Source style |
|---|---|---|
| `legaia-ntsc-u.gs.txt` | GameShark text dump | "R I 2 L 0 ADDR VALUE NAME" lines (one effect per write). |
| `legaia-ntsc-u.cht` | Mednafen `.cht` (TOML-like) | `cheatN_desc / cheatN_code / cheatN_enable` triplets; multi-write effects use `+` separators. |

Both files describe the **same** effects in two different on-disk
encodings. The `crates/cheats` parser ingests either; round-trip
between formats is part of the test suite.

## Why these are useful for RE

Each cheat code is a labelled `(address, value)` pair. The label is
human-written, but the address is empirical: cheat authors
discovered these locations by trial and binary search against a real
console. The effect descriptions ("Max HP", "Have all Arts", "No
Random Battles", "Save Anywhere") give us **named anchors** for
otherwise unannotated RAM cells, which we can cross-reference
against the runtime layout traced from `SCUS_942.54` and the
overlays.

The `crates/cheats classify` CLI groups codes by address range and
labels them against:

- per-character record offsets (`0x80084708 + n*0x414`),
- inventory slots (`0x80085958` + 2-byte stride),
- battle actor pool (`0x800EC9E8` + party stride),
- engine globals (camera, BGM, encounter counter, save flag),
- mini-game scratch (fishing, baka fighter, dance, slots),
- script-VM scratch (`0x8007B8xx`).

Where a cheat label disagrees with the runtime evidence, the
runtime evidence wins; the cheat label is recorded as a citation.
See `docs/reference/cheats.md` for the conflicts that matter.

## Licence

GameShark code listings are factual data (RAM address + numeric
value + short description). They are reproduced here under fair-use
for reverse-engineering and interoperability research. If you are
the original cheat author and would prefer your codes not be
included, open an issue.
