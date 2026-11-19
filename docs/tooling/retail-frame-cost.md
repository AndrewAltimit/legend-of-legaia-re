# Retail frame cost

`crates/retail-exec` prices retail code on PCSX-Redux's clock, the scale every
PCSX-Redux film and frame-time ring in the library is measured on. A routine
run in the interpreter reports its instructions and its data accesses by
region, and `2 * instructions + accesses` is what the emulator charges for it.
The `frame-cost` runner resumes a library state and prices each pass of the
next frames that way, with the arguments, camera, lists and scratchpad
retail hands them, and it lands on the emulator's own profile of the same
frame to the cycle.

```bash
cargo build --release -p psx-retail-exec
./target/release/retail-exec frame-cost --scenario son_field_door_hop --frames 4
./target/release/retail-exec frame-cost --scenario jou_field_card_walk --frames 1 --calls
./target/release/retail-exec frame-cost --state path/to.sstate --csv --funcs 80043390,8003D2C4
```

## What PCSX-Redux charges

| Work | Cycles | Where in PCSX-Redux |
|---|---|---|
| An instruction | `2` | `Emulator::BIAS` added in `InterpretedCPU::execBlock` (`src/core/psxinterpreter.cc`) |
| A data load or store (`lb` .. `lw`, `lwl`, `lwr`, `sb`, `sh`, `sw`) | `1` | `Memory::read8/16/32` with `ReadType::Data` and `write8/16/32` (`src/core/psxmem.cc`) |
| `swl`, `swr` | `2` | a `read32` then a `write32` (`psxSWL` / `psxSWR`) |
| `lwc2`, `swc2` | `1` | `GTE::LWC2` / `SWC2` go through `read32` / `write32` (`src/core/gte-transfer.cc`) |
| An instruction fetch | `0` | `readICache` reads with `ReadType::Instr` |
| A GTE command, a GPU primitive | `0` | no cycle accounting in `gte-instructions.cc`; the soft GPU draws at once |

The access charge is the same in every region: RAM, the scratchpad, the I/O
registers and the BIOS ROM all cost one. `Cpu::accesses` still keeps them apart
(`ram`, `scratch`, `io`, `bios`, `other`) for a timing model that does not. A
scanline is `33868800 / 60 / 263` cycles (`PCSX_CYCLES_PER_HBLANK`), the unit the
frame pacer's ring counts in.

On the interpreter:

- `Cpu::cost_pcsx()` is the last call's cycles; `Cpu::call_with_budget` raises
  the instruction budget for one call (a whole pass runs past the default).
- `Cpu::profile(va)` collects a routine's inclusive cost the way the emulator
  profiler does: reaching `va` opens a call on the `ra` it holds there, reaching
  that address closes it. `Cpu::spent(va)` sums the calls, `Cpu::profile_log`
  lists each one, and `Cpu::profile_mark` takes a reading as each opens and
  closes.
- `Cpu::resume(pc, halt, n, budget)` runs on from wherever a state's CPU stood
  until it reaches `halt` the `n`-th time.

## The frame runner

`pcsx_state::PcsxState` reads a PCSX-Redux `.sstate` (protobuf, bare or
gzipped): RAM, the 64 KiB hardware blob (scratchpad, then the I/O window), the
BIOS ROM, VRAM and the CPU, COP0 and GTE register files. `frame_runner::FrameRunner`
stands a CPU on it and resumes it at its saved `pc`: it runs the frame in
progress out to the next entry of the field mode's per-frame handler
`0x80025EEC` (`FUN_8001698C`, `FUN_80016444(1)`, `FUN_80016B6C`), then one
frame per `frame()` call. Every library field state is saved inside a frame
(mostly mid decoration pass), so the first whole frame is the one after the
state, the same frame the emulator profiles first.

Nothing is seated by hand. The master loop, the mode handler, the kernel's
syscalls (taken through the exception vector, `Cpu::syscall_exceptions`) and
libgpu all run as retail runs them, so each pass sees the camera, lists and
packet cursor its own frame built. The I/O window is a thin stand-in:

| Register | Behaviour |
|---|---|
| `GPUSTAT` `0x1F801814` | the saved status with bits 26..28 set: the GPU always takes a command, a VRAM read and a DMA, as PCSX-Redux's free GPU does |
| DMA `0x1F801080..0x1F8010EF` | hold what is written; a transfer completes at once (CHCR's start and busy bits drop) |
| DMA channel 6 | `ClearOTagR`'s table written as the DMA writes it, so ordering-table walks are sound |
| `I_STAT` / `I_MASK` | live (a write to `I_STAT` acknowledges) |
| Root counter 1 `0x1F801110` | counts scanlines off the clock, so `VSync(1)` reads elapsed time |
| everything else | reads as saved, writes dropped |

VRAM is kept as data only: `LoadImage` and `MoveImage` write the state's VRAM
and `StoreImage` reads it back into RAM as each is entered, and each then runs
on, priced. A routine that captures VRAM, such as the CLUT-cell cycler
`FUN_80019D50`, reads what retail reads on every frame; drawing never reaches
VRAM.

No interrupt is taken. The vblank still counts (`Cpu::vblank_counter` adds one
to libetc's counter `0x8007A894` each time the clock passes a vblank), so a
`VSync` wait spins until its vblank as it does on the emulator.

Each row the command prints is one routine over one frame: calls, instructions,
accesses by region, cycles, hblanks, and the primitives it linked into the
frame's ordering table (`linked_primitives` at each call's return less at its
entry: the chain `DrawOTag` takes, walked from `*(0x1F8003F4) + 4 * len - 4`).
`--calls` lists every call, `--csv` writes one row per routine per frame, and
`--funcs` adds routines to price.

## Held against the emulator

[`autorun_field_frame_profile.lua`](../../scripts/pcsx-redux/autorun_field_frame_profile.lua)
is the emulator side. Beside the per-vsync `frame_profile.csv` it writes
`calls.csv`, every call with the cycle counter at its entry (so calls group by
game frame on the handler's entries), and the time each spent inside the
exception handler on an interrupt (`irq_cycles`: a breakpoint on the vector
`0x80000080` with `Cause` code `0`, closed when the run reaches the `EPC` it
saved). `cycles - irq_cycles` is what the runner reproduces.

On the first whole frame of `son_field_door_hop`, `stone_field_door_hop`,
`jou_field_card_walk`, `retona_field_card_boot` and `bubu2_field_door_hop`
every pass lands on the profile to the cycle (hblanks, interrupts out):

| State | Decoration less ground | Ground | Actor draw | Tick | Fog walk |
|---|---:|---:|---:|---:|---:|
| `son_field_door_hop` | `337.9` | `22.3` | `206.9` | `11.9` | - |
| `stone_field_door_hop` | `256.9` | `309.6` | `68.2` | `15.9` | `10.9` |
| `jou_field_card_walk` | `120.0` | `150.6` | `212.9` | `111.6` | - |
| `retona_field_card_boot` | `299.4` | `47.2` | `57.2` | `20.2` | `14.2` |
| `bubu2_field_door_hop` | `202.4` | `120.2` | `199.8` | `44.6` | `11.0` |

The one miss is `jou`'s tick passes, `0.7` hblanks (`0.6%`) under the profile. The
cycler's own calls inside them agree: `jou`'s seventeen sixteen-texel rows cost
`10502..11145` cycles each on both sides. On the New Game opening's `opstati`
(a state played forward from `s1_newgame_field`), the narrow strip `(0, 473,
320, 2)` and the wide one `(0, 476, 320, 34)` come to `73.7` and `1171.0`
hblanks, within `0.3%` of the profile, and the frames that recolour a capture
of rows the frame before recoloured agree as well, through the VRAM the runner
keeps.

Frames after the first part slightly: the runner never runs the interrupt
handler, so what the vblank callbacks advance (sound, the pad) stands still,
and the tick passes that read it drift by up to a few percent. The render
passes stay on the profile.

Gates, disc- and library-gated: `crates/retail-exec/tests/frame_cost.rs` (the
five states, every pass within `3%`) and `clut_cell.rs`'s
`the_clut_cell_writer_costs_what_pcsx_redux_measures` (each cycler call within
`2%`; the `opstati` case runs where the library holds its state).

## What the interrupts cost

The interrupt time the runner leaves out is a large, steady term. On
PCSX-Redux the vblank interrupt (the kernel's dispatch, libetc's callbacks and
the pad exchange over SIO, which waits on each byte's acknowledge) costs
`11.6` hblanks (about `24900` cycles) on every vblank of these states, and a
DMA or CD-ROM interrupt `1..1.8`. A frame's measured time is its passes' work
plus whatever interrupts land inside it: `son`'s frame driver spends `580.4`
hblanks of work and `610` with them. The model a frame-cost derivation builds
needs that term apart from the per-pass work.

## Limits

- PCSX-Redux states only; a Mednafen state carries no register file this
  reader takes.
- Frames are cut at the per-frame handler `0x80025EEC`, which twelve of the
  mode table's per-frame modes share, the field and the battle among them. A
  battle state runs as it stands: on `party_basic_attack_vs_gobu_gobu` the
  actor render passes (`200.2` hblanks), the bodies `FUN_80048A08` (`135.8`)
  and the field render (`90.4`) land on the profile to the cycle; ask for the
  battle routines with `--funcs`. The world map (`0x80025F2C`) and the menu
  (`0x80025F74`) have handlers of their own.
- The GPU and DMA take no time, which is PCSX-Redux's scale and not
  hardware's: a DMA's busy window (PCSX-Redux keeps a chain busy for as many
  cycles as it holds words) and any wait on it are not modelled.
- Drawing never reaches VRAM, so a routine that reads back rendered pixels
  reads stale ones.
