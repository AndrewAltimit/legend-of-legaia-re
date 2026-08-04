# Wave B lane A - the round prompt was one frame late, and the park was the HP readout

Two measured defects from `handoff/lane3-battle-depth.md`, both re-derived and
both fixed. Neither cause was the one the residual named, and one of the two
reported symptoms does not exist.

`battle_depth_replay` now scores **10 / 10** (`BASELINE` raised 8 -> 10), with
rungs 7 and 8 moved back into the same fight as everything before them so the
park cannot be restarted around again.

## The retail round/turn model, from the disassembly

Two state machines share one context (`ctx = _DAT_8007BD24`), and the round is
a handshake between them. `ctx[+0x06]` is the command-flow SM `FUN_801D0748`
(the menu half); `ctx[+0x07]` is the action SM `FUN_801E295C`.

### `0x14` is the round-start arm, and the only writer of `0x1E`

```text
801d0ec4  lw    v1,-0x42dc(s0)      ; ctx
801d0ecc  sw    v0,0x880(v1)        ; highlight cursor = 0x8000 (the Left arm)
801d0ed0  jal   0x801d88cc          ; per-round actor sweep
801d0ed4  _sb   s5,0x0(s3)          ; ctx[+0x06] = 0x1E   (s5 = 0x1E at 0x801D0C98)
801d0ee4  jal   0x801d388c          ; open the prompt window (a0 = a1 = 0)
801d0ef4  lbu   v0,0x28a(v0)        ; round index
801d0efc  beq   v0,zero,0x801d0f0c  ; round 0 only: the tutorial arm
```

The store is unconditional. The ring `0x28` is entered only from `0x1E`'s
confirm (`0x801D108C`), so **no observer of retail ever sees the ring first**.

`0x14` is reached twice over:

- **battle open** - `0x0B`'s intro timer expires and branches on the
  back-attack byte: `ctx[+0x290] == 1` stores `0xFE` (the party loses its first
  round), otherwise `0x14` (`0x801D0E68..0x801D0EB8`);
- **every later round** - the action SM's `ctx[+0x07] == 0xFF` arm.

### The round boundary: `0x801E67E8`

```text
801e67e8  lui   a0,0x8008
801e67ec  lw    v1,-0x42dc(a0)
801e67f0  li    v0,0x14
801e67f4  sb    v0,0x6(v1)          ; ctx[+0x06] = 0x14  -> next round's prompt
801e6800  lbu   v0,0x28a(v1)
801e6808  addiu v0,v0,0x1
801e680c  jal   0x801f45a4
801e6810  _sb   v0,0x28a(v1)        ; ctx[+0x28A] += 1   (round index)
```

`ctx[+0x07] = 0xFF` is stored at `0x801E67E4` when the per-round action cursor
has passed every living actor (`0x801E679C..0x801E67C8`).

**This block has to be read off the jump table.** Nothing inside
`FUN_801E295C` branches to `0x801E67E8`; the only way in is the `jr v0` at
`0x801E2AAC` over the 256-slot table at `0x801CED44`, and slot `0xFF` of that
table is `0x801E67E8` (read out of `extracted/overlays/overlay_battle_action_0898.bin`
at file `+0x52C`, base `0x801CE818`). A pass that does not resolve the table
prints `Removing unreachable block (ram,0x801E67E8)` - one of the dumps in the
corpus says exactly that - and the round bump vanishes from the C. That
`ctx[+0x28A]` is the round index is corroborated by `0x14`'s own second reader:
under `_DAT_8007BD0C == 0xB6` (Muscle Dome) `0x801D0F94..0x801D0FA4` draws
`4 - ctx[+0x28A]`, the rounds remaining.

### `s2` is not the pad

Masks are **packed** throughout (byte-swapped against the raw BIOS word): Left
`0x8000`, Right `0x2000`, Down `0x4000`, Up `0x1000`. With a selection widget
up (`_DAT_800846C8 != 0` and `ctx[+0x275] != 0`), `0x801D07FC..0x801D0AC0` walks
a highlight into `ctx[+0x880]` and stamps `+0x1D` on the widget actors
(`ctx[+0x1114]`/`+0x1118`/`+0x111C`/`+0x1120` = Left/Right/Up/Down), and then
`0x801D0AC4..0x801D0B08` **rewrites `s2`**: confirm -> the stored `ctx[+0x880]`,
cancel -> the cancel mask, otherwise zero. So the handlers' direction tests read
as "take the highlighted chip". Without a widget, `0x801D0B0C` builds `s2` from
the plain packed pad and the direction tests are direct presses. `0x14` seeds
`ctx[+0x880] = 0x8000`, so a fresh prompt is highlighted on its Left arm.

| State | Left | Right | confirm `_DAT_800846D0` | cancel `_DAT_800846D4` |
|---|---|---|---|---|
| `0x1E` | Begin | Run -> `0x32` | Begin | - |
| `0x32` | run confirmed -> `0xFE` | back to `0x1E` | - | back to `0x1E` |
| `0x6E` | begin the round -> `0xFE` | step back | begin the round | step back |

`0x32`'s confirm stamps `+0x1DE = 5` on all three party actors
(`0x801D1174..0x801D1184`) before storing `0xFE`.

### Every command commits the same way

```text
801d16ac  jal   0x801db81c          ; next member after ctx[+0x13] awaiting a command
801d16b4  lw    v1,-0x42dc(s6)
801d16bc  lbu   v1,0x0(v1)          ; ctx[+0x00] = seated party count
801d16c4  bne   v0,v1,0x801d16d8    ; someone still owes one -> stay in 0x28
801d16cc  li    v0,0x6e
801d16d0  sb    v0,0x0(s3)          ; nobody does -> 0x6E
```

Ten call sites of `FUN_801DB81C` in the handler, one per commit path
(`0x801D16AC`, `0x801D22C4`, `0x801D24B4`, `0x801D2698`, `0x801D2830`,
`0x801D29C0`, `0x801D2AAC`, `0x801D2D74`, `0x801D2E64`, `0x801D2FE4`). The
sibling `FUN_801DBA04` scans from zero and serves `0x1E`'s confirm and `0x6E`'s
cancel. Both skip a member whose per-member byte `_DAT_8007BD10[i]` is already
`4`, whose live HP `+0x14C` is zero, or whose status word `+0x16E & 0xF84` is
set, and both return `ctx[+0x00]` when none is left. **No retail command path
can leave the flow parked.**

## Defect 1 - Run unreachable: the prompt was one frame behind the only edge

Not "no battle ever opens the round prompt". The port *did* open it - one tick
after the session appeared. `World::open_battle_command` built the session on
`CommandPhase::Menu`, and `arm_round_open_prompt` (top of the **next**
`live_battle_tick`) rewrote it to `RoundPrompt`.

Measured on `town01` formation 4:

```text
[entered]    cmd=Some("Menu { cursor: 0 }")        flow=TurnPrompt
[entered+1]  cmd=Some("RoundPrompt { cursor: 0 }") flow=TurnPrompt
```

`battle_command.is_some()` is the only edge an observer has, so every look at
the opening session - the ladder's `wait_for_command`, and any host drawing
between ticks - saw the ring. Run lives on that prompt and nowhere else, hence
"a player cannot flee at all".

**Fix.** `open_battle_command` builds the session with
`BattleCommandSession::new_round_open(..)` when the flow byte says the round is
opening (`Idle` at battle entry, `TurnPrompt` at a round boundary), which is
retail's unconditional `0x14 -> 0x1E`. Mid-round reopens (a submenu backed out
of) still open on the ring, matching `0x3C`/`0x46`'s cancel arms. The
constructor already existed and had **zero callers** - orphan wiring debt, not
missing code. `arm_round_open_prompt` stays as the backstop for a session
already open when the boundary parks the flow.

## Defect 2 - the park: one command, and it was the HP readout

Only **Item** parks. Measured from a fresh fight, one command at a time, with
the next command session waited on to 9000 ticks:

| command | next party command session |
|---|---|
| Summon (spell `0x81`, spawn raised) | 231 ticks |
| Spirit | 230 ticks |
| Item (Healing Leaf) | **never** (9000 ticks, mode still `Battle`) |

Summon and Spirit do not park, before or after the fix - the summon run was the
*first* command of its fight, so no item had touched anything. They share
Item's shape (they resolve without a strike), and that shared shape is what the
residual generalised from; the mechanism is not shape, it is party HP.

**Root cause.** The parked state is the action SM at `0x51` (`DoneFadeDown`),
waiting on `hp_bar_drain_pending` - retail's `FUN_801E7250`. `World::use_item`
writes live HP directly and stops there. That is complete out of battle, but
retail's in-battle applier `FUN_800402F4` also **assigns** the readout's pending
accumulator `-delta`:

```text
800408e8  sll  v0,s4,0x10
800408ec  sra  v0,v0,0x10      ; (i16) delta folded into the stat at 0x800408A8
800408f4  subu v0,zero,v0      ; -delta
800408fc  _sw  v0,0x10(v1)     ; assign, not accumulate
```

Three identical sites: `0x800408FC`, `0x80040D28`, `0x800410BC`. The ramp's only
guard is `+0x10 != 0` (`0x800474E8`), so `hp != hp_display` with a **zero**
accumulator is absorbing - `docs/subsystems/battle_hp_bar` calls this out as
the softlock class, and the port had a caller that produced it on *every* heal
rather than on a raced one. The next monster swing at the healed member then
parked the fight with no in-battle exit at all, not even winning it.

Trace of the park (pre-fix): `[item] resolved act=0xff` -> ... ->
`PARKED act=0x51 active=1 tc=0 keys=[0,0,...]`, i.e. the monster's Done band
waiting on party slot 0's bar.

**Fix.** New `BattleActor::assign_hp_bar(delta: i16)` in
`engine-vm/src/battle_action/types.rs` (the `-delta` assign,
`battle_hp_bar::assign_pending`, guarded on an armed readout), called from the
three port sites that write party HP outside `apply_battle_hp_delta`:

| Port site | Retail counterpart |
|---|---|
| `World::apply_battle_item` (new; the battle item menu's applier) | `FUN_800402F4` class 0 / 1 heal arms |
| `fold_spell_outcome`'s `Revive` arm | `FUN_800402F4` class 4 revive arm |
| `apply_final_heal_revives` | `FUN_801E6968`'s two `FUN_800402F4(4, 1, slot)` calls |

The last two were latent instances of the same defect - both write live HP
bare, neither had a caller in the ladder.

## Rungs

| # | rung | before | after |
|---|---|---|---|
| 1 | battle entered, command menu open | pass | pass |
| 2 | Attack | pass | pass |
| 3 | Arts entry | pass | pass |
| 4 | Arts executed | pass | pass |
| 5 | Magic | pass | pass |
| 6 | Summon | pass | pass |
| 7 | Item | pass (own fight) | pass (**same** fight) |
| 8 | Spirit | pass (own fight) | pass (**same** fight) |
| 9 | Run refused under `battle_no_escape` | **blocked** | pass |
| 10 | Run escapes | **blocked** | pass |

`BASELINE = 10`. Rungs 9 and 10 still take fresh battles, but now for the only
reason left: `no_escape` is copied into the session when it opens, so the flag
has to be set before entry and the two rungs want opposite values.

## Regressions added

- `engine-core` `battle_open_flow::the_round_prompt_is_up_on_the_frame_the_session_opens`
  - pad-driven, disc-free. Fails with the fix disabled:
  `the first frame a session exists must already be the round prompt, got Menu { cursor: 0 }`.
- `engine-core` `battle_open_flow::a_battle_item_heal_keeps_the_readout_and_the_turn_pump_alive`
  - pad-driven, disc-free, through the ring and the item menu. Fails with the
  seed disabled: `the readout never caught up to live HP - the absorbing pair is back`.
- The ladder's `item` and `spirit` rungs now run in the fight before them, so
  the `wait_for_command` opening each one is the park assertion.

The pre-existing `the_prompt_is_per_round_not_per_turn` passes either way - it
hands the session in on the ring and lets the next tick rewrite it, so it
cannot see this. Left alone; the new test is the one that measures the entry
point.

## One false citation corrected on the way past

`battle_input::step_round_prompt`'s doc claimed Circle-takes-Run "is what
retail's own arm does (`801d10bc`, the Circle test that jumps straight to flow
state `0x32`)". That test is `andi v0,s2,0x2000` on the **packed** pad word, so
it is **Right**, not Circle - packed Circle is `0x0020`. This is the same
raw-vs-packed trap the previous lane settled for the arts entry, re-appearing
one function over. Retail's `0x1E` chips *are* directions (Left = `Begin`,
Right = `Run`); its confirm reaches one only through the pre-dispatch `s2`
rewrite that replays the walked highlight in `ctx[+0x880]`.

Comment corrected; **behaviour left alone**. Converting the prompt to the
direction-picked form means the chrome stops drawing a cursor, and that lives
in `engine-ui`, which is off limits to this lane. Doing the input half alone
would leave a cursor on screen that nothing moves.

## Blocked / out of scope

**`crates/engine-core/src/world/tests/field_npc_motion.rs` does not compile on
this wave base**, and it is not mine to fix:

```text
error[E0559]: variant `npc_motion::WalkTouchEvent::Warp` has no field named `target_map`
   --> crates/engine-core/src/world/tests/field_npc_motion.rs:180:58
    = note: available fields are: `sub_id`
```

Present at `631a9bb9` ("The field VM's own mock now observes the minigame door
arm") before any edit of mine, and it blocks the **whole** `legaia-engine-core`
lib-test target - nothing in the crate can be run until it compiles. The
failure is not just a rename: the test also asserts
`pending_scene_transition == Some(3)`, which is the **old** "sub_id is a map id"
reading that same commit exists to falsify.

Fixed in **its own first commit** so an integrator can drop it whole if the
owning lane lands the same repair: the literal becomes `sub_id: 3` and the
assertion becomes `pending_minigame_warp == Some(3)`, which is what
`field_movement.rs:1942`'s `WalkTouchEvent::Warp { sub_id }` arm actually does
and what `world/tests/field_records.rs` already asserts for the field-VM half of
the same opcode. No production code touched.

The same commit also leaves `cargo fmt --all -- --check` failing on
`man_field_scripts/npc_motion.rs:1036` (a `WalkTouchEvent::Warp { sub_id }`
literal rustfmt wants on one line). **Left alone** - it is production code in an
off-limits module and the owning lane may be mid-edit. Every file in this lane's
own diff is fmt-clean.

Nothing else refused.
