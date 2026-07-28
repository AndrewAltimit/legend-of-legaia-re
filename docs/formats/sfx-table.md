# Sound-effect descriptor table

Every actor / battle sound effect is keyed to an 8-byte descriptor in a static
`SCUS_942.54` rodata table at `DAT_8006F198`. The descriptor tells the sound
system which VAB program + tone to play, how many SPU voices to fan the cue
across, and which mixer channel it belongs to. Ids `>= 0x200` come from a
**per-scene** extension of the same table instead -
[below](#ids--0x200-come-from-the-current-bundles-record-0).

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
| `+4` | `id` | category: picks the 12-byte mixer record (`DAT_80091510` / `DAT_80091513` are record 0's fields) **and** through its `+8` the VAB slot the cue keys - see [below](#category-is-a-bank-selector-and-four-banks-are-open-at-once) |
| `+5..7` | - | no observed runtime reader (zero across the whole table) |

The field names are the designer's own, recovered from the runtime debug format
string `"setbl p:%d t:%d l:%d n:%d id:%d"`.

### Ids `>= 0x200` come from the current bundle's record 0

The gate's other arm is not a second fixed table. Both readers resolve it the
same way, out of the sound subsystem's **current-bundle** slot `_DAT_8007B8D0`:

```text
id <  0x200:  desc = DAT_8006F198 + id*8              ; this page's table
id >= 0x200:  desc = _DAT_8007B8D0 + offsets[0]       ; the bundle's record 0
                     + (id - 0x200)*8
```

`FUN_800250D4` is the readable one (`0x800250F4..0x8002514C`): `slti v0,a2,0x200`
picks the arm, the `>= 0x200` side loads `0x8007B8D0`, applies the bundle
header's `+0x02` word to reach `offsets[0]`, and adds `(id - 0x200) * 8`;
`FUN_80016B6C` (`0x80016C24..0x80016CB0`) does the same before printing the
designer's `"setbl p:%d t:%d l:%d n:%d id:%d"` line off `+0..+4`. The row layout
is therefore the layout above, and the disc bears it out.

**Which** bundle sits in that slot is the whole content of the claim. It is
whatever loaded last: `FUN_8001FA88` puts `bse.dat` there at init, and the field
asset loader repoints it at the scene's prescript bundle on every field load
(`FUN_8001F7C0` `0x8001F864`). So in field mode the `>= 0x200` bank is the
**scene's own prescript record 0**, sized and authored per scene - jou reserves
96 rows and populates 40, `rugi` carries 21 - and it is neither `bse.dat` nor a
`.dpk` / `monster.snd`. Full treatment, including why record 0 must not be
spawned as a move-VM stager, in
[`field-ambient-fx.md`](../subsystems/field-ambient-fx.md#the-master-ambient-record-0---the-per-scene-sfx-descriptor-bank);
the init-time occupant of the slot in [`bse-dat.md`](bse-dat.md).

## Consumers

Two functions read the table, both indexing `&DAT_8006F198 + id*8` for
`id < 0x200` and the [current bundle's record 0](#ids--0x200-come-from-the-current-bundles-record-0)
above it:

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

### The ring is two arrays, aged by one function and drained by another

`DAT_8007B6D8[4]` (`i16` cue ids) has a sibling the doc's "4-entry ring" phrasing
hides: `DAT_8007C338[4]`, a `u32` **countdown in vsyncs** per slot. Two different
functions walk them, in a fixed order the per-frame mode handlers pin
(`FUN_8001698C` → `FUN_80016444` → `FUN_80016B6C`):

- **`FUN_8001698C` ages** (`0x80016AF4..0x80016B54`). A slot whose timer is zero
  has its id cleared to `-1`; a non-zero timer is decremented by the adaptive
  frame step `DAT_1F800393` and floored at zero. Retail stores the possibly
  negative difference and *then* overwrites it with zero - two stores - so a slot
  cannot skip past zero however large the frame step is.
- **`FUN_80016B6C` drains** (`0x80016BF8`). A slot plays only when its timer is
  **exactly zero** and its id is still `>= 0`.
- The producers sit between them. `FUN_80035B50` writes `id` plus `timer = 0`
  into slot `gp+0x158` and advances that cursor round-robin over the four.

So the contract is a **one-shot scheduled delay**, not a queue: a cue armed with
timer `N` plays on the frame its countdown first reads zero and is cleared before
the next drain sees it. Two consequences an approximate "queue with a per-cue
frame counter" gets wrong - the countdown is in **vsyncs** (at the field cadence
floor of 2, a `timer = 4` cue plays after two game ticks, not four), and there
are exactly four slots, so a fifth pending cue *replaces* one.

Port: `legaia_engine_audio::sfx_ring`.

### Voice allocation: one-shots descend from 23, sustained cues ascend from 7

`FUN_80016B6C`'s two key-on loops do not share a voice range.

| Branch | Voices | State |
|---|---|---|
| One-shot (`flags & 0x20 == 0`) | `23 - cursor`, descending | rolling cursor `gp+0x4BC`, wrapped when it *exceeds* the limit - so the limit value itself is used. Limit is `3`, or `1` in game modes `3` and `0x17`. |
| Sustained (`flags & 0x20`) | `7 .. 7 + count - 1`, ascending | held count `gp+0x5D0`; the previous run is released first, and the mixer-record pointer latches into `gp+0x40C`. |

Two details worth keeping. A one-shot **stops** each voice (`FUN_800653C8`)
immediately before reprogramming it. And the sustained held-count write lives
*inside* the key-on loop, so a sustained cue with a zero voice count releases the
old run but leaves `gp+0x5D0` unchanged - the next sustained cue re-releases the
same, already-stopped voices.

The channel gate is a 12-byte mixer record at `0x80091508 + channel * 12`: `+0`
is a `VabHdr` pointer, `+8` is the **VAB slot id** handed to `FUN_80065034` as
its second argument, `+0xB` is an enable byte and a zero there skips the cue
entirely, before any voice work. The two VAs this page's `+4` row names,
`DAT_80091510` and `DAT_80091513`, are the `+8` and `+0xB` fields of record 0 -
not two byte arrays. While `_DAT_8007BA88` is non-zero every cue is forced onto
channel `6`.

`+8` is a bank id and not a level, which the next hop settles: `FUN_80065034`
passes it to `FUN_80068b98`, which rejects it unless it is `< 0x10` **and** the
per-bank open-state byte `_DAT_801CE368[id] == 1`, then repoints the current-bank
globals (`VabHdr` / `ProgAtr` / `VagAtr` bases) at that slot. Across catalogued
save states record `N` holds `+8 == N` and `+0` == slot `N`'s live `VabHdr`, in
every record of every state - so **a cue's category byte selects its VAB slot**.

### Category is a bank selector, and four banks are open at once

| Category | Descriptors | Bank it keys |
|---|---|---|
| `0` | 16 | Slot-0 system bank = **PROT 0868**. Shared UI cues (`0x1A`, `0x20`, `0x21`, `0x23`, `0x37`). |
| `2` | 53 | Slot-2 class-2 bank = **PROT 0869** (`0875` when `DAT_8007BD11 == 4`). Battle / duel (`0x09`, `0x4C`). |
| `6` | 30 | Slot-6 field bank = **PROT 0876**. Field script cues (`0x2E`, `0x2F`) and the rest of the field/player set. |
| `11` | 1 | Slot-11 battle-reward bank = **PROT 0889**. The single cue `0x50`. |

PROT 0868's identity is a byte match, not a label: a live field state's slot-0
`VagAtr` program-0 page (512 bytes) occurs verbatim in extraction entry
`0868`, at VAB offset `+4`, with the header's `ps = 5` matching the live bank's.
Its CDNAME label reads `battle_data` and 0869's reads `monster_data`, which is
the usual reminder that a label is a hint.

The other four open slots carry no descriptors of their own: **`1` and `3` hold
variable banks**, not fixed entries - slot 1 is the scene's current BGM bank and
slot 3 a script-selected side-band bank, both re-filled per selection by
`FUN_800243F0` (see [below](#which-prot-entry-reaches-which-slot)) - and slots
`7` / `8` hold the battle's two `monster.snd` banks. That is why "the SFX bank is
the scene's music VAB" is a half-truth rather than a mistake: it is exactly true
of slot 1, and no descriptor keys slot 1.

### Which PROT entry reaches which slot

A bank reaches a slot through one call pair, and the pair names the binding at
every call site: `FUN_8001FC00(raw_toc_index, category, buf, append, len)`
streams the entry into a staging buffer, then
**`FUN_8001E54C(category, buf, len)`** installs it. The installer indexes the
same 12-byte mixer record the descriptors do (`0x80091508 + category*12`), takes
the header buffer from `+0` and the VAB slot from `+8`, and walks the streamed
chunk list: chunk type `1` / `3` goes to `FUN_8002630C` → `FUN_80068D34`
(`SsVabOpenHead`, sticky, with the SPU address read from the per-slot table at
`0x800917B0`) → `FUN_80069170` (`SsVabTransBody`). Raw TOC indices run two above
extraction indices ([numbering](cdname.md#numbering-space)).

| Slot | Filler | Call site |
|---|---|---|
| `0` | PROT 0868 | resident system bank |
| `1` | the scene's BGM bank (`music_01`, variable) | `FUN_800243F0`, index `*(0x8007BC64) + id - 2000` |
| `2` | PROT 0869 (raw `0x367`), `0875` alternate | battle scene loader `FUN_800520F0`, Baka init `FUN_801CF00C` |
| `3` | a `vab_01` side-band bank (variable) | `FUN_800243F0`, index `*(0x8007BBE4) + id - 2000` from `_DAT_8007BABC` |
| `6` | PROT 0876 (raw `0x36E`) | field init `FUN_801D6704` |
| `7` / `8` | the two `monster.snd` banks | `FUN_8003E104` + `FUN_8001E54C(7\|8, …)` from `FUN_800520F0` |
| `11` | PROT 0889 (raw `0x37B`) | battle-end reward resolution `FUN_8004E568` |

Both new pins carry an independent structural check. PROT 0876 holds **30** VAGs
for the 30 category-`6` descriptors, its populated program slots are `1..=7`, and
29 of the 30 descriptors name a program in that set. PROT 0889 is a one-program
bank whose only populated `ProgAtr` slot is **10** - which is exactly the program
the single category-`11` descriptor (`0x50`) names, with 2 voices against the
program's 2 tones; and the function that loads it, `FUN_8004E568`, is the same
one that fires cue `0x50`. Slot 6 and slot 1 are also byte-pinned: in a
catalogued field state the live header buffers match extraction 0876 and
(for that state's track) 0998 exactly, over the whole corpus of 218 disc VABs,
once the runtime-written `ProgAtr +8..0xF` words are excluded.

The `DAT_8007BD11 == 4` alternate for slot 2 shows up in the same structural
check: descriptors `0x40` / `0x41` name program 10, which PROT 0869 does not
populate and PROT **0875** does.

### The slots are aliased in pairs

`FUN_8001D424` (sound-system init, called from the boot init `FUN_80015E90`)
builds the 16 mixer records: it clears `+0` / `+9` / `+0xB` and writes
**`+8 = record index`** for every record (`sb a3,0x8(t0)` with `addiu t0,t0,0xc`,
`0x8001D68C`) - so the "category *is* the slot" identity is written by the
initialiser, not merely observed in states. It then assigns the header buffers
from one base, and four pairs share one: records `0`/`10`, `1`/`5`, `2`/`6`,
`8`/`11`.

`FUN_800265E8` installs the matching per-slot SPU addresses at `0x800917B0`, and
the same pairs share a base there too (`0`/`10` at `0x1010`, `1`/`5` at
`0x10010`, `2`/`6` at `0x33010`, `4`/`7` at `0x65010`), with `3` at `0x60010`,
`8` at `0x6C810` and `11` at `0x6F010`. So the **field bank (slot 6) and the
class-2 battle bank (slot 2) are the same physical bank**, used by two
categories in two modes - they can never be resident together, and neither can
the BGM slot 1 and its alias 5. The gaps are the retail allocation, not a
hardware cap: a bank larger than the gap to the next base simply overruns it,
which is legal exactly while the neighbour slot is closed.

The catalogued save states show the partition directly. The per-bank open-state
array `_DAT_801CE368` (`0` free, `1` open) takes exactly two shapes across them:

| Game mode | Slots open |
|---|---|
| `1` / `2` / `3` / `0x11` (field family) | `0`, `1`, `3`, `6` (+ `5`) |
| `0x0F` (battle) | `0`, `1`, `2`, `7` (+ `5`, `8`) |

Slot 2 is never open in the field and slot 6 never in battle, in any state - the
mutual exclusion the shared base predicts. Slot 11 is open in none of them,
which fits a bank the battle-end reward path loads after the point these states
were taken.

### What a single-bank port gets wrong, and why it is silent

A port that stages **one** resident SFX bank resolves only one category
correctly, and it fails *quietly* rather than audibly-broken. Both PROT 0868 and
PROT 0869 carry a one-VAG-per-semitone UI key map at program 0, so a
category-`0` id fired through the class-2 bank resolves to a **sibling sample**,
not to silence: a genuine retail blip, roughly twice as long and a fifth lower
than the field menu's, because 0869's `center` bytes are authored higher. Peak,
duration and "did a voice key on" all pass in that state. The only observable
that separates the two is which PROT entry the samples came from.

Both hosts stage two banks - slot 0 and slot 2 - and route each cue through
`slot_for_category(descriptor.category)`; the 31 category-`6`/`11` descriptors
fall back to the class-2 bank, which is exactly the behaviour they had before.
That fallback is now a **residency** limit rather than a gap in the map: slots 6
and 11 name real entries (0876 / 0889), and what stops a host staging them is
the [SPU budget](#spu-budget---both-banks-in-one-region) below. Retail does not
need the room because slot 6 *is* slot 2's region, refilled per mode; a host that
wants the field cues right has to reload the shared region on the field/battle
transition the way `FUN_801D6704` and `FUN_800520F0` do, not widen the reservation.

### The ring value **is** the descriptor index

`FUN_80016B6C` reads a ring slot and indexes `&DAT_8006F198 + ring_value * 8`
directly, so whatever a caller writes into `DAT_8007B6D8` is the table index -
no further mapping. That matters because overlay code often skips the dispatcher
`FUN_8004FCC8` (which stores `id - 1` for `id < 0x40`) and writes the ring
itself: the Baka Fighter overlay's cues, for instance, are plain `_DAT_8007b6d8 =
9` / `0x20` / `0x21` / `0x37` stores (see
[`minigame-baka-fighter.md`](../subsystems/minigame-baka-fighter.md#sound)), and
those literals are descriptor ids as-is.

The same four ids are **not** the duel's own: `0x20` confirm / `0x21` cursor /
`0x23` disabled-row buzz / `0x37` cancel are the *shared UI* cues, written by the
SCUS-resident kind-4 list kernel `FUN_80032A44` that pages every pause-menu list
window (`li a2,0x21` at `0x80032b9c`, `li a1,0x20` at `0x80032d24`, `li a1,0x23`
at `0x80032d0c`, `li a2,0x37` at `0x80032d74`, each with the ring store beside
it - see [`field-menu.md`](../subsystems/field-menu.md)). The Baka overlay is a
co-user of the global ids, not their source; attributing them to it is how the
browser play page came to label its own pause-menu cues a port pick.

### The UI cues live in program 0 of the class-2 bank

Program `0` of the class-2 bank (PROT 0869) is a purpose-built SFX key map, and
the descriptor table is authored against it: one distinct VAG per semitone with
single-note windows `min == max == 60 + i`, matching the UI descriptors' note
bytes 1:1 (`0x20` → tone 0 / note 60, `0x21` → tone 1 / note 61, `0x23` → tone 3
/ note 63, `0x09` → tone 9 / note 69). `0x37` is the one that does not line up -
tone 5, note 64, against a `[65,65]` window - which is the clearest single
illustration of why the fire path indexes the tone directly.

Two things follow for anyone rendering these through a clean-room SPU, and both
are settled.

**Retail does pitch them down.** That program's `center` bytes sit at `79..=88`
against notes `60..=69`, so the key-on puts every UI cue 12..26 semitones under
its centre (`0x20` is note 60 against center 83, a ×0.28 rate). That is the
authored sound, not a defect: the pitch a cue keys at is
`0x1000 * 2^((note - center + fine/128)/12)`, unity at `note == center`, with no
sample-rate factor of any kind - law and provenance in
[`audio.md`](../subsystems/audio.md#the-key-on-pitch-law---note-against-the-tones-center),
confirmed against retail's own staged pitch values in save-state RAM. A port
that also multiplies by a nominal `22050/44100` keys these an octave lower
again, which is what turned the browser play page's menu blips into thuds.

**But this is not the bank the pause menu sounds out of.** These four ids are
descriptor category `0`, and the category selects the VAB slot (above), so
retail's field menu keys them in the slot-0 system bank - **PROT 0868**, whose
program 0 is the same one-VAG-per-semitone shape over its own VAGs, authored
`center` bytes spread `72..=90` (so `0x20` lands at ×0.53 there rather than
×0.28, a shorter and brighter blip). The
class-2 copy is the one the minigame overlays key directly
(`FUN_80065034(voice, 2, 0, 0, 0x3c, 0x40, ...)` = vab 2 / program 0 / tone 0 /
note 60, the same triple as `0x20`), so both are real; they are simply different
banks' takes on the same UI blip, the class-2 one roughly twice as long. This
section's heading is about where the *class-2* copy lives; the routing decides
which copy a given cue keys.

### A cue names its tone by **index**, not by key range

The SFX fire path and the *sequencer's* note-on differ, and conflating them
silently drops cues. `FUN_80065034` is handed the descriptor's fields directly -
program `+0`, **region/tone `+1`** (`+ i` for voice `i` of a multi-voice cue),
note-level attr `+2` - so a cue's tone is an explicit index into the program's
tone list. It is *not* resolved by asking which tone's authored `min..=max` key
window contains the note, the way a sequencer NoteOn is. Several retail cues have
a descriptor note outside their tone's window - the menu cancel `0x37` is
program 0 / tone 5 / note 64 against a `[65,65]` window, disc-measured - so a
key-range lookup resolves **nothing** for them and renders silence. (The example
here used to be `0x1A` = "program 3 / tone 8 / note 67"; `0x1A`'s tone is `0`,
and the `tone 8` belongs to `0x4C`, as
[Provenance](#provenance) below already had it.) The engine models the SFX shape with
[`VabBank::play_tone`](../../crates/engine-audio/src/vab_bind.rs) (explicit
region index) alongside `play_note` (key-range, for the sequencer).

### Walking fires no cue - retail has no footstep sound

A player walking a field scene plays nothing through any of the paths above.
That is a measured negative, taken as a contrast rather than as a single
observation:
[`autorun_footstep_cue.lua`](../../scripts/pcsx-redux/autorun_footstep_cue.lua)
watches both ring producers, the dispatcher, the per-actor trigger, the voice
programmer and the four ring slots themselves at once, and runs one save state
twice for the same number of vsyncs - once standing still, once with the D-pad
held.

Standing still, nothing fires. Walking a house interior, nothing fires. Walking
the kingdom overworld, nothing fires: no ring store, no `FUN_800250D4` call,
and - the decisive one, because it does not depend on having guessed the right
producer - not one `FUN_80065034` voice program.

The one walk that does produce cues produces exactly two, `0x2E` then `0x2F`,
hundreds of vsyncs apart, both pushed from the `FUN_80035B50` call site inside
the field VM `FUN_801DE840` (at `0x801E0348`). That is the script SFX arm - op
`0x36`, bit-15-set sub `0` ([`script-vm.md`](../subsystems/script-vm.md)) -
firing scene-script literals as the player crosses triggers. Cadence is what
separates those from a footstep: a step sound recurs every few frames for as
long as the player moves, and these do not recur at all. The same run also
catches a per-actor `FUN_800250D4` trigger and several voice programs, so a
silent run is a silent game and not a blind probe.

`FUN_80018DB0`, the per-frame field cadence, ticks every vsync throughout and
never fires its step gate: `_DAT_8007B8A4` stays pinned at `2` - the
`0xF - (speed >> 4) >= 0xB` else-branch - so the speed words it reads
(`gp+0x614` / `gp+0x618`) never reach the `0x30` a step needs. Its two output
bytes `DAT_800915DA` / `DAT_800915DB` are not cue traffic either: no descriptor
is read for them and no voice is keyed, and they sit two-bytes-per-port inside
the `0x80`-byte block the pad init `FUN_8001D230` zeroes and registers
alongside the libpad report buffers `0x800840F8` / `0x8008411A`. Under capture
their values never change while the player walks.

This is the runtime confirmation of the static reading already recorded for the
field controller in [`functions/audio.md`](../reference/functions/audio.md)
("the step loop is silent"), and it widens it from one producer to all of them.
A port that wants a footstep has to author one - there is no retail id to copy.
`see ghidra/scripts/funcs/80018db0.txt`, `80035b50.txt`, `800250d4.txt`,
`8001d230.txt`.

## Program bank - selected by the cue's category

The descriptors' `program` / `tone` fields index a VAB, and **which** VAB is the
cue's own choice: `FUN_80065034` calls `FUN_80068b98(vab_id, program)` first,
which repoints the libsnd "current bank" globals - `_DAT_801ce33c` (VAB-header
base), `_DAT_801ce334` (`ProgAtr` at `+0x20`, stride `0x10`), `_DAT_801ce340`
(`VagAtr` at `+0x820`, stride `0x20`) - at the slot named by the cue's mixer
record, then reads them. So the cue is *not* keyed against whichever bank the
BGM sequencer last left open; the [category table above](#category-is-a-bank-selector-and-four-banks-are-open-at-once)
is the mapping, and several banks are open concurrently.

The globals are shared with the sequencer, which is why the two readings were
easy to conflate: a save state sampled between cues holds whatever bank keyed
last, and for a state sampled after a BGM note that is the music bank.

Pinned from the save-state catalogue:

- The bank **varies per scene** - across catalogued captures the open bank is 13
  distinct VABs (used-program counts ranging `1..=16`).
- For a `music_01`-scene state the live bank is **byte-identical to the disc**
  `music_01` VAB ([`field-pack`](field-pack.md)-style stream, PROT 1004 at
  offset `+4`): the `VabHdr` and every program's `ProgAtr` attribute bytes
  (`+0..7`) match exactly; only the PsyQ reserved per-program pointer field
  (`ProgAtr +8..15`) is runtime-patched to the RAM `VagAtr` address.

Because banks differ in size, a cue resolves only where its `program` / `tone`
exists - SFX availability depends on which slots are loaded, not on a guaranteed
reservation. The engine models the **pinned** part of the slot set: it stages
slot 0 and slot 2 into one reserved SPU region and plays each cue through the
bank its own category names, falling back to the class-2 bank for the unpinned
slots and to the scene's already-loaded BGM `VabBank` when nothing staged at all.
`SfxBank::from_descriptors` carries the playback fields (program + tone-region
index + note + voice count) and `SfxTable::cue_slots` the routing;
`SfxBank::play_one_shot(spu, vab)` fires the cue via `VabBank::play_tone` across
its `voices` consecutive regions - by explicit tone **index**, not by key range.

### SPU budget - both banks in one region

The two pinned banks are resident together, so the engine's reserved SFX region
has to hold both and the BGM region is whatever is left of the 512 KiB.

| | Bytes |
|---|---|
| PROT 0868 VAG bodies | 59 136 |
| PROT 0869 VAG bodies | 188 128 |
| Reserved SFX region (`SFX_BANK_SPU_BYTES`) | 249 856 (`0x3D000`) |
| BGM region (512 KiB − `0x1000` scratch − the above) | 270 336 |
| Largest scene BGM VAB on the disc that a BGM path stages | 269 632 |

Every VAG in both banks is already a multiple of the allocator's 16-byte ADPCM
block, so the packed footprint equals the raw total and 2 592 bytes stay free.
The figure is squeezed from both sides: one step larger (`0x3E000`) drops the
BGM region to 266 240 and starts silencing music that plays today, one step
smaller does not fit both banks. Both hosts use the same constant, and the two
must stay equal.

Retail's own map, from the initialiser `FUN_800265E8`, is the reason it does not
face this: the four fixed banks never sum, because slot 6 shares slot 2's SPU
base and only one of the two is open per mode.

| Slot | SPU base | Gap to the next base | Bank's VAG bodies |
|---|---|---|---|
| `0` / `10` | `0x1010` | 61 440 | PROT 0868 - 59 136 |
| `1` / `5` | `0x10010` | 143 360 | current BGM bank |
| `2` / `6` | `0x33010` | 184 320 | PROT 0869 - 188 128 / PROT 0876 - 174 192 |
| `3` | `0x60010` | 20 480 | side-band bank |
| `4` / `7` | `0x65010` | 30 720 | `monster.snd` bank A |
| `8` | `0x6C810` | 10 240 | `monster.snd` bank B |
| `11` | `0x6F010` | 69 616 | PROT 0889 - 19 344 |

The gaps are allocation, not enforcement - PROT 0869 is 3 808 bytes larger than
the gap below slot 3, and the largest `music_01` bank is nearly three times slot
1's gap. A bank overruns its neighbour's base whenever that neighbour is closed,
which for a mode-partitioned slot set is most of the time.

### The class-2 sound bank (PROT 0869)

Slot 2 of the [category map](#category-is-a-bank-selector-and-four-banks-are-open-at-once) is a **dedicated class-2 sound bank**,
extraction PROT **0869** (raw loader index `0x367`), and the battle-side code
loads it explicitly: the battle scene loader `FUN_800520F0` calls the streaming
loader with `a1 = 2` on `0x367` (swapping to raw `0x36D` = extraction 0875 when
`DAT_8007BD11 == 4`), and the Baka Fighter init `FUN_801CF00C` loads the same
`0x367` the same way. Its low programs (`0`, `3`) carry the cues the battle and
the duel fire, so every descriptor those two contexts use resolves in it.

The site's cue player (`crates/web-viewer/src/sfx_view.rs`) walks SCUS → this
table, then PROT → **the bank each cue's category names**, then descriptor → a
one-shot through the clean-room SPU. So the duel hit `0x09` renders out of
PROT 0869 and the shared UI blips `0x20` / `0x21` / `0x37` and the strike `0x1A`
out of PROT 0868, even though the same duel overlay writes all of them.

The live engine mirrors this: `BootSession` uploads both pinned banks into one
dedicated top region of SPU RAM at boot (`stage_sfx_vab`, one `SpuAllocator` so
they pack, with the scene-BGM allocator capped below the region so a BGM upload
can't stomp the SFX samples), and `AudioBgmDirector::tick_sfx_frame` fires each
cue against the bank its slot names - falling back to the class-2 bank for an
unpinned slot, and to the scene BGM `VabBank` when nothing staged at all. So the
Tactical-Arts strike cue (`0x1A`) sounds out of the system bank and the Baka
Fighter exchange-hit cue (`0x09`, queued by the duel rules kernel and drained by
the play-window) out of the bank the retail battle loader loads. The disc-gated
`sfx_cue_resident_bank` test (engine-shell) proves the routed cues key a voice
via the tone-index path and that both banks pack inside the reserved region;
`play_sfx_channel` (web-viewer) asserts a category-`0` and a category-`2` cue
resolve to *different* PROT entries.

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
fields plus `voice_count()` / `sustained()` / `is_active()` / `vab_slot()`.

The same module carries the **routing law**: `slot_for_category`,
`prot_index_for_slot` (`None` only for the slots whose bank is variable rather
than a fixed entry, so a host cannot mistake one for the other),
`prot_index_for_category`, the `SLOT_BANKS` pairs (every fixed-entry slot),
`PINNED_SLOT_BANKS` (the subset a single-region host stages),
`spu_base_for_slot` + `SLOT_ALIASES` (retail's SPU map and the pairs that share
a region), and `FALLBACK_VAB_SLOT` for the categories a host has not staged.
`SfxTable::cue_slots` / `slots_used` are the per-cue and per-table views.

The disc-gated
`sfx_table_real` test pins the layout + anchors against the real executable,
`sfx_table_live` (engine-shell) validates the parse against live RAM and feeds
the descriptors into `legaia_engine_audio::SfxBank::from_descriptors`, and
`sfx_vab_bank` (engine-shell) proves the program bank is the per-scene music VAB
(SFX programs resolve in the `music_01` bank; the live bank is byte-identical to
the disc bank; the bank varies per scene). CLI: `asset sfx-table <SCUS> [--json]`.

## See also

- [`subsystems/audio.md`](../subsystems/audio.md) - the SFX bank + scheduler and the per-actor SFX trigger.
- [`subsystems/audio.md`](../subsystems/audio.md#cd-xa-voice-clip-dispatchers-and-static-cue-census) - `FUN_8004FCC8` / `FUN_8004FE5C` are dual-purpose: an id `< 0x100` queues this SFX ring, but an id `>= 0x100` routes the same call to the CD-XA voice-clip player `FUN_8003D53C` (a different table, `0x801C6ED8`), where the census of static `(clip_id, chan)` voice cues lives.
- [Move-power table](move-power.md) - the `+0x0d` sound cue that feeds this table through `FUN_8004FCC8`.
- [VAB sound bank](vab.md) - the program / tone data the `p` / `t` fields index.
