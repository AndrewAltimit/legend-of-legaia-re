# Key functions: audio

Part of the [key function directory](../functions.md) - the conventions for reading these tables (bare hex = function entry, `0x`-prefixed = data / instruction, overlay-VA caveats) are on the [index page](../functions.md#how-to-use-this-page).

## Audio

| Address | Role |
|---|---|
| `8001FA88` | Sound subsystem init / `.dpk` loader. Loads `bse.dat` master bank then per-scene `.dpk` from `h:\main\bg\domepack\…`. |
| `8001FC00` | Streaming-asset loader. Builds paths under the `sound\` prefix; XA / `.pac` / `STR` consumer. |
| `800243F0` | Per-frame BGM/asset poller. Resolves BGM IDs via the PROT-relative offset scheme. |
| `800250D4` | Per-actor SFX trigger: `(sound_id, voice)`. Looks up sound table at `&DAT_8006F198 + sound_id*8` for `sound_id < 0x200`, or runtime-allocated table at `_DAT_8007B8D0` for higher IDs. Reads voice-count from `entry[3] & 0x1F`, calls `FUN_800653C8` (libSPU `SpuKeyOn`-equivalent) for each voice. Called from per-frame actor tick when `actor[+0xb4] != 0` or `actor[+0xac]` is staged. The static table is 100 8-byte descriptors (ids `0x00..=0x63`); layout + parser at [`sfx-table.md`](../../formats/sfx-table.md) (`legaia_asset::sfx_table`). |
| `80065034` | **SFX voice-attr setup** (libsnd `SpuSetVoiceAttr` analogue). `(voice, vol, program, note, level, ...)`. Reads the libsnd "current bank" globals - `_DAT_801ce334` (`ProgAtr`, stride `0x10`, by `program`) and `_DAT_801ce340` (`VagAtr`, stride `0x20`, by `note + program*0x10`) - to program one SPU voice. The current bank is the **active scene's music VAB** (`_DAT_801ce33c` header base): confirmed per-scene (13 distinct banks across the save catalogue) and byte-identical to the disc `music_01` VAB for a `music_01`-scene state. SPU programming itself is libsnd, out of clean-room scope; the bank-source finding is on [`sfx-table.md`](../../formats/sfx-table.md). `see ghidra/scripts/funcs/80065034.txt`. |
| `80016B6C` | **Frame-end driver** (the second half of every per-frame mode handler). Three jobs in order: the SFX cue-ring **drain** (ring contract, voice-allocation split and channel-mixer gate on [`sfx-table.md`](../../formats/sfx-table.md#the-ring-is-two-arrays-aged-by-one-function-and-drained-by-another)); the **adaptive cadence** that writes `DAT_1F800393` ([`actor-vm.md`](../../subsystems/actor-vm.md)); the **frame present** - `VSync`, DISPENV/DRAWENV swap (`0x74` stride at `0x8007BF30`, `drawenv.dtd` from `_DAT_8007BA66`), `DrawOTag`. Ports: `legaia_engine_audio::sfx_ring`, `legaia_engine_vm::actor_tick::FrameStepTelemetry` + `World::resolve_frame_step`. The SPU programming and the present are libsnd/libgpu, out of clean-room scope. `see ghidra/scripts/funcs/80016b6c.txt`. |
| `8001698C` | **Frame-begin driver** (the first call of every per-frame mode handler). Syncs the libsnd auto-poll flag, mirrors the debug bit from scratchpad `0x1F800394`, advances the actor sound state (`FUN_800267FC`), re-applies the audio-context volume on a `gp+0x5F8` edge. Then either **skips the frame** - returning `1` after only a pad poll and `VSync(0)`, when `gp+0x3D8` is set and neither `_DAT_8007B938` nor `gp+0x55C` carries bit `0x800` - or publishes the double-buffer cursors (`0x1F800390` index, `0x1F8003A0` prim-pool head, `0x1F8003F4` OT base) and `ClearOTagR`s. Its last act is the SFX ring **aging** pass. Returns `0` on a normal frame. Ports: `legaia_engine_audio::sfx_ring::SfxCueRing::age`, `World::take_frame_begin_skip`. `see ghidra/scripts/funcs/8001698c.txt`. |
| `80025EEC` / `80025F2C` / `80025F74` | **The three per-frame mode handlers**, one eight-instruction skeleton with three variables: `FUN_8001698C` (abort on non-zero) -> optional overlay hook -> mid-frame driver (abort on non-zero) -> `FUN_80016B6C`. `80025EEC` is the default (12 of the 14 per-frame modes) and calls `FUN_80016444(1)`. `80025F2C` (mode 13 MAPDISP) inserts `FUN_801CE850` and calls `FUN_80016444(**0**)`. `80025F74` (mode 23 CARD) substitutes `FUN_80017978` for the master driver entirely. Ported as `legaia_engine_core::mode::per_frame_stage`. `see ghidra/scripts/funcs/80025eec.txt`, `80025f2c.txt`, `80025f74.txt`. |
| `800266E0` | **Actor sound-source teardown.** `(actor)`. Silences and detaches an actor's bound sound voice. Clears the scratch word at `gp+0x808`; then, when the actor is active (`actor[+0x8] != 0`) AND the field/dual-mode gate `_DAT_8007B868 == 0` (retail field path, same gate `FUN_80020050` reads), resets the actor's directional-pan state via `FUN_8002657C(0, actor)` and stops the voice/sequence id at `actor[+0xa]` via `FUN_80064370` (a libsnd `SsSeqRewind` wrapper, `FUN_800641EC`), then clears `DAT_8007B708 = 0`. `see ghidra/scripts/funcs/800266e0.txt`. |
| `80026520` | **Actor sound-source release.** `(actor)`. Sibling of the teardown helper `800266E0` on the same actor struct (`+0x8` active flag, `+0xa` bound sequence id). When the field/dual-mode gate `_DAT_8007B868 == 0` AND the actor is active, syncs a frame via `FUN_8005FB84(0)` (libetc VSync), clears the active flag, then rewinds and closes the bound sequence id at `actor[+0xa]` via `FUN_80064370` (`SsSeqRewind` wrapper) and `FUN_80061E94` (`SsSeqClose` shim). Fully releases the actor's sound slot, where `800266E0` only detaches it. `see ghidra/scripts/funcs/80026520.txt`. |
| `80026740` | **Actor sound-source pan-reset + voice-stop.** `(actor)`. The lightweight sibling of `800266E0`/`80026520`: when the actor is active (`actor[+0x8] != 0`) and the field/dual-mode gate `_DAT_8007B868 == 0`, resets the actor's directional pan via `FUN_8002657C(0, actor)`, stops the bound voice id `actor[+0xa]` via `FUN_8006282C`, and clears `DAT_8007B708`. Game-side glue over libsnd; detaches the voice without the full slot teardown `800266E0` performs. `see ghidra/scripts/funcs/80026740.txt`. |
| `80026478` | **Actor sound-source attach / re-pan.** `(actor)`. The activate counterpart of the teardown/release family (`800266E0` / `80026520` / `80026740`): gated on `DAT_8007B438 == 0` AND the field/dual-mode gate `_DAT_8007B868 == 0` AND the actor active (`actor[+0x8] != 0`), it reads the bound voice id `actor[+0xa]`, applies CD/reverb attrs via `FUN_80062A0C(0,0,1)`, resets directional pan via `FUN_8002657C(0, actor)`, then **sets** the voice pan via `FUN_80062880(voice, 1, 1)` and raises the active flag `DAT_8007B708 = 1` (which the teardown `800266E0` clears to 0), finally re-panning by `FUN_8002657C(_DAT_8007B910 >> 1, actor)`. Game-side glue over libsnd/libspu. `see ghidra/scripts/funcs/80026478.txt`. |
| `80026410` | **Actor sound-source bind / open.** `(actor)`. The acquire half of the teardown/release family (`800266E0` / `80026520` / `80026740` / `80026478`), on the same actor struct. Gated on `_DAT_8007B868 == 0` AND the actor being **inactive** (`actor[+0x8] == 0` - the inverse of what the teardown pair tests, making the bind a one-shot): opens a slot via `SsSeqOpen` (`FUN_80062340`) on payload pointer `actor[+0x0]` and slot hint `actor[+0xC]`, raises `actor[+0x8] = 1`, latches the handle into `actor[+0xA]`, and pokes the pan byte `actor[+0x6] = 0xFFFF` so the next `FUN_8002657C` re-applies the mix. It is the last step of the SEQ installer's `0x02` / `0x0C` opcodes (`FUN_8001E54C`) - that "finalize" is this reopen, not a VRAM write. `see ghidra/scripts/funcs/80026410.txt`. |
| `8002657C` | **Actor directional-pan apply.** `(pan, actor)`. The shared pan primitive the actor-sound family (`800266E0` / `80026740` / `80026478`) calls. When `pan` differs from the actor's stored pan byte (`actor[+0x6]`), latches the new value and re-applies the voice mix: SPU master volume + reverb via `FUN_800643C4(0,0x3FFF,0x3FFF)` and `FUN_80062A0C(0,0,1)`, then the per-voice volume/pan through `FUN_80064890` (`SsSeqSetVol`-style setter). No game logic of its own - pure libsnd/libspu coordination, so the clean-room `engine-audio` voice pool subsumes it. `see ghidra/scripts/funcs/8002657c.txt`. |
| `80018DB0` | **Field ambient/footstep audio cadence tick.** Per-frame ambient-cue ticker (runs every field-mode-`0x03` frame): counts down a ~1200-frame periodic ambient trigger (on expiry reloads it and stops/rewinds a voice via `FUN_8005C034(9,…)`), ages the counters `_DAT_8007B8A4`/`_DAT_8007B8AC`, and when the footstep-active flag `DAT_8007B79C` is set derives a step interval and arms the per-voice trigger bytes (`DAT_800915DA`/`DB`). Reads `gp+0x614`/`gp+0x618` as speed inputs and never writes them; Ghidra does not resolve `$gp` here, so those two identities stay unpinned. Drives the player's footstep + ambient SPU cues; the SPU programming itself is libspu, out of clean-room scope. Surfaced by the playthrough gap-set trace. `see ghidra/scripts/funcs/80018db0.txt`. |
| `80018F94` | **Positional-voice slot update.** Sibling of `80018DB0`: updates one entry of the `0x800915D8` footstep/positional voice table - queries SPU voice state via the libspu helpers (`FUN_8006CA7C`/`CB3C`/`CDB0`/`CE30`) and sets the footstep-active flag `DAT_8007B79C` plus the per-voice trigger byte. Game-side glue over libspu; the clean-room `engine-audio` voice pool subsumes the per-voice lifecycle. `see ghidra/scripts/funcs/80018f94.txt`. |
| `800267FC` | **Timed sound-source auto-release**, serviced by the frame-begin driver `FUN_8001698C`. Three `gp` cells: armed flag `+0x808`, deadline `+0x814`, elapsed `+0x81C`. While `deadline - elapsed >= 0` the elapsed count advances by the adaptive frame step `DAT_1F800393` - so the deadline is denominated in **vsyncs** and is cadence-invariant. On expiry the flag clears first, then - only when the record at `0x8007052C` has `+8 != 0` **and** the gate `_DAT_8007B868` is zero - the pan resets (`FUN_8002657C`) and `record[+0xA]` stops (`FUN_80064370`). Both gates suppress the teardown, not the disarm. Ported as `legaia_engine_core::sound_state::SoundReleaseTimer`; the libsnd teardown is out of clean-room scope. `see ghidra/scripts/funcs/800267fc.txt`. |
| `800267A8` | **Timed sound-source auto-release - the arm half.** `(tag, deadline)`. The installer for `800267FC`, writing the three `gp` cells the tick consumes (armed `+0x808 = 1`, deadline `+0x814`, elapsed `+0x81C = 0`) plus `+0x810 = tag` and the latched level `+0x80C = _DAT_8007B910`. It then tail-calls the libsnd wrapper `FUN_80062004(*(i16*)0x80070536, (i16)(_DAT_8007B910 >> 1), deadline \| 1)`. The tick advances `+0x81C` by `DAT_1F800393`, so the deadline is in **vsyncs**. Its one dumped caller is the field VM's op `0x35` sub-`5` (`jal` at `0x801E01B4`), passing `tag = 0` and the op's s16 operand as the deadline. `see ghidra/scripts/funcs/800267a8.txt` (decompiled C only; row read from `extracted/SCUS_942.54` at file offset `0x16FA8`). |
| `8002689C` | **One-shot sound detach.** Gated on `gp+0x804`: a non-zero value returns immediately, so however many times the mode-INIT chain calls it (`FUN_80025C68`, mode 0 CONFIG INIT) only the first has any effect. That first call latches the flag and writes the two volume pairs `FUN_80065440(0x32, 0x32)` and `FUN_80062AA0(0x7F, 0x7F)` - both arguments the same value via a `move a1,a0` in the delay slot, which the decompiled C renders as one-argument calls. No dumped writer sets `gp+0x804` back to zero. Ported as `legaia_engine_core::sound_state::SoundDetachLatch`. `see ghidra/scripts/funcs/8002689c.txt`. |
| `8001E54C` | Asset / SEQ stream installer - [details ↓](#8001e54c) |
| `8001FF58` | **SEQ resource-slot release.** `(slot_id)`. Indexes the 12-byte-stride resource table at `0x80091508` (record = `base + id*0xC`; `+0x8` = SEQ handle, `+0xB` = loaded flag); when the loaded flag is set, calls the VAB close (`FUN_80068C80`: `SpuFree` the bank's allocation, clear its open-state slot) on the handle and clears the flag. Teardown counterpart of the asset/SEQ installer `FUN_8001E54C` (which sets the loaded flag on install) and of the asset/scene-reset path `FUN_8001DCF8`. `see ghidra/scripts/funcs/8001ff58.txt`. |
| `8002614C` | **Audio-context volume re-apply on state change.** `(state_id)`. Edge-gated on the audio-state word at `gp+0x800` - acts only on a transition. On change: resets SPU master volume to max + reverb attrs via `FUN_800643C4(0,0x3FFF,0x3FFF)` (`SpuSetCommonAttr` wrapper) and `FUN_80062A0C(0,0,1)` (CD/reverb attr wrapper), then re-applies per-sequence volume for up to four live sequencer slots in the 0x40-stride table at `0x8007051C` (`+0x16` id / `+0x18` active / `+0x1A` vol) through `FUN_80064890` (`SsSeqSetVol`-style setter). Game-side glue over libsnd/libspu; sibling of the actor-sound helpers `800266E0` / `80026520`. `see ghidra/scripts/funcs/8002614c.txt`. |
| `80035B50` | **SFX-cue enqueue (4-slot ring).** `(cue_id: u16)`. Writes `cue_id` into the next slot of the 4-entry u16 ring at `&DAT_8007B6D8`, parks `gp+0x15A` (previous head) at the slot just written, advances the head `gp+0x158` (wraps at 4), zeroes the parallel timing word `&DAT_8007C338[head]`. Common cue ids: `0x20` = action-button / menu-open accept, `0x21` = picker move. In the field controller it fires only in the pre-movement header - the step loop is silent (the earlier "0x20 = field step" reading is corrected). Callers: field controller header, tile-board walker, save-screen picker, dialog pager - see [`field-locomotion.md`](../../subsystems/field-locomotion.md), [`tile-board.md`](../../subsystems/tile-board.md), [`save-screen.md`](../../subsystems/save-screen.md). |
| `80035C00` | **Staged-character selector setter.** `(a: u16, b: u16)`. Four instructions: `sh` into `gp+0x858` (`_DAT_8007BB70`) and `gp+0x860` (`_DAT_8007BB78`), then `jr ra`. Those are the pair the pause-menu notify / message-box path reads back as a character-record selector - see [`field-menu.md`](../../subsystems/field-menu.md). The immediately following routine `80035C10` is a matching clear (`gp+0x854` / `+0x864` / `+0x86C` zeroed). Which callers stage the pair is not established here. |
| `80035BAC` | **SFX-cue delay set (current slot).** `(delay: i16)`. Writes the sign-extended `delay` into `DAT_8007C338[gp+0x15A]` - the per-slot vsync countdown described on [`sfx-table.md`](../../formats/sfx-table.md), at the slot index `80035B50` parked when it enqueued. The enqueue and the overwrite `80035BD0` both **zero** that word; this is the only writer that sets it non-zero, so it is how a caller schedules a cue instead of firing it immediately. Reached from the field VM's op `0x36` bit-15-set sub-`4` (`jal` at `0x801E03D8`). `see ghidra/scripts/funcs/80035bac.txt` (decompiled C only; row read from `extracted/SCUS_942.54` at file offset `0x263AC`). |
| `80035BD0` | **SFX-cue overwrite (current slot).** `(cue_id: u16)`. Same 4-slot ring as `80035B50` but writes only at the current head - does NOT advance `gp+0x158`. Used to replace an in-flight cue; in `FUN_801d01b0` it plays the deny buzz `0x23` when a menu-open is refused under the `_DAT_1f800394 & 0x8000000` lock (the earlier "bonk overrides the pending step on a blocked move" reading is corrected - wall contact is silent). The 4-slot ring is then drained by the audio per-frame tick (consumer side not yet pinned). |
| `8003D53C` | CD-XA streaming-clip start - [details ↓](#8003d53c) |
| `8003ED04` | **CD-XA streaming-clip stop.** `(mode)`. Stop / teardown counterpart of the clip-start `FUN_8003D53C`. Calls `FUN_8003EE7C(0)`, resets the play-state window (`gp+0x908 = 0`, `gp+0x910 = 0`, `gp+0x974 = 1000000`), and when a clip is armed (`gp+0x928 != 0`) clears the libcd completion callback (`FUN_8005BECC(0)`) then issues the drive command by `mode`: `mode == 0` → `FUN_8005C160(9,0,0)` + `FUN_8003F2B8(0)` (pause), else → `FUN_8005C034(9,0)` + `FUN_8003DE7C(0)` (stop), finally clearing the armed flag `gp+0x928`. Like the start path, the CD-drive half is out of clean-room scope (the engine plays decoded XA buffers through the mixer, not a CD transport). `see ghidra/scripts/funcs/8003ed04.txt`. |
| `8003EAE4` | **CD-XA streaming-clip start (by table index).** `(unused, clip_index)`. The simpler sibling of `FUN_8003D53C`: when no clip is armed (`gp+0x928 == 0`) it stops any in-flight read (`FUN_8003DE7C` if reading, then `FUN_8003ED04` / `FUN_8003EE7C`), looks up the 8-byte XA-clip-table entry at `0x801C6ED8 + clip_index*8` (skips when the `+0x4` length word is 0), sets the CD read location via `FUN_8005C160(2, entry, 0x8007BC10)` (logging `pos Set loc err` on failure), issues the read with `FUN_8005C034(0x15, entry)`, and marks the play state (`gp+0x908 = 1`, `gp+0x890 = clip_index`, `gp+0x910 = 1`). Like `FUN_8003D53C`, the CD-drive half is out of clean-room scope (the engine plays decoded XA through its mixer). `see ghidra/scripts/funcs/8003eae4.txt`. |
| `8004FCC8` | **Menu cue dispatch + voice trigger.** `(id)`. For `id < 0x100` enqueues a UI cue into the 4-entry ring at `&DAT_8007B6D8` (write index at `*(gp+0xA0C)+9`, wraps at 4): `id < 0x40` stores `id-1`, `0x40..0x100` stores `id`, both skipping the currently-selected cue `DAT_8007B724`. For `id >= 0x100` (gated on `*(gp+0xA0C)+0x276 == 0`) triggers a streamed voice via `FUN_8003D53C`: slot `= (id-0x100)>>3` (remapped 1→0x1A, 3→0x1B, 5→0x1C), sub-mode `id & 7`, pitch `= (DAT_800788B8[idx]*0x3C + 99)/100`. Dispatch decode ported as `legaia_engine_audio::classify_cue` (→ `CueDispatch::Ring`/`Voice`) + `voice_pitch`; the ring is `SfxScheduler` (`FUN_80035B50`), the voice gates + note-on stay with the caller. `see ghidra/scripts/funcs/8004fcc8.txt`. |
| `801E1AB0` | **Move-FX 2D afterimage / streak draw.** `(trail_id)`. Emits one semi-transparent textured quad (`POLY_FT4`, cmd `0x2e`, colour `0x808080`) per call: a billboard at the move actor's screen point (`+0x120` px down, `0x100` half-size, projected by GTE helper `FUN_800195a8`), jittered per corner via BIOS `rand` (`FUN_80056798`) - X `-2 + r%5`, top corners Y `-8 + r%9`, bottom `-4 + r%9` - with a brightness band `(r&3)<<5` picking a `0x20`-wide texture sub-column (UVs `band\|0x1f`/`band`, V `0..0x3f`), texpage `0x27`, CLUT `0x7700 + trail_id`. Packet assembly ported as `legaia_engine_render::afterimage::build_afterimage_quad` (injected rng, unit-tested); projection + OT link stay caller-side. `see ghidra/scripts/funcs/overlay_battle_action_801e1ab0.txt`. |
| `801E1D98` | **Move-FX 2D chained streak ribbon.** `(actor_point, trail_id)`. The sibling of `801E1AB0` in the same move-FX draw dispatcher (`0x801E0CA0` calls the single quad, `0x801E0CD0` this) over the same move-power `+0x0b` trail id. Projects one billboard through `FUN_800195A8`, then emits a chain of `POLY_FT4`s whose every segment reuses the previous segment's top edge as its own bottom edge, climbing until the baseline leaves the screen. Draw law, jitter magnitudes and packet fields: [`subsystems/battle.md` § Move-FX streak ribbon](../../subsystems/battle.md#move-fx-streak-ribbon-fun_801e1d98). Ported as `legaia_engine_render::afterimage::build_streak_ribbon`. `see ghidra/scripts/funcs/overlay_battle_action_801e1d98.txt`. |
| `801D3380` | **Casino slot-machine payline draw.** Emits the five paylines as `LINE_F2` prims (GP0 `0x43` - flat, semi-transparent) from the 5 x 16-byte geometry table `DAT_801d3680`, each endpoint projected on its own. Idle colour `0x808080`; the line whose index equals `DAT_801d3c8c` has only its three colour bytes overwritten with `(0xFF, 0xFF, 0x80)`, so a lit line keeps the `0x43` code. Ported as `legaia_engine_core::slot_machine::payline_prims`. [`subsystems/minigame-slot-machine.md`](../../subsystems/minigame-slot-machine.md#the-paylines-are-3d-lines---fun_801d3380). `see ghidra/scripts/funcs/overlay_slot_machine_801d3380.txt`. |
| `801E2524` | **Battle screen-flash ramp.** Per-frame pass over two battle-context bytes: the stage `ctx[+0x28B]` and the brightness level `ctx[+0x28C]`. Stage `0` is idle, `1..=4` emits four layers through `FUN_801E2650` and walks the level by `DAT_1F800393 << 3` (saturating at `0xF0`), `5..=8` **clears the stage and draws nothing** (the cancel band), `>= 9` returns without clearing. The first three layers are gated on the level being below `0xF0` / `0xE0` / `0xD0`, so they drop out as the flash brightens; the fourth is ungated and the only opaque one. Every layer passes `stage - 1` as the emitter's position selector. Ported as `legaia_engine_vm::battle_action::step_flash_ramp`. `see ghidra/scripts/funcs/overlay_battle_action_801e2524.txt`. |
| `801E2650` | **Battle screen-flash quad emitter.** The `FUN_801E2524` layer draw: `(offset, percent, semi, position)`. Scales `percent` to `0..=0xFF` (`v * 256 / 100`, clamped) and replicates it into RGB, builds the GP0 code as `(semi << 1) \| 0x2C` (so `0x2C` opaque / `0x2E` semi-transparent), and emits **two** `POLY_FT4`s per call whose vertical extent is derived from `ctx[+0x28C] - offset`. `position` selects among four X placements via a 4-way switch. **Not ported** - `FUN_801E2524`'s port hands the layers back as a list so this can land behind it. `see ghidra/scripts/funcs/overlay_battle_action_801e2650.txt`. |
| `8003E104` | Monster-sound bank loader: `(monster_idx, slot, dst_buf)`. Reads `h:\mpack\monster.snd` for the given monster. Bank index based at `0x801C8980`: entry count at `0x801C8984`, offsets array at `0x801C8988` (4-byte stride, monster `i` spans `[tbl[i], tbl[i+1])`). Those are **sector offsets relative to the bank base**, not absolute LBAs - the base is PROT entry `0x37D`'s start LBA (`*0x801C7EEC` → `gp+0x8F0`) plus the LBA of the MSF at `0x8007BC50`. The gate is `beq` at `0x8003E1FC`: the dev path (`_DAT_8007B8C2 == 0`) uses `FUN_800608F0`/`_920`/`_944`/`_910` (host trap / fseek / fread / fclose); the retail path (`!= 0`) stages `gp+0x97c` / `gp+0x894` and kicks `FUN_8003F128` (async CD read). Called twice from the battle scene loader `FUN_800520F0` (slots 7 and 8). |
| `80062340` | `SsSeqOpen` --allocates a sequencer slot from the 16-slot bitmap at `_DAT_801CD2B8`; emits `s_Can_t_Open_Sequence_data_any_mor_80015D34` on full. See [`subsystems/audio.md`](../../subsystems/audio.md) → "SsAPI sequencer". |
| `80061D18` | `SsSeqClose` - clears bitmap bit, memsets all 16 channel records (`0xB0` each) to defaults. |
| `8006275C` / `8006282C` | -SsSeqPlay` (ramped + 1-arg shim). |
| `800628F0` | `_SsSeqCtrl` --Stop / Pause / Resume internal. |
| `800641EC` | `SsSeqRewind`-- full slot reset to start of sequence. |
| `80062410` | `_SsSeqInit` - -EQ-header parser (`'Sp'` magic + version `0x01`). |
| `80061C68` | `_SsSeqGetVar` - MIDI-style varint delta-time decode. |
| `80061EDC` / `80067E9C` | `SsSeqSetVol` (per-channel + slot -ol/pan). Clamps `0..0x7F`. |
| `80066E50` / `80067550` |-`_SsPitchFromKey` + `_SsVoNoteOn` - note→pitch table at `_DAT_8007A940` + master×velocity×channel-vol×stereo-pan voice mixer. |
| `80062AA0` | `SsSetMVol` - packs `[cmd=3, x-0x81, y*0x81]`, calls `FUN_8006BCB4`. |
| `80068D94` | `SsVabOpenHead` core - validates `pBAV` magic, registers the bank's per-vab table pointers, builds the **program-number → packed-tone-page rank map** into the ProgAtr `+8` reserved words, allocates SPU memory, stashes per-VAG SPU addresses in ProgAtr `+0xC/+0xE`. Consumed by the program-change `FUN_80068B98`. (Earlier `SsSepOpen`/'VAP'-loader reading corrected from the disassembly.) Engine port: `engine-audio::VabBank::upload`. See [`subsystems/audio.md`](../../subsystems/audio.md#ssapi-seq-management-layer-above-libspu). |
| `80069B18` / `800697E0` / `80069DA8` | SPU transfer-engine. `_DA8` = top-level `SpuWrite` (picks DMA vs CPU copy on `_DAT_8007AF5C`); `_B18` = 4-mode DMA state machine (arm-read / arm-write / set-addr / commit); `_7E0` = CPU-copy alternative. See [`subsystems/audio.md`](../../subsystems/audio.md) → "SPU DMA transfer engine". |
| `8006A020` / `8006A04C` | `_spu_a` direction flips - set SPU command register bits `0x20000000` (read) / `0x22000000` (write). |
| `8006A078` | SPU register-s-ttling delay (60-iter busy-wait). |
| `8006A158` | `SsSpuMalloc` - bloc--table first-fit allocator over `_DAT_8007AFA4`. |
| `8006A420` | `SpuFree` -ompactor - coalesces adjacent free entries, shifts table down. |
| `8006A728` | `SpuFree` - block-tabl- free in `_DAT_8007AFA4`. |
| `8006BC9C` | `SpuIsTransferPaused` - `return _DAT_8007AF74 != 1`. |
| `8006ACBC` / `8006C048` | `SpuSetVoic-Attr` (mask dispatcher + 24-voice broadcaster). |
| `8006B1B4` | `SpuSetReverbModePa-am` - 30-attr reverb commit, writes regs `0x1C0..0x1FE`. |
| `8006BCB4` | `SpuSetCommonAt-r` - master vol L/R + reverb regs + SPUCNT bits. |
| `8006C6E4` | `_SsKey2Pitch` - `((key1*0x80+fine1) - (key2*0x80+fine2)) / 0x600` expon-ntial build. Returns 14-bit SPU PITCH. |
| `80026234` | SsAPI SEQ message-handler table install - writes 18 SEQ event-handler pointers (the `0x8006xxxx` status-byte handler cluster) into the dispatch table at `0x801D2220`. `see ghidra/scripts/funcs/80026234.txt`. |
| `8002666C` | Audio subsystem one-shot init (gated on `gp+0x818`) - master volume (`FUN_80062AA0`), SPU voice attrs (`FUN_800654D8` / `FUN_800655AC`), CD/reverb attrs (`FUN_80062A0C`), and the SEQ handler-table install `FUN_80026234`. `see ghidra/scripts/funcs/8002666c.txt`. |
| `8001D230` | Audio IRQ/event setup - opens then enables 8 SPU/DMA interrupt events (`0xF4000001` / `0xF0000011` classes; handles at `gp+0x6D8..0x6F8`) via the BIOS `OpenEvent`/`EnableEvent` thunks, after the SPU init steps `FUN_8006EE8C` / `FUN_8006EEE0`. Torn down by `FUN_8002035C`. `see ghidra/scripts/funcs/8001d230.txt`. |
| `80062AF0` (+ `80062D58` = start thunk) | SsAPI tick-timer install/start - selects a root-counter rate from the mode global `DAT_8007A908` (0/2/3/5, else a computed target `0x204CC0` or `0x409980 / rate`), programs the RCnt via `FUN_80062E28`, and installs the VSync/RCnt callback `FUN_8005FDB8`; the sequencer clock source. `see ghidra/scripts/funcs/80062af0.txt`. |
| `80062164` | SsAPI tick-timer stop - critical-section-guarded teardown of the RCnt callback that `FUN_80062AF0` installs. `see ghidra/scripts/funcs/80062164.txt`. |
| `80062E28` | Root-counter target program - `(counter 0..2, target, flags)`. Writes the RCnt target into the 0x10-stride table at `DAT_8007A924[counter]` and builds the RCnt mode word (`0x48` / `0x49` / `0x248` plus IRQ/repeat bits `0x10` / `0x100` from the flags). `see ghidra/scripts/funcs/80062e28.txt`. |
| `80062F60` | Root-counter target clear - zeroes `DAT_8007A924[counter]` for counter 0..2. `see ghidra/scripts/funcs/80062f60.txt`. |
| `80062D98` / `80062DD8` | RCnt callback trampolines - `_D98` invokes the two installed timer callbacks (`DAT_8007A914` then `PTR_FUN_8007A910`); `_DD8` is a one-shot reentry-guarded (`DAT_8007A91C`) invoke of `PTR_FUN_8007A910`. `see ghidra/scripts/funcs/80062d98.txt`. |
| `80064698` | SsAPI tick-period selector - resolves the sequencer tick count (`50` / `60` / `120` / `240`, stored at `0x801CD2BC`) from a mode via the jump table at `0x80015D94`. `see ghidra/scripts/funcs/80064698.txt`. |
| `8006AAE0` (+ `8006558C` off / `800655AC` on) | `SpuSetReverb` - toggles the SPU reverb-enable bit `0x80` in SPUCNT (`+0x1AA` of the SPU state block); `(0)` off, `(1)` on. `see ghidra/scripts/funcs/8006aae0.txt`. |
| `800654D8` | SPU reverb-type/depth setter - clamps the signed type to `\|t\|<10` (the ten reverb modes), records it at `0x801CE250`, disables reverb (`FUN_8006AAE0(0)`) for type 0, and commits via `FUN_8006ACBC`. `see ghidra/scripts/funcs/800654d8.txt`. |
| `8006688C` | SEQ voice match-and-release - scans the active-voice table (0x2D-byte stride, count `0x801D1CBC`) for a record matching the 4-field key `(a0,a1,a2,a3)` with tag `0xFF` and releases it via `FUN_80067428`. `see ghidra/scripts/funcs/8006688c.txt`. |
| `80067428` | SEQ voice-slot release - clears the note record (0x2D-byte stride) for voice `a0` and issues the SPU key-off `FUN_8006A7A4`. `see ghidra/scripts/funcs/80067428.txt`. |
| `80067C1C` | SEQ note-on dispatch - applies the program (`FUN_80068B98`) then triggers the note on each active voice via `FUN_80067A1C`, returning the count started. `see ghidra/scripts/funcs/80067c1c.txt`. |
| `80067D0C` | SEQ per-program attribute set - selects the program (`FUN_80068B98`), then writes `a2` into `+0x1` of the ProgAtr record `*0x801D...[prog*0x10]`; returns it. `see ghidra/scripts/funcs/80067d0c.txt`. |
| `80068568` | Active-note enumerator - walks the sounding-voice table, filters by the per-program key range (`+6..+7` of `*0x801D...[prog*0x20]`), and appends `(note, voice)` pairs to the caller's two buffers; returns the count. `see ghidra/scripts/funcs/80068568.txt`. |
| `800694D0` | SPU DMA-IRQ event setup (one-shot, `gp+0x50C4`-gated) - `FUN_8006A0E0` config, then opens+enables the SPU interrupt event (`0xF0000009`) via the BIOS thunks, latching the handle at `0x8008FAD4`. `see ghidra/scripts/funcs/800694d0.txt`. |
| `8006A0E0` | SPU DMA-callback register - `DMACallback(4, handler)` via `FUN_8005FDE8`. `see ghidra/scripts/funcs/8006a0e0.txt`. |
| `8006A104` | SPU DMA channel program - stages the DMA4 transfer descriptor (`madr = 0x40001010`, block control from a shift) when `mode > 0`; part of the SPU transfer engine. `see ghidra/scripts/funcs/8006a104.txt`. |
| `8006D358` | SPU DMA transfer kick - programs the SPU transfer-control registers (mode `0x1003` / `0x3003`, SPUCNT via `FUN_8006ED34`) and starts the block transfer to SPU RAM. `see ghidra/scripts/funcs/8006d358.txt`. |
| `8006D470` | SPU transfer state-machine pump - calls the current state handler from the 5-entry table at `0x8008B2E8`, advances/wraps the state index, and fires an error callback on a negative return. `see ghidra/scripts/funcs/8006d470.txt`. |
| `8006D768` | SPU transfer-ready spin - polls SPU status (`+0x4`) bit `0x2`. `see ghidra/scripts/funcs/8006d768.txt`. |
| `8006D794` | BIOS C0-vector 0x02 thunk (`li t2,0xC0; jr t2; li t1,0x2`); sibling of the C0 0x03 thunk `FUN_8006D7A4`. |
| `8006ED34` | SPU-transfer timeout latch - stores the pending SPU command `a0` and the current Timer2 value (`0x1F801120`) for the DMA-completion timeout check. `see ghidra/scripts/funcs/8006ed34.txt`. |
| `8006E8D4` | SEQ-stream callback install - stores the per-track tick handler `FUN_8006E8F8` and the ready/done probe `FUN_8006ECFC` into the transfer vtable at `0x801D1A5C` / `0x801D1A74`. `see ghidra/scripts/funcs/8006e8d4.txt`. |
| `8006E8F8` | SEQ streamed-track tick driver - dispatches on the track phase `+0x46` (`FUN_8006E06C` start/stop, the record's `+0x14` handler, or the block-advance `FUN_8006D7D0`). `see ghidra/scripts/funcs/8006e8f8.txt`. |
| `8006ECFC` | SEQ streamed-track done-probe - returns 1 when the track is idle (`+0xE6 == 0`) or finished (`+0x46 == 0xFF`). `see ghidra/scripts/funcs/8006ecfc.txt`. |
| `8006E06C` / `8006E08C` / `8006E0C0` / `8006E0E0` / `8006E100` | SEQ transfer state setters - each stamps a target SPU-command byte into track `+0x36` (`_06C`=0x43, `_08C`=0x45, `_0C0`=0x46, `_0E0`=0x47, `_100`=0x4B) and arms/clears the inline arg at `+0x24`. `see ghidra/scripts/funcs/8006e06c.txt`. |
| `8006D854` / `8006D7D0` / `8006D9A0` / `8006D9D8` | Streamed-audio block iterator family - `_854` is the phase machine on record `+0x46` that parses the streamed-resource header and walks its blocks; `_7D0` sub-dispatches on the phase; `_9A0` computes the current block size/offset; `_9D8` arms a block-transfer descriptor. `see ghidra/scripts/funcs/8006d854.txt`. |
| `8006EE8C` / `8006EEE0` | SPU init/teardown steps - critical-section-guarded BIOS B-vector calls (`FUN_8006EF48` = B0 0x4A, `FUN_8006EF58` = B0 0x4B, plus `FUN_8006EFD0` / `FUN_8006F088`) around `FUN_8005FD68(0)`. `see ghidra/scripts/funcs/8006ee8c.txt`. |
| `8006EFD0` | SPU voice-state install - BIOS B0-vector 0x56, then copies a 5-dword template (`0x8006EF78`) into the returned block `+0x28` and `FlushCache`; sibling of the `FUN_8006EF18` init trio. `see ghidra/scripts/funcs/8006efd0.txt`. |
| `_DAT_801CE564` / `_DAT_801CE574` (data) | Legaia-installed seq-context vfn pointers - `_564` resolves the active script-VM seq context, `_574` is a worker-availability check. Used by `FUN_8006CA7C / CB3C / CDB0 / CE30 / DDC8`. |

## SsAPI per-frame calc tier

`FUN_80062F98` is the sequencer's per-frame top - `SsSeqCalc` in PsyQ terms - and
everything below it is reached by one bit of the per-channel flag word. The
channel record is `_DAT_801CD2C0[slot] + channel * 0xB0`; the slot count is the
`i16` at `0x801CDB40`, the channel count the `i16` at `0x801CDB42`. Rows here are
read from the instruction stream, so the field offsets and the bit → handler map
are checkable against the dumps rather than against a C rendering.

| Address | Role |
|---|---|
| `80062F98` | **Per-frame sequencer top.** Re-entrancy-latched on `0x801CD2B4` (set on entry, cleared on the way out, early return when already set); calls the voice flush `FUN_80065BAC` once, then walks every `(slot, channel)` whose slot bit is set in the bitmap `0x801CD2B8` and dispatches on the channel's flag word `+0x98`. Bit `0x4` additionally **zeroes the whole flag word** after its handler runs. `see ghidra/scripts/funcs/80062f98.txt`. |
| `+0x98` bit map (data) | `0x1` → `FUN_80063974`; `0x10` → `FUN_8006320C`; `0x20` → `FUN_8006352C`; `0x40` **and** `0x80` → `FUN_800649B0`; `0x2` → `FUN_800638D8`; `0x8` → `FUN_8006418C`; `0x4` → `FUN_800641EC` (`SsSeqRewind`) + flag-word clear. The `0x10`/`0x20`/`0x40`/`0x80` tests are **nested inside** the bit-`0x1` arm, so a stopped channel runs no slide. |
| `80063974` | `short`-argument shim: sign-extends both arguments and tail-calls `FUN_800639A0`. The bit-`0x1` entry. `see ghidra/scripts/funcs/80063974.txt`. |
| `800639A0` | **Delta-time pump.** While the pending wait `+0x90` fits inside this frame's tick budget `+0x54`, executes the next event via `FUN_80063CEC` and accumulates; the leftover is written back to `+0x90`. When the wait does not fit, the sign of `+0x52` picks the clocking mode - see [details ↓](#800639a0). `see ghidra/scripts/funcs/800639a0.txt`. |
| `80063CEC` | **SEQ event decoder.** Post-increments the stream cursor `+0x0`, latches the running-status byte at `+0x16` and its low nibble at `+0x17`, and dispatches the high nibble through five installed handler pointers - see [details ↓](#80063cec). `see ghidra/scripts/funcs/80063cec.txt`. |
| `80063AA8` | **Track-end / loop-repeat handler.** Bumps the repeat counter `+0x21` against the target `+0x20` (`0` = loop forever) and rewinds the cursor to `+0xC` or `+0x4` (selected by flag `0x400`), zeroing `+0x88` / `+0x1C` / `+0x90`. On the last repeat it clears flags `0x1`/`0x2`/`0x8`, sets `0x200` + `0x4`, clears `+0x14`, kills the channel's notes via `FUN_800684CC(slot \| channel << 8)`, reloads `+0x90 = +0x54`, and - when `+0x22 != 0xFF` - starts the chained `(slot, channel)` at `+0x22` / `+0x23` through `FUN_80064090`. Its third argument is dead: the prologue overwrites `a2` with the channel byte offset. `see ghidra/scripts/funcs/80063aa8.txt`. |
| `80064090` | **Channel restart from the top.** `(slot, channel)`. Sets `+0x20 = 1` / `+0x21 = 0`, clears flags `0x100`/`0x8`/`0x2`/`0x4`/`0x200`, rewinds the cursor `+0x0 = +0x4`, sets `+0x14 = 1` and raises the play bit `0x1`. The chain target of `FUN_80063AA8`. `see ghidra/scripts/funcs/80064090.txt`. |
| `8006418C` | Sets the channel's `+0x14` byte to `1` and clears flag `0x8`. Four-word leaf, no callees. `see ghidra/scripts/funcs/8006418c.txt`. |
| `800638D8` | Kills the channel's sounding notes (`FUN_800684CC(slot \| channel << 8)`), clears `+0x14` and flag `0x2`. The inverse of `8006418C`. `see ghidra/scripts/funcs/800638d8.txt`. |
| `8006320C` / `8006352C` | **The two volume-slide ticks** - ascending (flag `0x10`, saturates at `0x7F,0x7F`) and descending (flag `0x20`, saturates at `0,0`). Same field set and same structure; see [details ↓](#8006320c--8006352c). `see ghidra/scripts/funcs/8006320c.txt`, `8006352c.txt`. |
| `800649B0` | **Tempo-slide tick** (flags `0x40` + `0x80`, both dispatched here). Steps the tempo `+0x94` toward the target `+0xAC` by `+0x4E` while the countdown `+0xA8` lasts, then recomputes the per-frame tick step - see [details ↓](#800649b0). `see ghidra/scripts/funcs/800649b0.txt`. |
| `800648F0` | **Per-channel volume set.** `(slot, channel, vol_l, vol_r)`. Commits straight through `FUN_80067E9C(packed, l, r, 1)` when the flag word is exactly `1`; otherwise only stages `+0x58` / `+0x5A`, which the note-on mixer folds in later. `see ghidra/scripts/funcs/800648f0.txt`. |
| `8006497C` | Packing shim over the channel-volume getter `FUN_800683D8` - builds `slot \| channel << 8` and forwards the two out-pointers unchanged. `see ghidra/scripts/funcs/8006497c.txt`. |

## Function details

Full write-ups for the rows above whose detail outgrew a table cell. Linked from each section table by **[details ↓]**.

### `8001E54C`

**Asset / SEQ stream installer.** `(packed_slot, record_array, budget)`. The opcode-driven asset-upload interpreter: walks a packed command array (each record carries a `u24` size + `u8` op at byte `+3`, advancing `size>>2` words), dispatching per op - `0x00` SEQ-slot release (`FUN_8001FF58`) + raw blit; `0x01` VAB/asset transfer (`FUN_8002630C`); `0x02`/`0x0C` actor-sound teardown+release (`FUN_800266E0`/`FUN_80026520`) then blit + finalize (`FUN_80026410`); `0x03` chunked `FUN_8002630C` transfer with a partial-budget split (returns the leftover); `0x04` raises the in-progress flag `gp+0x700`. Per-slot SEQ state lives in the 12-byte-stride table at `0x80091508` (the loaded flag at `+0xB` that `FUN_8001FF58` clears on release). Gated on the field/dual-mode flag `_DAT_8007B868`.

The install counterpart of the SEQ-slot release `FUN_8001FF58`; reused by the flame-effect loader `FUN_80020050`. `see ghidra/scripts/funcs/8001e54c.txt`.

### `8003D53C`

**CD-XA streaming-clip start.** `(clip_id, mode, duration_sectors)`. Starts a streamed XA-ADPCM clip (voice / streamed SFX). `clip_id` indexes the 8-byte XA-clip table at `0x801C6ED8` (`+0x0` = 6-byte BCD-MSF start address, `+0x4` = length/valid word; a zero `+0x4` is an empty slot → debug-log + abort via `FUN_8003EE00`). Stops any in-flight clip (`FUN_8003ED04` / `FUN_8003DE7C`), copies the MSF into the active-clip scratch at `0x8007BBF0`, resolves the start LBA via `msf_to_lba` (`FUN_8005C42C`), clamps `duration_sectors` to `0x2A30` and derives the end LBA at `gp+0x974` (`start + (dur*0x96 + 0x95)/0x3c`), records `mode` at `gp+0x954` and the playing state at `gp+0x908 = 2`, then arms the CD read via `FUN_8005BE8C` / `FUN_8005BECC` / `FUN_8005C034`.

`clip_id == 0x13 && mode == 2` takes a `+0x10`-LBA variant. `see ghidra/scripts/funcs/8003d53c.txt`.

### `800639A0`

**Delta-time pump.** `(slot, channel)`. `+0x54` is the tick budget for one frame
and `+0x90` the ticks still owed before the next event.

When `+0x90 - +0x54 <= 0` the frame can reach the next event: the function calls
`FUN_80063CEC` in a loop, re-reading `+0x90` after each call, and keeps going
while the running total stays below `+0x54` - so a run of zero-delta events all
fire in the same frame. The final `+0x90` is the accumulated total minus `+0x54`,
i.e. the debt carried into the next frame.

When the wait does not fit, the sign of `+0x52` selects between two clocking
modes. Negative: `+0x90 -= +0x54`, the ordinary "advance by this frame's budget"
step. Non-negative: `+0x52` is a countdown - positive values only decrement it,
and on reaching zero it reloads to `+0x54` and `+0x90` drops by **1**. That is a
divider for the case where a whole tick should take several frames rather than a
whole frame taking several ticks.

### `80063CEC`

**SEQ event decoder.** `(slot, channel) -> status`. Reads one byte through the
post-incrementing cursor at `+0x0`. With flags `0x401` both set it first tests
the cursor against the loop-end pointer `+0x10`; a hit calls `FUN_80063AA8` and
returns `-1`.

A byte with bit `0x80` set is a status byte: its low nibble is stored at `+0x17`,
the high nibble at `+0x16`, and the high nibble picks the arm. A byte without
bit `0x80` re-enters the same arms through the retained `+0x16`, so the format
keeps MIDI running status.

| High nibble | Data bytes consumed | Handler pointer |
|---|---|---|
| `0x90` | note + velocity, then a varint delay via `FUN_80061C68` into `+0x90` | `*0x801CD220` |
| `0xB0` | one | `*0x801CD230` |
| `0xC0` | one | `*0x801CD224` |
| `0xE0` | one, and the value is **not** passed in a register | `*0x801CD228` |
| `0xF0` / `0xFF` | one; the value `0x2F` instead ends the track via `FUN_80063AA8` | `*0x801CD22C` |

The return value is `1` only on the `0x2F` track-end path, `0` otherwise, and the
loop-end path returns `-1`; `FUN_800639A0` ignores it and keys off `+0x90`.

### `8006320C` / `8006352C`

**The two volume-slide ticks.** Both read the same channel fields - `+0xA0`
remaining ticks, `+0x9C` total ticks, `+0x4C` signed per-tick step, `+0x4A`
remaining level steps, `+0x48` the requested span - and both close by caching the
channel's live L/R volume into `+0x5C` / `+0x5E` through the getter
`FUN_800683D8`. `+0xA0` going negative clears the handler's own flag bit and ends
the slide.

They differ only in direction and endpoint. `8006320C` (flag `0x10`) walks the
volume **up** and, when a step would overshoot, commits `(0x7F, 0x7F)` through
`FUN_80067E9C` and clears flag `0x10`. `8006352C` (flag `0x20`) walks it **down**
and saturates at `(0, 0)`, clearing flag `0x20`.

Within each, the sign of `+0x4C` picks a granularity. Positive is the fine arm:
`+0xA0 % +0x4C` gates the step so the level moves one unit every `+0x4C` ticks -
this is the one `div` / `mfhi` pair in either function. Negative is the coarse
arm: the level moves `|+0x4C|` units every tick, guarded by
`(+0x9C - +0xA0) * |+0x4C| < +0x48` so the accumulated change cannot run past the
requested span.

The installer is `FUN_8006206C` (`_SsSetSlideVolume`), which writes exactly this
field set; `FUN_80061EDC` (`SsSeqSetVol`) is what arms it.

### `800649B0`

**Tempo-slide tick.** `(slot, channel)`. Flags `0x40` and `0x80` both dispatch
here and both are cleared when the ramp finishes, which is why the caller has two
bits pointing at one handler.

`+0xA8` is the remaining tick count (decremented per call; going negative clears
both flags immediately), `+0x4E` the signed per-tick step, `+0x94` the current
tempo and `+0xAC` the target. As in the volume slides, a positive `+0x4E` gates
on `+0xA8 % +0x4E` and moves the tempo by one, while a negative one moves it by
`|+0x4E|` per tick; both directions clamp at the target, and the comparisons are
**unsigned** (`sltu`), so the tempo field is a `u32`.

Every call then re-derives the pump's frame budget:

```
+0x54 = (+0x50 * +0x94 * 10) / (*(i32 *)0x801CD2BC * 60)      // floored, clamped to >= 1
```

`+0x50` is the resolution the SEQ header parser `FUN_80062410` installs and
`0x801CD2BC` is the sequencer tick rate (`50` / `60` / `120` / `240`) that
`FUN_80064698` selects. So this is the single place where retail's wall-clock
tempo becomes the integer tick step `FUN_800639A0` spends each frame - the retail
counterpart of `engine-audio`'s integer-sample sequencer clock. The ramp ends
when `+0xA8` reaches zero or `+0x94` equals `+0xAC`.
