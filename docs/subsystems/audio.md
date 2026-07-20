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
- [SsAPI sequencer](#ssapi-sequencer-0x80061-0x80067-cluster) - [globals](#globals) · [public SEQ API](#public-seq-api) · [SEQ internals](#seq-internals) · [voice / mixer](#voice--mixer-audible-output-critical-path) · [SPU command shims](#spu-command-shims-0x81-scaling--0127--016383) · [renderer-citation correction](#renderer-citation-correction)
- [libspu / SPU control](#libspu--spu-control-0x80068-0x8006d-cluster) - [SPU globals](#spu-globals) · [primitives](#libspu-primitives) · [DMA transfer engine](#spu-dma-transfer-engine) · [reverb model](#reverb-model-engine-audio) · [Gaussian resampler](#voice-resampler---4-point-gaussian-interpolation-engine-audio) · [SsApi seq-management layer](#ssapi-seq-management-layer-above-libspu)
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

Both `FUN_8001FA88` and `FUN_8001FC00` carry a dev/retail split via `_DAT_8007B8C2`. The dev branch loads via PROT indices directly; the retail branch uses dev-style paths through `FUN_8003E6BC` (the path-based opener that resolves `h:\main\bg\domepack\…` into the appropriate PROT entry through the [CDNAME-driven name map](../formats/cdname.md)). Both paths land at the same files.

## VAB sound banks

Sony's standard `VABp`-magic instrument bank format. Documented at [`formats/vab.md`](../formats/vab.md). The dominant on-disc carrier is the [scene-VAB-prefixed streaming](../formats/scene-bundles.md) shape - the VAB body is preceded by a 4-byte chunk0 header. Implementation: `crates/vab` (header parser + extractor + ADPCM decoder).

Bulk scan finds 1191 `VABp` headers across 239 PROT entries. Multi-bank archives at `0889_sound_data2`, `0890_sound_data2`, `0891_level_up`. The `vab_01` cluster (CDNAME indices 1072–1194) is the standard distributed-bank layout.

## Per-actor sound effects

`FUN_800250D4(sound_id, voice)` is the per-actor SFX trigger called from the actor tick (`FUN_80021DF4`) when `actor[+0xb4] != 0` (one-shot pulse) or `actor[+0xac]` is staged (continuous). It looks up a sound entry at `&DAT_8006F198 + sound_id*8` for `sound_id < 0x200`, or in the runtime-allocated table at `_DAT_8007B8D0` for higher IDs (the `.dpk` consumer's bank). The entry's `byte[3] & 0x1F` is the voice count; the helper then calls `FUN_800653C8` (libSPU `SpuKeyOn`-equivalent) for each of `voice..voice+count-1`.

`actor[+0xac]` (sound ID) and `actor[+0xb0]` (voice) are written by move-VM and field-VM opcodes; the move-VM tick in `FUN_80021DF4` re-fires the SFX whenever the trigger flag at `actor[+0xb4]` is set.

The static `&DAT_8006F198` table is **100 8-byte descriptors** (sound ids `0x00..=0x63`); the `< 0x200` runtime check is a bound, not the size (id `0x64` onward is the `\PSX.EXE` dev-path rodata). Besides `FUN_800250D4` above, the cue-ring drainer `FUN_80016B6C` reads it and programs each voice via `FUN_80065034` (the libsnd `SpuSetVoiceAttr` analogue). Each entry decodes as `[+0 program][+1 tone/region base][+2 note-level][+3 voice-count + sustained bit 0x20][+4 channel]`; full layout + provenance on [`docs/formats/sfx-table.md`](../formats/sfx-table.md). Parser `legaia_asset::sfx_table` (disc-decoded, byte-exact vs live save-state RAM); the SPU programming itself is libsnd, out of clean-room scope.

## Monster sound bank - `h:\mpack\monster.snd`

Battle-time monster sound banks live in a single packed `monster.snd` file. The loader is `FUN_8003E104(monster_idx, slot, dst_buf)` - called twice from the battle scene loader `FUN_800520F0` (slots 7 and 8, for the active battle's two monster sound banks). It reads the file's per-monster TOC at `0x801C8980 - 0x10` (4-byte stride, paired entries giving `[start_lba, end_lba+1]`), computes the LBA range, and dispatches:

- **Dev path** (`_DAT_8007B8C2 != 0`) - uses the standard library file API: `FUN_800608F0` (fopen) → `FUN_80060920` (fseek to record × 0x800) → `FUN_80060944` (fread) → `FUN_80060910` (fclose). Path string: `h:\mpack\monster.snd`.
- **Retail path** - stages `(size, dst)` into the gp window at `+0x97c` / `+0x894`, kicks the async CD read via `FUN_8003F128`. Sets a 120-frame timeout at `+0x91c`.

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
`990`, which is `MUSIC_BANK_EXTRACTION_BASE` in the extraction frame.

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

The engine port reuses this same dispatch for the **Battle↔Field music swap**: `World::set_battle_bgm` configures a battle track id, and the live gameplay loop queues an ordinary `FieldEvent::Bgm{sub_op: 1}` start for it on encounter (`swap_to_battle_bgm`) and resumes the stashed field track on battle end (`restore_field_bgm`). The host's `AudioBgmDirector` cross-fades both transitions over ~0.5 s through its existing `start_inner` path - no separate battle-audio code path. The battle id must resolve in the current scene's BGM table since the live loop doesn't load a distinct battle audio bundle.

### Global-pool BGM: the `music_01` bank

Every real music track on the disc lives in the **`music_01` bank** (extraction PROT `990..=1071`), not in scene-local slots - scenes carry no SEQ of their own (see [`reference/music-tracks.md`](../reference/music-tracks.md) for the sound-test join). A global-pool id (`>= 2000`) is `2000 + slot`, and each bank entry is one self-contained `[VAB][SEQ]` pair (a chunk-header, a `pBAV` VAB body, then a `pQES` score). Playing one means uploading **that entry's own VAB** into SPU RAM and driving the sequencer against it, rather than the scene VAB the field path stages.

The site's minigame pages take exactly this path per game (`crates/web-viewer/src/minigames.rs`): `render_music01_bgm` / `render_music01_loop` split the pair, `VabBank::upload` the VAB, and render through the clean-room `Spu` + `Sequencer` - the same components the live `AudioBgmDirector` uses. Minigame BGM sources are disc-pinned constants: the Baka Fighter overlay init loads `music_01` slot 53 (boss overture, extraction 1043); the dance overlay loads slots 58/64 (extraction 1048/1054, mode-selected - short chart-sized loops, see [`minigame-dance.md`](minigame-dance.md)); the slot machine and fishing/Muscle Dome start **no** track and inherit their host scene's op-`0x35` BGM. The `music01_bgm_render` WASM surface renders any bank slot for the dance's Sol-disco jukebox.

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

**Per-frame tick call graph.** The concrete chain behind the prose "hand the payload to `FUN_80062340` for playback": `FUN_80062F98` (per-slot fan-out) → `FUN_8006320C` / `FUN_8006352C` (the per-channel note/expression handlers over `_DAT_801CD2C0[slot]`) → `FUN_80066308` (note-trigger dispatch; `×0x81` velocity scale, per-slot status `_DAT_801CE34x`) → `FUN_80066B00` (voice-allocation scan) → `FUN_80065978` (`_SsVoKeyOnDirect`), with `FUN_80065BAC` / `FUN_800675C8` (the voice flush / release sweep below) carrying the result to the SPU. The SEQ-stream cursor advances through `FUN_80063CEC` (calls the varint decoder `FUN_80061C68`, steps `_DAT_801CD220..230`) with track-end / vab-release in `FUN_80063AA8`. This is `sequencer.rs`'s integer-accumulator event loop in retail form.

**Correction** (label ≠ role): `FUN_8006352C` / `FUN_8006320C` were tagged elsewhere as "fixed-point div" pitch kernels - they carry **no division** and are per-channel note/expression handlers. The fixed-point note→pitch math is confined to `FUN_80066E50` (`_SsPitchFromKey`) and `FUN_8006C6E4` (`_SsKey2Pitch`); no additional pitch kernel exists in this cluster.

### Voice / mixer (audible-output critical path)

| Function | Role |
|---|---|
| `FUN_80067550(voice, key, vel, ...)` | `_SsVoNoteOn` - master-vol × velocity × channel vol(`+0x58`)/pan(`+0x5A`) × four expression sliders × stereo-pan square law (`uV*uV/0x3FFF`); writes `&DAT_801CE080[voice]`, sets per-voice flags `0x7`, updates active-voice masks at `_DAT_801CDB48/4A/4C/4E` and `_DAT_801CE248/24A`. |
| `FUN_80067E9C(slot, vol, pan, ...)` | `_SsSeqNoteOn` - iterates `DAT_801CE344`, calls `FUN_80068B98` (program-change?), runs the same vol/pan chain as `FUN_80067550`. Sequence-driven keyon. |
| `FUN_80065978(...)` | `_SsVoKeyOnDirect` - consumes the **already-chosen** voice at `_DAT_801CE362` (the `FUN_80066B00` scan's winner): clears that voice's bit from all 16 silent-history ring words at `_DAT_801CE208`, sets its envelope word to `0x7FFF`, looks up region in `_DAT_801CE334` (stride `0x10`), writes pitch + base note to `&DAT_801CE088 + voice*2`, ORs flags `0x8/0x30` into `&DAT_801CE060`. |
| `FUN_80066E50(key, fine)` | `_SsPitchFromKey` - indexes 12-entry pitch table `&DAT_8007A940`, octave-shift by `(oct-5)`. Returns 16-bit SPU PITCH register value. |
| `FUN_80065B88` | `SsResetTranspose` - single-store stub: zeros `_DAT_801CE2E8` (a base-note offset shifted in by `FUN_80065978`). |

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
Provenance: per-instruction read of the decompiled reference for `FUN_80066B00` / `FUN_80065BAC` / `FUN_80065978` / `FUN_80066308`; no Ghidra dump exists for this tier.

### SPU command shims (`*0x81` scaling = 0..127 → 0..16383)

| Function | Role |
|---|---|
| `FUN_80062AA0(x, y)` | `SsSetMVol` - packs `[cmd=3, x*0x81, y*0x81]`, calls `FUN_8006BCB4` (SPU-cmd dispatcher). |
| `FUN_80065440(p1, p2)` | Single-shot SPU command (likely `SsUtKeyOn` or `SsUtPitchBend`) - `[cmd=6, p1*0x81, p2*0x81]`, calls `FUN_8006ACBC` (sister of `FUN_8006BCB4`). |

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
| `_DAT_801CE564 / _DAT_801CE574` | **Function-pointer hooks installed by Legaia.** `_564` resolves the active script-VM seq context; `_574` is a worker-availability check. Distinct from the standard PsyQ in-line slot lookup, so the actor / field VM is wiring callbacks here. |

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
| `FUN_8006A158` | `SsSpuMalloc` core | 712-byte block allocator. Walks the `_DAT_8007AFA4` block table, returns the start of the first free run of size `>= request`, marks header word `0x40000000` end-of-table where appropriate. Called from `FUN_80068D94` (SEP loader). |
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
| `FUN_80068B98(slot, track)` | `SsSeqOpen` - bounds-checks slot + track count `_DAT_801CE332`, populates seq-state globals. |
| `FUN_80068C5C / 80068C70` | Auto-poll on/off (`_DAT_801CE330 = 1 / 0`). |
| `FUN_80068C80(slot)` | `SsSeqClose` - calls `SpuFree` on resident addr at `+0x68`, decrements `_DAT_801CE3C0`. |
| `FUN_80068D34(...)` | `SsSeqPlay` 1-shot wrapper - tail-calls `FUN_80068D94` with `mode=1`. |
| `FUN_80068D94(seq_data, mode)` | **`SsSepOpen` / SEP loader core.** 988 bytes. Validates `0x564150` ('VAP' magic), reads SEQ header `numTracks` at `+0x12`, calls `FUN_8006A158` (`SsSpuMalloc`), patches per-track pointer table, writes MIDI body to SPU. |
| `FUN_80069170(slot)` | `SsSeqPlayResolved` - final play-start stage; calls `8006BB08(0)` (xfer-mode), `8006BAB0` (commit), `8006BA50` (data feed). |
| `FUN_80069230(...)` | Streaming SEP feeder - partial-buffer continuation via `_DAT_8007AAC4/AAC8`. |
| `FUN_80069390(...)` | `SsIsEos` - tail-call to `FUN_8006BBC8`. |
| `FUN_8006CA7C` | `SsSeqGetStatus` - resolves ctx via `_DAT_801CE564`, returns ctx `+0x49` with state-code normalization (`3↔1, 2→1, 6→4`). |
| `FUN_8006CB3C(attr_id)` | `SsSeqGetAttr` - switches on `attr_id`: `1` byte@`+0xE8`, `2` u16@`+0xE6`, `3` byte@`+0xE4`, `4` u16@`+0+idx*2`/count@`+0xE3`, `100` u32@`+0x4C`. |
| `FUN_8006CDB0` | `SsSeqSetCallback` - resolves ctx via `_DAT_801CE564`, tail-calls `FUN_8006DDC8`. |
| `FUN_8006CE30` | `SsSeqSetUserData` - resolves ctx via `_DAT_801CE564`, tail-calls `FUN_8006D7B4`. |
| `FUN_8006D7B4` | `_SsSeqSetUserDataInner` - `ctx[+0x28] = p2; ctx[+0x34] = p3`. |
| `FUN_8006DDC8` | `SsSeqSetMarkCallback` - installs trampolines at ctx `+0x14/+0x18`, sets active-flag at `+0x46`. |

The runtime sequencer chain is now nearly fully mapped: slot bitmap @ `_DAT_801CD2B8` → ptr table @ `0x801CD2C0` → per-slot record (stride `0x36`) at `0x801CDB60` → VAB program-attr (stride `0xB0`) at `0x801CD2C0[i] + prog*0xB0`.

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
`play_note` leaves the voice at `master × velocity × tone-vol`, tone-panned,
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
| [`vab_bind::VabBank`](../../crates/engine-audio/src/vab_bind.rs) | Bridges `legaia_vab::VabReport` into the SPU: `upload(spu, alloc, report, buf)` drops every VAG body into SPU RAM through the allocator, and `play_note(spu, voice, prog, note, velocity)` translates a MIDI key into voice config + key-on. Pitch math matches `_SsKey2Pitch` / libspu key-to-pitch. |
| [`AudioOut`](../../crates/engine-audio/src/lib.rs) | Owns a single cpal output stream that drains the `Spu` at 44.1 kHz and resamples to the host device rate (linear). Engines call `with_spu(|spu| ...)` from outside the audio thread to push voice attributes / key-on masks. |

What this **does not** model (out of scope for the first port pass):

- Pitch modulation, noise, FM. None of these are used by Legaia (verified against the libspu calls in the SCUS dumps - `SpuSetPitch` is the only pitch path).
- Asynchronous DMA timing. The transfer engine here is synchronous (the queue + drain are collapsed) - fine because the playback layer reads SPU RAM directly during voice ticks. The real hardware is asynchronous via the transfer engine described above; the model preserves the *API shape* (`set_transfer_start_units_8` / `set_direction` / `write`) so the libspu callers map cleanly.

## SFX bank + scheduler

Maps battle / field cue IDs (the `kind` byte the art-record `HitCue` / overlay scripts emit) to per-cue `SfxEntry` descriptors that describe how to fire a one-shot through the SPU. Engines populate the catalog at startup, then forward `ScheduledCue`-like requests through `SfxScheduler` which queues each request with its retail timing offset and dispatches when the per-frame tick reaches the firing frame.

`SfxBank::from_descriptors` builds the catalog straight from the disc-decoded static SFX table (`legaia_asset::sfx_table`): each active descriptor's `program` becomes the `program_index` and its `note` the `key`, so the cue ids `0x00..=0x63` resolve to the retail program/tone instead of a hand-authored stand-in.

The bank those programs index is **not a dedicated SFX VAB** - it is the active scene's music VAB. `FUN_80065034` reads the libsnd current-bank globals (`_DAT_801ce33c`/`_DAT_801ce334`/`_DAT_801ce340`), which point at the per-scene `scene_vab_stream` bank the BGM sequencer has open: across the save-state catalogue that bank is 13 distinct VABs, and for a `music_01`-scene state it is byte-identical to the disc `music_01` VAB. So the engine fires a cue with `SfxBank::play_one_shot(spu, scene_vab)` against the BGM `VabBank` it already loaded - no separate SFX bank. Because scene banks differ in size (`1..=16` used programs), a cue resolves only where its program/tone exists; see [`formats/sfx-table.md`](../formats/sfx-table.md).

| Cue ID | Meaning |
|---|---|
| `0x1A` | Generic SFX trigger ("play sound" hit cue). Catalog typically maps to per-strike weapon impact tones. |
| `0x4C` | Hit-effect visual (no sound on its own; engines that fold the visual into a synced sound use this slot). |
| `0x80..=0xFE` | Reserved per-character / per-art SFX IDs. Indexed from the per-actor `+0x9C0` table at retail. |

`SfxBank::play_one_shot` delegates to the existing `VabBank::play_note` for tone lookup, pitch math, and ADSR setup; the scheduler is a frame-driven queue that returns an `SfxFireBatch` per `tick_frame` call so engines can dispatch through the same `VabBank` they already wired for the BGM sequencer. A `PendingCue` with `frames_remaining = 0` fires on the next tick, so a cue queued mid-frame doesn't fire immediately and gives the host a chance to clear render state first - matching the retail timing where a `HitCue::timing_frames = 1` cue plays one frame after the strike begins.

Implementation: [`crates/engine-audio::sfx`](../../crates/engine-audio/src/sfx.rs).

## XA-ADPCM

`crates/xa` decodes CD-XA 4-bit ADPCM bit-exactly: on a real cutscene track its per-channel PCM matches an external lossless reference decode sample-for-sample. The on-disc `.XA` / `.STR` audio is standard CD-XA Mode 2 Form 2 - the earlier "non-standard interleave" was Form-1 truncation damage in the old extractor, not a bespoke format. The demuxer (`legaia_xa::demux`) splits raw 2352-byte sectors by `(file_no, ch_no)` and the group decoder reconstructs each channel. See [`formats/xa.md`](../formats/xa.md) for the sound-group decode (parameter/nibble layout, full-precision predictor) and [Cutscene / STR](cutscene.md) for the interleaved A/V path.

## Battle arts-voice shout path (engine)

The Tactical-Arts **shout** - each character's voice clip when an art executes - is CD-XA audio, not a VAB one-shot. Retail: the staged-animation materialiser (`FUN_8004AD80`) calls the cue selector `FUN_8004C140(char_id, action_constant, flag)`, which picks a channel from the art's candidate-channel pool (random, avoiding an immediate repeat) and fires the CD-XA clip player `FUN_8003D53C(clip_slot, channel, dur)`. Clip files are per character: Vahn=`XA2.XA`, Noa=`XA4.XA`, Gala=`XA6.XA` (16-channel short-mono banks). The SCUS cue tables are parsed by `legaia_art::arts_voice` (`ArtsVoiceTable`); the mapping is capture-verified (Vahn's Somersault → XA2 channels 0/6).

The engine wires this end-to-end:

- **Cue emission** (`engine-core`): executing an art through the live battle Arts menu pushes one `BattleShoutCue { cslot, action }` onto the world on the art's animation-start frame (`apply_battle_art`), keyed on the menu row's matched art-record action constant (`ArtRow::action`). Synthetic rows with no matched record carry no constant and stay silent - the same degradation retail applies to an art with no cue-table entry. Drain: `World::drain_battle_shout_cues`.
- **Bank staging** (`engine-shell` boot): `read_arts_shout_bank` demuxes `XA2/XA4/XA6` per channel from the **raw 2352-byte sectors** (`legaia_xa::demux` - the CD-XA subheader carries the channel number, which a 2048-byte ISO view strips), decodes each channel to mono PCM, and pairs it with the `ArtsVoiceTable` pools in a `legaia_engine_audio::ArtsShoutBank`. Disc-image boots only; extracted-directory boots leave arts silent.
- **Playback** (`engine-audio` / `engine-shell`): `AudioBgmDirector::play_art_shout` resolves the cue against the bank (deterministic pool pick, no immediate repeat - `// PORT: FUN_8004C140`) and stages the clip through `AudioOut::play_xa_shout`, which mixes decoded XA into the SPU output the way the PSX CD-input path does (never through the 24 voices).

Two timing behaviours model the retail CD/XA sequencing contract (the recomp cross-reference established that the shout **trails** the art animation - the XA response arrives after the animation begins, never before): a fixed response-presentation delay (`SHOUT_CD_RESPONSE_DELAY`, ~150 ms of 44.1 kHz samples - the modeled seek/first-sector latency) gates the clip silent after the animation-start request; and a back-to-back request while a shout is still sounding queues behind it rather than cutting it (only the most recent pending clip is kept), so consecutive arts don't drop the later voice line.
`OfflineMixer` exposes the same mixing core device-free; the disc-gated oracle `engine-shell/tests/arts_shout_battle.rs` drives an art through the live battle session and asserts the shout PCM lands in the mix only after the delay window, with `engine-core/tests/battle_shout_cue.rs` as the disc-free cue-emission check.

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
  duration (see `ghidra/scripts/funcs/8003eae4.txt`). All of its callers pass
  `clip_id` from a battle-action / magic-overlay data table, so it contributes
  no static `(clip_id, chan)` pair.

A caller census of `FUN_8003D53C` across the committed dumps splits into cues
whose `(clip_id, chan)` are compile-time immediates and cues whose operands are
computed at runtime.

**Confirmed literal `(clip_id, chan)` cues.** `clip_id` is the `0x801C6ED8`
descriptor slot; `dur` listed where the site supplies a literal operand.

| clip_id | chan | dur | context | site (dump) |
|---|---|---|---|---|
| `0x10` | `7` | `0x135` | scripted-scene / dialog fixed voice | `801d509c` (`overlay_0897_locomotion_cluster.txt`) |
| `0x1D` | `4` | `0x26` | battle / field encounter-engage cue | `801eeb44` (`overlay_0898_801ec3e4.txt`) |
| `0x1D` | `6` | `0x1A` | battle / field encounter-engage cue | `801eeb44` (`overlay_0898_801ec3e4.txt`) |
| `0x20` | `2`,`3`,`4`,`5`,`8`,`9`,`0xA`,`0xB`,`0xC`,`0xD`,`0xE`,`0xF` | `0xC`->`0x5A`, `0xD`->`0x66` | Baka Fighter announcer lines | `overlay_baka_fighter_*.txt` |

The Baka Fighter bank fires descriptor slot `0x20` across the twelve fixed
channels listed (call sites `801d3468` / `801cf388` / `801d21fc` / `801d5a24`);
two further duel sites take a runtime channel on slot `0x20` (`801d04ec`) and
slot `0x1F` (`801d5cc4`). Machine-readable form:
`legaia_art::arts_voice::STATIC_XA_CUES`.

**Runtime-derived cues** (operands not static; the pair is named by its decode
rule, not enumerable from the committed corpus):

| caller | clip_id | chan | note |
|---|---|---|---|
| `FUN_8004C140` arts shout | char `*2+1` = `1`/`3`/`5` | per-art pool pick | XA2/XA4/XA6; parsed by `arts_voice` |
| `FUN_8004FCC8` / `FUN_8004FE5C` jingle | `(id-0x100)>>3`, remapped to `0x1A`/`0x1B`/`0x1C` | `(id-0x100)&7` | Miracle / summon fanfare queue |
| field-VM XA opcode | `op>>3` | `op&7` | site `801e0420`; operands are per-scene MAN script literals |
| per-character voice | `table[char_id]` | `0` | dur `0x5A`; dance minigame + site `8020a264` |
| debug sound-test | menu variable | menu variable | site `801cef48` (overlay 0971) |

The field-VM opcode operands (`op>>3`, `op&7`) live in the per-scene MAN
scripts, which are disc-sourced and outside the committed dump corpus, so those
cues stay named by their decode rule. The arts-shout and jingle channels are
runtime pool / event-id picks; only their `clip_id` space is fixed.

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
