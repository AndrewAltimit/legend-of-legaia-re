# Audio

Three layers: the path-string cluster that builds audio file paths, the SCUS dispatchers that consume them, and the actual audio formats (VAB sound banks + the still-TBD `.dpk / .MAP / .PCH / .spk / .pac` family).

## Path-string cluster

The string cluster at `0x8007B380` holds the file extensions the sound subsystem appends to scene-asset paths. Full layout in [`formats/sound-driver.md`](../formats/sound-driver.md). Eight extensions in the cluster: `.spk`, `.LZS`, `.dpk`, `.MAP`, `.PCH`, `.pac`, `STR`, `bse.dat` (master file).

## Three SCUS consumers

| Function | Role |
|---|---|
| `FUN_8001FA88` | **Sound subsystem init / `.dpk` loader.** Loads `bse.dat` master bank, then per-scene `.dpk` from `h:\main\bg\domepack\…`. |
| `FUN_8001FC00` | **Streaming-asset loader.** Builds paths under the `sound\` prefix; the XA / `.pac` / `STR` consumer. |
| `FUN_8001EBEC` | **Mode-aware extension dispatcher.** Reads `_DAT_8007B824` as a mode index, then uses small per-mode tables to pick which extension to hit. |

Both `FUN_8001FA88` and `FUN_8001FC00` carry a dev/retail split via `_DAT_8007B8C2`. The dev branch loads via PROT indices directly; the retail branch uses dev-style paths through `FUN_8003E6BC` (the path-based opener that resolves `h:\main\bg\domepack\…` into the appropriate PROT entry through the [CDNAME-driven name map](../formats/cdname.md)). Both paths land at the same files.

## VAB sound banks

Sony's standard `VABp`-magic instrument bank format. Documented at [`formats/vab.md`](../formats/vab.md). The dominant on-disc carrier is the [scene-VAB-prefixed streaming](../formats/scene-bundles.md) shape — the VAB body is preceded by a 4-byte chunk0 header. Implementation: `crates/vab` (header parser + extractor + ADPCM decoder).

Bulk scan finds 1191 `VABp` headers across 239 PROT entries. Multi-bank archives at `0889_sound_data2`, `0890_sound_data2`, `0891_level_up`. The `vab_01` cluster (CDNAME indices 1072–1194) is the standard distributed-bank layout.

## Per-actor sound effects

`FUN_800250D4(sound_id, voice)` is the per-actor SFX trigger called from the actor tick (`FUN_80021DF4`) when `actor[+0xb4] != 0` (one-shot pulse) or `actor[+0xac]` is staged (continuous). It looks up a sound entry at `&DAT_8006F198 + sound_id*8` for `sound_id < 0x200`, or in the runtime-allocated table at `_DAT_8007B8D0` for higher IDs (the `.dpk` consumer's bank). The entry's `byte[3] & 0x1F` is the voice count; the helper then calls `FUN_800653C8` (libSPU `SpuKeyOn`-equivalent) for each of `voice..voice+count-1`.

`actor[+0xac]` (sound ID) and `actor[+0xb0]` (voice) are written by move-VM and field-VM opcodes; the move-VM tick in `FUN_80021DF4` re-fires the SFX whenever the trigger flag at `actor[+0xb4]` is set.

## Monster sound bank — `h:\mpack\monster.snd`

Battle-time monster sound banks live in a single packed `monster.snd` file. The loader is `FUN_8003E104(monster_idx, slot, dst_buf)` — called twice from the battle scene loader `FUN_800520F0` (slots 7 and 8, for the active battle's two monster sound banks). It reads the file's per-monster TOC at `0x801C8980 - 0x10` (4-byte stride, paired entries giving `[start_lba, end_lba+1]`), computes the LBA range, and dispatches:

- **Dev path** (`_DAT_8007B8C2 != 0`) — uses the standard library file API: `FUN_800608F0` (fopen) → `FUN_80060920` (fseek to record × 0x800) → `FUN_80060944` (fread) → `FUN_80060910` (fclose). Path string: `h:\mpack\monster.snd`.
- **Retail path** — stages `(size, dst)` into the gp window at `+0x97c` / `+0x894`, kicks the async CD read via `FUN_8003F128`. Sets a 120-frame timeout at `+0x91c`.

The same pattern (`h:\mpack\…` paths + per-record TOC at a small data structure) is the shape we expect for the rest of the still-TBD audio formats — read the `FUN_8003E104` dump as the canonical example.

## BGM dispatch

The field VM's opcode `0x35` writes the BGM ID to `_DAT_8007BAC8`. `FUN_800243F0` (the per-frame asset poller) resolves it to a PROT index — `bgm_id < 2000` is scene-local, `bgm_id >= 2000` is a global pool. There's no literal BGM table; the resolution is a PROT-relative offset into the [CDNAME](../formats/cdname.md) per-scene block.

See [`subsystems/script-vm.md`](script-vm.md) → "BGM lookup table" for the resolver code.

## SsAPI sequencer (`0x80061-0x80067` cluster)

Legaia statically links Sony's PsyQ **libsnd / SsAPI** sequencer for `.SEQ`-driven music. The cluster lives in SCUS at `0x80061B18..0x800681D8` and uses the standard SsAPI globals.

### Globals

| Global | Role |
|---|---|
| `_DAT_801CD2B8` | 16-bit slot-allocation bitmap (`MAX_SEQ_SLOTS = 16`). |
| `_DAT_801CD2C0[16]` | Per-slot pointer table — each entry points at a `0xB0`-byte SsAPI sequence-state struct. |
| `_DAT_801CD2C0[i] + 0x58/0x5A` | Per-slot vol/pan, clamped `0..0x7F`. |
| `_DAT_801CD2C0[i] + 0x88` | Running tick (advanced by the varint delta-time decoder). |
| `_DAT_801CD2C0[i] + 0x98` | Per-slot status flags (bit 0 = paused, bit 1 = active/playing, bit 2 = stopped, bit 3 = end-of-sequence, bit 4/5 = volume-ramp scheduling, bit 8 = ramp lock, bit 0xA = repeat). |
| `_DAT_801CE060` | Per-voice flag bank (32 voices, bit-packed). |
| `_DAT_801CE080..AC` | Voice-attribute slots (per-voice pitch + vol working state). |
| `_DAT_801CE088[voice]` | Voice base-note table (stride 2). |
| `_DAT_801CE208` | Voice-allocation bitmap. |
| `_DAT_801CE248 / _DAT_801CE24A` | Active-voice masks. |
| `_DAT_801CE2E8` | Pitch transpose base. |
| `_DAT_801CE334` | Program region table (stride `0x10`). |
| `_DAT_801CE344` | Sequence-active voice scan target. |
| `_DAT_8007A940` | 12-entry MIDI-key pitch table (used by `FUN_80066E50`). |
| `s_Can_t_Open_Sequence_data_any_mor_80015D34` | Error string emitted by `FUN_80062340` when the slot bitmap is full. |
| `s_This_is_not_SEQ_Data_*` / `s_This_is_an_old_SEQ_Data_Format_*` | Header-validation strings emitted by `FUN_80062410`. |

### Public SEQ API

| Function | Role |
|---|---|
| `FUN_80062340(seq_data, slot_hint)` | `SsSeqOpen` — walks the slot bitmap, marks the first free slot, calls `FUN_80062410`. Returns slot ID or `-1`. |
| `FUN_80061D18(slot)` | `SsSeqClose` — calls `FUN_80067E9C(slot,0,0,1)` + `FUN_800684CC`, clears bitmap bit, memsets all 16 channel records (size `0xB0`) to defaults (vol=`0x7F`, pan=`0x7F`). |
| `FUN_80061E94(seq_id)` | `SsSeqClose` short-arg shim — sign-extends, tail-calls `FUN_80061D18`. |
| `FUN_8006275C(slot,0)` | `SsSeqPlay` — clears flags 0/3 in `+0x98`, sets bit 1. Start-from-beginning. |
| `FUN_8006282C(slot)` | `SsSeqPlay` 1-arg shim — tail-calls `FUN_8006275C(slot,0)`. |
| `FUN_80062880(slot, mode, arg)` | Pause/Resume shim — tail-calls `FUN_800628F0(slot,0,mode,arg)`. |
| `FUN_800628F0(slot,_,mode,_)` | `_SsSeqCtrl` — `mode==1` resets read pointer, sets flag `0x1`, calls `FUN_80067E9C`; `mode==0` sets flag `0x2`; otherwise clears both. The Stop / Pause / Resume state core. |
| `FUN_800641EC(slot, channel)` | `SsSeqRewind` / `SsSeqReplay` — clears flags `0x1/0x2/0x8/0x400`, sets `0x4`, full slot reset to start. |

### SEQ internals

| Function | Role |
|---|---|
| `FUN_80062410(seq_data)` | `_SsSeqInit` — validates `'S'`/`'p'` magic + version byte `0x01`, reads PPQN base (`0x393_8700` = 60 000 000), BPM, ticks-per-quarter from the SEQ header. |
| `FUN_80061C68(slot)` | `_SsSeqGetVar` — MIDI-style 7-bit-with-continuation varint decode for delta-time bytes; accumulates into `+0x88` running tick. |
| `FUN_80061EDC(slot, channel, vol, ...)` | `SsSeqSetVol` — calls `FUN_800683D8` to fetch `(vol_l, vol_r)`, clamps target ≥ requested, calls `FUN_8006206C` (slewer), sets bit `0x20`, clears bit `0x10` in `+0x98`. |
| `FUN_8006206C(...)` | `_SsSetSlideVolume` — ramp from→to over N ticks. Touches `+0x48/0x4A/0x9C/0xA0/0x4C`, signed-divide per-tick delta. Gated by flags `4 & 0x100` in `+0x98`. |

### Voice / mixer (audible-output critical path)

| Function | Role |
|---|---|
| `FUN_80067550(voice, key, vel, ...)` | `_SsVoNoteOn` — master-vol × velocity × channel vol(`+0x58`)/pan(`+0x5A`) × four expression sliders × stereo-pan square law (`uV*uV/0x3FFF`); writes `&DAT_801CE080[voice]`, sets per-voice flags `0x7`, updates active-voice masks at `_DAT_801CDB48/4A/4C/4E` and `_DAT_801CE248/24A`. |
| `FUN_80067E9C(slot, vol, pan, ...)` | `_SsSeqNoteOn` — iterates `DAT_801CE344`, calls `FUN_80068B98` (program-change?), runs the same vol/pan chain as `FUN_80067550`. Sequence-driven keyon. |
| `FUN_80065978(...)` | `_SsVoKeyOnDirect` — allocates a voice from `_DAT_801CE208`, looks up region in `_DAT_801CE334` (stride `0x10`), writes pitch + base note to `&DAT_801CE088 + voice*2`, ORs flags `0x8/0x30` into `&DAT_801CE060`. |
| `FUN_80066E50(key, fine)` | `_SsPitchFromKey` — indexes 12-entry pitch table `&DAT_8007A940`, octave-shift by `(oct-5)`. Returns 16-bit SPU PITCH register value. |
| `FUN_80065B88` | `SsResetTranspose` — single-store stub: zeros `_DAT_801CE2E8` (a base-note offset shifted in by `FUN_80065978`). |

### SPU command shims (`*0x81` scaling = 0..127 → 0..16383)

| Function | Role |
|---|---|
| `FUN_80062AA0(x, y)` | `SsSetMVol` — packs `[cmd=3, x*0x81, y*0x81]`, calls `FUN_8006BCB4` (SPU-cmd dispatcher). |
| `FUN_80065440(p1, p2)` | Single-shot SPU command (likely `SsUtKeyOn` or `SsUtPitchBend`) — `[cmd=6, p1*0x81, p2*0x81]`, calls `FUN_8006ACBC` (sister of `FUN_8006BCB4`). |

### Renderer-citation correction

The cluster appears in xrefs from per-frame draw loops near `FUN_80026410+` only because battle / field code triggers SFX cues during render passes. None of these functions is libgpu / libgs — they're all libsnd. The "renderer / GPU primitives" inventory in `docs/reference/functions.md` previously listed `FUN_80061EDC / FUN_80067E9C / FUN_80066E50 / FUN_80067550` under the renderer; they belong here.

Interpretation: `_DAT_8007BAC8 = bgm_id` written by field-VM `0x35` is consumed by `FUN_800243F0` to load a `.SEQ` payload via the [streaming-asset path](../formats/scene-bundles.md), and that payload is then handed to `FUN_80062340` for sequencer playback. Engine reimpl can stub the entire cluster behind a `legaia-engine-audio::Sequencer` trait without touching the per-note math.

## libspu / SPU control (`0x80068-0x8006D` cluster)

Sits underneath the SsAPI sequencer and drives the SPU hardware directly. PsyQ `libspu` is statically linked here — the function names below correspond to the public PsyQ API.

### SPU globals

| Global | Role |
|---|---|
| `_DAT_8007AF40` | SPU register base pointer (SPU MMIO at `0x1F801C00..0x1F801E00`). |
| `_DAT_8007AF40 + 0x180/0x182` | `MAIN_VOL_L/R`. |
| `_DAT_8007AF40 + 0x1AA` | `SPUCNT` (control register). |
| `_DAT_8007AF40 + 0x1B0/0x1B2` | `REVERB_VOL_L/R`. |
| `_DAT_8007AF40 + 0x1C0..0x1FE` | Reverb config block (APF1, COMB1-4, IIR_ALPHA, …). |
| `_DAT_8007AF68` | SPU address-shift (typically `3` — the SPU 8-byte-word scale). |
| `_DAT_8007AF6C` | SPU address-alignment granule. |
| `_DAT_8007AFA4` | Block table base. Each entry: bit `0x80000000` = free, `0x40000000` = end-of-table. |
| `_DAT_8007AFF8` | Master attribute struct — 10 modes × `0x44` bytes = `0x2A8` bytes total. |
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
| `FUN_8006A728` | `SpuFree` | Block-table free — flips matching addr's high bit (`|= 0x80000000`), calls `FUN_8006A420` (compactor). |
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
| `FUN_80069B18(mode, addr, len)` | `_spu_t` core | 4-mode SPU transfer state machine. `mode=0`: arm READ (xfer-mode bits = `0x30`); `mode=1`: arm WRITE (`0x20`); `mode=2`: stage start address into SPU `+0x1A6`; `mode=3`: COMMIT — wait for SPUCNT bits `0x30` to settle, kick the DMA channel via `_DAT_8007AF44 / +0x48 / +0x4C` (DICR + BCR + CHCR) with packet `(addr, ((len+0x3F)>>6)<<16 \| 0x10, 0x1000201/0x1000200)`, then call `FUN_8006A020` (read) or `FUN_8006A04C` (write) to flip the SPU command-register direction bits. Times out at `0xF00` poll iterations and returns `0xFFFFFFFE`. |
| `FUN_800697E0(buf, len)` | `_SpuTransfer` outer wrapper | Saves SPUCNT `+0x1AE` mask, sets transfer addr `+0x1A6 = _DAT_8007AF58`, calls `FUN_8006A078` (settle), then loops over the transfer block in `0x40`-byte chunks. Alternative path to `FUN_80069B18` for non-DMA copies. |
| `FUN_80069DA8(addr, len)` | `SpuWrite` (top-level) | Picks between the two transfer paths: if `_DAT_8007AF5C == 0` (DMA mode), drives `FUN_80069B18` mode `2 → 1 → 3`; otherwise tail-calls `FUN_800697E0` (CPU copy). |
| `FUN_8006A020` | `_spu_a` (read direction) | Sets SPU command register `*_DAT_8007AF54` bits 24..27 = `0x2` (read) by clearing the field and OR-ing `0x20000000`. |
| `FUN_8006A04C` | `_spu_a` (write direction) | Sets SPU command register bits 24..27 = `0x22` by clearing the field and OR-ing `0x22000000`. The `0x2` upper-nibble flag selects write vs read direction. |
| `FUN_8006A078` | SPU register-settling delay | 60-iteration busy-wait spin (`for (i=0; i<0x3C; i++) {}`). Inserted between command-register write and transfer kick to give SPU MMIO time to latch. |
| `FUN_8006A158` | `SsSpuMalloc` core | 712-byte block allocator. Walks the `_DAT_8007AFA4` block table, returns the start of the first free run of size `>= request`, marks header word `0x40000000` end-of-table where appropriate. Called from `FUN_80068D94` (SEP loader). |
| `FUN_8006A420` | `SpuFree` compactor | 776-byte coalescer. Iterates the block table, merges adjacent free entries (high-bit `0x80000000` set), shifts entries down to fill gaps. Called from `FUN_8006A728` (`SpuFree`). |

### SsApi seq-management layer (above libspu)

| Function | Role |
|---|---|
| `FUN_800683D8(vab, prog)` | `SsVabTransfer`-shaped — VAB program-attr lookup at `DAT_801CD2C0[vab&0xFF] + (prog>>8)*0xB0 + 0x58/0x5A`. |
| `FUN_800684CC(vab_id)` | `SsVabClose` (by VAB-ID search) — iterates `0x801CDB60 + i*0x36`, matches `+0x0`, calls `FUN_80067480(0)`. |
| `FUN_80068B98(slot, track)` | `SsSeqOpen` — bounds-checks slot + track count `_DAT_801CE332`, populates seq-state globals. |
| `FUN_80068C5C / 80068C70` | Auto-poll on/off (`_DAT_801CE330 = 1 / 0`). |
| `FUN_80068C80(slot)` | `SsSeqClose` — calls `SpuFree` on resident addr at `+0x68`, decrements `_DAT_801CE3C0`. |
| `FUN_80068D34(...)` | `SsSeqPlay` 1-shot wrapper — tail-calls `FUN_80068D94` with `mode=1`. |
| `FUN_80068D94(seq_data, mode)` | **`SsSepOpen` / SEP loader core.** 988 bytes. Validates `0x564150` ('VAP' magic), reads SEQ header `numTracks` at `+0x12`, calls `FUN_8006A158` (`SsSpuMalloc`), patches per-track pointer table, writes MIDI body to SPU. |
| `FUN_80069170(slot)` | `SsSeqPlayResolved` — final play-start stage; calls `8006BB08(0)` (xfer-mode), `8006BAB0` (commit), `8006BA50` (data feed). |
| `FUN_80069230(...)` | Streaming SEP feeder — partial-buffer continuation via `_DAT_8007AAC4/AAC8`. |
| `FUN_80069390(...)` | `SsIsEos` — tail-call to `FUN_8006BBC8`. |
| `FUN_8006CA7C` | `SsSeqGetStatus` — resolves ctx via `_DAT_801CE564`, returns ctx `+0x49` with state-code normalization (`3↔1, 2→1, 6→4`). |
| `FUN_8006CB3C(attr_id)` | `SsSeqGetAttr` — switches on `attr_id`: `1` byte@`+0xE8`, `2` u16@`+0xE6`, `3` byte@`+0xE4`, `4` u16@`+0+idx*2`/count@`+0xE3`, `100` u32@`+0x4C`. |
| `FUN_8006CDB0` | `SsSeqSetCallback` — resolves ctx via `_DAT_801CE564`, tail-calls `FUN_8006DDC8`. |
| `FUN_8006CE30` | `SsSeqSetUserData` — resolves ctx via `_DAT_801CE564`, tail-calls `FUN_8006D7B4`. |
| `FUN_8006D7B4` | `_SsSeqSetUserDataInner` — `ctx[+0x28] = p2; ctx[+0x34] = p3`. |
| `FUN_8006DDC8` | `SsSeqSetMarkCallback` — installs trampolines at ctx `+0x14/+0x18`, sets active-flag at `+0x46`. |

The runtime sequencer chain is now nearly fully mapped: slot bitmap @ `_DAT_801CD2B8` → ptr table @ `0x801CD2C0` → per-slot record (stride `0x36`) at `0x801CDB60` → VAB program-attr (stride `0xB0`) at `0x801CD2C0[i] + prog*0xB0`.

## File-API leaf cluster

The dev/retail split for sound + monster-bank loading routes the dev branch through libapi-style file primitives at `FUN_800608E0..FUN_80060A04`: `fopen` / `fseek` / `fread` / `fclose` plus a `vsync_wait` (`FUN_8005FCCC`) and a `BREAK 0x105` trap at `FUN_80060A04`. These are PsyQ kernel-call wrappers around the BIOS `A()` table — `FUN_80056738` / `FUN_80056748` / `FUN_80056768` / `FUN_80057014` / `FUN_8005ACE8` are all `jr 0xA0` BIOS dispatchers. Engine reimpl can map the entire cluster to `std::fs` + a frame-paced sleep.

## Engine-audio model — clean-room SPU port

`crates/engine-audio` ports the SPU side of the audio stack as a clean-room model. No Sony bytes; the spec is this file plus the libspu API surface and the standard PSX SPU register layout. Surface:

| Module | Maps to |
|---|---|
| [`spu::Spu`](../../crates/engine-audio/src/spu/mod.rs) | The 24-voice mixer (one [`Voice`] per slot) + master volume + a stub reverb-mode register. |
| [`spu::voice::Voice`](../../crates/engine-audio/src/spu/voice.rs) | Per-voice state: sample address, loop point, pitch, ADSR, L/R volume — the libspu `SpuSetVoiceAttr` surface. |
| [`spu::adsr`](../../crates/engine-audio/src/spu/adsr.rs) | The 5-phase ADSR envelope (Attack-Decay-Sustain-Release-Off) with linear / exponential / increase / decrease modes per the standard PSX formula. |
| [`spu::adpcm`](../../crates/engine-audio/src/spu/adpcm.rs) | Streaming SPU-ADPCM block decoder (28 samples per 16-byte block). One stateful instance per voice carries the inter-block `prev1`/`prev2` history. |
| [`spu::ram`](../../crates/engine-audio/src/spu/ram.rs) | 512 KB SPU RAM model + libspu-shaped transfer engine (`SpuRam::set_direction` / `write` / `read` + `SpuAllocator` for `SsSpuMalloc` / `SpuFree`). |
| [`vab_bind::VabBank`](../../crates/engine-audio/src/vab_bind.rs) | Bridges `legaia_vab::VabReport` into the SPU: `upload(spu, alloc, report, buf)` drops every VAG body into SPU RAM through the allocator, and `play_note(spu, voice, prog, note, velocity)` translates a MIDI key into voice config + key-on. Pitch math matches `_SsKey2Pitch` / libspu key-to-pitch. |
| [`AudioOut`](../../crates/engine-audio/src/lib.rs) | Owns a single cpal output stream that drains the `Spu` at 44.1 kHz and resamples to the host device rate (linear). Engines call `with_spu(|spu| ...)` from outside the audio thread to push voice attributes / key-on masks. |

What this **does not** model (out of scope for the first port pass):

- Reverb. The reverb register is stored, never interpreted. Spirit Arts use it; needs work before those play correctly.
- Pitch modulation, noise, FM. None of these are used by Legaia (verified against the libspu calls in the SCUS dumps — `SpuSetPitch` is the only pitch path).
- Asynchronous DMA timing. The transfer engine here is synchronous (the queue + drain are collapsed) — fine because the playback layer reads SPU RAM directly during voice ticks. The real hardware is asynchronous via the transfer engine described above; the model preserves the *API shape* (`set_transfer_start_units_8` / `set_direction` / `write`) so the libspu callers map cleanly.

## XA-ADPCM (in-progress)

`crates/xa` decodes the format spec correctly on synthetic inputs. The on-disc `.XA` files use a non-standard interleave — ~90% of groups don't pass standard validation. Likely a custom event-trigger scheme rather than streamed audio. Pinning down the actual format needs runtime tracing.

## What's left

The byte-level layouts of `.MAP / .PCH / .spk / .dpk / .pac` are still TBD. The dispatch chain *into* them is fully traced; the next move is to read the body of `FUN_8001FA88` for the `.dpk` byte layout (specifically the field accesses on `_DAT_8007B8D0` after the path-based opener returns — `_DAT_8007B8D0 + 2` is read as a `ushort` and used as a divisor, almost certainly a record count).

Eventual home: a `crates/sound` companion to `crates/vab`.
