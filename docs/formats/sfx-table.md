# Sound-effect descriptor table

Every actor / battle sound effect is keyed to an 8-byte descriptor in a static
`SCUS_942.54` rodata table at `DAT_8006F198`. The descriptor tells the sound
system which VAB program + tone to play, how many SPU voices to fan the cue
across, and which mixer channel it belongs to. This is the static counterpart of
the runtime bank `_DAT_8007B8D0` (loaded from `.dpk` / `monster.snd`), which
covers ids `>= 0x200`.

## Table base + record layout

| | |
|---|---|
| Base address | `DAT_8006F198` (file offset `0x5F998` in `SCUS_942.54`) |
| Index form | `DAT_8006F198 + sound_id*8` |
| Stride | `0x8` bytes |
| Entry count | **100** descriptors (sound ids `0x00..=0x63`) |

The runtime readers gate on `sound_id < 0x200`, but that is an upper **bound**,
not the table size: only ids `0x00..=0x63` are real descriptors (every one is
populated — voice count `1..=3`, trailing bytes zero). Id `0x64` onward is
unrelated rodata, starting with the `\PSX.EXE` dev-path string, so the table's
true extent is 100 entries.

| Offset | Name | Field |
|---|---|---|
| `+0` | `p` | program / VAG index — selects the loaded bank's program-attr entry |
| `+1` | `t` | tone / ADSR-region base; a multi-voice cue uses consecutive regions (`+i` per voice) |
| `+2` | `l` | note-level voice attribute (MIDI-ish, clusters near `60`) |
| `+3` | `n` | low 5 bits = **voice count**; bit `0x20` = sustained / continuous mode |
| `+4` | `id` | category / mixer-channel index (selects a column in the channel-volume tables `DAT_80091510` / `DAT_80091513`) |
| `+5..7` | — | no observed runtime reader (zero across the whole table) |

The field names are the designer's own, recovered from the runtime debug format
string `"setbl p:%d t:%d l:%d n:%d id:%d"`.

## Consumers

Two functions read the table, both indexing `&DAT_8006F198 + id*8`:

- **`FUN_800250D4(sound_id, voice)`** — the per-actor SFX trigger (from the actor
  tick `FUN_80021DF4`). Uses only the voice count (`n & 0x1F`), `SpuKeyOn`-ing
  (`FUN_800653C8`) that many consecutive voices.
- **`FUN_80016B6C`** — the per-frame SFX cue-ring drainer. It walks the 4-entry
  ring `DAT_8007B6D8` (the same ring `FUN_8004FCC8` and the move-power `+0x0d`
  sound cues write into), then for each cue programs `voice_count` voices via
  `FUN_80065034` — the libsnd `SpuSetVoiceAttr` analogue that takes program
  (`+0`), note/region (`+1` `+i`), attr (`+2`), and the channel volume picked by
  category (`+4`).

The SPU programming itself (`FUN_80065034` → `SpuSetVoiceAttr`) is libsnd and out
of clean-room scope — the engine has its own SPU. What is portable is the static
**data**.

## Provenance

Decoded directly from the disc, and cross-checked **byte-for-byte against live
save-state RAM**: the table window at `0x8006F198` read out of a catalogued
mednafen state's main RAM parses to the identical 100 descriptors as the disc
`SCUS_942.54`, confirming the table is static rodata and the parser offset is
right. The two cue ids the engine's default SFX bank already references resolve
to `0x1A` = program 3 / note 67 and `0x4C` = program 3 / tone 8 (voice count 2).

## Parser

`legaia_asset::sfx_table::SfxTable::from_scus` resolves the table from a
`SCUS_942.54` image (PSX-EXE `t_addr` → file-offset map, identical to the
[item-name table](item-table.md) resolver); `from_table_bytes` parses a raw
table window straight out of save-state RAM. `SfxDescriptor` exposes the decoded
fields plus `voice_count()` / `sustained()` / `is_active()`. The disc-gated
`sfx_table_real` test pins the layout + anchors against the real executable, and
`sfx_table_live` (engine-shell) validates the parse against live RAM and feeds
the descriptors into `legaia_engine_audio::SfxBank::from_descriptors`.

## See also

- [`subsystems/audio.md`](../subsystems/audio.md) — the SFX bank + scheduler and the per-actor SFX trigger.
- [Move-power table](move-power.md) — the `+0x0d` sound cue that feeds this table through `FUN_8004FCC8`.
- [VAB sound bank](vab.md) — the program / tone data the `p` / `t` fields index.
