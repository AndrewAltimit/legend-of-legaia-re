# legaia-engine-cast

The slot-B cast / summon modules (PROT 0903..0966, loaded at `0x801F69D8`):
their tick bodies, the camera and countdown each module arms, and the
cast-arm kernels the battle action SM pages them through. Ported from each
module's disassembly at the slot-B base, with no bytes from the original
executable.

The crate sits one rank above
[`legaia-engine-battle-vm`](../engine-battle-vm/README.md): the cast bodies
call the action SM's bearing and target helpers, the battle formulas, the
damage wrappers and the battle camera script, and nothing in that crate names
a module here. Its other engine dependencies are
[`legaia-engine-system`](../engine-system/README.md) (trig, BIOS `rand`),
[`legaia-engine-camera`](../engine-camera/README.md) and
[`legaia-render-kernels`](../render-kernels/README.md), which holds the draw
packets a module body records for the hosts (PROT 0904's `TheederPacket`). `World`-free; the
caller is `engine-core`'s cast band (`World::run_cast_module_code` and the
cast-band directors). Consumers name the modules here directly
(`legaia_engine_cast::cast_module_ticks`). Free of wgpu, winit and cpal, so it
builds for native and `wasm32` alike.

The phase machine, the PROT 0898 entry tables an action reaches a module
through, the tick ABI and the damage shapes are in
[`cast-module.md`](../../docs/subsystems/cast-module.md).

## What belongs here

| Module | Covers |
|---|---|
| `cast_module_ticks` | The code half of the band: the capture-class and summon-creature tick bodies, the phase-chain bodies (`chain_bodies`), the capture trampolines, the module-side actor state mirror (`CastActorState`) and the per-arm stagers and spawn records. |
| `cast_arm_ticks` | The fourteen trampoline-reached capture-class bodies of PROT 0940 / 0941 / 0943 / 0944 / 0950 / 0956 / 0962, keyed on `(PROT entry, body VA)`. |
| `seru_magic_ticks` | The player Seru-magic bodies, spell ids `0x81..=0x8B` (PROT 0903..0913), one child module per Seru. |
| `cast_module_camera` | Each module's own camera arms and its countdown (`ModuleCountdown`, `ModuleFollow`), one director per module. The shot value itself, `ModuleShot`, is `battle_cam_script`'s in `legaia-engine-battle-vm`, which steps it. |
| `cast_fatal_decision` | PROT 0954, Fatal Decision: the thirteen-arm roulette, its wheel and outcome. |

What stays below, in `legaia-engine-battle-vm`, is the action SM's side of a
cast (`battle_cast_census`, `battle_cast_cue`, `battle_cast_dispatch`) and
the closed-form module heal amounts (`battle_formulas::vera_heal_amount` /
`orb_heal_amount` / `spoon_heal_amount`), which `legaia-engine-battle`'s
spell-XP kernel reads without naming this crate.

## Tests

`tests/integration.rs` gathers the crate's integration tests into one
binary:

- `cast_module_trampoline_bodies` - disc-free property tests over the
  trampoline-reached bodies and the battle overlay's effect-child hit arm.
- `cast_chain_bodies_real` - disc-gated: every phase-chain body descriptor
  re-derived from its owning image's bytes.
- `cast_arm_retail_gating` - the arm walk retail entered per action id,
  measured under PCSX-Redux and pinned.

Disc-gated tests skip and pass when `LEGAIA_DISC_BIN` is unset.
