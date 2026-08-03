# Audio

Everything that makes sound: music, sound effects, character voice, and the
streamed CD audio under cutscenes - plus the PsyQ sound stack the game drives it
all through.

**The stack, top to bottom.** The path-string cluster builds audio file paths;
the SCUS dispatchers consume them; underneath sit the actual formats, VAB sound
banks and SEQ sequences. The per-scene `.dpk` / `sound_data2` pack decodes as a
[VAB + SEQ bundle](../formats/sound-driver.md#the-dpk--sound_data2-payload-is-a-vab--seq-bundle);
the `.MAP` / `.PCH` / `.spk` / `.pac` PsyQ intermediates are **not** present as
separate retail chunks.

**Where it lives.** All SCUS-resident: the SsAPI sequencer at the
`0x80061-0x80067` cluster, libspu / SPU control at `0x80068-0x8006D`.

**Port counterpart.** `crates/engine-audio` - a clean-room SPU plus an
SsAPI-shaped `Sequencer`, mixed through cpal. `crates/vab`, `crates/seq` and
`crates/xa` parse the formats; `mednafen-state spu` is the parity oracle.

**The thing that catches people out:** Legaia's SEQ is **not** stock PsyQ SEQ.
The version field is u32 BE (not u16), and its meta events carry **no** MIDI
variable-length `length` byte - `0xFF 0x51` is followed directly by three tempo
bytes. Reading a phantom length byte swallows the first-body tempo override and
pins playback ~3x fast against the 240 BPM placeholder header. See
[`formats/seq.md`](../formats/seq.md).

**A second one:** most retail BGM lives at a **non-zero offset** inside its
entry - `[u32 chunk_header][VAB][chunk1_header][SEQ]`. Slice past the wrapper
with `SceneAssets::seq_in_stream_entries` / `bgm_seq_offset`.

## Contents

- [Path-string cluster](#path-string-cluster) · [SCUS consumers](#scus-consumers) · [File-API leaf cluster](#file-api-leaf-cluster)
- [VAB sound banks](#vab-sound-banks) · [per-actor SFX](#per-actor-sound-effects) · [monster sound bank](#monster-sound-bank---hmpackmonstersnd)
- [BGM dispatch](#bgm-dispatch) · [global-pool BGM (`music_01`)](#global-pool-bgm-the-music_01-bank)
- [SsAPI sequencer](#ssapi-sequencer-0x80061-0x80067-cluster) - [globals](#globals) · [public SEQ API](#public-seq-api) · [SEQ internals](#seq-internals) · [voice / mixer](#voice--mixer-audible-output-critical-path) · [VAB attr accessors](#vab-attribute-accessors--utility-note-triggers) · [key-on pitch law](#the-key-on-pitch-law---note-against-the-tones-center) · [SPU command shims](#spu-command-shims-0x81-scaling--0127--016383) · [per-channel event handlers](#per-channel-event-handlers-over-_dat_801cd2c0-the-0x80060a1c0x80061bf8-family) · [further libsnd leaves](#further-libsnd--libspu-leaves) · [renderer-citation correction](#renderer-citation-correction)
- [libspu / SPU control](#libspu--spu-control-0x80068-0x8006d-cluster) - [SPU globals](#spu-globals) · [primitives](#libspu-primitives) · [init / reset / key](#spu-init--reset--key-registers) · [DMA transfer engine](#spu-dma-transfer-engine) · [reverb model](#reverb-model-engine-audio) · [Gaussian resampler](#voice-resampler---4-point-gaussian-interpolation-engine-audio) · [SsApi seq-management layer](#ssapi-seq-management-layer-above-libspu)
- [Engine-audio: Sequencer port](#engine-audio-model---sequencer-port) · [clean-room SPU port](#engine-audio-model---clean-room-spu-port) · [SFX bank + scheduler](#sfx-bank--scheduler) · [XA-ADPCM](#xa-adpcm)
- [Battle arts-voice shout path](#battle-arts-voice-shout-path-engine) · [Audio-trace parity oracle](#audio-trace-parity-oracle) · [What's left](#whats-left)

## Path-string cluster

The string cluster at `0x8007B380` holds the file extensions the sound subsystem appends to scene-asset paths. Full layout in [`formats/sound-driver.md`](../formats/sound-driver.md). Eight extensions in the cluster: `.spk`, `.LZS`, `.dpk`, `.MAP`, `.PCH`, `.pac`, `STR`, `bse.dat` (master file).

## SCUS consumers

| Function | Role |
|---|---|
| `FUN_8001FA88` | **Sound subsystem init / `.dpk` loader.** Loads `bse.dat` master bank, then per-scene `.dpk` from `h:\main\bg\domepack\…`. |
| `FUN_8001FC00` | **Streaming-asset loader.** Builds paths under the `sound\` prefix; the XA / `.pac` / `STR` consumer. |

`FUN_8001EBEC` was previously listed here as a third "mode-aware extension dispatcher"; that is a misread. The decomp shows it is the graphics-side character-TMD equipment-conditional group-transform swap (it reads `DAT_8007C018[_DAT_8007B824 + 0..2]`, the loaded battle-character TMD pointers), not a sound consumer - see [`formats/sound-driver.md`](../formats/sound-driver.md#consumers) and [`formats/character-mesh.md`](../formats/character-mesh.md#10-group-cap--equipment-conditional-swap).

Both `FUN_8001FA88` and `FUN_8001FC00` carry a dev/retail split via `_DAT_8007B8C2`. The **retail** branch (`!= 0`, the value retail boots with) loads via PROT indices directly. The **dev** branch (`== 0`) opens an `h:\` path through `FUN_8003E6BC`, which is a plain host-trap wrapper - `strcpy`, then `FUN_800608F0` (`break 0x103`), then fseek/fread/fclose. It performs no name resolution of any kind, and the paths it opens do not exist on the disc, so retail never takes it. Note the opening gate in both functions is the unrelated word `_DAT_8007B868`; they reach `_DAT_8007B8C2` further into the body.

## VAB sound banks

Sony's standard `VABp`-magic instrument bank format. Documented at [`formats/vab.md`](../formats/vab.md). The dominant on-disc carrier is the [scene-VAB-prefixed streaming](../formats/scene-bundles.md) shape - the VAB body is preceded by a 4-byte chunk0 header. Implementation: `crates/vab` (header parser + extractor + ADPCM decoder).

Bulk scan finds 1191 `VABp` headers across 239 PROT entries. Multi-bank archives at `0889_sound_data2`, `0890_sound_data2`, `0891_level_up`. The `vab_01` cluster (CDNAME indices 1072–1194) is the standard distributed-bank layout.

## Per-actor sound effects

`FUN_800250D4(sound_id, voice)` is the per-actor SFX trigger called from the actor tick (`FUN_80021DF4`) when `actor[+0xb4] != 0` (one-shot pulse) or `actor[+0xac]` is staged (continuous). It looks up a sound entry at `&DAT_8006F198 + sound_id*8` for `sound_id < 0x200`, or in the runtime-allocated table at `_DAT_8007B8D0` for higher IDs (the `.dpk` consumer's bank). The entry's `byte[3] & 0x1F` is the voice count; the helper then calls `FUN_800653C8` (libSPU `SpuKeyOn`-equivalent) for each of `voice..voice+count-1`.

`actor[+0xac]` (sound ID) and `actor[+0xb0]` (voice) are written by move-VM and field-VM opcodes; the move-VM tick in `FUN_80021DF4` re-fires the SFX whenever the trigger flag at `actor[+0xb4]` is set.

The static `&DAT_8006F198` table is **100 8-byte descriptors** (sound ids `0x00..=0x63`); the `< 0x200` runtime check is a bound, not the size (id `0x64` onward is the `\PSX.EXE` dev-path rodata). Besides `FUN_800250D4` above, the cue-ring drainer `FUN_80016B6C` reads it and programs each voice via `FUN_80065034` (the libsnd `SpuSetVoiceAttr` analogue). Each entry decodes as `[+0 program][+1 tone/region base][+2 note-level][+3 voice-count + sustained bit 0x20][+4 channel]`; full layout + provenance on [`docs/formats/sfx-table.md`](../formats/sfx-table.md). Parser `legaia_asset::sfx_table` (disc-decoded, byte-exact vs live save-state RAM); the SPU programming itself is libsnd, out of clean-room scope.

## VAB slots - one installer, twelve records

Every bank retail opens goes through one installer, and its argument is the same
category byte the SFX descriptors carry. `FUN_8001FC00(raw_toc_index, category,
buf, append, len)` streams the entry into a staging buffer; **`FUN_8001E54C(category,
buf, len)`** then walks the streamed chunk list and installs it, taking the
header buffer from the 12-byte mixer record at `0x80091508 + category*12` (`+0`)
and the VAB slot from that record's `+8`, and opening the bank through
`FUN_8002630C` → `SsVabOpenHead` (sticky, at the SPU address the per-slot table
at `0x800917B0` holds) → `SsVabTransBody`. So every call site's
`FUN_8001FC00` / `FUN_8001E54C` pair names one `(PROT entry, slot)` binding
outright.

| Slot | Bank | Installed by |
|---|---|---|
| `0` | PROT 0868 system bank | resident |
| `1` | the current BGM bank (`music_01`, variable) | `FUN_800243F0` |
| `2` | PROT 0869 class-2 bank (`0875` alternate) | `FUN_800520F0`, `FUN_801CF00C` |
| `3` | a `vab_01` side-band bank (variable) | `FUN_800243F0`, from `_DAT_8007BABC` |
| `6` | PROT 0876 field bank | field init `FUN_801D6704` |
| `7` / `8` | the two `monster.snd` banks | `FUN_8003E104` (below) |
| `11` | PROT 0889 reward bank | `FUN_8004E568` |

The record initialiser `FUN_8001D424` writes `+8 = record index` for all 16
records, then assigns their header buffers from one base with four pairs sharing
one - and `FUN_800265E8` gives those same pairs one SPU base. Slot 6 and slot 2
are therefore the same physical bank in two modes, which is why retail needs no
extra SPU room for the field cues. Slot sizes, the SPU map and the structural
checks behind each pin: [`formats/sfx-table.md`](../formats/sfx-table.md#which-prot-entry-reaches-which-slot).

The seeder and the reader are ported at opposite ends and never meet:
`legaia_engine_core::scus_leaf_kernels::seed_boot_offset_table` writes the
twelve-word image and `legaia_asset::sfx_table::spu_base_for_slot` answers the
query the engine's audio host actually makes, returning the same values as
literals. `crates/engine-core/tests/infra_boot_offset_table.rs` asserts the two
agree slot by slot, including the one slot the seeder skips and the four
aliased pairs, so the duplication is guarded rather than silent.

## Monster sound bank - `h:\mpack\monster.snd`

Battle-time monster sound banks live in a single packed `monster.snd` file. The loader is `FUN_8003E104(monster_idx, slot, dst_buf)` - called twice from the battle scene loader `FUN_800520F0` (slots 7 and 8, for the active battle's two monster sound banks). It reads the file's per-monster TOC at `0x801C8980 - 0x10` (4-byte stride, paired entries giving `[start_lba, end_lba+1]`), computes the LBA range, and dispatches:

The gate is `beq v0,zero,0x8003E25C` at `0x8003E1FC`, so the **zero** arm is the one that jumps to the path-based open:

- **Dev path** (`_DAT_8007B8C2 == 0`) - `0x8003E25C` onward, using the host-trap file API: `FUN_800608F0` (`break 0x103`) → `FUN_80060920` (fseek to record × 0x800) → `FUN_80060944` (fread) → `FUN_80060910` (fclose). Path string: `h:\mpack\monster.snd`.
- **Retail path** (`_DAT_8007B8C2 != 0`, the fall-through) - runs `FUN_8003EE7C` / `FUN_8003ED04`, stages `(size, dst)` into the gp window at `+0x97c` / `+0x894`, kicks the async CD read via `FUN_8003F128`. Sets a 120-frame timeout at `+0x91c`.

The same pattern (`h:\mpack\…` paths + per-record TOC at a small data structure) is the shape we expect for the rest of the still-TBD audio formats - read the `FUN_8003E104` dump as the canonical example.

## BGM dispatch

The field VM's opcode `0x35` writes the BGM ID to `_DAT_8007BAC8`. `FUN_800243F0` (the per-frame asset poller) resolves it to a PROT index - `bgm_id < 2000` is scene-local, `bgm_id >= 2000` is a global pool. There's no literal BGM table; the resolution is a PROT-relative offset into the [CDNAME](../formats/cdname.md) per-scene block.

See [`subsystems/script-vm.md`](script-vm.md) → "BGM lookup table" for the resolver code. For the human-readable map between each track's debug sound-test ID, the scene it plays in, and its official OST title, see [`reference/music-tracks.md`](../reference/music-tracks.md).

### Resolver arithmetic

`FUN_800243F0` reads the id at `_DAT_8007BAC8` and branches on `slti … 0x7d0`:

| Branch | Resolved PROT index | Globals |
|---|---|---|
| `bgm_id < 2000` (scene-local) | `*(0x80084540) + 6 + bgm_id` | `0x80084540` = scene block base |
| `bgm_id >= 2000` (global pool) | `*(0x8007BC64) + (bgm_id - 2000)` | `0x8007BC64` = `music_01` bank base |

The result is stored to `0x8007BAB8` and compared against the currently-loaded
index at `0x8007BA9C`, so a re-select of the playing track is a no-op. Both
laws are readable at runtime: on a running retail image `0x8007BC64` holds
`990` - the **raw** in-RAM TOC pool base. Extraction-frame indices run two
below raw (the +2 filename skew, see [`../formats/cdname.md`](../formats/cdname.md)),
so the bank's low range sits at extraction `988`; the engine maps a sound-test
index to its extraction entry through the piecewise
`legaia_engine_core::music_labels::prot_entry_for_bgm_id` (a 2-entry gap at
extraction `1056`/`1057` splits it, see [`../reference/music-tracks.md`](../reference/music-tracks.md)).

### Which track a scene plays

The track is **script-selected, not table-driven**: nothing maps a scene to a
track. The scene's own event script picks it with an op-`0x35` operand, so the
resolution is recovered by running the scene's prescript and observing the
emitted id - `crates/engine-shell/tests/bgm_scene_resolution.rs` does this
across the CDNAME corpus.

The law that sweep establishes: **every scene that starts BGM selects a
global-pool id.** The scene-local branch of the resolver is never taken by a
field scene, and a scene's own `scene_vab_stream`-wrapped SEQ
(`SceneAssets::seq_in_stream_entries`) is *not* its music source. Attempts to
identify a playing track by fingerprinting it against the bank fail for this
reason - the scene-local corpus they search is the wrong one.

A linear disassembly walk over a scene's event records is **not** a substitute
for running the prescript: it decodes data bytes as instructions and yields
implausible ids (values far outside the `2000..=2077` band) mixed in with the
real ones.

**The sweep sees a scene's entry track and nothing else.** A prescript emits
op `0x35` sub-op 1; a scene's *cutscenes* change music with sub-op 9, from
partition-2 timeline records the sweep never reaches. So "which track does
scene X play" has more than one answer per scene, and a defect confined to
the sub-op 9 path is invisible here - see
[`script-vm.md`](script-vm.md#sub-op-9-is-a-start-not-a-queue).

### The track-swap handshake (`FUN_800243F0` + op-`0x35` sub-op `0xA`)

The resolver above is one stage of a staged swap protocol. `FUN_800243F0`
runs a stage counter at `gp+0x744` through a 7-entry jump table
(`0x800108C8`), entered only while `_DAT_8007BAB8 != _DAT_8007BA9C` (a track
change is in flight). The stages: wait for CD idle and arm a 30-frame settle
delay (`gp+0x768 = 0x1E`); tear down + close the BGM slot `0x8007052C`
(`FUN_800266E0` + `FUN_80026520`) **unless** `_DAT_8007B750` bit 0 is set,
then kick the async payload load (`FUN_8001FC00`); count the settle delay
down; install the SEQ (`FUN_8001E54C`); re-attach the slot
(`FUN_80026478`) **unless** bit 1 (pause) is set; latch
`_DAT_8007BA9C = _DAT_8007BAB8`.

The sound flag word `_DAT_8007B750` coordinates it with the script. The full
writer census (SCUS + every based overlay image; store-offset scan, see
[`../tooling/address-reference-scan.md`](../tooling/address-reference-scan.md)):

| Bit | Meaning | Set by | Cleared by |
|---|---|---|---|
| 0 | script-owned start pending (defer the slot teardown to the script) | sub-op 9 (`0x801E0260`) | poller commit (`0x8002472C` clears 0/3/4), scene entry `FUN_8003AEB0` |
| 1 | BGM slot paused / detached | sub-op 2 (`0x801E0150`), sub-op 3 (`0x801E0174`), dance overlay `0x801CF328` | sub-op 1, sub-op 4, sub-op `0xA`, scene entry, game-over `FUN_8003C7EC` |
| 2 | script flag (opaque to the sound side) | sub-op 6 (`0x801E01D8`) | field overlay `0x801D7348` |
| 3 | **load settled** - payload staged and the settle delay elapsed | the poller, one site only: `0x800246D0` (`\| 8`) | poller commit `0x8002472C` |
| 4 | **release-ack** - script has released the old slot occupant | sub-op `0xA` (`0x801E02B8`) | poller commit `0x8002472C` |

Bits 3 / 4 / 0 are the handshake: after setting bit 3 the poller **stalls**
(`0x800246E0..E8`) while bit 0 is set and bit 4 is not - i.e. when the swap
was started by sub-op 9, the old track keeps its slot until the *script*
commits. Sub-op `0xA` (arm `0x801E0264`) is that commit: it waits for bit 3,
releases the paused occupant (`FUN_800266E0` detach + `FUN_80026520` close -
the close is what the poller's own teardown also does; `FUN_80026520`
additionally clears the source's active flag and `SsSeqClose`s the handle
where `FUN_800266E0` only rewinds and detaches), then sets bit 4 and clears
bit 1. So a cutscene picks the exact beat the outgoing score dies on, and a
port that honours the sub-op 2 pause but drops the `0xA` commit leaves the
music paused after the cutscene. The arm's early-return when
`_DAT_8007B868 != 0` mirrors the whole actor-sound family (`FUN_800266E0` /
`FUN_80026520` / `FUN_80026478` all no-op behind the same gate): that word
has **no setter in SCUS or any based overlay** - its only store is a bit-1
clear in the boot mode-init `FUN_8001DCF8` - so it reads as the dev/dual-mode
flag it is everywhere else, zero in retail play.

Engine side: the host resolves BGM bytes synchronously, so bit 3's wait is
satisfied on arrival (the same reasoning as sub-op 9's barrier) and
`SceneHost::route_bgm_events` routes sub-op `0xA` straight to
`BgmDirector::unhalt_pause` - release the source only if the pause latch is
still set, then clear the latch unconditionally. `see
ghidra/scripts/funcs/800243f0.txt`, `800266e0.txt`, `80026520.txt`.

### Entry-script pauses and free-roam picker staging

A scene's entry script can start its track and immediately pause it for a
story moment: town01's `P1[0]` starts global id 2016 then, while system flag
`0x225` is clear, issues a sub-op 2 pause (`+0x5D..+0x91` of the record) -
the opening's silent dawn, repaired by the opening cutscene records' own
sub-op 9 restarts (`35 E7 07 09`, `35 E0 07 09` + sub-`0xA` commits in the
same MAN). The retail s3 free-roam capture confirms both halves: flag `0x225`
still clear, pause bit `_DAT_8007B750` bit 1 clear.

A scene-picker / `--scene` entry runs the entry script with none of that
choreography behind it, so the authored pause would park the BGM forever
("the music dies a second into the scene"). The engine's free-roam staging
(`World::seed_free_roam_story_baseline`) drops a sub-op 2 issued inside the
scene-entry window on picker entries only; the new-game chain keeps the
authored pause. The same staging seeds story-twin scenery flags (e.g.
`town0c`'s blown gate). Disc pins: `engine-core/tests/free_roam_staging_disc.rs`.

The engine port reuses this same dispatch for the **Battle↔Field music swap**: `World::set_battle_bgm` configures a battle track id, and the live gameplay loop queues an ordinary `FieldEvent::Bgm{sub_op: 1}` start for it on encounter (`swap_to_battle_bgm`) and resumes the stashed field track on battle end (`restore_field_bgm`). Both transitions run through the host's `AudioBgmDirector` `start_inner` path - no separate battle-audio code path. The battle id must resolve in the current scene's BGM table since the live loop doesn't load a distinct battle audio bundle.

Retail BGM changes are **hard cuts** (or short `SsSeqSetVol` ramps), so
`start_inner` swaps tracks the faithful way: when a track is already playing it
calls `AudioOut::swap_bgm`, which key-offs the outgoing sequencer (its notes
release through their own ADSR envelopes, so nothing hard-cuts to a
discontinuity) and installs the new sequencer to tick from its first event that
same instant. The incoming track's intro is audible immediately - only a brief
click-guard fade-in on the SPU master (a couple of frames) softens the onset.
This replaces the earlier serial cross-fade (`crossfade_to`), which faded the
old track down to silence *before* installing the new one and then faded that
back up, holding the incoming intro near-silent for its first half-second - both
an artifact and less faithful than retail. `crossfade_to` and its `pending_seq`
fade-out-then-swap machinery remain for callers that genuinely want a symmetric
cross-fade; BGM transitions no longer use it.

### Global-pool BGM: the `music_01` bank

Every real music track on the disc lives in the **`music_01` bank**, not in scene-local slots - scenes carry no SEQ of their own (see [`reference/music-tracks.md`](../reference/music-tracks.md) for the sound-test join). A global-pool id (`>= 2000`) is `2000 + slot`, and each bank entry is one self-contained `[VAB][SEQ]` pair (a chunk-header, a `pBAV` VAB body, then a `pQES` score). The bank is **piecewise** in extraction space (`988 + i` for index `i <= 67`, `990 + i` for `i >= 68`, a 2-entry gap at `1056`/`1057`); `music_labels::prot_entry_for_bgm_id` owns that map. Playing one means uploading **that entry's own VAB** into SPU RAM and driving the sequencer against it, rather than the scene VAB the field path stages.

The site's minigame pages take exactly this path per game (`crates/web-viewer/src/minigames.rs`): `render_music01_bgm` / `render_music01_loop` split the pair, `VabBank::upload` the VAB, and render through the clean-room `Spu` + `Sequencer` - the same components the live `AudioBgmDirector` uses. Minigame BGM sources are disc-pinned extraction constants (base-independent): the Baka Fighter init loads extraction 1043 (#55 `M112` "Sol disco fever"); the dance overlay loads extraction 1048/1054 (#60/#66, the Sol disco finals, mode-selected, see [`minigame-dance.md`](minigame-dance.md)); the slot machine and fishing/Muscle Dome start **no** track and inherit their host scene's op-`0x35` BGM. The `music01_bgm_render` WASM surface renders any bank slot for the dance's Sol-disco jukebox.

## SsAPI sequencer (`0x80061-0x80067` cluster)

Legaia statically links Sony's PsyQ **libsnd / SsAPI** sequencer for `.SEQ`-driven music. The cluster lives in SCUS at `0x80061B18..0x800681D8` and uses the standard SsAPI globals.

### Globals

| Global | Role |
|---|---|
| `_DAT_801CD2B8` | 16-bit slot-allocation bitmap (`MAX_SEQ_SLOTS = 16`). |
| `_DAT_801CD2C0[16]` | Per-slot pointer table - each entry points at a `0xB0`-byte SsAPI sequence-state struct. |
| `_DAT_801CD2C0[i] + 0x58/0x5A` | Per-slot vol/pan, clamped `0..0x7F`. |
| `_DAT_801CD2C0[i] + 0x88` | Running tick (advanced by the varint delta-time decoder). |
| `_DAT_801CD2C0[i] + 0x98` | Per-slot status flags (bit 0 = paused, bit 1 = active/playing, bit 2 = stopped, bit 3 = end-of-sequence, bit 4/5 = volume-ramp scheduling, bit 8 = ramp lock, bit 0xA = repeat). |
| `_DAT_801CE060` | Per-voice flag bank (32 voices, bit-packed). |
| `_DAT_801CE080..AC` | Voice-attribute slots (per-voice pitch + vol working state). |
| `_DAT_801CE088[voice]` | Voice base-note table (stride 2). |
| `_DAT_801CE204` | Ring index (0..15) into `_DAT_801CE208`, advanced once per `FUN_80065BAC` flush. |
| `_DAT_801CE208` | **16-word silent-history ring**: one word per recent flush frame, bit `v` set when voice `v`'s envelope read zero that frame. AND of all 16 = "silent 16 consecutive frames", the condition that unreserves a voice. (Not a free/busy bitmap - that earlier reading came from the gap-map fingerprints and is corrected by the per-instruction read.) |
| `_DAT_801CDB50` | Per-voice driver records (24 × stride `0x36`): `+0x02` allocation age, `+0x06` live envelope level, `+0x1A` note priority, `+0x1D` in-use marker. The state the allocation scan (`FUN_80066B00`) reads. |
| `_DAT_801CE362` | Chosen-voice halfword: the allocation scan's winner, consumed by `_SsVoKeyOnDirect` (`FUN_80065978`). |
| `_DAT_801CDB48 / _DAT_801CDB4A` | **Key-ON mask accumulator** (lo/hi 16 of the 24-voice key-on word). OR'd by the voice-alloc path, flushed to the SPU by `FUN_8006C048`, cleared at flush. Register-for-register the retail twin of `engine-audio`'s `Spu::key_on_mask`. |
| `_DAT_801CDB4C / _DAT_801CDB4E` | **Key-OFF mask accumulator** (lo/hi 16), set by the release sweep. Twin of `Spu::key_off_mask`. |
| `_DAT_801CE248 / _DAT_801CE24A` | Currently-sounding voice mask (lo/hi 16). |
| `_DAT_801CE2E8` | Pitch transpose base. |
| `_DAT_801CE334` | Program region table (stride `0x10`). |
| `_DAT_801CE344` | Sequence-active voice scan target. |
| `_DAT_8007A940` | 12-entry MIDI-key pitch table (used by `FUN_80066E50`). |
| `s_Can_t_Open_Sequence_data_any_mor_80015D34` | Error string emitted by `FUN_80062340` when the slot bitmap is full. |
| `s_This_is_not_SEQ_Data_*` / `s_This_is_an_old_SEQ_Data_Format_*` | Header-validation strings emitted by `FUN_80062410`. |

### Public SEQ API

| Function | Role |
|---|---|
| `FUN_80062340(seq_data, slot_hint)` | `SsSeqOpen` - walks the slot bitmap, marks the first free slot, calls `FUN_80062410`. Returns slot ID or `-1`. |
| `FUN_80061D18(slot)` | `SsSeqClose` - calls `FUN_80067E9C(slot,0,0,1)` + `FUN_800684CC`, clears bitmap bit, memsets all 16 channel records (size `0xB0`) to defaults (vol=`0x7F`, pan=`0x7F`). |
| `FUN_80061E94(seq_id)` | `SsSeqClose` short-arg shim - sign-extends, tail-calls `FUN_80061D18`. |
| `FUN_8006275C(slot,0)` | `SsSeqPlay` - clears flags 0/3 in `+0x98`, sets bit 1. Start-from-beginning. |
| `FUN_8006282C(slot)` | `SsSeqPlay` 1-arg shim - tail-calls `FUN_8006275C(slot,0)`. |
| `FUN_80062880(slot, mode, arg)` | Pause/Resume shim - tail-calls `FUN_800628F0(slot,0,mode,arg)`. |
| `FUN_800628F0(slot,_,mode,_)` | `_SsSeqCtrl` - `mode==1` resets read pointer, sets flag `0x1`, calls `FUN_80067E9C`; `mode==0` sets flag `0x2`; otherwise clears both. The Stop / Pause / Resume state core. |
| `FUN_800641EC(slot, channel)` | `SsSeqRewind` / `SsSeqReplay` - clears flags `0x1/0x2/0x8/0x400`, sets `0x4`, full slot reset to start. |

### SEQ internals

| Function | Role |
|---|---|
| `FUN_80062410(seq_data)` | `_SsSeqInit` - validates `'S'`/`'p'` magic + version byte `0x01`, reads PPQN base (`0x393_8700` = 60 000 000), BPM, ticks-per-quarter from the SEQ header. |
| `FUN_80061C68(slot)` | `_SsSeqGetVar` - MIDI-style 7-bit-with-continuation varint decode for delta-time bytes; accumulates into `+0x88` running tick. |
| `FUN_80061EDC(slot, channel, vol, ...)` | `SsSeqSetVol` - calls `FUN_800683D8` to fetch `(vol_l, vol_r)`, clamps target ≥ requested, calls `FUN_8006206C` (slewer), sets bit `0x20`, clears bit `0x10` in `+0x98`. |
| `FUN_8006206C(...)` | `_SsSetSlideVolume` - ramp from→to over N ticks. Touches `+0x48/0x4A/0x9C/0xA0/0x4C`, signed-divide per-tick delta. Gated by flags `4 & 0x100` in `+0x98`. |
| `FUN_8006171C(vab, prog, ev)` | Per-program SEQ controller/meta dispatch - post-increments the program's stream cursor (`_DAT_801CD2C0[vab] + prog*0xB0`, deref `+0`), switches on the event byte `ev`, routes through the installed handler vector `_DAT_801CD238..248`, and falls to the varint decoder `FUN_80061C68` for value events, storing the result at `+0x90`. |

**Per-frame tick call graph.** The concrete chain behind the prose "hand the
payload to `FUN_80062340` for playback": `FUN_80062F98` (per-slot fan-out) →
`FUN_8006320C` / `FUN_8006352C` (the **volume-slide ticks** over
`_DAT_801CD2C0[slot]` - see below) → `FUN_80067E9C` (`_SsSeqNoteOn`) →
`FUN_80066308` (note-trigger dispatch; `×0x81` velocity scale, per-slot status
`_DAT_801CE34x`) → `FUN_80066B00` (voice-allocation scan) → `FUN_80065978`
(`_SsVoKeyOnDirect`), with `FUN_80065BAC` / `FUN_800675C8` (the voice flush /
release sweep below) carrying the result to the SPU. The SEQ-stream cursor
advances through `FUN_80063CEC` (calls the varint decoder `FUN_80061C68`, steps
`_DAT_801CD220..230`). The per-`+0x98`-flag-bit map of everything `FUN_80062F98`
fans out to is tabulated once, in
[`reference/functions/audio.md`](../reference/functions/audio.md); this page
carries only the handlers whose labels had been wrong.

**The volume-slide pair.** `FUN_8006320C` and `FUN_8006352C` are the
**ascending** and **descending** halves of one slide, not note or expression
handlers. Three things identify them together. They read exactly the field set
their installer `FUN_8006206C` (`_SsSetSlideVolume`) writes - `+0x48` / `+0x4A` /
`+0x4C` / `+0x9C` / `+0xA0`. They fetch `(vol_l, vol_r)` through `FUN_800683D8`,
the same helper `SsSeqSetVol` uses. And their arithmetic mirrors: `FUN_8006320C`
adds the step and bumps both sides (`addu` at `0x8006331C`, `addiu …,1` at
`0x8006332C` / `0x80063340`), `FUN_8006352C` subtracts it and lowers both
(`subu` at `0x8006363C`, `addiu …,-1` at `0x8006368C` / `0x80063698`).

### The calc tier's two shared conventions

Everything `SsSeqCalc` fans out to is ported across two modules: the envelope
kernels as `legaia_engine_audio::seq_calc`, and everything that reads a stream
byte - the start / stop arms, the delta-time pump and the event decoder - as
`legaia_engine_audio::seq_events`. [`Sequencer`](#engine-audio-model---sequencer-port)
remains the engine's playback replacement for the tier; these kernels are the
reference it has to agree with, and `note-trace --seq-calc` is the host that
runs them over a real `music_01` SEQ body. Two conventions recur across the
whole family and are worth stating once.

**The flag word is re-read from memory before every test.** `FUN_80062F98` does
not snapshot `+0x98`; it reloads it ahead of each `andi`. So a handler that
clears its own bit is observed immediately by the next test, and the dispatch is
a sequence of decisions rather than one decoded mask. Two consequences fall out
of that directly. The bit-`0x4` arm runs `SsSeqRewind` and then **zeroes the
whole word**, so the `0x200` "finished" flag `FUN_80063AA8` sets on a track's
last repeat - alongside `0x4` - never survives into the next frame. And because
`0x40` and `0x80` both dispatch to the tempo slide, a tempo tick that does *not*
settle leaves both bits standing and is therefore called **twice** in one frame,
while one that settles clears both and is called once.

**The sign of a step field selects the rate mode, not the direction.** Both the
volume slide (`+0x4C`) and the tempo slide (`+0x4E`) read a signed step and
branch on `blez`. A **positive** step means "move one unit every `step` ticks" -
the tick is gated on `remaining % step == 0` and skips entirely otherwise. A
**non-positive** step means "move `|step|` units every tick", clamped at the
target. Direction is carried by the function (`FUN_8006320C` up toward
`(0x7F, 0x7F)`, `FUN_8006352C` down toward `(0, 0)`) or by the target
(`+0xAC` for tempo), never by the step's sign. A `step` of exactly `0` lands in
the second arm and moves nothing.

### Where wall-clock tempo becomes an integer tick step

`FUN_800649B0`'s tail is the single place the sequence's tempo turns into the
per-frame budget the delta-time pump spends:

```text
+0x54 = (+0x50 * +0x94 * 10) / (*0x801CD2BC * 60)      ; unsigned divide
if ((s16) +0x54 <= 0) +0x54 = 1
```

`+0x50` is the sequence resolution (ticks per quarter), `+0x94` the current
tempo, and `0x801CD2BC` a runtime divisor. The floor at `1` is what keeps a very
slow tempo from stalling the pump outright. The multiply is signed on `+0x50` but
the divide is `divu`, so a negative tempo yields a huge quotient rather than a
negative one, and the `i16` truncation is what the floor then catches.

The recompute is skipped entirely on the sub-step early-out (a positive `+0x4E`
off its boundary returns before reaching the tail), so the budget only moves on
frames the tempo itself moved.

The shape `(ticks/quarter × beats/minute × 10) / (divisor × 60)` reads as tenths
of a tick per frame with `divisor` the frame rate, which would make `+0x54` a
fixed-point ×10 quantity. `0x801CD2BC` itself has still not been read from a
live capture, so the port takes the divisor as a parameter and bakes no `60` in.
The **×10 half of that reading is no longer an inference**, though: the varint
delta-time reader `FUN_80061C68` multiplies every decoded delta by `10` before
returning it and before accumulating it into `+0x88`, so the pump's
`+0x90 >= +0x54` comparison is tenths against tenths on both sides. Two
independent routines agreeing on a scale is a measurement; one formula's shape
was not. The remaining risk is the divisor alone - and it matters, because the
engine `Sequencer` clocks in exact integer SPU samples, so a wrong constant here
is an audible tempo error that stays perfectly self-consistent under any test
written against the same wrong constant.

### The decoder does not consume a whole event

`FUN_80063CEC` reads a status byte, latches running status at `+0x16` and the
channel nibble at `+0x17`, and reads only *some* of the operands: two plus the
delta-time for `0x9n`, one for `0xBn` / `0xCn`, one skipped unread for `0xEn`,
and the kind byte for a meta. The rest of each event belongs to the **installed
handler** it tail-calls through the 17-entry vector `FUN_80026234` writes at
`0x801CD220`:

| class | vector slot | handler | further operands | reads the delta |
|---|---|---|---|---|
| `0x9n` note | `+0x00` | `FUN_80061B24` | 0 | no - the decoder did |
| `0xCn` program | `+0x04` | `FUN_80061BF8` | 0 | yes |
| `0xEn` bend | `+0x08` | `FUN_8006166C` | 1 | yes |
| `0xFF` meta | `+0x0C` | `FUN_80061954` | 3 | yes |
| `0xBn` control | `+0x10` | `FUN_8006171C` | 1 | yes |

So the stream is the conventional `[status][operands][delta]`, and a walker
needs both halves. Reading the decoder alone as a complete event consumer is
wrong and fails visibly: every program change comes back paired with a phantom
running-status program change whose operand is `0`, because the trailing delta
byte is re-decoded as the next status. `FUN_80062410` (the SEQ open) reads the
body's **leading** delta before the first frame, so a host seeding a channel by
hand has to do the same.

`FUN_80061954` reads its three bytes as a big-endian value and computes
`60000000 / v` into `+0x94`, which independently confirms the tempo meta's
three-operand, no-length-byte layout recorded in
[`seq.md`](../formats/seq.md).

**Correction** (label ≠ role): `FUN_8006352C` / `FUN_8006320C` were tagged
elsewhere as "fixed-point div" pitch kernels. Neither is a pitch kernel - but the
earlier stated reason, that they carry no division, is itself wrong. Each carries
exactly one `div`, and it is a **modulo of the slide tick counter, not a
fixed-point pitch divide**: `FUN_8006320C` at `0x8006329C..0x800632C4` and
`FUN_8006352C` at `0x800635BC..0x800635E4`, both dividing the just-decremented
remaining-tick counter `+0xA0` by the signed per-tick step `+0x4C` and reading
the **remainder** back with `mfhi`. A non-zero remainder skips the update, so the
pair is a sub-tick divider - one volume unit every `N` ticks rather than `N`
units every tick. The divisor is positive on that path: a `blez` diverts a
non-positive step to its own arm first. The fixed-point note→pitch math is
confined to `FUN_80066E50` (`_SsPitchFromKey`) and `FUN_8006C6E4`
(`_SsKey2Pitch`); no additional pitch kernel exists in this cluster.

**Track end is a loop-repeat chain, not a vab release.** `FUN_80063AA8` handles
the last repeat of a track by **chaining to another `(slot, channel)`** named by
its own `+0x22` / `+0x23` bytes: `beq +0x22, 0xFF` at `0x80063C84` skips the
chain, otherwise `FUN_80064090(+0x22, +0x23)` starts the successor and `+0x14` is
zeroed. Both arms then kill the finished track's notes through `FUN_800684CC`.
Nothing in the body releases a VAB.

### Voice / mixer (audible-output critical path)

| Function | Role |
|---|---|
| `FUN_80067550(voice, key, vel, ...)` | `_SsVoNoteOn` - the key-on volume chain: `vel × bank_mvol(hdr+0x18) × 0x3FFF / 0x3F01`, then `× prog_mvol(801CE352) × tone_vol(801CE355) / 0x3F01`; seq path folds channel vol L/R (`+0x58/+0x5A`, `/0x7F` per side), then three one-sided pan attenuations (tone pan, prog `mpan`, staged channel pan), a mono fold on `_DAT_801CE330`, and - seq path only, not SFX slot `0x21` - a closing square taper `v²/0x3FFF` per side. Writes `&DAT_801CE080[voice]`, flags `0x7`, active-voice masks `_DAT_801CDB48/4A/4C/4E` + `_DAT_801CE248/24A`. Engine port: `VabBank::fire` (head + pans) and `sequencer::channel_mix` (channel fold + taper). |
| `FUN_80067E9C(slot, vol, pan, ...)` | `_SsSeqNoteOn` - iterates `DAT_801CE344`, calls `FUN_80068B98` (the VAB program-change - see the [SsApi seq-management layer](#ssapi-seq-management-layer-above-libspu)), runs the same vol/pan chain as `FUN_80067550`. Sequence-driven keyon. |
| `FUN_80065978(...)` | `_SsVoKeyOnDirect` - consumes the **already-chosen** voice at `_DAT_801CE362` (the `FUN_80066B00` scan's winner): clears that voice's bit from all 16 silent-history ring words at `_DAT_801CE208`, sets its envelope word to `0x7FFF`, looks up region in `_DAT_801CE334` (stride `0x10`), writes pitch + base note to `&DAT_801CE088 + voice*2`, ORs flags `0x8/0x30` into `&DAT_801CE060`. |
| `FUN_80066E50(key, fine)` | `_SsPitchFromKey` - indexes 12-entry pitch table `&DAT_8007A940`, octave-shift by `(oct-5)`. Returns 16-bit SPU PITCH register value. |
| `FUN_80065B88` | `SsResetTranspose` - single-store stub: zeros `_DAT_801CE2E8` (a base-note offset shifted in by `FUN_80065978`). |

### VAB attribute accessors + utility note triggers

Between the sequencer event loop and the raw voice registers sits a band of SsAPI utility accessors (the `SsUt*` family shape) that copy VAB metadata in and out of the open-bank tables, gated on the per-vab open-state byte `_DAT_801CE368[vab] == 1` (they return `-1` when the bank is closed). These are the retail source of the tone/program attributes `crates/engine-audio`'s `VabBank` reads at upload and play time.

| Function | Role |
|---|---|
| `FUN_80064CF0(vab, prog, out[8])` | Program-attribute getter - copies the 8-byte ProgAtr record at `_DAT_801CE334 + prog*0x10` (mvol / mpan / prior / mode) into the caller buffer. |
| `FUN_80064DF8(vab, prog, note, out[0x18])` | Tone-region **getter** - selects the tone page via `FUN_80068B98`, then copies the 0x18-byte tone descriptor at `_DAT_801CE340 + (note + tone_page*0x10)*0x20` (ADSR words, pitch, SPU addr, pan) into the buffer. |
| `FUN_800655CC(vab, prog, note, in[0x18])` | Tone-region **setter** - the exact mirror of `FUN_80064DF8`, writing the 0x18-byte block back into the tone table. |
| `FUN_8006861C(packed, seq, prog, vel, dur)` | Utility velocity key-on - runs the full `FUN_80067550` vol/pan chain (bank/prog/tone vol, channel vol `+0x58/+0x5A` each `/0x7F`, three one-sided pan attenuations) after matching the voice-driver record (stride `0x36` at `_DAT_801CDB50`) on `(seq, prog, note)`. |
| `FUN_80067A1C(voice, seq, note, prog, wheel)` | **Pitch-bend apply** - offsets the key by the sounding tone's own bend range (`tone+0xD` = pbmax for `wheel > 0x40`, `tone+0xC` = pbmin for `wheel < 0x40`), calls `FUN_80066E50` (`_SsPitchFromKey`), writes the SPU PITCH to `&DAT_801CE084[voice]` and ORs flag `0x4` into `&DAT_801CE060`. |
| `FUN_80066F4C(voice)` | **Per-voice vol/pan/reverb recompute** - re-derives one sounding voice's L/R from the current channel vol (`prog_attr +0x58/+0x5A`), prog/tone vol, three pan attenuations and the `_DAT_801CE330` mono fold, commits reverb depth via `FUN_8006AA90(_DAT_801CE34A - _DAT_801CE358)`, and stages the result into `&DAT_801CE080/082[voice]` with flags `0x3`. |

`FUN_80067A1C` is the retail source for the per-tone pitch-bend range the engine ports as `VabBank::pitch_bend_range`: the wheel scales by the *sounding tone's* own `pbmin`/`pbmax` bytes, so a `(0,0)`-range tone does not bend - exactly the law [`engine-audio`'s sequencer](#engine-audio-model---sequencer-port) applies. `FUN_80066F4C` is the retail twin of `sequencer.rs`'s `remix_channel` (re-derive every sounding voice on a mid-note CC7/CC10 change) - the same vol/pan chain as `FUN_80067550`, run standalone rather than at note-on. Provenance: `see ghidra/scripts/funcs/80064cf0.txt`, `80064df8.txt`, `800655cc.txt`, `8006861c.txt`, `80067a1c.txt`, `80066f4c.txt`.

### The key-on pitch law - `note` against the tone's `center`

Both key-on paths converge on one arithmetic, and it is the value written
straight into the SPU voice's pitch register - nothing rescales it afterwards.

```text
  fine, carry = per-path from the tone's shift byte
  n           = note + 60 - center + carry
  pitch       = PITCH[(n % 12) * 16 + fine]  shifted by  (n / 12 - 5)
```

`PITCH` is the 192-entry `u16` table at `DAT_8007A940` (`SCUS_942.54` file
offset `0x6B140`), which is exactly `floor(0x1000 * 2^(k/192))` for every
entry - one octave at 1/16-semitone resolution. `n / 12` and `n % 12` truncate
toward zero (MIPS `div`). So `note == center` selects `PITCH[0] = 0x1000`:

- **Unity is 44.1 kHz, and it is what a tone plays at on its own centre note.**
  There is no separate source-sample-rate factor. A 22.05 kHz VAG body is
  authored with `center` twelve semitones above the key it is meant to sound
  at, so the same law lands on `0x800`. Folding a `22050/44100` ratio in as
  well - the shape a "libspu key-to-pitch formula" write-up invites - puts
  every voice, BGM note and sound effect alike, an octave low.
- **`shift` raises the pitch** by `shift/128` of a semitone, quantised to 1/16.
  It is the tone's fine-tune, positive, in 1/128-semitone units - not
  centi-semitones and not a downward correction.

The two paths differ only in how the fine index is formed:

| Path | Entry | Fine index |
|---|---|---|
| Sequencer note-on | `FUN_80066308` → `FUN_80066d8c` | `min(shift >> 3, 15)`; saturates, never carries a semitone |
| SFX / direct key-on | `FUN_80065034` → `FUN_80066e50` | `(0x40 + shift) >> 3`; `>= 16` carries one whole semitone and keeps the remainder |

`FUN_80065034`'s sixth argument is that `fine`, and it is the literal `0x40` at
every traced call site - the cue-ring drainer `FUN_80016B6C` (both arms), the
per-actor trigger under `FUN_80021DF4`, and the slot-machine / dance / debug
overlays' direct key-ons. So a **cue keys half a semitone above** where the
sequencer would put the same `(tone, note)` pair. Both paths hand the result to
`FUN_80067550`, which stores it into the shadow register file at
`0x801CE084 + voice*16` (SPU voice `+4` = pitch) and ORs the flush flags.

Measured, not just read: across catalogued mednafen states, 126 of the 128
voices whose libsnd note-staging record (`0x801CDB50 + voice*54`) holds a
non-zero pitch have exactly the value this law computes from that record's
`(note, program, tone)` against the live bank's `center` / `shift` - sequencer
voices and SFX voices (record `+0x10 == 0x21`) alike. The two misses are
records whose bank was swapped after the key-on, so the reconstruction reads a
`center` that is no longer the one used.

Port: `compute_pitch` + `PitchPath` in
[`crates/engine-audio/src/vab_bind.rs`](../../crates/engine-audio/src/vab_bind.rs);
`play_note` takes the sequencer arm and `play_tone` the cue arm. The table is
computed from its closed form rather than carried as data.
`see ghidra/scripts/funcs/80065034.txt`, `80066e50.txt`, `80066d8c.txt`,
`80067550.txt`, `80066308.txt`.

### Voice allocator + key-on/off flush (the middle tier)

Between the SEQ event dispatch above and the documented 24-voice SPU broadcaster `FUN_8006C048` sits the voice allocator + key-on/off mask accumulator - the tier `engine-audio`'s `spu::voice` + `Spu::key_on_mask` / `key_off_mask` reimplement, so parity is decided here (not in the already-documented SPU-register or pitch layers).

| Function | Role |
|---|---|
| `FUN_80066B00()` | **The voice-allocation scan** (winner lands at `_DAT_801CE362`). Ascending scan over the `_DAT_801CDB50` records: the **first** unreserved + envelope-silent voice wins, scan stops. Else steal the minimum-priority voice with priority `<=` the request (threshold starts at the tone `prior`, tightens per lower priority seen); ties: lowest envelope, then largest age. No candidate → returns the voice count as an out-of-range sentinel; the note is **dropped**. On success every age increments, the winner's resets and adopts the request priority. (`0x63` is the sentinel 99 "no voice", not a loop count - the gap-map "cold-init fill" reading is corrected.) |
| `FUN_80065BAC()` | **Per-frame voice flush** (SsSeqCalc tier). Advances ring index `_DAT_801CE204`, clears the new ring word, services each voice via `FUN_8006C9A8`, records envelope-silent voices into `_DAT_801CE208[ring]`; voices silent across all 16 ring words get the in-use marker cleared (marker-2 → reverb release `FUN_8006A7A4`). Stages per-voice vol/pitch/addr/ADSR attrs per the `_DAT_801CE060` flag bits through `FUN_8006C048`, flushes sounding/key-on/key-off masks to the SPU, zeroes the sounding + key-on accumulators. (It does not choose voices - the earlier "claims a slot from the bitmap" reading is corrected.) |
| `FUN_800675C8()` | **Key-OFF / release sweep** (no callees, pure state). Scans sounding voices, clears the per-voice flag `_DAT_801CE060`, sets the key-off accumulator `_DAT_801CDB4C/4E`, updates the sounding mask `_DAT_801CE248/24A`. |
| `FUN_80065FE8()` | **All-voice reset / calc-top.** Zeroes every mask (`DB48/4A/4C/4E`, `E248/24A`) + voice flags, drives `FUN_80065BAC` over the active set, installs the SPU transfer-callback block (`FUN_8006BC70`). A `Spu` reset + one `Sequencer` tick pass. |

**engine-audio port.** `sequencer.rs`'s `alloc_voice` implements the retail scan order (`// PORT: FUN_80066B00`): first-idle-ascending with early stop, the tightening-threshold steal tier keyed on the VAB tone `prior` byte (`VabBank::tone_prior`), the envelope-then-age tie-breaks (with the retail signedness quirk - challenger age sign-extends, incumbent zero-extends), the drop-when-outranked case, and the age bookkeeping.
Engine stand-ins: "reserved" = bound to an active sequencer note; "envelope" = the live ADSR level. The engine keeps no 16-frame silent-history ring - a released voice unreserves when its owning note drops, and its decaying tail stays steal-visible through the envelope tie-break.
Provenance: per-instruction read of `FUN_80066B00` / `FUN_80065BAC` / `FUN_80065978` / `FUN_80066308`. The "no Ghidra dump exists for this tier" caveat this line used to carry is **stale** - all four now have dumps carrying full disassembly sections (163 / 271 / 132 / 353 instructions), so the readings above are checkable against the instruction stream rather than a C rendering. `see ghidra/scripts/funcs/80066b00.txt`, `80065bac.txt`, `80065978.txt`, `80066308.txt`.

### SPU command shims (`*0x81` scaling = 0..127 → 0..16383)

| Function | Role |
|---|---|
| `FUN_80062AA0(x, y)` | `SsSetMVol` - packs `[cmd=3, x*0x81, y*0x81]`, calls `FUN_8006BCB4` (SPU-cmd dispatcher). |
| `FUN_80065440(p1, p2)` | Single-shot SPU command (likely `SsUtKeyOn` or `SsUtPitchBend`) - `[cmd=6, p1*0x81, p2*0x81]`, calls `FUN_8006ACBC` (sister of `FUN_8006BCB4`). |

### Per-channel event handlers over `_DAT_801CD2C0` (the `0x80060A1C..0x80061BF8` family)

Sixteen SCUS routines share one prologue shape, and reading that shape is what
identifies the family: each takes `(seq_no, channel_no, value)` as sign-extended
halfwords, resolves `seq_tab = *(u32 *)(_DAT_801CD2C0 + seq_no*4)`, adds
`channel_no * 0xB0`, and operates on the resulting per-channel record. All but
`FUN_80061B24` close by calling the varint decoder `FUN_80061C68` and storing
its result at `+0x90`, the record's stream cursor - i.e. they are the handler
leaves of the same dispatch `FUN_8006171C` drives, one per MIDI-style event
class, and `+0x90` is where each leaf republishes the advanced cursor.

The `0xB0` stride and the `+0x90` cursor are the two facts to carry away; they
are what make an unfamiliar `0x80060xxx`/`0x80061xxx` routine recognisable as a
member rather than as unported game logic. Provenance for the whole family is
the disassembly of each dump named below.

| Function | Record bytes written | Reading |
|---|---|---|
| `FUN_80060A1C` | `+0x00` (cursor), `+0x26` | Re-seeds the program/bank byte from the stream and re-publishes the cursor. |
| `FUN_80060A94` | (voice fan-out only) | The largest leaf: walks the channel's active-voice list and calls `FUN_80064CF0` / `FUN_80064DF8` / `FUN_800655CC` per voice. Note-level event. |
| `FUN_80060EBC` | `+0x60` (per-voice halfword) | Writes a per-voice halfword then re-triggers through `FUN_8006861C`. |
| `FUN_80060F8C` | `+0x27` | Sets the tone/region byte then re-triggers through `FUN_8006861C`. |
| `FUN_80061054` | - | Calls `FUN_80067D0C` then `FUN_8006861C` with the `+0x60` halfword - a re-key of sounding voices. |
| `FUN_8006113C` | - | Branches on `value < 0x40`: `FUN_80065B88` (transpose reset) or `FUN_80065B98` (transpose mode 2). |
| `FUN_800611E4` | - | Forwards `value` to the SPU command shim `FUN_80065440`. |
| `FUN_8006126C` | `+0x15`, `+0x1A`, `+0x1C`, `+0x1D`, `+0x1F` | Loop/repeat bookkeeping: latches a pending count at `+0x1D`, clears `+0x1C`, sets the busy flag `+0x15`. |
| `FUN_8006139C` | `+0x00`, `+0x08`, `+0x15`, `+0x1B`, `+0x1C`, `+0x1D`, `+0x1F`, `+0x90` | The loop-close twin of the above: rewinds the cursor from the saved pointer at `+0x08` and clears the busy flag. |
| `FUN_800614D0` | `+0x18`, `+0x1E` | Sets state byte `+0x18`, bumps the nesting counter `+0x1E`. |
| `FUN_80061540` | `+0x19`, `+0x1E` | Same shape for the sibling state byte `+0x19`. |
| `FUN_800615B0` | `+0x18`, `+0x19` | Clears both of the above, then `FUN_8006558C` / `FUN_80065B88`. |
| `FUN_8006166C` | `+0x00` | Cursor-only skip; forwards to `FUN_80067C1C`. |
| `FUN_80061954` | `+0x00`, `+0x52`, `+0x54`, `+0x94` | The widest leaf: writes the channel volume pair `+0x52/+0x54` and the secondary cursor `+0x94`. |
| `FUN_80061B24` | - | The one member that does **not** end in `FUN_80061C68`: it calls `FUN_80066308` (note-trigger dispatch) and `FUN_8006688C` directly. |
| `FUN_80061BF8` | - | Minimal leaf - decode the varint, store the cursor, return. |

Provenance: `see ghidra/scripts/funcs/80060a1c.txt`, `80060a94.txt`,
`80060ebc.txt`, `80060f8c.txt`, `80061054.txt`, `8006113c.txt`, `800611e4.txt`,
`8006126c.txt`, `8006139c.txt`, `800614d0.txt`, `80061540.txt`, `800615b0.txt`,
`8006166c.txt`, `80061954.txt`, `80061b24.txt`, `80061bf8.txt`.

### Further libsnd / libspu leaves

| Function | Role |
|---|---|
| `FUN_80064BD0` | VAB-slot teardown: builds a default voice-attr block on the stack (pitch `0x1000`, `0x80FF`, `0x4000`) and loops `_DAT_801CDB44` times over the `0x36`-stride voice-driver records from `_DAT_801CDB52`, clearing each and calling `FUN_8006C048` then `FUN_80067480(1)`. |
| `FUN_80065B98` | Transpose-mode setter - stores `2` to `_DAT_801CE2E8`, the same global `FUN_80065B88` zeroes. The `< 0x40` / `>= 0x40` branch in `FUN_8006113C` picks between the two. |
| `FUN_80066D8C` | Octave/semitone → SPU pitch-step converter: divides the key by 12 for the octave, indexes the 16-entry halfword semitone table at `DAT_8007A940`, then shifts by `octave - 5` (left when positive, right when negative). Sibling of `FUN_80066E50`. |
| `FUN_80068C70` | `SsSetStereo` - zeroes the mono-fold flag `_DAT_801CE330`. (Its `SsSetMono` twin `FUN_80068C5C` sets it to `1`.) |
| `FUN_800693B8` | One-argument shim - tail-calls `FUN_800693D8(0)`, the SPU key/reset routine. |
| `FUN_8006B684` | Two-constant shim - calls `FUN_8006A7C8(a0, a1, 0xCC, 0xCD)`; the constants select the SPU register pair the callee programs. |
| `FUN_8006C9E4` / `FUN_8006CA04` / `FUN_8006D2F0` / `FUN_8006E600` | **Not libspu** - argument-free shims and helpers of the libpad driver; see [the `0x801CE628` cluster](#not-ssapi-the-0x801ce628-cluster-is-libpad). |
| `FUN_8005EBFC` | Single-hop shim onto `FUN_8005F024`. |

Provenance: `see ghidra/scripts/funcs/80064bd0.txt`, `80065b98.txt`,
`80066d8c.txt`, `80068c70.txt`, `800693b8.txt`, `8006b684.txt`, `8006c9e4.txt`,
`8006ca04.txt`, `8006d2f0.txt`, `8006e600.txt`, `8005ebfc.txt`.

### Renderer-citation correction

The cluster appears in xrefs from per-frame draw loops near `FUN_80026410+` only because battle / field code triggers SFX cues during render passes. None of these functions is libgpu / libgs - they're all libsnd. The "renderer / GPU primitives" inventory in `docs/reference/functions.md` previously listed `FUN_80061EDC / FUN_80067E9C / FUN_80066E50 / FUN_80067550` under the renderer; they belong here.

Interpretation: `_DAT_8007BAC8 = bgm_id` written by field-VM `0x35` is consumed by `FUN_800243F0` to load a `.SEQ` payload via the [streaming-asset path](../formats/scene-bundles.md), and that payload is then handed to `FUN_80062340` for sequencer playback. Engine reimpl can stub the entire cluster behind a `legaia-engine-audio::Sequencer` trait without touching the per-note math.

## libspu / SPU control (`0x80068-0x8006D` cluster)

Sits underneath the SsAPI sequencer and drives the SPU hardware directly. PsyQ `libspu` is statically linked here - the function names below correspond to the public PsyQ API.

### SPU globals

| Global | Role |
|---|---|
| `_DAT_8007AF40` | SPU register base pointer (SPU MMIO at `0x1F801C00..0x1F801E00`). |
| `_DAT_8007AF40 + 0x180/0x182` | `MAIN_VOL_L/R`. |
| `_DAT_8007AF40 + 0x1AA` | `SPUCNT` (control register). |
| `_DAT_8007AF40 + 0x1B0/0x1B2` | `REVERB_VOL_L/R`. |
| `_DAT_8007AF40 + 0x1C0..0x1FE` | Reverb config block (APF1, COMB1-4, IIR_ALPHA, …). |
| `_DAT_8007AF68` | SPU address-shift (typically `3` - the SPU 8-byte-word scale). |
| `_DAT_8007AF6C` | SPU address-alignment granule. |
| `_DAT_8007AFA4` | Block table base. Each entry: bit `0x80000000` = free, `0x40000000` = end-of-table. |
| `_DAT_8007AFF8` | Master attribute struct - 10 modes × `0x44` bytes = `0x2A8` bytes total. |
| `_DAT_8007AAC4 / _DAT_8007AAC8` | Pending-stream length / current slot (streaming SEP feeder). |
| `_DAT_801CDB60` | Per-slot SsApi record. Stride `0x36`. Indexed by VAB ID. |
| `_DAT_801CD2C0[i]` | Per-VAB program-attr table. Stride `0xB0` per program (`prog * 0xB0 + 0x58/0x5A`). |
| `_DAT_801CE344` | Open-seq-slot count. |
| `_DAT_801CE368` | Per-slot status byte (`0` = free, `1` = open, `2` = playing). |
| `_DAT_801CE564 / _DAT_801CE574` | **Not SPU globals** - the libpad driver's socket → port-context resolver and its port-busy check, installed by `PadInitDirect` / `FUN_8006E8D4`. See [the `0x801CE628` cluster](#not-ssapi-the-0x801ce628-cluster-is-libpad). |

### libspu primitives

| Function | PsyQ name | Notes |
|---|---|---|
| `FUN_80069E98` | `_SpuSetReg16` | Direct SPU register writer. |
| `FUN_80069EE0` | `_SpuAddrAlign` | Aligns + shifts an SPU address; conditionally writes to a register slot. |
| `FUN_8006A728` | `SpuFree` | Block-table free - flips matching addr's high bit (`|= 0x80000000`), calls `FUN_8006A420` (compactor). |
| `FUN_8006AC30` | `SpuMallocCheck` | Returns `1` if address is inside a live block. |
| `FUN_8006A7A4 / 8006A7C8` | `SpuSetReverbVol` (3-mode wrapper) | Modes: `0` clear, `1` or, `8` write. |
| `FUN_8006AA90` | `SpuSetReverbDepth` | Clamps `0..0x3F`, writes bits 8..13 of SPUCNT (`0x1AA`). |
| `FUN_8006ACBC` | `SpuSetVoiceAttr` | Mask-driven dispatcher (`mask=0..9` selects defaults from `_DAT_8007AFF8 + i*0x44`). 1272 bytes. |
| `FUN_8006B1B4` | `SpuSetReverbModeParam` | 30-attr reverb commit; writes regs `0x1C0..0x1FE`. |
| `FUN_8006B6A8` | `SpuSetReverbWorkAreaStart` | SPU-RAM zero-fill via 0x400-byte DMA chunks. |
| `FUN_8006BA50` | `SpuSetTransferStartAddr` | Clamps `<= 0x7EFF0`. |
| `FUN_8006BAB0` | `SpuGetTransferStartAddr` | Read-back of above; saves to `_DAT_8007AF58`. |
| `FUN_8006BB08` | `SpuSetTransferMode` | `_DAT_8007AF5C = (mode == 1)`. |
| `FUN_8006BB3C` | `SpuWrite` | Streaming-write continuation. |
| `FUN_8006BBC8` | `SpuIsTransferCompleted` | Polls the kernel event flag via `FUN_80056658` (`TestEvent` BIOS thunk). |
| `FUN_8006BC70` | `SpuSetTransferCallback` (block flag) | `_DAT_8007AF74 = (param != 1)`. |
| `FUN_8006BC9C` | `SpuIsTransferPaused` | Trivial predicate: `return _DAT_8007AF74 != 1`. |
| `FUN_8006BCB4` | `SpuSetCommonAttr` | Master vol L/R + reverb regs + SPUCNT bits. 7-mode jump table (`0x8000..0xE000` = master-vol attenuation). |
| `FUN_8006C048` | `SpuSetVoiceAttr` (24-voice broadcaster) | Loops `i=0..23` over `1<<i` mask, writes per-voice regs at `+i*0x10` (full SPU voice block: vol-L/R, pitch via `FUN_8006C6E4`, ADSR, env mode). 1548 bytes. |
| `FUN_8006C6E4` | `_SsKey2Pitch` | Two-octave-table pitch math: `((key1*0x80+fine1) - (key2*0x80+fine2)) / 0x600`, exponential build via `0x103B` factor. Returns 14-bit SPU PITCH (clamps `0x3FFF`). |

### SPU init / reset / key registers

The bottom of the libspu stack: cold init, the SPU-RAM transfer reset, and the raw KON/KOFF register writer. All are direct SPU MMIO or global-state resets - documented, not ported (the clean-room `Spu` models the KON/KOFF masks and the reset at the register-value level, never the hardware poke).

| Function | PsyQ shape | Notes |
|---|---|---|
| `FUN_800693D8(mode)` | `SpuInit` / `SsInit` core | `FUN_8006954C` transfer reset, then (mode 0) fills the 0x18 reverb registers with `0xC000`, zeroes the whole SsApi transfer-state block (`_DAT_8007AAxx` / `_DAT_8007AFxx` masks + flags), and `FUN_80069E98(0xD1, reverb_base, 0)`. |
| `FUN_8006954C(mode)` | SPU transfer/DMA reset | ORs `0xB0000` into `SPUCNT`, warm-transfers 0x10 bytes via `FUN_800697E0`, and spins `FUN_8006A078` settle delays while polling for the reset to settle; on timeout logs `"SPU_T/O:%s"` (`wait` / `reset`). |
| `FUN_80062228()` | voice-block hardware clear | Zeroes the 24-voice register block from `0x1F801C00` and the reverb work area at `0x1F801D80`, then `FUN_80065FE8` (all-voice reset). The SPU half of a full audio-subsystem reset. |
| `FUN_800699AC()` | SPU DMA settle+kick | `FUN_8006A078` settle then `FUN_8005BD30(0xF0000009, 0x20)` - kicks the SPU DMA channel for a zero-fill sweep. |
| `FUN_8006B854(mode, mask24)` | `SpuSetKey` (KON/KOFF) | Writes a 24-bit voice mask to the SPU **KOFF** register (`_DAT_8007AF40 +0x18C/+0x18E`, `mode==0`) or **KON** (`+0x188/+0x18A`, `mode==1`). When the transfer-busy flag `_DAT_8007AF38 & 1` is set it stages the mask into shadow accumulators (`_DAT_801CE518..51E`, `_DAT_8007AB00/AB04`) for a deferred flush instead of touching hardware. |

Provenance: `see ghidra/scripts/funcs/800693d8.txt`, `8006954c.txt`, `80062228.txt`, `800699ac.txt`, `8006b854.txt`.

### SPU DMA transfer engine

Sits between the SsApi seq layer and the libspu register primitives. This is the path SEQ/VAG bytes take when moving from PSX RAM into SPU RAM.

| Function | PsyQ name | Notes |
|---|---|---|
| `FUN_80069B18(mode, addr, len)` | `_spu_t` core | 4-mode SPU transfer state machine. `mode=0`: arm READ (xfer-mode bits = `0x30`); `mode=1`: arm WRITE (`0x20`); `mode=2`: stage start address into SPU `+0x1A6`; `mode=3`: COMMIT - wait for SPUCNT bits `0x30` to settle, kick the DMA channel via `_DAT_8007AF44 / +0x48 / +0x4C` (DICR + BCR + CHCR) with packet `(addr, ((len+0x3F)>>6)<<16 \| 0x10, 0x1000201/0x1000200)`, then call `FUN_8006A020` (read) or `FUN_8006A04C` (write) to flip the SPU command-register direction bits. Times out at `0xF00` poll iterations and returns `0xFFFFFFFE`. |
| `FUN_800697E0(buf, len)` | `_SpuTransfer` outer wrapper | Saves SPUCNT `+0x1AE` mask, sets transfer addr `+0x1A6 = _DAT_8007AF58`, calls `FUN_8006A078` (settle), then loops over the transfer block in `0x40`-byte chunks. Alternative path to `FUN_80069B18` for non-DMA copies. |
| `FUN_80069DA8(addr, len)` | `SpuWrite` (top-level) | Picks between the two transfer paths: if `_DAT_8007AF5C == 0` (DMA mode), drives `FUN_80069B18` mode `2 → 1 → 3`; otherwise tail-calls `FUN_800697E0` (CPU copy). |
| `FUN_8006A020` | `_spu_a` (read direction) | Sets SPU command register `*_DAT_8007AF54` bits 24..27 = `0x2` (read) by clearing the field and OR-ing `0x20000000`. |
| `FUN_8006A04C` | `_spu_a` (write direction) | Sets SPU command register bits 24..27 = `0x22` by clearing the field and OR-ing `0x22000000`. The `0x2` upper-nibble flag selects write vs read direction. |
| `FUN_8006A078` | SPU register-settling delay | 60-iteration busy-wait spin (`for (i=0; i<0x3C; i++) {}`). Inserted between command-register write and transfer kick to give SPU MMIO time to latch. |
| `FUN_8006A158` | `SsSpuMalloc` core | 712-byte block allocator. Walks the `_DAT_8007AFA4` block table, returns the start of the first free run of size `>= request`, marks header word `0x40000000` end-of-table where appropriate. Called from `FUN_80068D94` (the VAB-open head). |
| `FUN_8006A420` | `SpuFree` compactor | 776-byte coalescer. Iterates the block table, merges adjacent free entries (high-bit `0x80000000` set), shifts entries down to fill gaps. Called from `FUN_8006A728` (`SpuFree`). |

### Reverb model (engine-audio)

The retail SPU implements reverb as a same-side / different-side IIR reflection pair feeding a 4-tap comb early-echo and two all-pass stages, run at 22050 Hz over a work buffer at the top of SPU RAM (`mBASE = 0x80000 - work_size`). The 9 standard libspu modes (`Room` / `StudioA-C` / `Hall` / `Space` / `Echo` / `Delay` / `Pipe`) plus `Off` each select a 32-register set (work-area size + IIR/comb/all-pass coefficients + tap addresses).

The `engine-audio` clean-room port reproduces that network register-for-register in [`spu::reverb`](../../crates/engine-audio/src/spu/reverb.rs): each [`ReverbMode`](../../crates/engine-audio/src/spu/reverb.rs) loads the standard libspu preset (public PSX hardware-reference constants - the same tables every open SPU emulator ships, not Sony game data) into a recirculating `i16` work buffer sized to that mode's work area. Address-type registers are in 8-byte units, taps wrap within the work area, and the reverb multiply is `(sample * coeff) / 0x8000` (signed Q15, so a `0x8000` coefficient inverts phase exactly as the hardware does).

Per-voice routing is opt-in: `Voice::reverb_send = true` (libspu `SpuSetVoiceReverb` analogue) sums the voice's pre-master output into the reverb send bus; the wet output is mixed back into the master in `Spu::tick`.

#### Retail reverb routing - Studio C, always on (capture-confirmed)

A pure-Rust sweep of the save-state corpus (`mednafen-state spu <state>`, reading the SPU register shadow via [`PsxSpu::reverb_registers`](../../crates/mednafen/src/spu.rs) / `voice_reverb_mask` / `reverb_master_enabled`) pins what retail actually runs, and it falsifies the earlier "Spirit-Arts / echo cues selectively opt in, everything else dry" reading:

- **The reverb network is master-enabled in every captured state** (`SPUCNT` bit 7 set) - field, town, battle, summon, title, minigames. There is no scene or cue that toggles it on.
- **The mode is `Studio C` everywhere.** The 32 reverb coefficient/address registers (`0x1F801DC0..0x1F801DFF`) are byte-identical across all 45 mednafen states and match the `StudioC` libspu preset exactly (`dAPF1=0x00E3`, `dAPF2=0x00A9`, work area `0x6FE0`). [`ReverbMode::identify`](../../crates/engine-audio/src/spu/reverb.rs) resolves the captured block to `StudioC`.
- **Per-voice reverb-send (`EON`) is broad and always populated** - typically 15–22 of the 24 voices in any given state, including BGM and SFX voices, not a handful of "echo" voices. So reverb is the *default* routing, applied to nearly every keyed-on voice, not a per-cue effect.

So the C7-REVERB blocker dissolves: there is no per-cue reverb-enable source to trace. The live engine matches retail by calling [`Spu::set_retail_reverb`](../../crates/engine-audio/src/spu/mod.rs) once at SPU init (the `StreamResampler` in [`engine-audio`](../../crates/engine-audio/src/lib.rs) does this) - it selects `ReverbMode::StudioC` and routes every voice into the reverb send. (Output depth - `vLIN`/`vROUT`, set separately by `SpuSetReverbDepth` - is the one piece not fixed by the preset; the engine applies a fixed half-scale depth, overridable via `Reverb::set_output_volume`. The EON mask's exact per-voice membership varies per frame with which voices happen to be sounding; the engine routes all voices, a faithful approximation of the broad mask.)

Boundaries:
- Mode selection via `Spu::write_reverb_mode_byte(raw)` matches the libspu byte API (1=Room, 2=StudioA, …, 9=Pipe). Out-of-range bytes fall back to `Off`. This is the engine half of `SpuSetReverbModeParam` (`FUN_8006B1B4`, the 30-attribute commit).
- The hardware's 39-tap FIR input/output resampler (44.1 kHz ↔ 22.05 kHz) is approximated by decimation + zero-order hold; the tail's character comes from the network, the FIR only affects high-frequency detail.
- Output volume (`vLOUT`/`vROUT`) isn't part of the mode preset on hardware (libspu sets it separately via `SpuSetReverbDepth`); the engine applies a fixed depth, overridable with `Reverb::set_output_volume`.

### Voice resampler - 4-point Gaussian interpolation (engine-audio)

Each SPU voice resamples its ADPCM stream through the hardware's fixed
512-entry Gaussian coefficient ROM: pitch-counter fraction bits 4..11 form the
8-bit interpolation index, and the output mixes the four most recent decoded
samples (`gauss[0xFF-i]`, `gauss[0x1FF-i]`, `gauss[0x100+i]`, `gauss[i]`, each
product `>> 15`). Table + formula are the published PSX hardware spec (no$psx
"4-Point Gaussian Interpolation") - the same provenance class as the libspu
reverb presets. This matters audibly: Legaia's VAG bodies are 22.05 kHz played
through the 44.1 kHz SPU, so *every* voice runs at a non-unity pitch step -
nearest-sample resampling aliases everything. The engine model is
[`spu::gauss`](../../crates/engine-audio/src/spu/gauss.rs), applied per tick in
[`spu::voice`](../../crates/engine-audio/src/spu/voice.rs) with a 4-sample
history that survives ADPCM block boundaries. The pitch step clamps at
`0x4000` (4.0×, 176.4 kHz), matching hardware.

### SsApi seq-management layer (above libspu)

| Function | Role |
|---|---|
| `FUN_800683D8(vab, prog)` | `SsVabTransfer`-shaped - VAB program-attr lookup at `DAT_801CD2C0[vab&0xFF] + (prog>>8)*0xB0 + 0x58/0x5A`. |
| `FUN_800684CC(vab_id)` | `SsVabClose` (by VAB-ID search) - iterates `0x801CDB60 + i*0x36`, matches `+0x0`, calls `FUN_80067480(0)`. |
| `FUN_80068B98(vab_id, program)` | **VAB program-change.** Bounds-checks `vab_id < 0x10` + open-state, `program < _DAT_801CE332` (the bank's program-slot count), then installs the current-bank globals (`_DAT_801CE334` prog base / `_DAT_801CE33C` header / `_DAT_801CE340` tone base) and `DAT_801CE34F` = the `ProgAtr[program]+8` **packed tone-page index** the open wrote (below). Earlier "SsSeqOpen / track count" label corrected from the disassembly. |
| `FUN_80068C5C` / `FUN_80068C70` | `SsSetMono` / `SsSetStereo` - `_DAT_801CE330 = 1 / 0`, the mono-fold flag `FUN_80067550` reads. (Earlier "auto-poll" label corrected.) |
| `FUN_80068C80(vab_id)` | VAB close (per-vab tables) - if the open-state byte at `0x801CE368+vab` is set, `SpuFree`s the bank's allocation from the addr table `0x801CE3C8+vab*4`, clears the state, decrements the open-bank count `_DAT_801CE3C0`. |
| `FUN_80068D34(hdr, vab_id, addr)` | `SsVabOpenHeadSticky`-shape wrapper - tail-calls `FUN_80068D94` with the caller-supplied SPU address (skips the `SsSpuMalloc`). |
| `FUN_80068D94(hdr, vab_id, sticky, addr)` | **`SsVabOpenHead` core.** Validates `pBAV` magic, sets `_DAT_801CE332` to 0x40 (0x80 for version >= 5), checks `ps` (`+0x12`) against it, registers header / ProgAtr / tone-region base pointers in the per-vab tables, and builds the **program-number → packed-tone-page rank map** into the ProgAtr `+8` reserved words ([`vab.md`](../formats/vab.md#program-slots-vs-packed-tone-pages)). Sums VAG sizes, `SsSpuMalloc`s (`FUN_8006A158`) unless sticky, stashes per-VAG SPU addresses `>>3` in ProgAtr `+0xC/+0xE`. Engine port of the rank map: `VabBank::upload`. (Earlier "SsSepOpen / 'VAP' SEP loader" reading falsified - [`re-do-not-re-walk.md`](../reference/re-do-not-re-walk.md#audio--sound-driver).) |
| `FUN_80069170(slot)` | `SsSeqPlayResolved` - final play-start stage; calls `8006BB08(0)` (xfer-mode), `8006BAB0` (commit), `8006BA50` (data feed). |
| `FUN_80069230(...)` | Streaming SEP feeder - partial-buffer continuation via `_DAT_8007AAC4/AAC8`. |
| `FUN_80069390(...)` | `SsIsEos` - tail-call to `FUN_8006BBC8`. |

The runtime sequencer chain is now nearly fully mapped: slot bitmap @ `_DAT_801CD2B8` → ptr table @ `0x801CD2C0` → per-slot record (stride `0x36`) at `0x801CDB60` → VAB program-attr (stride `0xB0`) at `0x801CD2C0[i] + prog*0xB0`.

### Not SsAPI: the `0x801CE628` cluster is libpad

`0x801CE628` is **not** a sequencer worker table. It is libpad's two-port
driver-context array - stride `0xF0`, `0x1E0` bytes total, one context per
controller socket - and every entry that resolves a context through the
`_DAT_801CE564` hook is a libpad API call. Nothing in the cluster touches an
SPU register, a VAB, or a voice key. The correction is recorded on this page
because this is where the corpus filed the cluster.

The chain anchors on the game's controller + memory-card init `FUN_8001D230`,
which `bzero`s `0x44` = 2 x `0x22` bytes at `0x800840F8` and hands the two
halves to `FUN_8006E2B4` (`addiu a1,a0,0x22`). Those two 34-byte buffers are
what the pad pump `FUN_8001822C` decodes as `[status][type nibble][buttons:
inverted u16]`, port 1 at `+0x22`/`+0x23`.

| Function | PsyQ entry | What the instructions show |
|---|---|---|
| `FUN_8006E2B4(buf0, buf1)` | `PadInitDirect` | Clears `0x1E0` = 2 x `0xF0` at `0x801CE628`, stores `buf0`/`buf1` at each context `+0x30`, seeds each report buffer `[0] = 0xFF` / `[1] = 0`, and fills the six bytes at context `+0x5D` with `0xFF` - the actuator-alignment table's unassigned default. |
| `FUN_8006CE30(socket, table, len)` | `PadSetAct` | **Three** arguments: `a0` passes through untouched into the context resolver, `a1`/`a2` are forwarded to `FUN_8006D7B4`. Ghidra's C drops `param_1`. |
| `FUN_8006D7B4(ctx, table, len)` | `PadSetAct` inner | `ctx[+0x28] = table`, `ctx[+0x34] = (u8)len` - the per-port actuator buffer pointer and its length. |
| `FUN_8006CDB0(socket, align)` | `PadSetActAlign` | Two arguments, tail-calling `FUN_8006DDC8`: stores `align` at `ctx+0x20`, installs trampolines at `ctx+0x14`/`+0x18`, sets the port state byte `ctx+0x46 = 1`. |
| `FUN_8006CA7C(socket)` | `PadGetState` | Tests the report buffer's status byte through `ctx+0x30`, then normalises the port state byte `ctx+0x49` (`3 → 1`, `2 → 1`, `6 → 4`). |
| `FUN_8006CB3C(socket, term, offs)` | `PadInfoMode` | `term = 4` returns the id-table length `ctx+0xE3` when `offs < 0`, else `((u16 *)ctx[0])[offs]` bounds-checked against it - `InfoModeIdTable`'s contract verbatim. `1` → byte `+0xE8`, `2` → u16 `+0xE6`, `3` → byte `+0xE4`, `0x64` → u32 `+0x4C`. |
| `FUN_8006D1E0` / `FUN_8006D2AC` | `PadStartCom` / `PadStopCom` | Mirrored pair inside a BIOS critical section: `ChangeClearRCnt(3, 0)` against `(3, 1)`, hooking / unhooking the `_DAT_801CE540` vector. `FUN_8006C9E4` / `FUN_8006CA04` are their argument-free shims. |
| `FUN_8006E600(ctx)` | actuator payload build | Clears the 6-byte staging area `ctx+0x57`, bails when the extended-mode offset `ctx+0xE6` or the act-table pointer `ctx+0x28` is zero, clamps the act length `ctx+0x34` to `6`, and maps the caller's actuator values through the align table at `ctx+0x5D` into the outgoing poll packet. |
| `FUN_8006E46C(ack)` | per-port service step | Advances the port cursor `_DAT_8007B2B4` by one `0xF0` context from the base `_DAT_8007B2A8` and services it via `FUN_8006E9C0` / `FUN_8006EC24`. |
| `FUN_8006DAAC(ctx)` | per-port state dispatch | Branches on the port state byte `ctx+0x46` into `FUN_8006E0A0` / `_E0C0` / `_E0E0` / `_E100`, passing `ctx+0x47`. |
| `FUN_8006D2F0` | per-port transfer kick | Latches `_DAT_8007B2C4` into the cursor, calls `FUN_8006D358` on `base + idx*0xF0`, and on failure invokes the installed callback `_DAT_801CE560` with `0xFFFF`. |
| `FUN_8006CF9C` | hook installer | `_DAT_801CE544 = FUN_8006D030`, `_DAT_801CE548 = FUN_8006CFC8`; called at the tail of `PadInitDirect`. |

The BIOS thunks around it agree. `FUN_8005FD68` = `ChangeClearPAD` (B0 `0x5B`)
is called by `FUN_8006EE8C` / `FUN_8006EEE0` to hand the pad off from the BIOS
handler to the direct driver; `FUN_8005FD78` = `ChangeClearRCnt` (C0 `0x0A`);
`FUN_8006EF48` / `FUN_8006EF58` / `FUN_8006EF68` = `InitCARD` / `StartCARD` /
`StopCARD` (B0 `0x4A` / `0x4B` / `0x4C`); `FUN_80056618` = `_bu_init` (A0
`0x70`). `FUN_8001D230`'s eight `OpenEvent` / `EnableEvent` pairs on classes
`0xF4000001` / `0xF0000011` (specs `0x0004` / `0x8000` / `0x0100` / `0x2000`,
mode `0x2000`, no handler) are the **memory-card** event set, not SPU or DMA
interrupts.

**What put the SsAPI label there.** Three things, each individually
reasonable: the `0x8006C000..0x8006F000` band does hold genuine libspu /
libsnd code; a vtable of installed hooks over a stride-`0xF0` record array
with an `0xFF` idle fill and a per-record state byte reads exactly like the
sequence-worker table; and with `param_1` dropped, `FUN_8006CE30` renders as a
two-argument "set user data on a resolved context". It fails on the buffers
(`ctx+0x30` is provably the button report `FUN_8001822C` decodes), on
`FUN_8006CB3C`'s `term = 4` branch (an id-table query with no sequencer
analogue), and on the record count, which is 2 - the number of controller
sockets, not a sequencer's slot count.

**Consequence.** `DAT_800915DA` / `DAT_800915DB` are port 0's two actuator
bytes, so the per-frame kernel `FUN_80018DB0` that writes them is a **rumble**
cadence, not an audio one - see the
[`80018DB0` row](../reference/functions/audio.md) and
[`re-settled-threads.md`](../reference/re-settled-threads.md#fun_80018db0-is-a-rumble-cadence-not-an-audio-one).

Provenance: `see ghidra/scripts/funcs/8006e2b4.txt`, `8006ce30.txt`,
`8006d7b4.txt`, `8006cdb0.txt`, `8006ca7c.txt`, `8006cb3c.txt`, `8006d1e0.txt`,
`8006d2ac.txt`, `8006e600.txt`, `8006e46c.txt`, `8006daac.txt`, `8001d230.txt`,
`8001822c.txt`.

## File-API leaf cluster

The dev/retail split for sound + monster-bank loading routes the dev branch through libapi-style file primitives at `FUN_800608E0..FUN_80060A04`: `fopen` / `fseek` / `fread` / `fclose` plus a `vsync_wait` (`FUN_8005FCCC`) and a `BREAK 0x105` trap at `FUN_80060A04`. These are PsyQ kernel-call wrappers around the BIOS `A()` table - `FUN_80056738` / `FUN_80056748` / `FUN_80056768` / `FUN_80057014` / `FUN_8005ACE8` are all `jr 0xA0` BIOS dispatchers. Engine reimpl can map the entire cluster to `std::fs` + a frame-paced sleep.

## Engine-audio model - Sequencer port

The `legaia-engine-audio::Sequencer` is the runtime side of the SsAPI
sequencer cluster above. Surface mirrors `SsSeqOpen` / `SsSeqPlay` /
`SsSeqClose` / `SsSeqSetVol` without copying any Sony bytes:

| Method | Maps to |
|---|---|
| `Sequencer::new(seq, bank)` | `SsSeqOpen` - bind one SEQ + one VAB bank, allocate channel state |
| `Sequencer::tick_sample(spu)` | production playback clock - advance exactly one SPU sample (44.1 kHz) |
| `Sequencer::tick_us(spu, dt_us)` | wall-clock / per-frame poller (parity oracles, tests) - converts µs to whole samples with a carry |
| `Sequencer::set_master_vol(vol)` | `SsSeqSetVol` master |
| `Sequencer::set_loop_to(idx)` | external loop-point fallback (`_DAT_801CD2C0[i] + 0x98` repeat bit equivalent) for tracks with no in-stream markers |
| `Sequencer::stop(spu)` | `_SsSeqCtrl(mode=1)` - silences and freezes |
| `Sequencer::rewind_to(idx, spu)` | `SsSeqRewind` |

Voice allocation follows the retail scan order (`alloc_voice`,
`// PORT: FUN_80066B00` - see the "Voice allocator + key-on/off flush"
section above): first idle voice in ascending order, else steal the
minimum-priority voice at or below the note's VAB tone `prior`
(quietest-envelope then oldest-age tie-breaks), else drop the note. The
sequencer tracks `(channel, key) → voice` so the matching key-off can
shut down the right slot. Tempo events from the SEQ override the running
tempo at the event's absolute tick (matching libsnd's mid-stream
`0xFF 0x51`).

**Pitch bend (`0xEn`).** The retail score uses pitch bend - the corpus
sweep (`engine-audio/tests/real_seq_expressive_events.rs`) finds thousands
of `0xEn` events concentrated in a handful of music banks - so the
sequencer acts on it: a bend sets the channel's 14-bit wheel
(`ChannelState::pitch_bend`, center `0x2000`), re-pitches every voice
already sounding on that channel, and is folded into subsequent NoteOns.
Each `ActiveNote` keeps its unbent base pitch so repeated bends scale the
base rather than compounding.

The bend **range is a per-tone disc value**, not a global constant: each VAB
tone carries `pbmin`/`pbmax` (downward/upward bend in semitones), and the
wheel scales by the sounding tone's own range - `+pbmax` semitones at
full-up, `-pbmin` at full-down (`VabBank::pitch_bend_range`, captured into
the `ActiveNote` at NoteOn). A tone with a `(0, 0)` range does not respond
to the wheel at all, exactly as libsnd applies the per-tone range. A
disc-wide tone census (`engine-audio/tests/real_vab_tone_attributes.rs`)
pins this: the common non-zero range is 2 semitones (the GM default, which
is why a global `±2` would approximate it), with a few tones at 4/12/24/40;
vibrato (`vibw`/`vibt`) and portamento (`porw`/`port`) are zero on every
tone, so the voice model needs no LFO.

Channel and polyphonic aftertouch (`0xDn` / `0xAn`) are parsed but the
expressive-event sweep confirms the retail score never emits them, so they
have no consumer to drive.

**Loop points.** SEQ loop markers are read from the stream: the NRPN-style
control changes on `0xB0` (controller 99 value 20 = Loop Start, value 30 =
Loop Forever; see [`formats/seq.md`](../formats/seq.md)). A Loop Start records
the position immediately after the marker; a later Loop Forever - or an
end-of-track that follows a Loop Start - rewinds there rather than to event 0,
so looped BGM repeats from the correct bar instead of restarting the whole
track. The rewind resets the integer sample-clock, so the looped body re-fires
on the same sample offset every pass. `set_loop_to` is the fallback for the
four retail tracks with no markers.

`Sequencer::loop_count` exposes a monotonic rewind counter (bumped on every
`rewind_to`), and `render_bgm_loop_region` (in `legaia-engine-audio`) uses it
to render one **seamless loop period** off-line: it renders until the second
rewind and returns the PCM trimmed to that boundary plus the
`[loop_start, loop_end)` sample offsets. The playhead tick alone can't mark the
boundary - on a zero-delta EOT the tick peaks and resets inside a single sample
- which is why the counter exists. The site plays this as an
`AudioBufferSourceNode` with `loopStart`/`loopEnd` set to one true period, so
minigame BGM repeats without the seam a fixed-window hard-loop leaves.

**Controller census.** A disc-wide sweep of every SEQ-bearing PROT entry
(`engine-audio/tests/real_seq_expressive_events.rs`) fixes which control
changes the retail score actually emits: CC7 (channel volume) and CC10 (pan)
carry the bulk; CC99 carries **only** the two loop-marker values 20 and 30
(so the loop handler drops nothing); and CC6 (Data Entry) is a constant 127
emitted ~once per track (a fixed init the engine ignores - it varies nothing,
so it is not a per-track parameter). Notably **absent**: expression (CC11)
and reverb-depth (CC91). So per-channel volume swells and per-cue reverb
sends are not encoded in the SEQ stream - consistent with the capture
finding above that reverb is a fixed global (Studio C, master-on, voices
routed by default), not a per-cue or per-channel parameter the score drives.

**Dynamic channel expression (CC7 volume + CC10 pan).** Volume and pan are
the two most-used controllers, and both are **dynamic** - the score swells
volume and pans voices around mid-note, not just at note-on (a corpus sweep
finds the majority of CC7 events fire while a note is already sounding). The
sequencer treats them as channel-expression layered over a per-note base:
`play_note` leaves the voice at `master × velocity × tone-vol` (scaled into
the register's `0..=0x3FFF` domain, not the `0..=127` input domain), tone-panned
by the same law described below - libsnd applies this attenuation once per pan
source, so the tone and channel sources share it -
with **no** channel volume or pan; each `ActiveNote` stores that channel-free
base L/R (mirroring `base_pitch` for bend). `channel_mix` then folds in the
channel's CC7 volume (scale both sides by `volume/127`) and CC10 pan, where
pan uses libsnd's voice-volume law (`FUN_80067550`): a pan left of center
(`< 0x40`) attenuates the **right** by `pan/0x3f`, a pan right of center
attenuates the **left** by `(0x7f - pan)/0x3f`. A mid-note CC7 or CC10 event
re-derives every sounding voice on the channel from its base (`remix_channel`),
so successive changes don't compound, and a fresh NoteOn picks up the
channel's current volume + pan. A full-volume, centered channel is the
identity, so this is faithful over the prior note-on-only behavior.

**Timebase.** The production playback path ticks the sequencer once per SPU
sample (`tick_sample`), so the music clock is locked to the audio clock.
Timing is computed with an **exact integer accumulator** (units of
`sample × ppqn × 1_000_000`; an event of delta `d` fires when the accumulator
reaches `d × tempo_us × 44100`) - no per-tick float, no long-track drift, and
bit-deterministic for the replay oracle. Note the SEQ tempo gotcha documented
in [`formats/seq.md`](../formats/seq.md): the header tempo is a 240 BPM
placeholder, immediately overridden by the first body `0xFF 0x51` (which, in
PSX SEQ, carries its 3 tempo bytes with **no** MIDI length prefix). Mis-parsing
that override pinned playback at the 240 BPM placeholder (~3x too fast).

See [`crates/engine-audio/src/sequencer.rs`](../../crates/engine-audio/src/sequencer.rs)
for the implementation; tests use synthetic SEQs + a stubbed `VabBank`.

## Engine-audio model - clean-room SPU port

`crates/engine-audio` ports the SPU side of the audio stack as a clean-room model. No Sony bytes; the spec is this file plus the libspu API surface and the standard PSX SPU register layout. Surface:

| Module | Maps to |
|---|---|
| [`spu::Spu`](../../crates/engine-audio/src/spu/mod.rs) | The 24-voice mixer (one [`Voice`] per slot) + master volume + the [`spu::reverb`] network. |
| [`spu::voice::Voice`](../../crates/engine-audio/src/spu/voice.rs) | Per-voice state: sample address, loop point, pitch, ADSR, L/R volume - the libspu `SpuSetVoiceAttr` surface. |
| [`spu::adsr`](../../crates/engine-audio/src/spu/adsr.rs) | The 5-phase ADSR envelope (Attack-Decay-Sustain-Release-Off) with linear / exponential / increase / decrease modes per the standard PSX formula. Increasing phases step by the `+7..+4` (`7 - step_bits`) StepValue table; every *decreasing* phase (decay, linear/exponential release, sustain-decrease) steps by the `-8..-5` (`-8 + step_bits`) table - the two sign tables differ by one unit, so a decreasing phase driven from the increase table fades ~one step slow. The `(adsr1, adsr2)` words are read verbatim off the VAB tone metadata (a decoded tone's ADSR word equals the SPU `ADSRControl` register libspu writes at key-on - no transform). |
| [`spu::adpcm`](../../crates/engine-audio/src/spu/adpcm.rs) | Streaming SPU-ADPCM block decoder (28 samples per 16-byte block). One stateful instance per voice carries the inter-block `prev1`/`prev2` history. |
| [`spu::ram`](../../crates/engine-audio/src/spu/ram.rs) | 512 KB SPU RAM model + libspu-shaped transfer engine (`SpuRam::set_direction` / `write` / `read` + `SpuAllocator` for `SsSpuMalloc` / `SpuFree`). |
| [`vab_bind::VabBank`](../../crates/engine-audio/src/vab_bind.rs) | Bridges `legaia_vab::VabReport` into the SPU: `upload(spu, alloc, report, buf)` drops every VAG body into SPU RAM through the allocator, and `play_note(spu, voice, prog, note, velocity)` translates a MIDI key into voice config + key-on. Pitch math matches `_SsKey2Pitch` / libspu key-to-pitch; key-on volume is `bank x prog x vel / 127^3 x 0x3FFF`, and the tone pan applies the same `FUN_80067550` attenuation as the channel pan. |
| [`AudioOut`](../../crates/engine-audio/src/lib.rs) | Owns a single cpal output stream that drains the `Spu` at 44.1 kHz and resamples to the host device rate (linear). Engines call `with_spu(|spu| ...)` from outside the audio thread to push voice attributes / key-on masks. |

What this **does not** model (out of scope for the first port pass):

- Pitch modulation, noise, FM. None of these are used by Legaia (verified against the libspu calls in the SCUS dumps - `SpuSetPitch` is the only pitch path).
- Asynchronous DMA timing. The transfer engine here is synchronous (the queue + drain are collapsed) - fine because the playback layer reads SPU RAM directly during voice ticks. The real hardware is asynchronous via the transfer engine described above; the model preserves the *API shape* (`set_transfer_start_units_8` / `set_direction` / `write`) so the libspu callers map cleanly.

## SFX bank + scheduler

Maps battle / field cue IDs (the `kind` byte the art-record `HitCue` / overlay scripts emit) to per-cue `SfxEntry` descriptors that describe how to fire a one-shot through the SPU. Engines populate the catalog at startup, then forward `ScheduledCue`-like requests through `SfxScheduler` which queues each request with its retail timing offset and dispatches when the per-frame tick reaches the firing frame.

`SfxBank::from_descriptors` builds the catalog straight from the disc-decoded static SFX table (`legaia_asset::sfx_table`): each active descriptor's `program` becomes the `program_index` and its `note` the `key`, so the cue ids `0x00..=0x63` resolve to the retail program/tone instead of a hand-authored stand-in.

**The bank those programs index is named by the cue itself.** `FUN_80065034` calls `FUN_80068b98(vab_id, program)` *before* the program lookup, and that repoints the libsnd current-bank globals (`_DAT_801ce33c`/`_DAT_801ce334`/`_DAT_801ce340`) at the slot the cue's `+4` category selects. The older reading - "the globals hold whichever VAB is open, so a cue plays out of the scene's music bank" - was a save-state artefact: the globals really are shared with the sequencer, so a state sampled after a BGM note holds the music bank, and across the catalogue that is 13 distinct VABs. Full law, the category -> slot -> PROT map and its four entries: [`formats/sfx-table.md`](../formats/sfx-table.md#category-is-a-bank-selector-and-four-banks-are-open-at-once).

Practically, that makes the low id range emphatically **not** the scene's music VAB. The class-2 bank (PROT 0869) that the battle scene loader `FUN_800520F0` (`a1 = 2`) and the Baka init `FUN_801CF00C` stage carries a purpose-built SFX key map at **program 0**: one distinct VAG per semitone, single-note windows `min == max == 60 + i`, lining up 1:1 with the descriptor notes of the UI cue ids (`0x20` note 60, `0x21` note 61, `0x23` note 63, `0x09` note 69). The slot-0 system bank (PROT 0868) carries its own copy of that map, and it is the one the shared UI cues - which are category `0` - actually key.

A scene *music* VAB's program 0 is an ordinary melodic instrument instead - in the `town01` case two tones spanning keys `0..=68` and `69..=101`, with tones 3/5/9 empty - so those same ids would resolve there to an arbitrary instrument note or to nothing at all. The host therefore stages the two pinned SFX banks itself and fires each cue with `SfxBank::play_one_shot(spu, vab)` against the one its category names, falling back to the scene `VabBank` only when nothing staged.

| Cue ID | Meaning |
|---|---|
| `0x1A` | Generic SFX trigger ("play sound" hit cue). Catalog typically maps to per-strike weapon impact tones. |
| `0x4C` | Hit-effect visual (no sound on its own; engines that fold the visual into a synced sound use this slot). |
| `0x80..=0xFE` | Reserved per-character / per-art SFX IDs. Indexed from the per-actor `+0x9C0` table at retail. |

`SfxBank::play_one_shot` delegates to `VabBank::play_tone` - tone by explicit **region index**, the retail cue shape, not the sequencer's key-range `play_note` - for sample, ADSR and the [cue-arm pitch](#the-key-on-pitch-law---note-against-the-tones-center); the scheduler is a frame-driven queue that returns an `SfxFireBatch` per `tick_frame` call so engines can dispatch through the same `VabBank` they already wired for the BGM sequencer. A `PendingCue` with `frames_remaining = 0` fires on the next tick, so a cue queued mid-frame doesn't fire immediately and gives the host a chance to clear render state first - matching the retail timing where a `HitCue::timing_frames = 1` cue plays one frame after the strike begins.

Implementation: [`crates/engine-audio::sfx`](../../crates/engine-audio/src/sfx.rs).

## XA-ADPCM

`crates/xa` decodes CD-XA 4-bit ADPCM bit-exactly: on a real cutscene track its per-channel PCM matches an external lossless reference decode sample-for-sample. The on-disc `.XA` / `.STR` audio is standard CD-XA Mode 2 Form 2 - the earlier "non-standard interleave" was Form-1 truncation damage in the old extractor, not a bespoke format. The demuxer (`legaia_xa::demux`) splits raw 2352-byte sectors by `(file_no, ch_no)` and the group decoder reconstructs each channel. See [`formats/xa.md`](../formats/xa.md) for the sound-group decode (parameter/nibble layout, full-precision predictor) and [Cutscene / STR](cutscene.md) for the interleaved A/V path.

## Battle arts-voice shout path (engine)

The Tactical-Arts **shout** - each character's voice clip when an art executes - is CD-XA audio,
not a VAB one-shot. Retail: the staged-animation materialiser (`FUN_8004AD80`) calls the cue
selector `FUN_8004C140(char_id, action_constant, flag)`, which picks a channel from the art's
candidate-channel pool (random, avoiding an immediate repeat) and fires the CD-XA clip player
`FUN_8003D53C(clip_slot, channel, dur)`. Clip files are per character: Vahn=`XA2.XA`,
Noa=`XA4.XA`, Gala=`XA6.XA` (16-channel short-mono banks). The SCUS cue tables are parsed by
`legaia_art::arts_voice` (`ArtsVoiceTable`); the mapping is capture-verified two ways -
PCSX-Redux call-site traces (Vahn's Somersault → XA2 channels 0/6), and recomp-runtime battle
rounds instrumented by `scripts/recomp/xa_cue_capture.py` (frame-tagged reads of the
`FUN_8003D53C` cue globals), whose per-art witnessed picks are committed as
`arts_voice::CAPTURED_ART_CHANNELS`. The captures also pin *which* first-half table variant a
live battle uses - see
[battle-action.md](battle-action.md#battle-voice-cues---the-xa30-grunt-vs-the-xa2xa4xa6-arts-shout).

The engine wires this end-to-end:

- **Cue emission** (`engine-core`): executing arts through the live battle Arts
  command input pushes one `BattleShoutCue { cslot, action }` **per art the turn
  performs** (`apply_battle_art`), each keyed on that art's own record action
  constant. Retail stages every art's animation separately and the materialiser
  calls the cue selector per staging, so a three-art entry - the ordinary case,
  since entry runs until the AP pool is spent - requests three shouts. The port
  has no per-art animation timeline in the live loop, so the list is requested
  together on the animation-start frame, in performed order. A Miracle / Super
  replacement answers a single constant, its finisher: the per-constant staging
  inside a replacement queue is not captured, so it is not expanded. Unmatched
  directions (plain swings) and synthetic arts carry no constant and stay silent
  - the same degradation retail applies to an art with no cue-table entry.
  Drain: `World::drain_battle_shout_cues`.
- **Bank staging** (`engine-shell` boot): `read_arts_shout_bank` demuxes `XA2/XA4/XA6` per channel from the **raw 2352-byte sectors** (`legaia_xa::demux` - the CD-XA subheader carries the channel number, which a 2048-byte ISO view strips), decodes each channel to mono PCM, and pairs it with the `ArtsVoiceTable` pools in a `legaia_engine_audio::ArtsShoutBank`. Disc-image boots only; extracted-directory boots leave arts silent.
- **Playback** (`engine-audio` / `engine-shell`): `AudioBgmDirector::play_art_shout` resolves the cue against the bank (deterministic pool pick, no immediate repeat - `// PORT: FUN_8004C140`) and stages the clip through `AudioOut::play_xa_shout`, which mixes decoded XA into the SPU output the way the PSX CD-input path does (never through the 24 voices).

Two timing behaviours model the retail CD/XA sequencing contract (the recomp cross-reference established that the shout **trails** the art animation - the XA response arrives after the animation begins, never before): a fixed response-presentation delay (`SHOUT_CD_RESPONSE_DELAY`, ~150 ms of 44.1 kHz samples - the modeled seek/first-sector latency) gates the clip silent after the animation-start request; and a back-to-back request while a shout is still sounding queues behind it rather than cutting it (only the most recent pending clip is kept), so consecutive arts don't drop the later voice line.
`OfflineMixer` exposes the same mixing core device-free; the disc-gated oracle `engine-shell/tests/arts_shout_battle.rs` types an art into the live Arts command input and asserts the shout PCM lands in the mix only after the delay window, with `engine-core/tests/battle_shout_cue.rs` as the disc-free cue-emission check - one art, three arts in one entry, and the silent synthetic baseline.

### The second shout trigger - the animation cue track (`FUN_800508DC`)

`FUN_8004C140` above is not the only route to a shout. A playing battle action
entry also carries its own **cue track** at `entry + 0x54` - eight `(u16 frame,
u16 cue)` pairs, terminated by `cue == 0` - which `FUN_800508DC` walks once per
animation frame, resuming from a persistent cursor in the battle actor at
`+0x1F6`. Each call fires every cue whose trigger frame the clip has reached and
parks on the first it has not. Everything it fires goes out through the cue router
`FUN_8004FE5C`. Port: `legaia_engine_audio::anim_cue` (`walk_anim_cues`,
`AnimCueState`).

On a **party** seat (battle slot `< 3`) the cue-id band `0xC8..=0xFF`, minus the
single hole at `0xFA`, is the arts voice: the id is re-based by `+0x38`, which is
exactly what lifts `0xC8` to `0x100` and so puts the whole band in the `>= 0x100`
namespace `FUN_8004FE5C` routes to `FUN_8003D53C` instead of the SPU ring. Three
ids inside it are the per-character shout, and they map onto the same clip slots
the `FUN_8004C140` path uses:

| cue id | re-based | clip slot | character | XA file |
|---|---|---|---|---|
| `0xD7` | `0x10F` | `26` | Vahn | `XA2.XA` |
| `0xE7` | `0x11F` | `27` | Noa | `XA4.XA` |
| `0xF7` | `0x12F` | `28` | Gala | `XA6.XA` |

Those three, and only those three, get a **two-take coin flip**: one BIOS `rand()`
draw and `id + 0x38 - (r % 2)`, so the shout alternates between channels `7` and
`6` of the character's bank. They also bump a per-character tally at the live
`0x414`-stride record's `+0x98`, before any gate, and they honour a mute bit at
record `+0xF8 & 0x2000` that suppresses the shout outright.

The coin flip is further conditional on the CD being **free**. While a load is in
flight (`_DAT_8007BC20 != 0`) no XA stream can start, so the shout degrades to a
fixed SPU ring cue through `FUN_8004FCC8` - and the roster mapping there is not
monotonic: Vahn `0x56`, Noa `0x62`, Gala `0x5C`.

Cue ids below the band (and `0xFA`, and every id on a monster seat) route
unchanged except for a `+1` nudge when the entry's staged anim id is exactly
`0x12`; on a party seat whose record carries the `0x2000` bit that nudge becomes a
**suppression** for ids `>= 0x4D`. Source: `ghidra/scripts/funcs/800508dc.txt`
(disassembly).

### CD-XA voice-clip dispatchers and static cue census

Two SCUS entry points drive CD-XA voice/clip playback off the clip descriptor
table at `0x801C6ED8` (stride 8; `[+4]` = slot-valid flag, `[+0]` = the
descriptor word copied into the CD-read staging window):

- `FUN_8003D53C(clip_id, chan, dur)` - one-shot clip player. `clip_id` is the
  descriptor slot, `chan` the CD-XA channel inside that clip's interleave, `dur`
  the physical read span (clamped `<= 0x2A30`). Issues CD command `2`
  (see `ghidra/scripts/funcs/8003d53c.txt`).
- `FUN_8003EAE4(_, clip_id)` - streaming / loop start for one descriptor slot
  (CD command `0x15`); its first argument is unused and it takes no channel or
  duration (see `ghidra/scripts/funcs/8003eae4.txt`). Most callsites pass a
  compile-time literal `clip_id` - see the streamed-cue census below.
- `FUN_80019794(clip_id)` - SCUS wrapper around `FUN_8003EAE4`: a resumable
  five-state starter SM (state word `0x8007B9C8`, jump table `0x800103E4`) that
  arms the CD-busy byte, stops any in-flight read (`FUN_8003DE7C`), issues
  `FUN_8003EAE4(0, clip_id)` and finishes via `FUN_8003F2B8(1)`. Returns 1
  while in progress, 0 when the stream is running. The field overlay is its
  only caller (both sites below).

#### The clip-table writer - `FUN_801CFA78` (PROT 0895 `init.pak`)

The filler is not in `SCUS_942.54` and not baked into any disc file - it lives
in the **boot init overlay**, PROT entry 0895 (`init.pak`, CDNAME-labelled
`bat_back_dat`), which links at the slot-A base `0x801CE818`. Base recovery is
capture-free: the blob's own format strings (`\XA\XA%d.XA;1` at file `+0x124`,
`xa %s` `+0x134`, `not xa file %d` `+0x13C`, `\LEGAIA\MOV\MV2.STR;1` `+0x14C`)
are addressed by the code as `0x801CE93C`/`0x801CE94C`/`0x801CE954`/`0x801CE964`,
all four consistent with `base = 0x801CE818`, and every internal `j`/`jal`
resolves in-file under that base.

`FUN_801CFA78` (file `+0x1260`) fills all 34 slots at boot:

1. Zero-clears every slot's `+4` length word (loop from slot 33 down) and the
   counters `_DAT_8007BC20` / `_DAT_8007BBF8`.
2. For `i = 0..=0x21`: `sprintf(buf, "\XA\XA%d.XA;1", i+1)` (`FUN_800567B8`),
   debug-log `xa %s` (`FUN_800567A8`), then ISO9660 directory lookup
   `FUN_8005DBB4(&file_info, buf)` - the CdSearchFile-shape resolver that fills
   `{msf[3], size}` from the disc directory (its per-directory `CdlFILE` cache
   at `0x801CAE08` is why a title capture shows the `XA` directory resident).
3. On success, stores the three BCD-MSF bytes at slot `+0..+2` (byte `+3` stays
   zero) and the byte size at `+4`, then increments `_DAT_8007BBF8`. On a miss
   it logs `not xa file %d` and retries (retry budget 4; the retail flag
   `_DAT_8007B8C2` gates an immediate-retry variant).
4. After the loop, one extra lookup of `\LEGAIA\MOV\MV2.STR;1` - a dev-disc
   path that misses on the retail layout; it only re-targets the directory
   cache.

Caller: the init overlay's boot tick at `0x801CF500` (phase word == 3, one-shot
guarded by `_DAT_8007B868`), followed by `FUN_8003F120`. This closes the loop
on three earlier observations: the table is title-capture byte-exact vs the
disc's `XA/XA1.XA..XA34.XA` because it is *built from* the ISO directory at
boot (slot `i` = file `XA<i+1>.XA` by constructed name, not directory order -
the raw directory is alphabetical: `XA1, XA10, XA11, ...`); no `XA` filename
exists anywhere in SCUS because the names are `sprintf`-generated inside the
overlay; and a disc relayout stays safe because no absolute XA LBA is stored
anywhere on the disc (see [`formats/disc.md`](../formats/disc.md)).

#### One-shot cue census (`FUN_8003D53C`)

Byte-level `jal` sweep over `SCUS_942.54` + the full static-overlay corpus
(a decoded `jal` target is a property of the bytes - see
[`call-target-integrity.md`](../tooling/call-target-integrity.md)), deduplicated
against PROT entry over-read: a site only counts for the entry whose **true
extent** (`next_start_lba - start_lba`) contains it, because consecutive
entries' extraction footprints over-read into each other (the field-overlay
file carries PROT 0898's bytes from `+0x25000`, the slot-machine file carries
PROT 0976's from `+0x6000` - see
[`dump-corpus-integrity.md`](../tooling/dump-corpus-integrity.md)). Every
"field" hit above `+0x25000` and every "slot machine" `FUN_8003D53C` hit is
such an alias; the historical per-character-voice site "`0x8020a264`" is the
same double-shift (PROT 0897 file `+0x4A264` mapped at `0x801C0000`) and is
really battle-overlay VA `0x801F3A7C`.

**Literal `(clip_id, chan, dur)` cues** (`clip_id` = `0x801C6ED8` slot; slot
`i` = `XA<i+1>.XA`):

| clip | chan | dur | context | callsite |
|---|---|---|---|---|
| `0x10` (XA17) | `7` | `0x135` | scripted-scene fixed voice | field 0897 `0x801D509C` |
| `0x1D` (XA30) | `0` | `0x26` | normal-move grunt | battle 0898 `0x801EEB44` |
| `0x1D` (XA30) | `4` | `0x2E` | normal-move grunt | battle 0898 `0x801EEB44` |
| `0x1D` (XA30) | `6` | `0x1A` | normal-move grunt | battle 0898 `0x801EEB44` |
| `0x20` (XA33) | `1` | `0x36` | Baka Fighter duel line | 0976 `0x801D04EC` |
| `0x20` (XA33) | `2` | `0x45` | Baka Fighter announcer | 0976 `0x801D3968` |
| `0x20` (XA33) | `3` | `0x6D` | Baka Fighter announcer | 0976 `0x801D38E4` |
| `0x20` (XA33) | `4` | `0x35` | Baka Fighter announcer | 0976 `0x801D38A0` |
| `0x20` (XA33) | `5` | `0x39` | Baka Fighter announcer | 0976 `0x801D39BC` |
| `0x20` (XA33) | `8` | `0x4A` | Baka Fighter announcer | 0976 `0x801D1264` |
| `0x20` (XA33) | `9` | `0x4E` | Baka Fighter announcer | 0976 `0x801D0DF4` |
| `0x20` (XA33) | `0xA` | `0x46` | Baka Fighter announcer | 0976 `0x801D2220` |
| `0x20` (XA33) | `0xB` | `0x4D` | Baka Fighter announcer | 0976 `0x801D2258` |
| `0x20` (XA33) | `0xC` | `0x5A` | Baka Fighter announcer | 0976 `0x801D22FC` |
| `0x20` (XA33) | `0xE` | `0x3F` | Baka Fighter announcer | 0976 `0x801D5A50` |
| `0x20` (XA33) | `0xF` | `0x76` | Baka Fighter announcer | 0976 `0x801D5A98` |
| `0x1F` (XA32) | runtime (`0x801DBF8C`) | `0x48` | Baka Fighter duel line | 0976 `0x801D5CC4` |

Machine-readable form: `legaia_art::arts_voice::STATIC_XA_CUES`.

**Runtime-derived cues** (operands computed; the pair is named by its decode
rule):

| caller | clip_id | chan | note |
|---|---|---|---|
| `FUN_8004C140` arts shout | char `*2-1` = `1`/`3`/`5` | per-art pool pick | XA2/XA4/XA6; sites `0x8004C45C`/`0x8004C5B4`; parsed by `arts_voice` |
| `FUN_8004FCC8` / `FUN_8004FE5C` jingle | `(id-0x100)>>3` (odd slots 1/3/5 remap to `0x1A`/`0x1B`/`0x1C`) | `(id-0x100)&7` | dur `(u16[0x800788B8+n*2]*0x3C+99)/100`; sites `0x8004FD74`/`0x8004FF18` |
| `FUN_8004AD80` Hyper fanfare | char `*2` = `0`/`2`/`4` (XA1/XA3/XA5) | per-art pair, `rand()%2` flip | anim-`0x1A` block via the jingle queue; pinned in [battle-action.md](battle-action.md); mirror `legaia_art::hyper_fanfare` |
| `FUN_8004AD80` Super/Miracle fanfare | char `*2` (same banks) | `1` (generic, ids `0x101`/`0x111`/`0x121`) | Super-mark / scratch-word branch of the same block |
| anim cue track (`FUN_800508DC`) | `(cue+0x38-0x100)>>3` | `(cue+0x38)&7` | party cue ids `0xC8..=0xFF`; Miracle finisher witnessed at `0x12D` = XA29 ch 5 |
| field-VM XA opcode, `dur != 0` | `op>>3` | `op&7` | site `0x801E0420`; operands are per-scene MAN script literals |
| per-character voice | `char_byte + 0x19` = `0x1A`..`0x1C` (XA27..29) | `0` | dur `0x5A`; battle 0898 `0x801F3A7C` |
| debug sound-test | menu variable | menu variable | site `0x801CEF48` (overlay 0971) |

The field-VM opcode operands (`op>>3`, `op&7`) live in the per-scene MAN
scripts, which are disc-sourced and outside the committed dump corpus, so those
cues stay named by their decode rule. The arts-shout channel is a runtime pool
pick; the Hyper-fanfare channel pair and the Super/Miracle generic ids are
compile-time immediates of `FUN_8004AD80` (Confirmed - disassembly + recomp cue
captures; the full per-art table lives in
[battle-action.md](battle-action.md#battle-voice-cues---the-xa30-grunt-vs-the-xa2xa4xa6-arts-shout)).

#### Streamed cue census (`FUN_8003EAE4` / `FUN_80019794`)

Same sweep + dedupe. A streamed cue plays the whole clip (no channel filter).
The world-map-render (0901) and gameover (0902) raw hits are pure over-read
aliases - neither overlay starts an XA stream of its own.

| clip | file | context | callsite |
|---|---|---|---|
| `0` (XA1) | slot-machine ambience | casino slot machine entry | 0975 `0x801CF0AC` |
| `0x1F` (XA32) | Baka Fighter crowd/bed | duel start + round restart | 0976 `0x801CF6CC` / `0x801CFD90` |
| `0x21` (XA34) | long battle stream | battle actions `0x2E`/`0x2F` | battle 0898 `0x801EBDD4` |
| `0x800787AF` table (heroes `0x08` = XA9) | battle voice stream | `FUN_801E295C` SM state `0x6E` | battle 0898 `0x801E4F40`; same table in SCUS `FUN_8004DA00` |
| `(char-1)*2` = `0`/`2`/`4` (XA1/3/5) | per-character long bank | `FUN_8004DA00` battle stream selector | SCUS `0x8004DAFC` |
| `char + 0x19` = `0x1A`..`0x1C` (XA27..29) | per-character fanfare stream | `FUN_8004DA00` (spell-table class `< 0x14`) | SCUS `0x8004DB70` / `0x8004DBC4` |
| `7` (XA8) | fallback battle stream | `FUN_8004DA00` (other spell classes) | SCUS `0x8004DB9C` |
| `0x10` (XA17) | scripted-scene voice stream | field voice player, whole-clip variant | field 0897 `0x801D4FCC` via `FUN_80019794` |
| `op>>3` | MAN-script literal | field-VM XA opcode, `dur == 0` path | field 0897 `0x801E0430` via `FUN_80019794` |
| `7` (XA8) | Ra-Seru summon stream | summon overlays 0903/0904/0905/0906/0907/0908 | each at its own `0x801F6Cxx`-`0x801F71xx` site (slot-B base `0x801F69D8`) |
| `6` (XA7) | summon stream | PROT 0909 (outside the static corpus; head decoded from PROT.DAT) | 0909 file `+0x218` |
| `0x11` (XA18) | attack-art stager stream | stagers 0924/0925/0926 | 0924 `0x801F6C80`; 0925/0926 file `+0x240` |
| `0xE` (XA15) | high-summon / evil-god stream | summons 0927..0934 | each at its own `0x801F6Cxx`-`0x801F6Dxx` site |

The three SCUS rows all belong to one resident selector, and it is not called
from anywhere: `FUN_8004DA00` is the `+0x08` tick of the
[static actor template](../reference/functions/runtime-libs.md#static-actor-templates)
at `0x800767F4`, which the battle scene-loader `FUN_800513F0` spawns into the
system actor pool as its last act (`0x80051D3C`). So the party voice selector
is a per-frame pass that goes resident when the battle loads and stays up for
its duration, arming at most one clip per action behind the `_DAT_8007BDB0`
latch. The port models the choice, not the residency:
`legaia_engine_audio::battle_voice`.

The field-VM XA opcode thus has **two shapes**: a non-zero third operand plays
one channel one-shot (`FUN_8003D53C(op>>3, op&7, dur)`); a zero operand streams
the whole clip (`FUN_80019794(op>>3)`).

## What a normal attack sounds like, and why the port's is silent

The plain melee swing is the most common sound in a fight, and it is **two**
emissions from one routine. `FUN_801EC3E4` (battle overlay 0898, the melee
roll pair) calls:

- `FUN_8003D53C(0x1D, chan, dur)` at `0x801EEB44`, the XA30 grunt - three arms
  select `(0, 0x26)` / `(4, 0x2E)` / `(6, 0x1A)`, the same character-indexed
  channel spacing the XA2/XA4/XA6 shout banks use. Already tabulated under the
  [one-shot cue census](#one-shot-cue-census-fun_8003d53c).
- `FUN_8004FE5C(0x10C, cat)` at `0x801EEBE8`, the cue router. `0x10C` is above
  `0x100`, so for a party attacker it takes the router's **XA voice** leg -
  clip `(0x0C >> 3) = 1` remapped to `26`, channel `0x0C & 7 = 4` - and for a
  non-party attacker the high element-tinted ring leg (`id + 0x19C = 0x2A8`).

Both are `see ghidra/scripts/funcs/overlay_battle_action_801ec3e4.txt`
(disassembly, not the C). Note what that makes retail's impact sound: a
**streamed CD-XA clip**, not an SPU descriptor one-shot.

That is the shape of the port's silence, and it is specifically a *sound
effect* silence: a battle does have music, because the field track keeps
playing through the swap (the [audio-trace section](#audio-trace-parity-oracle)
records why no battle track resolves). What a whole fight produces is zero
cues. Three gaps, in order of what they cost:

1. **The live battle loop emits no cue at all.** `World::fold_battle_event`
   turns an `ApplyArtStrike` outcome's `is_sound` cues into `battle_sfx_cues`,
   and the only caller of that fold is the `BattleSession` path. The live loop
   (`live_battle_tick`) resolves damage inline and queues a presentation-only
   `BattleHitFx`, so a fight driven by the window host or the play page produces
   an empty cue queue for its whole duration.
2. **No CD-XA bank is staged for the battle voice clips.** Boot demuxes
   `XA2`/`XA4`/`XA6` for the arts shouts (`read_arts_shout_bank`) and nothing
   else, so even a correctly routed `0x10C` or `0x1D` cue has no PCM to play.
   `legaia_engine_core::sfx_cue::route_sfx_cue` - a complete port of
   `FUN_8004FE5C` - has no caller for the same reason.
3. **The cue queue is `u16` and the descriptor bank is `u8`.** The battle action
   SM's cast cues run to `0x20E`; truncating them into the descriptor space did
   not silence them, it played a *different populated descriptor*
   (`0x20C` → `0x0C`). The consumer now classifies with `classify_cue` and drops
   the voice band instead. The low band's retail `id - 1` resolution is **not**
   applied: the one live producer feeds this queue an art-record `HitCue::kind`
   the bank is already indexed by, and moving it without an oracle would break
   the one cue that works.

## Audio-trace parity oracle

Mirror of the VRAM-byte and mode-trace parity oracles on a third axis: per-frame voice activity. The retail side has two capture shapes, with the same `AudioTraceFrame` JSONL wire format on both:

1. **Single-cycle snapshot** lifted from a mednafen save state's `SPU` section via `legaia_mednafen::PsxSpu` (24 voice records, master volume sweep, voice-on/-off masks, reverb mode, 512 KiB SPU RAM). One `.mc{slot}` save → one retail `AudioTraceFrame`. Convergence is "did any engine frame in the window match retail's voice mask?".
2. **Multi-frame trace** captured by [`autorun_audio_trace.lua`](../tooling/pcsx-redux-automation.md#runtime-probes-lua-autorun) running inside PCSX-Redux: per-vsync `PCSX.createSaveState()` calls, the SPU sub-message sliced out via FFI pointer arithmetic, decoded offline into JSONL by [`extract_audio_trace_from_sstates.py`](../../scripts/pcsx-redux/extract_audio_trace_from_sstates.py). Convergence becomes "for every retail vsync with audio playing, did the engine ever match?", applied frame-by-frame via [`first_audio_trace_divergence_multi`](../../crates/engine-shell/src/audio_trace_oracle.rs).

The engine side runs a standalone `legaia_engine_audio::Spu` + optional `Sequencer` alongside a headless `BootSession::tick`, sampling voice / master / reverb state after each frame. JSONL records: `AudioTraceFrame { frame, sequencer_playhead_ticks, sequencer_finished, master_volume, reverb_mode, active_voice_mask, voices[24] }`. Convergence rule per retail frame: at least one engine frame's `active_voice_mask` is a superset of retail's mask AND for every retail-active voice the engine matches `start_addr` (when both sides report it).

PCSX-Redux's Lua API does not expose the SPU register file directly
(`SPUInterface::lockSPURAM` is C++-internal, not bound). The probe leans on
`PCSX.createSaveState()` which returns the full state as a protobuf slice
(~20 MiB); the autorun script walks the slice in-place via FFI and writes only
the ~600 KiB SPU sub-message to disk so per-vsync GC pressure doesn't disrupt
`GPU::Vsync` event delivery (same shape as the `readAt(2 MiB)` caveat in
[`pcsx-redux-automation.md`](../tooling/pcsx-redux-automation.md)). The SPU
schema is the one declared in PCSX-Redux's `src/core/sstate.h` +
`src/spu/types.h`: `Channel.Data.on || .stop` is the retail-side "audible"
criterion (`ADSRInfoEx.state` is the configured next-attack shape and reads as
Sustain even for unused voices, so it's not a reliable audibility signal).

Two known asymmetries the diff function explicitly models:

1. **Headless engine SPU.** `BootSession` only attaches a real cpal `AudioOut` when `enable_audio = true`, which fails in CI. The oracle constructs a standalone `Spu` in parallel and routes scene-resolved BGM events into it. Not bit-identical to the retail SPU, but the voice-activity envelope is.
2. **Retail capture shape.** The single-snapshot case freezes one SPU cycle; the multi-frame case carries per-vsync state. Engine produces `frames + 1` records either way. `NoFrameMatched` stays tolerable drift in both modes; `VoiceStartAddrMismatch` and `MasterVolumeMismatch` are hard failures.

Entry points:

- Library: [`engine_shell::audio_trace_oracle`](../../crates/engine-shell/src/audio_trace_oracle.rs) - `build_engine_audio_trace`, `load_runtime_audio_trace_from_save`, `load_runtime_audio_trace_jsonl`, `first_audio_trace_divergence`, `first_audio_trace_divergence_multi`, JSONL round-trip.
- CLI: `legaia-engine audio-trace --scene NAME` (explicit), `--scenario LABEL` (single-snapshot vs `.mc{slot}` SPU), or `--retail-jsonl PATH` (multi-frame vs PCSX-Redux capture).
- Disc-gated tests:
  - [`audio_trace`](../../crates/engine-shell/tests/audio_trace.rs) - auto-discovers scenarios with both `expected_active_scene` and an on-disk `.mc{slot}` save.
  - [`audio_trace_multi`](../../crates/engine-shell/tests/audio_trace_multi.rs) - same scenario walk but skips unless `LEGAIA_AUDIO_TRACE_JSONL_DIR` points at a directory containing `<label>.jsonl` files from the PCSX-Redux probe.

The engine drives BGM through a private `TraceBgmDirector` that routes field-VM op `0x35` events into a headless `Sequencer` in lock-step with `SceneHost::route_bgm_events`. `NoFrameMatched` is treated as tolerable drift (scene prescript may not emit op `0x35` within the trace window, or may target a different track than retail captured); `VoiceStartAddrMismatch` and `MasterVolumeMismatch` are hard failures.

The **Field↔Battle BGM-swap** is *not* yet observable through this
voice-activity oracle, and not for an oracle reason: the engine's opening
battle is a `SceneMode::Battle` overlay on the loaded field scene
(`enter_battle_from_formation` does not load a distinct battle audio bundle),
and a field scene's per-scene BGM table carries no battle track - `town01`
resolves *zero* battle ids through `SceneAssets::bgm_seq_entry`, so the
`swap_to_battle_bgm` start event resolves to no SEQ bytes and no battle voices
key on. The swap *contract* (track stash → battle start → field restore) is
modeled and regression-tested at the `World` level
(`battle_bgm_swaps_on_encounter_and_restores_on_finish`); the *audible* swap
stays blocked on the engine resolving a battle track from the (currently
unloaded) battle bundle. So the v0.1 playthrough oracle pins the Field→Battle
transition on the mode-trace axis (`v0_1_battle_leg_mode_trace_matches_expected`),
not the audio axis.

## What's left

The byte-level layouts of `.MAP / .PCH / .spk / .dpk / .pac` are still TBD. The dispatch chain *into* them is fully traced; the next move is to read the body of `FUN_8001FA88` for the `.dpk` byte layout (specifically the field accesses on `_DAT_8007B8D0` after the path-based opener returns - `_DAT_8007B8D0 + 2` is read as a `ushort` and used as a divisor, almost certainly a record count).

Eventual home: a `crates/sound` companion to `crates/vab`.

## See also

**Reference** -
[VAB sound bank](../formats/vab.md) ·
[SEQ sequence](../formats/seq.md) ·
[Sound-driver outputs](../formats/sound-driver.md) ·
[Cutscene / STR](cutscene.md)
