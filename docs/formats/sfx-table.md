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
populated - voice count `1..=3`, trailing bytes zero). Id `0x64` onward is
unrelated rodata, starting with the `\PSX.EXE` dev-path string, so the table's
true extent is 100 entries.

| Offset | Name | Field |
|---|---|---|
| `+0` | `p` | program / VAG index - selects the loaded bank's program-attr entry |
| `+1` | `t` | tone / ADSR-region base; a multi-voice cue uses consecutive regions (`+i` per voice) |
| `+2` | `l` | note-level voice attribute (MIDI-ish, clusters near `60`) |
| `+3` | `n` | low 5 bits = **voice count**; bit `0x20` = sustained / continuous mode |
| `+4` | `id` | category / mixer-channel index (selects a column in the channel-volume tables `DAT_80091510` / `DAT_80091513`) |
| `+5..7` | - | no observed runtime reader (zero across the whole table) |

The field names are the designer's own, recovered from the runtime debug format
string `"setbl p:%d t:%d l:%d n:%d id:%d"`.

## Consumers

Two functions read the table, both indexing `&DAT_8006F198 + id*8`:

- **`FUN_800250D4(sound_id, voice)`** - the per-actor SFX trigger (from the actor
  tick `FUN_80021DF4`). Uses only the voice count (`n & 0x1F`), `SpuKeyOn`-ing
  (`FUN_800653C8`) that many consecutive voices.
- **`FUN_80016B6C`** - the per-frame SFX cue-ring drainer. It walks the 4-entry
  ring `DAT_8007B6D8` (the same ring `FUN_8004FCC8` and the move-power `+0x0d`
  sound cues write into), then for each cue programs `voice_count` voices via
  `FUN_80065034` - the libsnd `SpuSetVoiceAttr` analogue that takes program
  (`+0`), note/region (`+1` `+i`), attr (`+2`), and the channel volume picked by
  category (`+4`).

The SPU programming itself (`FUN_80065034` → `SpuSetVoiceAttr`) is libsnd and out
of clean-room scope - the engine has its own SPU. What is portable is the static
**data**.

### The ring value **is** the descriptor index

`FUN_80016B6C` reads a ring slot and indexes `&DAT_8006F198 + ring_value * 8`
directly, so whatever a caller writes into `DAT_8007B6D8` is the table index -
no further mapping. That matters because overlay code often skips the dispatcher
`FUN_8004FCC8` (which stores `id - 1` for `id < 0x40`) and writes the ring
itself: the Baka Fighter overlay's cues, for instance, are plain `_DAT_8007b6d8 =
9` / `0x20` / `0x21` / `0x37` stores (see
[`minigame-baka-fighter.md`](../subsystems/minigame-baka-fighter.md#sound)), and
those literals are descriptor ids as-is.

### A cue names its tone by **index**, not by key range

The SFX fire path and the *sequencer's* note-on differ, and conflating them
silently drops cues. `FUN_80065034` is handed the descriptor's fields directly -
program `+0`, **region/tone `+1`** (`+ i` for voice `i` of a multi-voice cue),
note-level attr `+2` - so a cue's tone is an explicit index into the program's
tone list. It is *not* resolved by asking which tone's authored `min..=max` key
window contains the note, the way a sequencer NoteOn is. Several retail cues
have a descriptor note outside their tone's window (the generic strike cue
`0x1A` = program 3 / tone 8 / note 67 is one), so a key-range lookup resolves
**nothing** for them and renders silence. The engine models the SFX shape with
[`VabBank::play_tone`](../../crates/engine-audio/src/vab_bind.rs) (explicit
region index) alongside `play_note` (key-range, for the sequencer).

## Program bank - the active scene's music VAB

The descriptors' `program` / `tone` fields index a VAB, and that VAB is **not a
dedicated SFX master** - it is the per-scene [`scene_vab_stream`](scene-bundles.md)
bank the BGM sequencer has open. `FUN_80065034` reads the libsnd "current bank"
globals: `_DAT_801ce33c` (VAB-header base), `_DAT_801ce334` (`ProgAtr` at `+0x20`,
stride `0x10`), `_DAT_801ce340` (`VagAtr` at `+0x820`, stride `0x20`) - so a
sound effect plays through the low programs of the same bank the music does.

Pinned from the save-state catalogue:

- The bank **varies per scene** - across catalogued captures the open bank is 13
  distinct VABs (used-program counts ranging `1..=16`).
- For a `music_01`-scene state the live bank is **byte-identical to the disc**
  `music_01` VAB ([`field-pack`](field-pack.md)-style stream, PROT 1004 at
  offset `+4`): the `VabHdr` and every program's `ProgAtr` attribute bytes
  (`+0..7`) match exactly; only the PsyQ reserved per-program pointer field
  (`ProgAtr +8..15`) is runtime-patched to the RAM `VagAtr` address.

Because scene banks differ in size, a cue resolves only where its `program` /
`tone` exists - SFX availability is **scene-dependent**, not a guaranteed
reservation. The engine therefore needs no separate SFX bank load:
`SfxBank::from_descriptors(...)` (this table) plays through the scene's
already-loaded BGM `VabBank` via `SfxBank::play_one_shot(spu, vab)`.

### The class-2 sound bank (PROT 0869)

Alongside the per-scene music VAB there is a **dedicated class-2 sound bank**,
extraction PROT **0869** (raw loader index `0x367`), and the battle-side code
loads it explicitly: the battle scene loader `FUN_800520F0` calls the streaming
loader with `a1 = 2` on `0x367` (swapping to raw `0x36D` = extraction 0875 when
`DAT_8007BD11 == 4`), and the Baka Fighter init `FUN_801CF00C` loads the same
`0x367` the same way. Its low programs (`0`, `3`) carry the cues the battle and
the duel fire, so every descriptor those two contexts use resolves in it.

This is what the site's cue player renders against
(`crates/web-viewer/src/sfx_view.rs`): SCUS → this table, PROT 0869 → the VAB,
descriptor → a one-shot through the clean-room SPU. Whether a given *field*
scene's cues sound out of this bank or out of the scene's music VAB depends on
which bank the libsnd current-bank globals hold at the time; the two readings
are not in conflict for battle/minigame cues, which is where the traced loads
are.

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
`sfx_table_real` test pins the layout + anchors against the real executable,
`sfx_table_live` (engine-shell) validates the parse against live RAM and feeds
the descriptors into `legaia_engine_audio::SfxBank::from_descriptors`, and
`sfx_vab_bank` (engine-shell) proves the program bank is the per-scene music VAB
(SFX programs resolve in the `music_01` bank; the live bank is byte-identical to
the disc bank; the bank varies per scene). CLI: `asset sfx-table <SCUS> [--json]`.

## See also

- [`subsystems/audio.md`](../subsystems/audio.md) - the SFX bank + scheduler and the per-actor SFX trigger.
- [Move-power table](move-power.md) - the `+0x0d` sound cue that feeds this table through `FUN_8004FCC8`.
- [VAB sound bank](vab.md) - the program / tone data the `p` / `t` fields index.
