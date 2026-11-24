# psx-retail-exec

A user-mode R3000 interpreter: integer instructions, `hi` / `lo`, unaligned
loads and stores, RAM and the scratchpad, COP0 register moves, and a GTE
(`gte`) written from the hardware documentation independently of the engine's
own, so camera, projection and lighting routines run too. The crate's
disc-gated tests load `SCUS_942.54` and the overlays off the user's disc, run a
retail routine over generated inputs and compare what it wrote with the Rust
kernel whose `// PORT:` tag names it.

The method and how to add a routine: [`docs/tooling/retail-kernel-differential.md`](../../docs/tooling/retail-kernel-differential.md).

No Sony bytes live here. Without `LEGAIA_DISC_BIN` the disc-gated tests skip.

Every `Cpu::call` notes its entry in a thread-local ledger (`ledger`), which is
how a test proves it ran the routine it claims: each test opens with
`differential!(..)` (in `tests/common.rs`), and `scripts/ci/differential-coverage.py`
counts those declarations against the `// PORT:` tags.

Built with the `branch-cov` feature and run with `PSX_RETAIL_EXEC_BRANCHES`
set to a file, `branch_cov` also walks each
ledger routine's control flow statically to enumerate its conditional
branches and notes which way each one went, so
`scripts/ci/branch-coverage.py` can report how much of each routine the suite
exercises, not just whether it entered it.

Built with the `branch-flip` feature and run with `PSX_RETAIL_EXEC_FLIP`
naming one branch of one routine image, `branch_flip` inverts that branch on
the retail side only, so `scripts/ci/checked-arms.py` can tell which taken
directions a test's comparison actually sees (a test that still passes did
not see the arm). See
[checked arms](../../docs/tooling/retail-kernel-differential.md#checked-arms).

## Pricing retail code

Each call also counts its data accesses by region (`Cpu::accesses`: RAM,
scratchpad, I/O, BIOS) the way PCSX-Redux charges them, so `Cpu::cost_pcsx()`
(`2 * instructions + accesses`) is a routine's time on the clock the
PCSX-Redux films are measured on. `Cpu::profile` collects routines' inclusive
costs over a run, `Cpu::resume` runs on from a save state's CPU, and
`pcsx_state` reads a PCSX-Redux `.sstate` (RAM, hardware window, BIOS, VRAM,
register files) into a CPU. `frame_runner::FrameRunner` resumes a library
state frame by frame and prices each pass; the `retail-exec frame-cost`
binary prints it:

```bash
cargo run --release -p psx-retail-exec -- frame-cost --scenario son_field_door_hop --frames 4
```

`field_units` breaks a frame's field passes into the units they repeat
(loop iterations by path, dispatcher draws and groups, prim-leaf iterations
by kind and outcome), each priced from its own instructions
(`retail-exec field-units --scenario <label>`); `--interrupts` on
`frame-cost` takes the vblank interrupt through the state's kernel.
`retail-exec actor-draws --scenario <label>` lists each actor-list entry's
draws, groups, primitives, cycles, morph vertices and colour word;
`actor-ticks` lists each tick-pass entry's handler and cycles.

Held against the emulator's own profile in
[`docs/tooling/retail-frame-cost.md`](../../docs/tooling/retail-frame-cost.md).

`battle_cost::PartRunner` splits a battle frame into its units: every
dispatcher primitive by leaf and fate (linked, off-screen, culled), every
group, call and body draw, the grid, the template tick, the effect walker visit by
visit, the kind-4 mesh builds. `retail-exec battle-cost --mode parts` prints
them (`series`, `walker`, `prims`, `bodies`, `builds` and `spans` are narrower
views); the derived table they price is
`legaia_engine_battle_loop::derived_cost`.
