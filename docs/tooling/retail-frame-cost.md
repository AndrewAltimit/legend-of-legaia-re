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

By default no interrupt is taken. The vblank still counts (`Cpu::vblank_counter`
adds one to libetc's counter `0x8007A894` each time the clock passes a vblank),
so a `VSync` wait spins until its vblank as it does on the emulator.
`--interrupts` (`FrameRunner::take_interrupts`) takes the vblank interrupt
instead, through the kernel the state holds, as PCSX-Redux does (after a delay
slot, with `SR` bits 0 and 10 set): the kernel's dispatch and libetc's
callbacks run and their time lands in the routine they interrupt. The pad and
card exchange over SIO (`FUN_8006CFC8`, chained on the vblank) would wait on a
port nothing answers, so it returns at once; the rest of the interrupt costs
about `24100` cycles against PCSX-Redux's `24400..26200`.

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

Frames after the first part slightly in the tick passes, by up to a few
percent (`son`'s third frame `11.9` hblanks against `13.0`); the render passes
stay on the profile. Taking the vblank interrupt does not close it (`son` and
`jou` tick alike either way), so what the ticks read is advanced by something
else the runner does not model, the DMA and CD-ROM interrupts or a DMA's busy
window among the candidates.

Gates, disc- and library-gated: `crates/retail-exec/tests/frame_cost.rs` (the
five states, every pass within `3%`) and `clut_cell.rs`'s
`the_clut_cell_writer_costs_what_pcsx_redux_measures` (each cycler call within
`2%`; the `opstati` case runs where the library holds its state).

## What the interrupts cost

The interrupt time the runner leaves out is a large, steady term. On
PCSX-Redux the vblank interrupt (the kernel's dispatch, libetc's callbacks, the
pad exchange over SIO) costs `11.4..12.2` hblanks (`24400..26200` cycles) on
every vblank of these states, and the DMA and CD-ROM interrupts (`2081..4500`
cycles each) add `0.6..3.8` hblanks a vsync (mean `2.3`): `12.2..15.4` hblanks
a vsync in all. A frame's measured time is its passes' work plus whatever
interrupts land inside it: `son`'s frame driver spends `580.4` hblanks of work
and `610` with them. A derived model carries the term apart from the per-pass
work (`field_frame_units::VBLANK_INTERRUPT_CYCLES`,
`OTHER_INTERRUPT_CYCLES_PER_VSYNC`).

## Per-unit costs

`retail-exec field-units --scenario <label>` breaks the first whole frame's
decoration pass, ground emitter, actor render passes and fog records into the
units they repeat, each with its count and the cycles it ran
(`pass,unit,count,cycles,cycles_per_unit`). It times every loop iteration
between two arrivals at the loop's head, names the path it took by the marks
it passed, and takes a nested routine's time out of the iteration that called
it (`psx_retail_exec::field_units`, `PASSES` and `LEAVES`). What a pass spends
outside its loops is its per-call cost, so the units add up to the pass to the
cycle.

The pooled means over ten library field states (`son`, `stone`, `jou`,
`retona`, `bubu2`, `kor3`, `uru`, `town01`, `doman`, `korb2`) are the table in
[`field_frame_units.rs`](../../crates/engine-core/src/world/field_frame_units.rs),
which nothing reads yet. A path that does not depend on the data costs the same
on every state to the cycle; the rest vary by what the path computes:

| Unit | Cycles | hblanks | Engine count it needs |
|---|---:|---:|---|
| decoration call | `1622` | `0.76` | one a frame |
| decoration row | `24` | `0.011` | rows of the pass's window |
| decoration cell outside the walk box | `54` | `0.025` | window cells failing `0x1F800384..87` |
| decoration cell inside the box, no record | `99` | `0.046` | window cells inside the box |
| decoration cell with a record, not drawn | `130` | `0.061` | cells whose record fails `+0x12 & 4` or the reach test |
| decoration cell drawn | `503` | `0.234` | cells the gate draws (`ViewCells::decoration_visible`) |
| dispatcher draw | `200` | `0.093` | one per drawn mesh (decoration cell, placed mesh, model object) |
| dispatcher group | `71` | `0.033` | TMD groups of each drawn mesh |
| leaf group set-up | `40` / `46` | `0.019` / `0.021` | the same groups, tri / quad leaf |
| ground call | `129` | `0.060` | one a frame |
| ground row / window cell | `24` / `35` | `0.011` / `0.016` | the emitter's window, rows and cells |
| ground walk-visible cell | `282` | `0.131` | cells with `0x1000` |
| ... with the sort bit | `314` | `0.146` | those whose sorted path runs (`0x801F6FA0`) |
| actor list entry | `50..61` | `0.025` | every entry of the five lists |
| model actor | `148 + 1297` | `0.67` | each NPC / skinned actor drawn |
| model object | `919` / `1252` posed | `0.43` / `0.58` | each object of a drawn model |
| pose | `253` + `22` a track | `0.12` | each posed object, its tracks |
| morph vertex | `61` | `0.028` | each vertex a morph lane blends (`FUN_8005B038`) |
| placed actor | `1202` | `0.56` | each placed object drawn |
| placed mesh | `75` | `0.035` | each of its meshes |
| fog record | `635..677` | `0.30` | each live fog record |
| fog half-sheet | `149` culled / `233` kept | `0.069` / `0.109` | each half-sheet the walk emits |

A primitive costs by its leaf (kind `flags >> 1`, bank) and what became of it.
The decoration draws through the depth-cued leaves with the off-screen test
(colour word `0x40`), the actors through the opaque leaves without it:

| Leaf | Rejected (back face, near) | Off-screen rejected | Linked |
|---|---:|---:|---:|
| `k19` cued quad (gouraud textured) | `96` | `210` | `331` |
| `k18` cued tri | `81` | `248` | `281` |
| `k17` cued quad (flat textured) | `97` | `217` | `302` |
| `k15` / `k14` opaque gouraud quad / tri | `97` / `66` | - | `159` / `125` |
| `k19` / `k18` opaque gouraud textured quad / tri | `97` / `72` | - | `184` / `151` |
| `k13` / `k12` opaque flat quad / tri | `98` / `73` | - | `142` / `120` |

Every primitive of a drawn mesh is transformed (`RTPT`, then `NCLIP` against
the dispatcher's winding mask), so a mesh costs its whole primitive count; the
link count is only the visible part. `son_field_door_hop`'s decoration pass
transforms `2480` primitives in `92` drawn cells (`504` `k17`, `736` `k18`,
`1240` `k19`), rejects `1087` by winding and `1254` past an edge of the screen
(`FUN_80044798`), and links `139`: the engine's `2480` decoration primitives
are retail's transformed count, not an overcount. A derived model needs each
draw's primitive count by leaf, and the share that faces the camera and lands
on screen; at the shares these states hold, a decoration primitive costs
`150..205` cycles (`0.070..0.096` hblanks) and an actor primitive `115..168`
(`0.054..0.078`).

Priced from the table off each state's own counts, every pass lands within
`4%` of what it ran (`crates/retail-exec/tests/field_units.rs` holds `5%`, and
the fixed-path units within `2%`):

| State | Decoration | Ground | Actor | Fog |
|---|---:|---:|---:|---:|
| `son` | `-0.26%` | `+0.00%` | `+2.99%` | - |
| `stone` | `-0.57%` | `+0.03%` | `+2.68%` | `-1.11%` |
| `jou` | `-0.89%` | `+0.00%` | `+0.50%` | - |
| `retona` | `+0.50%` | `-0.33%` | `-0.65%` | `-2.22%` |
| `bubu2` | `+0.25%` | `+0.00%` | `+1.02%` | `+1.52%` |
| `kor3` | `-1.03%` | `+0.00%` | `+2.06%` | - |
| `uru` | `-0.19%` | `+0.03%` | `-3.85%` | - |
| `town01` | `+2.03%` | `+0.00%` | `-0.47%` | - |
| `doman` | `-1.08%` | `+0.01%` | `-3.03%` | - |
| `korb2` | `-1.01%` | `+0.00%` | `-0.22%` | `+4.04%` |

The actor rows carry what the table averages: a model's set-up and its objects
vary with the model (`695..1785` and `777..1084` cycles), and a non-model kind
(`gate1+gate2+kind`, `103..1670`) with what it draws.

### Against the fitted weights

| Fitted term | Fitted hb | Derived | Reading |
|---|---:|---|---|
| base | `33` | interrupts `12.2..15.4` a vsync, pass calls `~1.1` | mostly the interrupts a frame's work spans |
| decoration primitive | `0.091` | `0.077..0.103` a transformed primitive with its share of draws and groups (`stone`, many small draws, `0.145`) | close |
| decoration loop cell | `0.054` | `0.046` in the box, `0.025` outside, `0.23 + 0.09` a drawn cell | close; carries the drawn cells' set-up |
| ground cell | `0.158` | `0.131..0.146` a walk cell, `0.016` a window cell | close; carries the window cells and rows |
| placed primitive | `0.118` | `0.054..0.078` an actor primitive | about twice: carries each placed actor's `0.56` and each mesh's draw |
| NPC | `24.8` | about `9.3` set-up for `son`'s models (`12` objects each), the rest its primitives | the fitted NPC carries its mesh |
| fog record / half-sheet | `0.20` / `0.006` | `0.30` / `0.069..0.109` | under: the fog walk's time sits in the base |

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
