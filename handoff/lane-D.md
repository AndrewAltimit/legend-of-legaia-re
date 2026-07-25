# Lane D - the actor `+0x0C` handler, and `801d4a60`

Two tasks. Both done.

## Task 1 - actors carry a per-frame handler

`crates/engine-core/src/actor_handler.rs`. `ActorHandler` is a VA-preserving
enum: named variants for the handlers whose bodies are ported, `Retail(va)` for
the rest (fully comparable - the sweep and the two list leaves work on it
identically), and `HandlerKernel` as the dispatch half so the frame loop has
something to run. `Actor` gained `handler`, `state_50` (`+0x50`), `state_54`
(`+0x54`), `colour_tween` and `tint_push`.

**The named VAs came off the disc, not out of a code reference.** Scanning
`extracted/overlays/overlay_field_0897.bin` at base `0x801CE818` for the
allocator's descriptor shape (`[+0 0][+4 0xFFFF0000][+8 handler][+0xC flags]`)
resolves six of them at their `+8` word, which is stronger evidence than a
`jal` - a descriptor word is what `FUN_80020DE0` actually stamps:

| descriptor | file | handler | allocator |
|---|---|---|---|
| `0x801F2760` | `0x023F48` | `0x801D84D0` | `FUN_801D9C3C` submode open |
| `0x801F27EC` | `0x023FD4` | `0x801DA930` | `FUN_801DDE34` fade family |
| `0x801F2810` | `0x023FF8` | `0x801DBE9C` | `FUN_801DE478` scene actor |
| `0x801F2888` | `0x024070` | `0x801DDC20` | `FUN_801DE2B0` colour tween |
| `0x801F26D8` | `0x023EC0` | `0x801D4A60` | `FUN_801D5A24` scripted scene |
| `0x801F28A0` | `0x024088` | `0x80037174` | narration roller |

### Status of the four addresses the brief named

| Addr | Status | Chain |
|---|---|---|
| `801d7518` | **live** | `SceneHost::load_scene` (gated on a scene already being loaded = retail's `_DAT_8007B8B8 == 2` warp) → `World::scene_transition_actor_sweep` → `sweep_actor`. Roots: `enter_field_scene` → `BootSession` / `legaia-engine run` / `play-window` |
| `801d9c3c` | **live** | `SceneHost::load_scene` → `World::man_load_actor_reset` → `open_submode`, with `handler_present` from the `FUN_8003CF04` port |
| `801de478` | **live** | same call, one step later - `FUN_8003AEB0` issues `FUN_801DE478(0xF)` at `0x8003B9B0` |
| `801ddc20` | **still disclosed** | dispatched every game tick by `World::tick_handler_actors`, but nothing spawns a tween, so it runs over zero actors. See below. |

### New ports

`FUN_8003CF04` (finder), `FUN_8003CF40` (retire sweep), `FUN_801DE2B0`
(colour-tween spawner).

### Why `801ddc20` is still disclosed

Because a kernel dispatched over zero actors is indistinguishable from an
unwired one, and calling it live would have been the exact failure this wave is
auditing for. The frame loop reaches it; what is missing is a *producer*. The
only retail spawner is the field VM's screen-effect fade arm (`FUN_801DE840` at
`0x801DFD68` / `0x801DFEE8` → `FUN_801DE2B0`), and the `FieldHost` trait that
would carry a hook for that sub-op lives in `engine-vm`. `World::spawn_colour_tween`
is the seam; a sibling lane owning `engine-vm` closes it in one hook.

### Two corrections out of the bytes

1. **`LAB_801DA930` is the fade-family handler**, not a callback. It is the
   `+8` word of descriptor `0x801F27EC`, the one the fade spawner `FUN_801DDE34`
   allocates from. So the field VM's `4C 9F` / `4C 87` "register callback" ops -
   `func_0x8003CF40(_DAT_8007C34C, LAB_801DA930)` - **cancel a running fade**.
   `FUN_8003CF40`'s fifteen instructions have no return value and write nothing
   but the flag word; there is no registration anywhere in it. `cutscene.md`
   had already noticed the mechanism ("only sets `node[+0x10] |= 8`") without
   the identity that explains it. Written up in `script-vm.md`.
   (`script-vm-menuctrl.md` still carries the "callback registration" label in
   two places - out of this lane's scope.)

2. **`FUN_801DE2B0` and `FUN_80024E80` consume the same 13-`i16` block.** The
   field VM builds one on its stack and picks an arm on `_DAT_1F800394 &
   0x800000`. Both decodes agree field for field, which corroborates each - and
   the tween names two words `fade.rs` records as unpinned: template `[10]` is
   its ramp **delay**, `[11]` its **hold**. `[12]` is the one word the tween
   arm never reads.

### Two divergences, both deliberate and both documented at the call site

- **Handler actors allocate from the top of the pool down.** Retail keeps them
  on their own list (`_DAT_8007C34C`), disjoint from the script-addressed scene
  actors. The engine has one pool whose *low* slots are named - `ensure_actor(id)`
  addresses them by script id, `init_scene_animations` binds actor `k` to scene
  TMD `k` - so a bottom-up allocation would have handed the submode driver slot
  0, i.e. the player.
- **The MAN-load spawns retire their own predecessor first.** Retail allocates
  a fresh node each load and relies on the per-scene actor-list teardown to
  reclaim the old one; the engine has no teardown, so an unqualified spawn
  leaks a slot per scene change. Applied to `ActorHandler::SceneActor` and
  `ActorHandler::ScriptedScene`, each scoped to its own handler class so it
  cannot touch anything a script owns. (The submode driver needs none - the
  open adopts a live one instead of spawning.)

## Task 2 - `801d4a60` ported

`crates/engine-core/src/field_actor_program.rs`, 756 instructions, all 23 live
arms. Doc: `field-locomotion.md` § "The scripted-scene actor".

**It is not a locomotion handler.** `+0x50` is a **program selector**: the entry
state computes `+0x54 = (+0x54 + 1) + (+0x50) * 10`, so the state space is four
programs on a stride of ten entered at 1 / 11 / 21 / 31 - exactly the table's
live blocks. The fifteen epilogue slots (`6..=10`, `16..=20`, `27..=30`) are the
unused tails of each ten-wide block, not fifteen dead states. Without the `×10`
the table reads as a sparse mess; with it, four short programs of 5, 5, 6, 7.

**Programs 0 and 1 are openers, 2 and 3 their closers.** The MAN loader
`FUN_8003AEB0` spawns program 2 when flag `0x17` is set (`0x8003BB10`) and
program 3 when flag `0x0C` is set (`0x8003BB38`) - and those are precisely the
flags programs 0 and 1 *set* and 2 and 3 *clear*. So the loader is not starting
cutscenes, it is finishing ones a scene change interrupted; the flag is the
handshake that survives the scene boundary. That rule is ported and **live**
(`World::man_load_resume_programs`, off `load_scene`), so the actor is spawned
in a real session.

The step itself is disclosed. Missing input is specific: `ProgramEnv`'s middle
three fields. The BGM request/ack pair `_DAT_8007BABC` / `_DAT_8007BAA0` and the
CD-XA in-flight counter `_DAT_8007BC20` have no `engine-core` counterparts, and
states `0x02`, `0x16` and `0x19` each park indefinitely on them - invented
values would either stall a program forever or run it through its voice line in
one frame. Everything else it needs already exists.

Also ported: `FUN_801D5A24` (the spawner, `+0x54 = 0` / `+0x50 = program`).

### Corroborations that came from outside the function

- The audio census reached it from the other direction: `audio.md` already
  lists field 0897 `0x801D4FCC` as clip `0x10` (XA17), "scripted-scene voice
  stream". That is program 2's state `0x16`, and it is why the family is a
  voice-over cutscene rather than an approach controller.
- The lift leg (state `0x18`) is the same `+0x8E` / `+0x16` idiom as
  `FUN_801EE328`'s rise-up arm.

### The short dump, measured three ways

`overlay_0897_801d4a60.txt` stops at 690 instructions. Five live-RAM field
captures say 756, and so does capstone over the static image at the committed
base (`overlay_field_0897.bin` file `0x006248`), which runs to the `jr ra` at
`0x801D5628`. The 66 dropped instructions are states `0x22`..`0x25` plus the
shared tail - i.e. most of program 3 - which is exactly how a four-program
machine came to be documented as one controller.

## Note to whoever owns `actor-vm.md`

Its "Scene-load actor fix-up" section lists `FUN_801D7518`'s three retire
handlers as `0x80025000`, **`0x801E1C20`**, `0x8002174C`. The middle one is
wrong: the disassembly is `lui v0,0x801e; addiu v0,v0,-0x23e0` = **`0x801DDC20`**
(the colour tween). `0x801E1C20` would need `addiu v0,v0,0x1c20`. Out of this
lane's scope; `field_actor_kernels::RETIRED_HANDLERS` has it right.

## Tests

`field_actor_program` 19, `world::handler_actors` 12, `field_actor_kernels`
+1. Two properties worth naming, both written from the disassembly's intent
rather than the port's behaviour:

- `a_swept_actor_actually_leaves_the_pool_on_the_next_tick` - marking bit 8 has
  to end in a deactivated slot, or the whole teardown is a no-op with a flag
  word to show for it.
- `program_2_engages_the_player_and_releases_it_again` - whatever a cutscene
  program does in between, the player must come out unlocked with its speed
  back. A program that never releases is a softlock.

One test caught one of my own errors before commit
(`the_frames_push_carries_the_actors_own_kind_and_blend` asserted a push at a
clock past the hold expiry, where retail retires and draws nothing).
