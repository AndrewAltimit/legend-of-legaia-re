# Key functions: game modes, input + title

Part of the [key function directory](../functions.md) - the conventions for reading these tables (bare hex = function entry, `0x`-prefixed = data / instruction, overlay-VA caveats) are on the [index page](../functions.md#how-to-use-this-page).

## Input + debug subsystem

| Address | Role |
|---|---|
| `8001822C` | **Per-frame pad pump** / debug dispatcher. Builds the packed held mask `_DAT_8007B850` from the libpad reports: digital `0x4x` decode `~(b2<<8\|b3)` (port 1 into the high half), DualShock `0x7x` stick fold-ins (right stick onto face buttons, left onto dpad; deadzone `<0x30`/`>0xD0`), then Left+Right / Up+Down SOCD-cancel. Retail (`_DAT_8007B98C == 0`) truncates to port 0's 16 bits and skips every debug binding. Edge words + the 32-vsync held ring / 8-vsync auto-repeat: see the memory-map debug-flags table. Ported as `legaia_engine_core::retail_pad::RetailPadState::pump`. `see ghidra/scripts/funcs/8001822c.txt`. |
| `80016230` | Dev-print driver. Loads `program_no=%d` / `..\..\FIELD\PROGRAM\....\%d` strings only when debug enable is non-zero. |
| `8001AA68` | Fixed-cell debug string drawer - [details ↓](#8001aa68) |
| `8001CE34` | **Dev 3-D line emitter.** `(x, y, z, dx, dy, dz)`. Projects both endpoints through the RTPS wrapper `FUN_8003D368`, writes a 3-word GP0 line packet (cmd = `DAT_8007B714` colour + `0x40000000`) at the scratchpad OT cursor `_DAT_1F8003A0` and links it (`FUN_8003D2C4`) into OT bucket `_DAT_1F8003F4 + 8`. All twelve static callers are `FUN_8001CAD8` - it is not a shared boot utility, despite the in-degree. No caller outside the dev-draw cluster in the captured corpus. `see ghidra/scripts/funcs/8001ce34.txt`. |
| `8001CAD8` | **Dev wireframe-box drawer.** `(x, y, z, dx, dy, dz)`. Emits the 12 edges of the axis-aligned box spanned by the corner + extents via 12 `FUN_8001CE34` calls. Debug visualisation (bounding boxes / trigger volumes); zero callers across SCUS + the captured overlays. `see ghidra/scripts/funcs/8001cad8.txt`. |
| `8001CCFC` | **Dev 2-D line emitter.** `(x0, y0, x1, y1, cmd_color, ot_bucket)`. Same 3-word GP0 line packet as `FUN_8001CE34` but takes pre-projected screen coords, the full command/colour word and the OT bucket index directly. `see ghidra/scripts/funcs/8001ccfc.txt`. |
| `801F1F4C` | **Debug-gated actor state pick** (field overlay 0897, file `+0x23734`). `(actor)`. Both arms stash `-1` into the scene block `*_DAT_801C6EA4 + 0x2E` and the actor's outgoing `actor[+0x50]` into `+0x40`; the new state is `0x13` when the tile-board install global `_DAT_8007B450` is clear **and** the master debug gate `_DAT_8007B98C` is set **and** the held pad mask `_DAT_8007B850` carries bit `0x100`, otherwise `0x30`. Then `actor[+0x54] = 0` and `jr ra`. Retail leaves `_DAT_8007B98C` at 0, so a shipped disc only takes the `0x30` arm. A leaf - no stack frame - which is why frame-shaped entry tests miss it; `0x801F1FC8` and the second `jr ra` at `0x801F1FD4` are inside this body. |
| `8001C7A0` | **Dev tiny-digit number drawer.** `(value, digit_count, x, y)`. Renders `value` right-to-left as `digit_count` 4x8 semi-transparent textured sprites (GP0 `0x66`-family; UV `u = digit*4 - 0x58`, `v = 0xF8`, CLUT `0x7F83`, colour from `DAT_8007B7A4`), plus a minus glyph (`u = 0xD0`) for negatives. The 4x8 sibling of the debug-HUD digit drawer `FUN_8001ABC8`; zero callers in the captured corpus. `see ghidra/scripts/funcs/8001c7a0.txt`. |

## Move / animation subsystem

| Address | Role |
|---|---|
| `800204F8` | Move-buffer consumer. Sole reader of both `_DAT_8007B888` (MOVE) and `_DAT_8007B840` (MOVE2). Resolves `move_id` to a buffer record and stages it onto the actor - does **not** run opcodes itself; that's `FUN_80023070`. |
| `80020740` | **Morph-weight ramp envelope.** Move-buffer pre-tick helper, called from `FUN_800204F8` when actor flag bit `0x1000` is set. Ramps each lane's weight at `+0xA0 + lane*2` up to `0x1000` and back down, cascading lane by lane, with the completion bitfield at `+0x7C`. The up / down velocities at `+0xB8 + lane*2` / `+0xC8 + lane*2` are read **per lane** (`0xb8(a1)` with `a1 = actor + lane*2`), authored one triple per lane by move-VM op `0x0A`. Ported as `legaia_engine_vm::move_buffer::envelope_tick`. `see ghidra/scripts/funcs/80020740.txt`. |
| `80023070` | **Move-table opcode interpreter.** 71 opcodes (`0x00..0x46`); JT at `0x80010778`. Walks the per-actor move buffer at `actor[+0x48]` indexed by PC at `actor[+0x70]` (u16 units). Opcode `0x2F` escapes to `FUN_801D362C`. See [`subsystems/move-vm.md`](../../subsystems/move-vm.md). |
| `8003774C` | **Per-actor walk kernel: interprets the field-VM yield-class ops in place** from the pointer the dispatcher parks at `actor[+0x94]` (`0x37/0x41/0x47`, `0x38` with nonzero duration), resolving the same `0x80` extended-target convention. `0x37/0x41` = glide-step (axis `_DAT_80073F14[b0 & 7]`, base step `4 << ((b0>>5 & 4)\|(b1>>6))`, distance `(b1 & 0x3F) * base`), `0x38` = bearing ramp toward `0x80073F04[b0 & 0xF]`, `0x47` = XZ approach at base step `4 << (b2 & 7)`, mode nibble `b2 >> 4`, `0x4C` = line-of-sight. Reads `_DAT_1F800393` (dt); cursor `actor[+0x54]`; drives `+0x14/+0x18/+0x26`. Dispatched per frame by `FUN_8003BC08` on flag `0x400` (the HALT bit the yield ops set). No separate motion-bytecode stream exists - the record bytes are the stream. |
| `80021934` | **Scene-transition streaming actor** (5-state SM over `actor+0x1A`, jump table `0x80010760`; real entry 3 insns before the `0x80021940` prologue, in never-analyzed space). Spawned on demand by `FUN_8001FD44` via `FUN_80020DE0` from the spawn descriptor at `0x80070734` (of the phase-misaligned family `0x800705FC..0x80070763`, NOT a mode-table row). Streams the destination scene's `DATA\FIELD\<scene>.LZS` bundle (raw `scene_base+3`) into `_DAT_8007B85C`, then hands off with `_DAT_8007B83C = 2` (MAIN INIT). Closes the `_DAT_8007B85C` staging question in [`asset-loader.md`](../../subsystems/asset-loader.md). [details ↓](#80021934). Port [`engine-core::scene_transition_actor`](../../../crates/engine-core/src/scene_transition_actor.rs). |
| `80021B04` | Actor-spawn helper. Builds per-actor OBJECT pointer table at `actor[0x44]+4`. Calls `FUN_80023070` once at spawn to run the initialisation opcodes in the move buffer. |
| `80050ED4` | **Summon / effect-actor pool allocator.** Scans the 0x60-slot pointer pool at `DAT_801C90F0`; on the first null slot calls `FUN_80021B04` (the actor-spawn helper above), stores the new actor pointer into the slot, and returns it (returns `0` when the pool is full). The alternate spawn path the effect VM takes for the "summon" effect kind instead of the generic spawn helper. Cited from `crates/engine-vm/src/effect_vm.rs` (`func_0x80050ed4` summon handler). `see ghidra/scripts/funcs/80050ed4.txt`. |
| `800252EC` | **Per-scene prescript stager installer.** `(id, a, b)` resolves a move-VM stager record from the `scene_event_scripts` / `scene_v12_table` prescript at `_DAT_8007b8d0` - `record = _DAT_8007b8d0 + (offsets[id] & ~1)` (the `[u16 count][u16 offsets]` table) - and calls `FUN_80021B04(a, b, record, 0x1000)` to spawn a move-VM actor on it. Called by the field VM `FUN_801DE840`. The sibling `FUN_8001FA88` runs the same `[count][offsets]`-at-`_DAT_8007b8d0` read for the `bse.dat` sound bank (the other tenant of that slot). See [`formats/scene-bundles.md`](../../formats/scene-bundles.md#scene_event_scripts---prescript-only). `see ghidra/scripts/funcs/800252ec.txt`. |
| `80021DF4` | Per-frame actor tick. Updates `actor[+0x54]` (wait timer), `+0x22` (rotation), state-2/5/6 animation slots; then calls `FUN_80023070` to step the move VM. |
| `8002174C` | **Morph-weight apply pass.** `(actor)`. The `+0x8` handler word of spawn descriptor `0x8007068C`. Two passes over the block at `actor[+0x4C]` (`[u32 count][slots]`), against the `0x1C`-stride TMD object table at `actor[+0x48] + 0xC`: pass one restores each named group's rest pose from the shared `actor[+0x90]` stream, pass two applies that record's deltas at the live weight `actor[+0x6E]` through the blend kernel `FUN_8005B038`. The tail is a **ping-pong** weight ramp between `0` and `0x1000`. Sibling of the stager `FUN_8001C604`. Full write-up: [details ↓](#8002174c). Port [`engine-core::morph_weight_apply`](../../../crates/engine-core/src/morph_weight_apply.rs). `see ghidra/scripts/funcs/8002174c.txt`. |
| `8002149C` | **Camera-relative glide actor tick** - the per-frame partner of the spawner `FUN_80021248`, and the `+0x8` handler word of the descriptor `0x8007071C` that spawner allocates from. A leaf with no stack frame, which is why Ghidra never carved it out (force-disassembled by `ghidra/scripts/dump_scus_gaps.py`). Walks the ten camera globals onto the ten `(step, target)` pairs the spawner normalized into `actor+0x80`, then retires. Full write-up: [details ↓](#8002149c). Port [`engine-core::camera_rel_glide`](../../../crates/engine-core/src/camera_rel_glide.rs). `see ghidra/scripts/funcs/8002149c.txt`. |
| `801D362C` | Move-VM overlay extension dispatcher. 61 sub-opcodes (`0x00..0x3C`); JT at `0x801CE868` (PROT 0897 file `+0x50`). Reached only via move-VM opcode `0x2F`, a fixed-VA call - and the dispatcher exists **only in the field overlay 0897**: the world-map / dialog / cutscene capture dumps are byte-identical to the 0897 copy (0897-hosted modes, not separate overlays), while every other mapped slot-A overlay carries unrelated bytes at this VA. The "per-overlay copies with own JT contents" reading is falsified - see [`move-vm-overlay-ext.md`](../../subsystems/move-vm-overlay-ext.md#overlay-residency---one-copy-in-the-field-overlay-only). Sub-handlers include `0x801D31B0` (per-scanline POLY_FT4 strip emitter), `0x801D32F8`, `0x801D3444`, `0x801D3748`, `0x801D52D0`. |
| `8002519c` | Per-frame actor-list iterator (328 bytes). Walks a linked-list head, dispatching each node by `jalr node[+0xC]`. Five lists at `_DAT_8007C34C..._DAT_8007C36C` are iterated per frame from `FUN_80016444` (one call per render pass). Per node: `+0x00` = next ptr, `+0x0C` = tick fn ptr, `+0x10` = flags (bit `0x8` selects early-return path, bit `0x200` is the "already-emitted" guard), `+0x44` = optional prim-chain head to free. Known `+0xC` handlers: `FUN_8003BC08` (field actors) and the battle anim-node tick `FUN_80047430` (live-pinned: the `jalr` at `0x800252B4` is its only dispatch site in a mid-battle capture). |
| `8003BC08` | **Per-actor tick for the `_DAT_8007C354` list** (field static-object / NPC actors; one of the `+0xC` tick fns `FUN_8002519c` dispatches). The unifying per-frame driver: per actor it calls the field-overlay draw helper `FUN_801D79E8` (live actors, `+0x5C >= 0`), frame-smooths facing `+0x16` toward `FUN_80019278(actor)`, then dispatches by flags `+0x10` to the inline-dialogue SM `FUN_80039B7C` (`0x100`), motion VM `FUN_8003774C` (`0x400`), `FUN_80038158` (`+0x80`), and the move-table VM (`+0x5C > 0` / `0x1000`). Live-confirmed as the hottest field-overlay caller (`FUN_801D79E8` ~420x/frame). `see ghidra/scripts/funcs/8003bc08.txt`. |
| `80038158` | **Per-actor motion / bytecode VM** (second motion VM; dispatched by `FUN_8003BC08` when actor `+0x10 & 0x80`), stream at `actor+0x80` + PC `+0x84`. Op-`7` **sets** / op-`8` **clears** the story-flag bank `DAT_80085758` (flag = `operand[1] \| operand[2] << 8`). Carrier = **MAN tail-section 1** (installer `FUN_8003A9D4`, parser `legaia_asset::man_motion`); full layout + op-width table in [`motion-vm.md`](../../subsystems/motion-vm.md#the-second-motion-vm---fun_80038158). Disc-wide census `man-scripts --motion-flag-census`: no spine gate appears in any motion stream - `0x142`/`0x482`/`0x1BE` are field-VM script bytes in the streaming variant MAN carriers ([script-vm.md](../../subsystems/script-vm.md)); `549` remains a direct code path. `see ghidra/scripts/funcs/80038158.txt`. |

## Game-mode state machine

The 28 × 24-byte table at `0x8007078C` is detailed in [`subsystems/boot.md` § Game-mode state machine](../../subsystems/boot.md#game-mode-state-machine). The full index → handler/param/name map is recovered from the disc by [`legaia_asset::mode_table`](../../../crates/asset/src/mode_table.rs) (`asset mode-table SCUS_942.54`; disc-gated `mode_table_real`).

| Address | Role |
|---|---|
| `0x8007078C` (data) | Mode table - 28 entries × 24 bytes. `+0x00` = name string ptr; `+0x10` = handler fn ptr; `+0x14` = parameter. |
| `gp[0x524]` (data) | Current-mode register (i16). |
| `_DAT_8007B83C` (data) | Master game-mode index, u16. Title overlay writes `0x1A` (= STR FMV mode 26) on attract countdown underflow; FMV id slot at `_DAT_8007BA78` is zeroed in the same block → `MV1.STR`. |
| `80015E90` | **`main()` - cold-boot init + master mode loop.** Called once from the entry stub `FUN_80026C28`. Runs the subsystem-init sequence (GTE/GPU/CD/SPU/libsnd, heap, DISPENV, mode table) then loops dispatching the current mode's handler from the `0x8007078C` table until the mode index goes negative. Full walk-through: [`subsystems/boot.md` § The main loop](../../subsystems/boot.md#the-main-loop-fun_80015e90). `see ghidra/scripts/funcs/80015e90.txt`. |
| `800179C0` | **Debug mode-advance chord** - the only `SCUS_942.54` site that writes `_DAT_8007B83C` from a mode-table row's `next` field. Gated on `_DAT_8007B98C != 0`, so retail never enters the body. Decrements the hold-repeat countdown `_DAT_8007B890` and returns while it is non-zero; on expiry tests `_DAT_8007B850` against `0x900` (when `_DAT_8007B868 == 0`) or `0x100`, the latter also accepting `pad & 0xF == 0xF`. A trigger reads the `i16` at `+0xA` of the 24-byte row `0x8007078C + mode*24`, treats negative as "no transition", and writes it back - except from mode 3 with `_DAT_8007B8C8` set, which jumps to mode `0x0E`. That read is emitted twice, the second copy correct only because the delay slot at `0x80017A60` reloads the table base. |
| `80017978` | **Mode 23 (CARD) frame body**, the `FUN_80016444` substitute named by `FUN_80025F74`. Three calls: the debug mode-advance `FUN_800179C0`, an indirect call through the CARD actor's tick handler `(*_DAT_8007B8E0)[+0x0C]`, and the dev readout HUD `FUN_800188C8(_DAT_1F800393)`. Two of the three are debug-gated, so the actor tick is the whole retail body - CARD runs **no** actor/render passes and no display flip through the master driver. `_DAT_8007B8E0` is an actor, not a mode-table row: `FUN_80020DE0(&DAT_800706D4, _DAT_8007C34C)` stores it at `0x800257AC`. The body ends `move v0, zero`, so the caller's abort test can never fire. Ported as `legaia_engine_core::mode::CARD_FRAME_BODY`. `see ghidra/scripts/funcs/80017978.txt`. |
| `8001DAF8` | **Display-environment + GTE screen-setup.** `(width_selector)`. Builds the two double-buffered DISPENV/DRAWENV structs at `0x8007BF30` / `0x8007BFA4` (via the libgpu env fillers `FUN_8005731C` / `FUN_8005724C`) and the framebuffer-rect globals at `0x1F800388`. `0x400` selects the 640x480 (`0x280`x`0x1E0`) wide mode; any other value (retail passes `0x140` = 320) selects the default-width / `0xE0`-height mode. Stores the active width at `0x8007B810`, then primes the GTE projection via `FUN_8005B818` (`SetGeomScreen`, H=0x78) and `FUN_8005B7F8` (`SetGeomOffset`, OFX=width/2). Called from the mode-init handlers (`FUN_8002574C`, `FUN_80025FB4`, `FUN_80055B6C`) and the field initializer `FUN_801D6704`. `see ghidra/scripts/funcs/8001daf8.txt`. |
| `8001E3B8` | **Primitive-packet + ordering-table allocator.** `(packet_size)`. Allocates the GPU primitive-packet buffer (base/end at `0x8007B728` / `0x8007B72C`, mirrored to `0x8007B908` / `0x8007B90C`) via the malloc wrapper `FUN_80017888` (`size << 1` bytes; `size == 0` borrows the static region `0x8007B85C`); a dev flag at `gp+0x704` swaps in fixed buffer addresses. Then calls `FUN_8001F690` to allocate the ordering table (`1 << depth` entries, depth from `DAT_1F8003A5`, default 10) into the same display-env struct at `0x8007BF30 + 0x70` / `+0xE4`. When `_DAT_8007B83C == 0x14` it allocates an extra `0x1000`-byte buffer at `0x8007B814`. Paired with `FUN_8001DAF8` at every game-mode display init. `see ghidra/scripts/funcs/8001e3b8.txt`. |
| `80025EEC` | Default per-frame mode handler - used by **12 of the 14** odd-indexed (per-frame) modes; the exceptions are Mode 13 (MAPDISP MODE, `0x80025F2C`) and Mode 23 (CARD MODE, `0x80025F74`), which carry their own. Pipeline: `FUN_8001698C → FUN_80016444(1) → FUN_80016B6C`. (Disc-confirmed by `legaia_asset::mode_table`.) |
| `80025C68` | Mode 0 (CONFIG INIT) handler - **loads PROT 971, the dev debug-menu overlay** ("DEBUG MODE" / FOG / MAP NAME / TMD NO strings) via `FUN_8003EBE4(0x4C)`. Despite the dev name "CONFIG", this is the debug-menu mode, not a game-config init. (The earlier "PROT 973 slot-machine debug" reading was the loader-math off-by-2 plus 973's over-read tail carrying the slot overlay's image; the casino slot machine is PROT 975, mode-24 warp sub-id 3.) Runs the sound detach `FUN_8002689C` + the shared state reset `FUN_80025CB4` first, then hands into the loaded overlay at `FUN_801CE8EC`; stage plan mirrored at `legaia_engine_core::mode::mode_init_stage`. |
| `80025B64` | Mode 2 (MAIN INIT) handler - **field/town gameplay INIT.** Loads the field overlay via `FUN_8003EBE4(2)` (PROT 897, the static-overlay-map field entry) then calls the per-scene initializer `FUN_801D6704`, which hands off to mode 3 (field per-frame). The title screen's NEW GAME path launches this mode (`_DAT_8007B83C = 2` at `0x801DFC00`). Despite the dev name "MAIN" / older "options menu" notes, this is the field entry; the options strings live in the menu overlay PROT 899, loaded by the mode-22 CARD pair (`FUN_8002574C` → `FUN_8003EBE4(4)`). |
| `80034A6C` | **New-game data-init.** Establishes the fresh-game world state: writes party gold `_DAT_8008459C = 500` (hardcoded), zeroes a ~`0x200`-byte story-flag region, sets assorted party-default globals, and calls `FUN_800560B4` to expand the starting-party stat template. Does **not** set the opening scene, prompt for a name, or trigger the opening cutscene. Reached via the boot mode initializer `FUN_8001DCF8` (and `FUN_8001FFA4`). Engine mirror: `World::begin_new_game` + `NEW_GAME_STARTING_GOLD`. Dump has no disassembly, so widths are confirmed against `SCUS_942.54` directly: `sw` of `0x1F4` at `SC+0x45C`, 512-byte `sb` clear from `SC+0x1618`. |
| `800560B4` | **Starting-party stat seed.** Expands the static `SCUS_942.54` starting-party template at `0x80078C4C` (`[8×u16 stats][10-byte name]`, stride `0x1A`, 4 records Vahn/Noa/Gala/Terra) into the live per-character records (stride `0x414`), copying stats + the template's **default name** (via `FUN_80056758`). Called by `FUN_80034A6C`. Parser: [`legaia_asset::new_game`](../../formats/new-game-table.md); engine mirror `World::seed_starting_party`. |
| `80025DA0` | Mode 12 (MAPDSIP INIT - disc spelling) handler - the **world-map display** init, *not* field/town. (The earlier "field/town init / the actual gameplay-mode entry" label was wrong: field/town gameplay is modes 2/3 MAIN, pinned by saves to `game_mode 0x03`. MAPDISP 12/13 is the map-display mode whose per-frame handler routes the world-map render tick - see [`world-map.md`](../../subsystems/world-map.md#per-frame-dispatch-scus-resident).) |
| `80025F2C` | Mode 13 (MAPDISP MODE) handler - world-map display per-frame; routes the world-map render tick. |
| `80025E68` | Mode 8 (EFECT TEST INIT) handler - effect-bundle test mode. |
| `8002611C` | Mode 4 (MONSTER TEST INIT) handler. |
| `8002612C` | Mode 16 (READ INIT) handler. |
| `80025B30` | Mode 18 (GAME OVER INIT) handler: `FUN_8003EBE4(7)` + read-wait, then the loaded overlay's init `FUN_801CE844`. Retail-unreachable (no static writer of mode 18). Stage plan mirrored at `legaia_engine_core::mode::mode_init_stage`. |
| `80025980` | Mode 24 (OTHER INIT) handler - the `0x3E` minigame door-warp stager (full decode: [`script-vm.md § 0x3E warp`](../../subsystems/script-vm.md#0x3e-warp-mode-24-minigame-door-warp)). Staging plan (`overlay param = 0x4D + sel`, `+2` first when `sel > 5`; per-sub-id entry table at `0x80010AE4`) ported as `legaia_engine_core::mode::other_warp_init_stage`. |
| `8001C604` | **VDF morph stager.** `(actor, group_idx)`. Copies the TMD group's rest-pose vertices into the scratch window at the top of the `_DAT_8007B85C` asset buffer (`+0x62C00 - count*8`), retargets the group's vertex pointer, then for each actor morph slot (`+0xB0` VDF sub-entry index / `+0xA0` u16 weight, count `+0x6C`) walks the sub-entry's records (`[u32 group][u32 dst_index][u32 count][count x 8-byte deltas]` via the `0x80083E58` pointer table) applying matching records through the `FUN_8005B038` blend. Facial / mesh morph ("set_mime") staging. Ported as `legaia_engine_vm::vdf_morph` (`stage_group_morph`). `see ghidra/scripts/funcs/8001c604.txt`. |
| `80025CB4` | **Shared mode-INIT core state reset** (called by `FUN_80025C68` after the scene-name sync `FUN_8001D7F8`). Store list: brightness `DAT_8007B718 = 0x80`, GTE `H` `DAT_8007B6F4 = 0xA0`, field warm-entry `_DAT_8007B8B8 = 0`, `DAT_8007B648 = 0`, game mode `_DAT_8007B83C = 1`, pad edge `_DAT_8007B874 = 0`, `B830`/`B8C8 = 0` (the recomp shows `B8C8` stored twice - a benign duplicate the Ghidra C folds), DATA_FIELD staged-index `DAT_8007B768 = 0xFFFF`, `B6FC`/`B6C8`/`B9C4 = 0`, scratch mirrors `0x1F80037D/91/93` reloaded, retail leg (`_DAT_8007B98C == 0`) `_DAT_8007BA36 = 1` + `DAT_8007B71C = 1`, `_DAT_8007B900 = -1`. Ported as `legaia_engine_core::mode::CORE_STATE_RESET`. `see ghidra/scripts/funcs/80025cb4.txt`. |
| `800565D8` | Mode 20 (BATTLE INIT) handler - battle-scene setup entry. |
| `8002574C` | Mode 22 (CARD INIT) handler - memory-card save/load mode entry (calls the screen-setup `FUN_8001DAF8`). |
| `80025F74` | Mode 23 (CARD MODE) handler - memory-card per-frame (one of the two non-shared per-frame handlers). |
| `80024190` | **In-field save/load screen driver** - the actor tick that carries a field session through the overlay swap into the memory-card UI and back. 11-state SM over `actor[+0x1A]`, jump table `0x80010898`. Spawn descriptor `0x800706BC` (handler word at `0x800706C4`), spawned by the field/world fade SM `FUN_801EE5D4`; **not** the mode-23 CARD actor, whose descriptor `0x800706D4` names the overlay handler `0x801E36A0`. Save vs load is `actor[+0x5C]`. Full state walk: [details ↓](#80024190). Port [`engine-core::field_save_screen_actor`](../../../crates/engine-core/src/field_save_screen_actor.rs). `see ghidra/scripts/funcs/80024190.txt`. |
| `80025980` | Mode 24 (OTHER INIT) handler - the **minigame door-warp entry** reached by field-VM op `0x3E` (`op0 >= 100`; sub-id `_DAT_8007BA34 = op0 - 100`). Backs up the active scene name (`memcpy(0x8007BAE8, 0x80084548, 8)`) and `_DAT_80084540` (→ `gp+0x7ac` = `0x8007BAC4`), loads the per-sub-id minigame overlay via `FUN_8003EBE4(sub_id + 0x4D)` (`+2` first when `sub_id >= 6`; extraction PROT 972..977, 980), calls its init entry (switch on the sub-id), sets mode 0x19. Exit via `FUN_80026018`. Full sub-id table in [`subsystems/script-vm.md`](../../subsystems/script-vm.md#0x3e-warp-mode-24-minigame-door-warp). (The old "loads PROT 896" note is refuted - see [`subsystems/boot.md`](../../subsystems/boot.md).) `see ghidra/scripts/funcs/80025980.txt`. |
| `80025FB4` | Mode 26 (STR INIT) handler - cutscene / STR FMV mode entry. This is the mode the title-overlay attract-loop underflow falls through to (`_DAT_8007B83C = 0x1A`). |
| `8001DCF8` | Boot-time mode initializer. 1212-byte function. NOT the per-frame dispatcher. |
| `80017714` | **Subsystem reset + diagnostic dump** (Ghidra label `main_mode_dispatch`). Resets the graphics + sound stack in order - `ResetGraph` (`FUN_80057C44`), `DrawSync`, `VSync`, `PutDrawEnv`, XA-clip stop (`FUN_8003ED04`), SPU teardown - then tears down the two sound sources at `0x8007052C` and `+0x40` (`FUN_800266E0` + `FUN_80026520` each), and formats a diagnostic record at `0x8008B354` through the `sprintf`-shaped `FUN_80056728`, printed via `FUN_800567A8`. A mode-teardown / diagnostic path; the Ghidra name does not reflect a per-frame dispatch loop (that is `FUN_80015E90`). `see ghidra/scripts/funcs/80017714.txt`. |

## Title overlay

| Address | Role |
|---|---|
| `FUN_801DD35C` (**title overlay**, 12 104 bytes / 3 026 instructions) | Per-frame title-overlay tick. Pinned via PCSX-Redux watchpoint on the attract countdown - the BP captured `pc=0x801DDCCC` on the `sw` that writes the decremented value back. Decrements `_DAT_801EF16C` by the per-frame scalar at `_DAT_1F800393`; `bgez` branches to `0x801DFC3C` while still counting; underflow falls through and writes `_DAT_8007B83C = 0x1A` (= STR FMV mode 26). Capture pipeline: `scripts/pcsx-redux/autorun_countdown_trigger.lua`; dump at `ghidra/scripts/funcs/overlay_title_801ddccc.txt`. |
| `0x801DDCCC` (instruction) | The `sw v0, -0xe94(a0)` that writes the decremented countdown back. Acts as the watchpoint-pinning anchor for `FUN_801DD35C`. |
| `0x801DFC3C` (branch target) | Normal per-frame attract loop (rendering, input, cursor logic). Reached via `bgez v0` from inside `FUN_801DD35C` when the countdown is still positive. Not yet dumped. |
| `FUN_8005DA40` | **Not a real function** - `0x8005DA40` is an instruction (`lui v1, 0x8008`) inside `FUN_8005D9A0` (the CD-DMA-channel-3 read primitive). Ghidra promotes the intra-function label to a fake `FUN_8005DA40` entry. Earlier notes claimed this function "walks `_DAT_800795B4` and stamps `0x8000` into BSS"; that's wrong. The title state struct (including the `0x8000` countdown initial value) is populated by DMA from disc bytes, not by code. See [`subsystems/boot.md` § Title-overlay state struct](../../subsystems/boot.md#title-screen-overlay-state). |

## Function details

Full write-ups for the rows above whose detail outgrew a table cell. Linked from each section table by **[details ↓]**.

### `8001AA68`

**Fixed-cell debug string drawer.** `(str: *const u8, x: i16, y: i16)`. Walks an ASCII string and, per character, emits one sprite primitive into the scratchpad ordering-table pointer `_DAT_1F8003A0` (advancing it 4 words per glyph; tag `0x3000000`). The glyph's source cell is chosen by character class - digits `0x30..0x39` → cell-row `v=0xF8`, `u=(c-0x30)*8`; upper `0x41..0x5A` and lower `0x61..0x7A` → `v=0xF0`, `u=(c-0x40)*8` / `(c-0x60)*8`; `'='`/`'-'`/`'_'` map to fixed cells; space `0x20` and `'.'` `0x2E` advance without drawing; any other byte ends the string. This is the **dev / CONFIG-test-screen monospaced text path** (the `0x8007078C` mode-label strings `FUN_800188C8` fetches are drawn through it), distinct from the proportional dialog font (`legaia-font`).

High fan-in across the debug-menu / world-map dev overlays. `see ghidra/scripts/funcs/8001aa68.txt`.

### `80024190`

**In-field save/load screen driver.** The field overlay cannot host the save UI -
that code lives in the menu overlay (PROT 899) - so entering a save point or the
pause menu's save row means paging one overlay out and the other in. This actor
is what sequences that, and it retires itself once the field overlay is back.

State 0 registers the actor at `_DAT_8007B8E0`, sets the master game mode
`_DAT_8007B83C = 0x16` (mode 22, CARD INIT) and advances. States 1, 3, 5, 7 and 9
are the same wait: poll `FUN_8003DE7C(1)` and advance when it returns zero. State
2 loads overlay slot `4` (the menu overlay) via `FUN_8003EBE4(4, 0)`. State 4 is
the UI itself - `actor[+0x5C] == 0` calls the load-side dispatcher `FUN_801DD35C`
(menu-overlay copy), non-zero calls the save-write flow `FUN_801DC6B4`; either
advances only on a non-zero return, so the actor idles for as long as the player
is in the UI. State 6 loads overlay slot `2` (the field overlay, PROT 897) back,
state 8 restores the slot-B pair through `FUN_80025BA0`, and state 10 sets
`_DAT_8007B83C = 3` (field per-frame), clears `DAT_8007B648` and sets
`actor[+0x10] |= 8` to retire.

States 0, 1 and 2 - the frames where an overlay is mid-swap - additionally emit a
cover fill each tick: a five-word GP0 quad (code word `0x2BFFFFFF`, so
semi-transparent flat white) spanning `y = -4` to the screen height, with the
extents read from the scratchpad framebuffer rect at `0x1F80038C` / `0x1F80038E`,
plus a three-word draw-mode packet from `FUN_80059010(p, 0, 0, 0x4E, 0)`. Both
are allocated out of the primitive cursor `0x1F8003A0` (`0x18` and `0x0C` bytes)
and linked with `FUN_8003D2C4`.

**The ordering-table bucket is `0`, not `*(u16 *)0x1F8003A6 - 1`.** The index
computation at `0x800241FC..0x8002420C` is

```text
addiu v1, v1, -0x1        ; v1 = ot_size - 1
bgez  v1, 0x80024260      ; taken for any non-empty OT ...
_clear s1                 ;   ... with s1 = 0 in the delay slot
j     0x80024260
_move s1, v1              ; only reached when ot_size == 0
```

so `s1 = min(0, ot_size - 1)` - zero for every real ordering table, and the
negative `ot_size - 1` only in the degenerate empty case. Reading `s1` as the
deepest bucket inverts the depth the cover is drawn at.

The actor also arranges its own dispatch. State 0 writes its actor pointer to
`_DAT_8007B8E0` *and* sets mode `0x16`; mode 23's frame body `FUN_80017978` is
`(*_DAT_8007B8E0)[+0x0C]()`, so from the next frame the CARD pair ticks this
actor and nothing else. Mode 22's own init `FUN_8002574C` would overwrite
`_DAT_8007B8E0` with the standalone card actor (descriptor `0x800706D4`,
handler `0x801E36A0`), but its whole body sits behind `gp+0x7E8 != 0`
(`0x8002576C`), so the in-field registration survives.

### `80021934`

**Scene-transition streaming actor.** Ghidra's body starts at the
`addiu sp, sp, -0x120` prologue, but three instructions sit in front of it, in
never-analyzed space (read from `extracted/SCUS_942.54` at file offset
`0x12134`; the load base is the EXE header's `dst = 0x80010000` at file `0x800`):

```text
80021934  lui  v0, 0x1f80
80021938  lbu  v0, 0x393(v0)     ; v0 = DAT_1F800393, the frame-skip factor
8002193c  lw   v1, 0x710(gp)     ; v1 = the transition countdown
80021940  addiu sp, sp, -0x120   ; <- what Ghidra calls the entry
...
80021960  subu v1, v1, v0        ; countdown -= dt
80021968  sw   v1, 0x710(gp)
```

A call landing on `0x80021940` would run that subtract on uninitialised
registers. The decrement precedes the state dispatch, so it happens even for
out-of-range states.

| state | target | body |
|---|---|---|
| 0 | `0x80021990` | `DAT_8007B648 = 0x80`, seed the countdown `gp+0x710 = 0x46`, advance only while the start gate `_DAT_8007BC20` is zero |
| 1, 3 | `0x800219FC` | advance when the CD queue `FUN_8003DE7C(1)` reports idle |
| 2 | `0x800219BC` | index arm: stream chunk `DAT_8007B768 + 3`, cache the byte size at `gp+0x73C`, advance |
| 4 | `0x80021A20` | wait for the countdown to go negative, then stream by path and hand off |

States 1 and 3 share a jump-table target, so the table has five rows and four
bodies.

`FUN_8001EEF0` resolves **by index** only when
`_DAT_8007B868 == 0 && _DAT_8007B8C2 != 0` (`0x8001EEF0..0x8001EF3C`), by path
otherwise. This actor splits on the same `_DAT_8007B8C2`: state 2 issues its
stream when it is non-zero, state 4 when it is zero. `_DAT_8007B8C2` lives past
the loaded image (`dst + size = 0x8007B800`), i.e. in BSS, and no dump in the
corpus writes it - so a retail boot reads `0` and **state 4 is the arm that
runs**, with state 2's stream the inert dev/index arm. State 2 advances either
way.

State 4 builds `DATA\FIELD\` + the active scene name + `.LZS` - prefix from the
12-byte literal at `0x800106C4`, suffix from `0x8007B3CC`. Ghidra names the
prefix symbol `s_DATA_FIELD_`, which is its identifier-safe mangling of the
backslashes; the bytes are `DATA\FIELD\`, matching the `DATA\FIELD\<scene>.MAP`
sidecars the field loader stages. Before building it, state 4 rotates the three
scene-name buffers `0x80084558 <- 0x80084548 <- 0x800915C8`.

`see ghidra/scripts/funcs/80021940.txt`.

### `8002174C`

**Morph-weight apply pass.** Two passes over the morph block at `actor[+0x4C]`,
which is `[u32 count]` followed by one slot per record. A record's header is
three words - `[u32 group_id][u32 first_vertex][u32 delta_count]` - with 8-byte
`[i16 dx][i16 dy][i16 dz][pad]` deltas from `+0xC`, the same record body the VDF
morph stager `FUN_8001C604` walks.

Pass one (`0x80021794..0x8002180C`) copies each named group's whole rest pose -
`n_vert` 8-byte GTE vertices, moved with unaligned `lwl`/`lwr` + `swl`/`swr`
pairs - out of the `actor[+0x90]` stream, whose cursor advances *continuously
across records*. Pass two (`0x80021824..0x8002187C`) then applies every record's
deltas at the live weight `actor[+0x6E]` via `FUN_8005B038`. The split is
load-bearing: two records may name the same group, and a fused loop would have
the second record's restore wipe the first record's deltas.

The **cursor advance is not the record body length**. Both passes step by
`group.n_vert * 0x60` (`0x800217B4`, `0x80021860`) - a fixed slot sized off the
*object's* vertex count, not off `delta_count`. Since `0xC + delta_count*8` is
bounded by `0xC + n_vert*8`, a slot always contains its record with slack; it is
a fixed-pitch slab rather than a packed stream. What the remaining reservation
is for is not established - only that both passes use the same expression and so
stay in phase.

The tail (`0x80021880..0x8002190C`) is a **ping-pong** ramp, not a one-shot:
`+0x6E` moves by `actor[+0x3C] * DAT_1F800393` while the direction halfword
`actor[+0x40]` is zero and by `actor[+0x3E] * DAT_1F800393` while it is not, and
each clamp flips the direction - underflow sets weight `0` and direction `0`
(rising), overflow sets `0x1000` and direction `1` (falling). Landing exactly on
`0` is **not** a turn; the low rail is reached by 16-bit underflow, so an actor
sitting at zero spends one more frame descending before it turns.

### `8002149C`

**Camera-relative glide actor tick.** Three facts pin it, none needing the
decompiled C:

1. Spawn descriptor `0x8007071C` carries `0x8002149C` at `+0x8`, and
   `0x8007071C` is the descriptor `FUN_80021248` allocates from (`0x800212AC`) -
   so this leaf is that family's `actor+0x0C` tick.
2. It consumes exactly what that spawner produces. `FUN_80021248` writes a
   normalized 20-halfword record to `actor+0x80` and folded angle deltas to
   `actor+0x24/26/28`; this tick reads ten `(step, target)` pairs from
   `actor+0x80` and three `u16` budgets from `actor+0x24`, driving the ten
   camera globals in the same order and widths the spawner compared against -
   angles `0x8007B790/92/94`, eye-space trio `0x800840B8/BC/C0`, focus trio
   `0x80089118/1C/20`, GTE `H` `0x8007B6F4`.
3. Its terminal handshake inverts the spawner's. `FUN_80021248` runs
   `_DAT_1F800394 &= ~0x80; ... |= 0x100`; on all ten channels arriving this
   tick runs `&= ~0x100; |= 0x80` (`0x80021720`) and raises `actor[+0x10] |= 8`.
   Bit `0x100` means a glide of this family is live, bit `0x80` that the last
   one finished.

Per frame each channel advances by `step * DAT_1F800393`, and a `step` of `0`
parks the channel while still counting as arrived - which is how a beat leaves
an axis alone, and why the terminal test counts to ten rather than latching. The
three channel classes detect arrival differently: the angle channels drain the
`actor+0x24` distance budget and snap on 16-bit underflow (the budget is what
makes the 12-bit wrap correct - the spawner already folded the shortest arc);
the six `i32` channels use an overshoot test against a sign-extended halfword
target; `H` uses the same overshoot test on its `u16` global. All three store
the accumulated value before the arrival test and overwrite it with the target
on arrival, so a channel lands exactly, never a frame past.

The earlier "role not established" reading also recorded a read of the frame
scratch `0x1F80037D`. There is none: every scratch access in the body resolves
to `0x1F800393` (`0x1F800314 + 0x7F`) or `0x1F800394` (`+ 0x80`).
