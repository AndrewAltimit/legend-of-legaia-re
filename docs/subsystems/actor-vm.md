# Actor / sprite VM

The simplest of Legaia's five runtime VMs: a small fixed-width bytecode VM driving
the title screen's animated sprite cluster. It is a sprite-walk loop and nothing
more - there is no cross-context targeting and no subroutine call, so an actor's
bytecode only ever runs against itself.

It lives in the title-screen overlay at **`FUN_801D6628`**, with a **13-opcode**
dispatch table at `0x801CED70`. Port:
[`legaia_engine_vm`](../../crates/engine-vm/src/lib.rs) (this was the first VM
ported, and its `Host`-trait shape is the pattern the other VM ports follow).

**What catches people out: this page covers two different things.** The actor VM
proper is one of them; the other is the per-actor *anim tick* `FUN_80021DF4`, a
separate `SCUS_942.54` function documented [below](#per-actor-anim-tick---fun_80021df4).
They are related only in that both touch actor records. In particular the actor VM
does **not** consume `actor[+0x4C]` - see
[Spawn-record consumption](#spawn-record-consumption-actor0x4c-is-overloaded).

For how this VM relates to the other four, see
[the runtime VM family](move-vm.md#the-runtime-vm-family).

## Overview

The VM walks an actor list of fixed-size structs; each actor has a small amount of per-instance state and a bytecode cursor that advances over time. Opcodes are 1 byte (no operand-byte prefix), and the operand structure is per-opcode - typically zero or one byte.

## Opcodes

The 13 opcodes cover the basics every sprite-animation system needs:

- Spawn / despawn actors.
- Set / clear a per-actor flag bit (mirrors the lower script-VM banks).
- Position writes (immediate and packed).
- Motion: linear interpolation between two endpoints.
- Trigger an animation (an ANM container indexed by id).
- Wait / yield.
- Conditional skip on a flag.
- Terminator.

Full opcode table + Rust port: `crates/engine-vm/src/lib.rs`.

## Why it's separate from the field VM

The actor VM is a fixed-width 13-opcode dispatcher tailored to the title screen's sprite-walk loop. The field VM (`FUN_801DE840`) is a 43-opcode variable-length dispatcher with cross-context targeting, halt-acquire semantics, sub-dispatcher families, and far richer ctx state. They serve different layers of the engine - actors at the rendering primitive level, scripts at the gameplay-event level - and were almost certainly written by different people on the dev team.

## Connection to ANM

Opcode "trigger animation" hands off an ANM container ID to the animation runner. The container is parsed by `crates/anm`; per-record playback is driven by the per-actor anim tick described below.

## Per-actor anim tick - `FUN_80021DF4`

The per-frame anim driver lives in `SCUS_942.54`, not in an overlay. `FUN_80021DF4` is the static-binary tick the field/battle scenes call once per **game tick** for every active actor. A game tick is not a vsync - see [Tick cadence](#tick-cadence-dat_1f800393) below.

### Tick cadence (`DAT_1F800393`)

One game tick spans `DAT_1F800393` vsyncs. `FUN_80016B6C` rewrites that byte every frame from two independent inputs (see `ghidra/scripts/funcs/80016b6c.txt`):

```text
adaptive = if frameskip_enabled && worst > 0xF0 {
    if worst > 0x2D0 { 4 } else if worst > 0x1FE { 3 } else { 2 }
} else { 1 };
DAT_1F800393 = max(adaptive, DAT_8007B9D8);
```

**Adaptive frame-skip.** `FUN_800173BC` returns `VSync(1)` - the hblank (scanline) duration of the frame just rendered. `FUN_80016B6C` keeps a 16-entry ring of those samples at `DAT_80084098` and takes the running maximum, so the factor is sticky against the worst of the last 16 frames. The thresholds `0xF0 / 0x1FE / 0x2D0` sit just under 1 / 2 / 3 NTSC fields (263 hblanks each): the game advances the simulation proportionally when it misses vsync, keeping wall-clock speed constant. It is gated on a boot-time config word (`gp+0x4CE == 0x10`), read at exactly this one site. On hardware keeping up, the adaptive term is `1`.

**Per-mode floor `DAT_8007B9D8`.** This is the deterministic half, installed by mode rather than by performance:

| Installer | Floor | Mode |
|---|---|---|
| `FUN_801D6704` | 2 | Field scene loader - ordinary field / town play |
| `FUN_801CFDA0` | 3 | Field-to-battle intro transition |
| `FUN_801DC6B4` / `FUN_801DE234` / `FUN_801DD35C` | 1 | Menu family; save/restore idiom |
| `FUN_801CF678` | 1 / 4 | Baka Fighter duel / scripted beat |
| `FUN_801D362C` | script | Cutscene dialogue; operand at `param_2 + 4` |

`FUN_801D6704`'s install is `sw s0,-0x4628(v0)` at `0x801D6990`, with `li s0,0x2`
in the preceding instruction at `0x801D6988` (`overlay_0897`, base `0x801CE818`).

`FUN_801CFDA0` is more than a floor installer - it is the field-to-battle intro
particle builder (dump `overlay_field_battle_intro_801cfda0.txt`). After setting
the floor-3 cadence and (on the first frame) fading to `0x101010`, it loops
`0x488` times over a per-particle source stride of `0x2C`, building one GTE
`0x2C`-byte GPU packet per particle straight into the ordering-table cursor
`_DAT_1F8003A0`: it stamps the shared colour/geometry via `FUN_8003D1A4` /
`FUN_8003D344` / `FUN_80026988`, transforms through `FUN_8005BAC8`
(RotTransPers-class), applies `>>1` velocity nudges scaled by the tick byte
`DAT_1F800393`, and screen-clips to `X in [-8,0x148)`, `Y in [-8,0xF8)` before
linking the packet into the OT. It is a direct GTE/GPU-packet emitter, not a
draw-list builder, so it is documented rather than ported into the clean-room
render path.

Two worklist addresses in this overlay band are VA-aliased and not
independently portable. `0x801CEE80` is a field-VM interior label (jump-table
slot `[8]` of `FUN_801DE840` in `overlay_0897`) that the base-program dump
renders as a standalone gauge-fill helper reading an uninitialised `v0`; its one
`jal` caller (`FUN_80025980` mode switch) sets no arguments, confirming the alias.
`0x801D5A68` and `0x801D7B50` are real functions in the *field* overlay - the
ambient-motion direction resolver (`REF` in `engine-vm::ambient_motion`, see
[motion-vm.md](motion-vm.md)) and the sub-area window rebuild
([field-locomotion.md](field-locomotion.md)) respectively - but their
cutscene/menu-overlay dumps land mid-`FUN_801D5944` / `FUN_801D7B40`, so those
dumps are interior slices, not the owning entry.

**`FUN_801C6C78` is not an installer, despite an earlier row here saying so.**
Its own 441-instruction disassembly (`0x801C6C78..0x801C7358`) writes exactly one
global - `0x8007AA14`, via 17 copies of `sw t0,-0x55ec(at)` - and has no store to
`DAT_8007B9D8` under any addressing form, absolute or `gp`-relative. The
`_DAT_8007b9d8 = 2` that produced the row exists only in the *decompiled* half of
`overlay_0896_801c6c78.txt`, and it is `FUN_801D6704`'s own store, pulled in
because Ghidra decompiled past the function into the field-overlay bytes that
PROT 0896's footprint over-reads. The same C body cites strings (`MAP_NAME`,
`field_read_size`) and unreachable blocks (`0x801D6844`, `0x801D6854`) that all
belong to the field scene loader.

The offsets close exactly, which is what makes this diagnosable rather than a
guess. PROT 0896 dumps are printed at `0x801C5818` and PROT 0897 at `0x801CE818`,
`0x9000` apart, and 0896's bytes from file offset `0x9000` on *are* 0897's. So
for any 0896 file offset `X >= 0x9000` the printed VA `0x801C5818 + X` equals the
field overlay's true VA `0x801CE818 + (X - 0x9000)` - the over-read region prints
correct addresses by accident, which is precisely why the spliced-in code looked
legitimate. `FUN_801C6C78` itself sits at file offset `0x1460`, *below* that
seam, so its bytes are PROT 0896's own head at a base that is unrecovered
([`static-overlays.toml`](../../crates/asset/data/static-overlays.toml)) - the
label `0x801C6C78` is not a runtime VA. It also falls inside the window
[`call-target-integrity.md`](../tooling/call-target-integrity.md#scope-the-overlay_0896-window-below-0x801ce818)
marks untrustworthy, and it shows the documented signature: 18 of its 42 `jal`s
target `0x8002CDD0` and `0x8002D988`, the two non-enterable addresses that page
names. Separately, PROT 0896's head is a Japanese-build options menu the USA
build never runs (`crates/engine-core/src/options.rs`), so even a correct reading
of it would not describe a retail mode.

The menu family drops the floor to `1` on entry and writes the saved value back from `DAT_801EF19C` on exit, which is why the field floor survives a pause-menu round trip.

The consequence for ordinary field play: `DAT_8007B9D8 = 2`, so **actor motion advances every second vsync** (~30 Hz), not every vsync.

**Durations stay cadence-invariant.** Everything measuring a duration accumulates `DAT_1F800393` rather than `1` - the camera mover's `t = min(t + DAT_1F800393, d)` is the canonical case. A glide with `apply = 600` therefore arrives after 600 *vsyncs* at any cadence (600 ticks x 1, or 300 ticks x 2). Retail durations are denominated in vsyncs, so a port running at cadence 1 reaches the same endpoints at the same wall-clock moments. What changes is the **sample rate**: at cadence 2 retail emits a pose only every second vsync, so a port ticking every vsync shows intermediate poses retail never draws. That is the entire field-motion divergence - identical endpoints, double the samples between them.

`legaia_engine_vm::actor_tick::FrameCadence` models this law; `TickScalars::for_cadence` feeds it into the dispatcher multiplier.

#### Engine wiring

`World::tick` drives the pool on the same clock. It banks a vsync per retail frame and runs the per-actor passes (`tick_actor_physics` / `tick_actors` / `tick_actor_motions`) once every `World::frame_step` of them; the pass that fires carries `frame_step` as the dispatcher's `frame_delta` rather than a constant `1`.

The gate and the scalar are one change, not two. Gating alone would halve wall-clock motion; scaling alone would double it. Together they conserve vsyncs-per-second, which is what leaves every duration where it was and moves only the sample rate. `World::tick_field_npc_ambient` rides the same gate, so op `0x0D` stays in lockstep with its ramp scheduler (see [`motion-vm.md`](motion-vm.md#the-ambient-vms-own-facing-ops)).

The property is pinned directly in `crates/engine-core/src/world/tests/actor_cadence.rs`: across cadences `1..=4` the integrated displacement and the timer drain are identical while the pose count scales as `1 / cadence`. A change that moves a duration is a regression, not a reason to retune the assertion.

### Actor record fields

The tick reads three fixed offsets on the per-actor record:

| Offset | Type | Field | Notes |
|---|---|---|---|
| `+0x4C` | `u32` | `record_ptr` | Per-record byte pointer; written by `FUN_80024CFC` when a new animation is registered. |
| `+0x5A` | `u16` | `dispatch_byte` | Selects the per-opcode handler block (`0x01..=0x07`). |
| `+0x68` | `u16` | `frame_counter` | Initialised to `100` by `FUN_80024CFC`; advanced each tick by `actor[+0x6A]` (per-actor frame delta). |

The `crates/engine-vm` constants `ACTOR_RECORD_PTR_OFFSET`, `ACTOR_DISPATCH_BYTE_OFFSET`, and `ACTOR_FRAME_COUNTER_OFFSET` mirror those addresses.

### Dispatch byte values

`FUN_80021DF4` ladders through the dispatch byte (`actor[+0x5A]`) and routes to per-opcode handler blocks. Reading the comparison ladder at `0x80021E78..0x80022F04`:

| Byte | Mnemonic | Handler block | Notes |
|---|---|---|---|
| `0x01` | `Plain` | none - no `== 1` test exists anywhere in the function | Common stages only (pre-update, default movement, late-update): plain kinematics with no keyframe / path / SFX / damp / spline arm. The earlier "pose-snap variant with a to-be-found handler block" reading is retired - the comparison ladder tests `2/6`, `5`, `3`, `3\|\|5`, `7`, `4`, `6` and never `1`. `see ghidra/scripts/funcs/80021df4.txt`. |
| `0x02` | `KeyframeAlt` | shares with `0x06` at `0x80021E90..` | Per-bone keyframe-style. |
| `0x03` | `Path` | `0x800226DC..` | State-write logic shared with `0x05`. |
| `0x04` | `VramScroll` | `0x80022CBC..0x80022EE4` | VRAM texture-rect wrap-scroll: StoreImage leading band → MoveImage remainder → LoadImage re-insert on the actor's `+0xD0` rect (countdown `+0xC6`, per-axis step `+0xCC/+0xCE`). Installer: move-VM op `0x1E` (body `0x80023694`, sets `+0x5A = 4` + seven literal u16 operands); op `0x45` (body `0x8002409C`) is the dispatch-`7` sibling. The "damping / spring-decay" label was a decompiled-C-era reading. NB the extraction-0874 atlas residue is **not** this mechanism - it is a field-VM `4C 60` face-frame stamp; see [character-mesh.md](../formats/character-mesh.md#runtime-scroll-cell-residue-why-a-live-vram-dump-can-differ-from-the-tim). |
| `0x05` | `PathAlt` | `0x800228B0..0x80022B80` | Reads geometry from `actor[+0x80]` and writes pose state. |
| `0x06` | `Keyframe` | `0x80021EA0..0x80021FA4` and `0x80022F00..0x80023040` | The dominant path. Per-bone keyframe interpolation; **fully ported in [`legaia_anm::AnimPlayer`]**. |
| `0x07` | `Spline` | `0x80022C24..0x80022CC0` | Spline / curve-driven variant. |

`crates/engine-vm`'s `DispatchByte` enum exposes those values as a typed dispatch and reports `handled_natively()` for the cases the keyframe pose decoder can drive on its own (currently only `Keyframe`). The per-actor *physics* arms - the position / velocity / acceleration math common to every dispatch byte - are ported in [`crates/engine-vm/src/actor_tick.rs`](../../crates/engine-vm/src/actor_tick.rs).

### Per-arm physics tick

`FUN_80021DF4` is best understood as a layered pipeline rather than a per-opcode jump table - the dispatch byte selects which subset of side-effects fires:

| Stage | Runs for | Behaviour |
|---|---|---|
| Common pre-update | every dispatch byte | Drains the per-frame timer at `+0x54` and the rotation accumulator at `+0x22`. |
| Keyframe accel | `0x02` / `0x06` | Adds `+0xC0..+0xCA` * scalar >> 6 into the shake envelopes at `+0xB4..+0xC8`. |
| Positional SFX emitter | `0x05` | Either ramps a fade between `(+0x90, +0x92)` and `(+0x94 + +0x98, +0x96 + +0x9A)` over `+0xBC` frames, or simply integrates `+0x98 / +0x9A` into `+0x90 / +0x92`. Issues SsAPI `key-on` (`FUN_80065034`), `volume-only update` (`FUN_800657D0`), or `release` (`FUN_800250D4`) calls based on listener distance, channel authority, and the `release_pending` (`+0xB4` as i32) flag. Audio effects surface as `TickEvent::SfxUpdate` / `TickEvent::SfxRelease`. |
| Path interpolation | `0x03` | Adds `+0x96 / +0x98 / +0x9A` velocities into `+0x90 / +0x92 / +0x94`. Advances the zoom envelope at `+0x68` (clamped at `0x100`). The `+0x9C` path step counter caps at `1000` and triggers a "skip default movement" shortcut once non-zero. |
| Default movement | every dispatch byte except `0x05` | Adds `+0x80..+0x84` into `+0x24..+0x28`. Runs the trig-LUT-driven world-position update via `apply_world_rotation` (engine supplies sin / cos LUTs). Accumulates the camera-shake envelopes at `+0x72 / +0x78 / +0x7A`. |
| Common late-update | every dispatch byte | Caps the focal envelope at `0x1000`, the shake envelope at `15000`. Optionally fires the move VM kick (`FUN_800204F8`), the unlink helper (`FUN_801D79E8`), and the per-arm render: line-draws for `0x04` (`SplineDraw` / `DampDraw` events), scene-graph triangle for `0x07`. For `0x06` with a present record pointer, writes the keyframe pose (`KeyframePoseWritten` event). |

The `actor_tick` port surfaces every cross-cutting effect via the `TickEvent` enum so engines can fold them into their own audio mixer / scene graph / move-VM driver. The arithmetic mirrors the retail decompilation field-for-field; the only intentional simplifications are the use of `i64` multiply-shift in place of the MIPS `MULT` + `MFLO` pair (functionally equivalent) and the saturation-clamp helper in place of the explicit "`if (val < 0) val = 0`" / "`if (val > N) val = N`" pairs the compiler emitted.

### `+0xB4` aliases two dispatch arms

`+0xB4` (4 bytes) is read as `i32` by the SFX emitter (the "key-on done, release pending" flag) and as two `i16`s by the keyframe arms (`kf_shake[0]` and `kf_shake[1]`). The retail layout aliases these uses - the same actor record never runs the SFX emitter and the keyframe arms in the same frame, so the alias is benign. The Rust port keeps both views as named fields (`release_pending: i32`, `kf_shake: [i16; 4]`) and documents the alias in the field comments.

### Mednafen-state diff signature

Diffing the actor pool (`0x801C9594..0x801C9F7F`, 0x60-byte stride per anim slot) between a battle-intro idle save and an active-art-strike save shows the dispatch byte and the per-record pointer flipping in lockstep - the dispatch byte's lane (record `+0x0F`/`+0x10`) carries values like `0x04` (idle) and `0x06`/`0x06` (playing) across the same slot. The per-record pointer (`+0x00` of each anim slot, mirroring `actor[+0x4C]`) similarly flips between a self-reference (idle / sentinel pose) and a real RAM address that points into the scene-loaded ANM payload.

## Spawn-record consumption (`actor[+0x4C]` is overloaded)

`actor[+0x4C]` is **a multi-purpose pointer field whose semantic depends on which spawn path created the actor**, not on a per-frame dispatch lookup. Two writers + multiple readers populate it with structurally distinct payloads; the retail engine relies on disjoint actor classes for them never to collide.

### Writers

| Writer | Payload | When |
|---|---|---|
| `FUN_801D77F4` (overlay actor allocator, field-VM `0x4C 0xD8` host hook) | VDF body bytes (`[u32 record_count][record_0]...[record_n]` where each record is 12 bytes starting `[u32 group_idx]`) | Synchronous spawn of a background actor whose mesh comes from the global TMD pool. See [`docs/subsystems/script-vm.md`](script-vm.md). |
| `FUN_80024CFC` (ANM keyframe registrar) | Pose-output buffer (`[u8 bone_count][u8 ?][u16 1][...][u8 1][...] @ +0x0F: per-bone 8-byte data`) | Animation transition - bound when the engine starts a new keyframe arm. |

### Readers

| Reader | What it does with `actor[+0x4C]` |
|---|---|
| `FUN_801D77F4` itself | Walks the VDF body's record table at spawn time to compute the per-actor vertex pool malloc size and to copy per-vertex bytes out of the indexed TMD groups into `actor[+0x90]`. The body is consumed *once at spawn*; the persisted pointer is a retention reference, not actively re-read. |
| `FUN_80021DF4` case `0x06` (Keyframe arm) | Writes per-bone interpolated pose bytes into the buffer at offsets `+0x00` (count), `+0x02..+0x03` (= 1), `+0x06` (= 1), `+0x0F..` (per-bone 8-byte stride). |
| `FUN_8001BE80` (per-bone pose interpolator, GTE-side render path) | Reads `*(int *)(actor + 0x4C) + bone_idx * 8 + 8` as a second pose snapshot for per-vertex lerp between two keyframes. Indexed at 8-byte stride starting at offset 8 - matches the case-`0x06` writer's per-bone layout. |
| `FUN_800495C8` (animation envelope sampler) | Reads `*(int *)(actor + 0x4C) + 4` as a per-bone curve walker (4-byte header skip; per-record byte ranges describe interpolation envelopes). |
| `FUN_8003A1E4` (foreground actor spawner) and `FUN_801DE840` (field VM) | Both read `*(ushort *)(actor[+0x4C] + 2)` as an animation-period u16 (modulo target for the current frame index). Matches the case-`0x06` writer's `puVar15[2..3] = 1`. |

### Implications for the clean-room port

1. **The actor VM at `FUN_801D6628` is *not* a consumer of `actor[+0x4C]`.** That function is a per-frame command-list interpreter walking an *external* 4-byte-stride bytecode stream (passed in as `param_1`); it dispatches each command through a 13-entry jump table at `0x801CED70` and routes side-effects to actor records *looked up by the slot byte* (`param_1[+1]`), not by following `actor[+0x4C]`.
2. **No PC-bootstrap entry is needed.** The earlier framing - "the actor VM starts by resetting PC to 0 of the spawn record" - doesn't apply: VDF-spawned actors are driven by the vertex-pool render pipeline (`actor[+0x90]`), not by ticking their `+0x4C` body bytes as actor-VM opcodes.
3. **`Actor::spawn_record` in `legaia_engine_core` is a retention/observation slot.** Mirroring the retail `actor[+0x4C] = VDF_body_ptr` write keeps the bytes alive for diagnostic inspection but doesn't need to be fed back into any clean-room VM tick. The downstream consumer that *would* matter is the per-actor vertex-pool allocator (mirror of `FUN_801D77F4`'s second pass) - already wired in the host hook, with the "stride mystery" (12-byte first-pass cursor vs `vertex_count*8` second-pass cursor) still open.
4. **`legaia_engine_vm::actor` does *not* need an `entry_with_spawn_record` constructor.** The 13-opcode dispatcher consumes an external command list, not the VDF body. The host hook already mirrors the retail spawn-time writes; no further VM-side dispatch on the VDF body bytes happens in retail.

### VDF body header (Q2 from the actor-spawn handoff)

The memory note's "live snapshot" at `0x8011A2FC` shows what looks like a 16-byte header at the top of body 0:
```
+0x00  02 00 00 00     <- record_count = 2
+0x04  0b 00 00 00     <- record 0: group_idx = 0x0B
+0x08  00 00 00 00     <- record 0: trailing 8 bytes...
+0x0C  0f 00 00 00     <- record 0: trailing 8 bytes...
+0x10  00 00 4a 00     <- record 1: group_idx = 0x0000004A (or trailing bytes of record 0?)
+0x14  c6 ff 00 00
+0x18  04 00 0d 00
+0x1C  e5 ff 00 00
...
```

Read against `FUN_801D77F4`'s walker - `*puVar11 = record_count`, `puVar10 = puVar11 + 1` then `*puVar10 = group_idx`, advances `puVar10 += 12` bytes per record - the first u32 is the record count and the records start 4 bytes in. The "16-byte header" framing was off-by-12. The actor VM does **not** skip any metadata header before dispatch because **the actor VM never dispatches on this buffer at all** (per Implication 1 above).

## Field-spawned sprite-tick actors

Two field-overlay (PROT 0897) functions spawn and drive *attached-sprite* actors
on the shared actor list `_DAT_8007C34C` - the same list the
[field VM](script-vm.md#per-frame-scheduling) walks - rather than being actor-VM
opcode handlers themselves.

`FUN_801D25EC` is the **position-tween spawner**: given a source actor, a target
`xyz`, and a duration, it allocates an actor from template `0x801F227C`
(`func_0x80020DE0(0x801F227C, _DAT_8007C34C)`), records the source in `+0x90`,
copies the source position `+0x14/+0x16/+0x18`, stores the target in
`+0x24/+0x26/+0x28`, seeds the midpoints `+0x3C/+0x3E/+0x40`, and sets the
per-frame step `+0x9E = 0x1000 / duration` (fixed-point `1.0` over the duration).
`see ghidra/scripts/funcs/overlay_cutscene_dialogue_801d25ec.txt`.

`FUN_801E4470` is the **per-frame tick** for such an actor: it reads the parent
`+0x90`, adds the parent's world position to its own, screen-projects through the
GTE wrapper `func_0x800195A8` (using the actor's `+0x3C/+0x3E` bbox), computes the
projected midpoint + span, and draws via `FUN_801E3984` (control word `+0x74`,
`+0x88`, byte `+0x5A`). Because it calls the GTE projection and builds a draw
primitive it is render-track - documented, not ported. Its direct `overlay_0897`
dump is a truncated alias; the real 83-instruction body is in the
cutscene-dialogue field capture.
`see ghidra/scripts/funcs/overlay_cutscene_dialogue_801e4470.txt`.

## See also

**Reference** -
[Field/event VM](script-vm.md) ·
[Move-table VM](move-vm.md) ·
[Motion VM](motion-vm.md) ·
[ANM animation](../formats/anm.md) ·
[Legaia TMD](../formats/tmd.md)
