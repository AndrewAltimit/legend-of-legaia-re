# Battle command flow and stage overlays

## The battle open flow - `ctx[+0x06]` from the intro timer to the first swing

The battle command UI is **not one menu**. `FUN_801D0748` walks the flow byte
`ctx[+0x06]` through three separate selection surfaces, each a small cluster of
plate chips around a D-pad glyph, and none of them is a scrolling list.
The whole sequence is readable off the disc: the dispatcher's state chain is a
binary-search `beq` ladder at `0x801D0C84`, and every chip's seat and label
comes from the [screen-element placement table](battle-hud.md#the-widget-class-table---where-every-chrome-sprite-comes-from)
plus the two string pools below.

### The state chain

Each row is `ctx[+0x06]`, the arm's entry, and what it does. Addresses are in
PROT entry `0898` at base `0x801CE818`.

| `ctx[+0x06]` | Arm | Behaviour |
|---|---|---|
| `0x00` | `0x801D0DD0` | init (`FUN_801D84C0`), then `0x0A` |
| `0x0A` | `0x801D0DE0` | party plates + formation banner (`FUN_801D9D3C`); intro timer `ctx[+0x6D6] = 0x5A`, or `0x78` when `ctx[+0x290] != 0`; then `0x0B` |
| `0x0B` | `0x801D0E3C` | count the timer down; on expiry `0xFE` if `ctx[+0x290] == 1`, else `0x14` |
| `0x14` | `0x801D0EC4` | one-frame turn setup; sets `ctx[+0x06] = 0x1E` unconditionally |
| `0x1E` | `0x801D102C` | the round-open `Begin` \| `Run` prompt |
| `0x28` | `0x801D1188` | the four-arm command ring |
| `0x32` | `0x801D10F8` | escape |
| `0x78` | `0x801D16E8` | the `Auto` \| `Command` attack-mode prompt |
| `0xFE` | `0x801D31E8` | round armed; hands the frame to the action SM (`ctx[+0x06] = 0xFF`, `ctx[+0x07] = 0`) |

The prompt at `0x1E` is a property of the **round**, not of the turn: `0x14`
is the only way into it, and the action SM's round end (`0x801E67E8`) writes
`ctx[+0x06] = 0x14` and bumps the round counter `ctx[+0x28A]`. So every round
opens with `Begin` / `Run`, and each party member then picks from the ring in
turn.

### Flow `0x0C` is the boss stage module's baton

The evolved-Cort fight is the one battle whose intro leaves `ctx[+0x06]` on a value
the ladder above has no arm for, and the byte that unsticks it is written from
**outside the battle overlay**. `scripts/pcsx-redux/autorun_w4d_cort_flow_writer.lua`
watches the byte from the pre-battle field state; the sequence is:

| vsync | Event |
|---|---|
| 291 | game mode reaches `0x15`; `_DAT_8007BD24` = `0x800EB654` |
| 444 | `0x80051C94` (battle init) writes `0x00` |
| 507 | `0x801D0DDC` writes `0x0A` |
| 510 | `0x801D0DE4` writes `0x0B`, then `0x801D0E0C` writes `0x0C` in the same frame |
| 631 | loader-B tracker `0x8007BC4C` goes `0x05` -> `0x49`; slot B's head matches the in-fight state |
| 3717 | `0x801F713C` (`ra = 0x800564A0`, tracker `0x49`) writes `0x0B`; `0x801D0EB8` writes `0x14` the same frame |
| 3719 | `0x801D0ED4` writes `0x1E` - the state the in-fight capture is parked on |

Both `0x0A`-arm stores run on the same pass - the arm writes `0x0B` unconditionally
and the `0xB5` branch **overwrites** it with `0x0C`, it does not choose between them.

**No input moves it.** The probe sits pad-free through the park and then holds each
of the ten pad buttons for 60 vsyncs, twice around; across ~1450 vsyncs of held
buttons the byte takes no write at all. The gate is a clock, not a press.

**The module that holds the baton is PROT 0968**, the stage overlay for this fight
(loader-B id `0x49`; extraction index = id + `0x37F`), and the probe catches it
paging into slot B *during* the park, after the flow byte is already `0x0C`. It is
ticking the whole time: its entry `0x801F69F4` re-seeds `ctx[+0x6D6] = 0x100` every
tick, which is exactly the constant `256` the probe reads off the intro timer while
parked. Its head is a **7-word jump table at `0x801F69D8` indexed by `ctx[+0x289]`**
(`lbu a0,0x289(v1)`, `sltiu v1,a0,7`, `jr`), and that phase byte is observed walking
`0` through `6` - roughly 500 vsyncs a phase - while the flow byte holds `0x0C`. Phase 0's arm advances only once the
camera word `0x800840BC` passes `0xC00` - a dt-driven zoom-in - and then fires cue
`0x20A` through `FUN_8004FCC8`; a later arm spawns its own centred banner through the
SCUS text-actor spawner `FUN_8003541C` at `0x801F7098`, which is *why* the `0x0A` arm
skips the standard composer for this formation. The module walks the phase byte
itself: the side-band's stage-2 arm (`0x80056480..0x800564A0`) only ticks it, and
its `0x289` writes (`0x800562E8`, `0x8005640C`) belong to the stage-1 arm.

**The hand-back is a single store.** A scan of the whole 0968 image for
`sb ?,0x6(?)` finds exactly one, at `0x801F713C`, in the last phase arm
(`0x801F70D8`):

```
801F70E4  lbu  v1,0x7f(s3)        ; s3 = 0x1F800314 -> the scratchpad frame-step byte
801F70E8  lw   v0,0x73f8(a0)      ; a0 = 0x801F0000 -> module-local countdown 0x801F73F8
801F70F0  subu v0,v0,v1           ; countdown -= dt
801F70F4  bgtz v0,0x801F71D4      ; still positive -> keep waiting
801F7120  sb   zero,-0x49b6(v0)   ; stage id 0x8007B64A = 0
801F7128  sh   zero,0x6d6(v1)     ; intro timer = 0
801F712C  sb   zero,0x289(v1)     ; phase = 0
801F7138  addiu v0,zero,0xb
801F713C  sb   v0,0x6(v1)         ; ctx[+0x06] = 0x0B
```

The same block clears the stage id `0x8007B64A` (the `2` that paged this module in,
`966 + id` in extraction space), which is the module signing off. The write is
witnessed live at the row above: **3207 vsyncs** - about 53 s of game time - after
the `0x0C` park, with no input at any point, `ra` naming SCUS `0x800564A0` as the
caller that ticks the module (the side-band pass `FUN_80056208`). So it hands the flow back as `0x0B` - a value the ladder
*does* have an arm for - with the intro timer already zeroed, and `0x0B` expires on its next tick into
`0x14`, which sets `0x1E` unconditionally. That is exactly where the in-fight capture
`cort_evolved_battle_first_menu` sits. Flow `0x0C` is therefore not a dead state: it
is the "a stage module owns this frame" parking value, and what it waits on is that
module's own multi-phase intro, ending on the dt countdown at `0x801F73F8`.

Two cautions for anyone re-running this. Exec breakpoints on slot-B VAs are **not**
attributable on their own - the same addresses are live code in whichever module is
resident. The run demonstrates it: the breakpoint at `0x801F713C` fires six times,
five of them in the first thirteen vsyncs while the 0900 co-resident still held the
slot, and only the sixth is a write to the flow byte. Read those hits beside the
tracker column, and take the data side (the phase byte, the timer constant, the
tracker) plus the image scan as the load-bearing evidence. And the intro is long:
the park outlasts a 3400-vsync capture window, so a run that ends early reads as
"stuck forever" - which is what an earlier reading of this state concluded.

**The countdown is in battle-frame units, not vsyncs.** Every phase arm drains
the module word `0x801F73F8` by the frame step `*(0x1F800393)` and re-arms it with
an immediate: `0x80` (phase 0's exit, `0x801F6B84`), then `+0x100` three times
(`0x801F6CC8`, `0x801F6E00`, `0x801F6F30`), `+0x1E0` (`0x801F6FDC`) and `+0xB4`;
phase 0 itself waits on the camera word `0x800840BC` climbing `4 * step` a pass
to `0xC00`. That is about 2000 units from residency to hand-back. The same
probe extended with a phase-change log (pad-free, from `cort_evolved_pre_battle`)
times each phase in vsyncs:

| Phase | Starts at vsync | Lasts | Countdown units | Step read |
|---|---|---|---|---|
| 0 | 291 (mode `0x15`; the module pages in near 631) | 624 | camera `1280 -> 0xC00` | 4 |
| 1 | 915 | 283 | `0x80` | 3-4 |
| 2 | 1198 | 944 | `0x100` | 4 |
| 3 | 2142 | 660 | `0x100` | 4 |
| 4 | 2802 | 256 | `0x100` | 4 |
| 5 | 3058 | 479 | `0x1E0` | 4 |
| 6 | 3537 | 181 | `0xB4` | 2 |

Phases 4 to 6 run at exactly one unit a vsync. Phases 1 to 3 - the drop, the
descent trail and the landing, which spawn records every eighth frame - run at a
quarter to a half of that: the step the module reads stays at `4` while a pass
takes up to fifteen vsyncs, so the countdown spans more vsyncs than it has units.
That is frame lag in the capture, not a different count. The port runs the same
arithmetic at one unit a tick (`battle_stage_module::arrival_tick`), so its
arrival hands back after about 2000 ticks where this capture took about 3400
vsyncs from mode `0x15`.

### Each surface is a D-pad map

There is no face-button map. Every chip is seated on a **D-pad arm** and its
`s2` test is the **packed direction mask** for that arm (packed = byte-swapped
against the raw BIOS word - the trap
[`s2` is not the pad](#s2-is-not-the-pad-and-how-a-command-commits) catalogues;
an earlier revision of this table read the same masks raw and attributed the
arms to Triangle / Square / Circle / Cross). That is why a **D-pad glyph** sits
at the centre of each cluster (`FUN_801DB8F4(x, y)`, the textured-quad emitter,
drawn every frame of all three states). Capture cross-check from the
`cort_evolved_battle_first_menu` state
(`scripts/pcsx-redux/autorun_battle_item_window_capture.lua`): in the ring, a
single Up press opened the item window within three vsyncs while nineteen
Triangle presses changed nothing.

| State | Packed mask | Chip | Next |
|---|---|---|---|
| `0x1E` | Left `0x8000` / confirm mask `0x800846D0` | `Begin` | `0x28` (or `0x6E`) |
| `0x1E` | Right `0x2000` | `Run` | `0x32` |
| `0x28` | Up `0x1000` (`0x801D1364`) | `Item` (up arm) | `0x3C` |
| `0x28` | Left `0x8000` (`0x801D1404`) | `Attack` (left arm) | `0x78` / `0x5A` / `0x50` by option `0x800846C4` |
| `0x28` | Right `0x2000` (`0x801D136C`) | Ra-Seru magic (right arm) | `0x46` |
| `0x28` | Down `0x4000` (`0x801D1544`) | `Spirit` (down arm) | commit; `0x6E` on the last member |
| `0x28` | cancel mask `0x800846D4` | - | back to `0x1E` on the round's first member, else the previous member's `0x28` ([the ring's cancel](#the-rings-cancel-steps-back-a-member)) |
| `0x78` | Left / confirm mask | `Auto` | `0x5A` (target cursor) |
| `0x78` | Right | `Command` | `0x50` (directional arts entry) |
| `0x78` | cancel mask | - | back to `0x28` |

With the selection widget up (`_DAT_800846C8` / `ctx[+0x275]`), the same table
reads as highlight-then-confirm: the pre-dispatch block walks the highlight on
direction presses and rewrites `s2` to the highlighted arm's mask on the
confirm press, so each handler's direction test doubles as "take the
highlighted chip".

`Attack` is therefore **not** the plain strike: it is the door to the
attack-mode prompt, and option `0x800846C4` decides whether that prompt is shown
(`0`), skipped straight to auto-target (`1`), or skipped straight to the
directional entry (`2`).

### The ring's cancel steps back a member

The cancel mask is the ring handler's first test (`0x801D11B4`, ahead of the
four arms from `0x801D12C0`), and it forks on the step counter `ctx[+0x1F]` -
how many members the cursor has walked past this round, reset to `0xFF` and
stepped once by the round reset `FUN_801D88CC`:

- counter `0`: `FUN_801D388C(2)` and `ctx[+0x06] = 0x1E` - the round's first
  member goes back to `Begin | Run` (`0x801D11D8..0x801D11E4`);
- otherwise `FUN_801D388C(0x10)` (`0x801D1278`), whose case body ends in
  `FUN_801D32BC(1)` (`0x801D4010`): the cursor scans down to the previous
  member with `+0x14C != 0` and no `+0x16E & 0xF84` bit, and the flow stays
  on `0x28` - now that member's ring. If the member it lands on had
  committed an item (`+0x1DE == 1`), the copy its commit consumed goes back
  through `FUN_800421D4(+0x1DF, 1)` (`0x801D12AC`).

Nothing clears the landed member's commit: the ring simply takes its next
choice over the old one. The commit-confirm screen `0x6E` has the other
backward entry - its cancel or `Reselect` (the Right arm) keys cue `0x23`,
re-scans with `FUN_801DBA04` and re-enters `0x28` through
`FUN_801D388C(0x21)`, whose body steps back with the same `FUN_801D32BC(1)`
(`0x801D3040..0x801D30C8`, `0x801D4750`).

The port runs the ring's cancel as `World::step_back_battle_command` over the
ported cursor step (`legaia_engine_vm::battle_cursor_pose::step_actor_cursor`),
dropping the landed member's typed commit and its Spirit stance and refunding
an item. It stages no `0x6E` screen - the last commit begins the round - so the
`Reselect` entry has no seat.

### Where the words come from

Two pools, and which one a label lives in follows from who writes it into the
placement record's `+0x14` payload pointer. Parser
`legaia_asset::battle_ui_strings`; the coordinates are pinned, never the text.

| Chip | Record | Source |
|---|---|---|
| `Begin` | 0/1/2 | `SCUS_942.54` `0x8007B688`, static on the disc |
| `Run` | 3/4/5 | `SCUS_942.54` `0x8007B684`, static |
| `Item` | 8 | `SCUS_942.54` `0x8007B67C`, static |
| `Attack` | 9 | `SCUS_942.54` `0x8007B674`, static |
| magic | 10 | overlay, written at runtime - see below |
| `Spirit` | 11 | overlay `0x801F4B98`, written by `0x801D8F98` |
| `Auto` | 85 | `SCUS_942.54` `0x8007B658`, static |
| `Command` | 84 | `SCUS_942.54` `0x8007B660`, static |
| `Reselect` | 19/20/21 | `SCUS_942.54` `0x800152D4`, static |

Each disc-static record's seats are the pinned rects the packet walk already
measured: record 1 lives at `(104, 88)` and record 4 at `(180, 88)` with content
width `36`, which is exactly `CLUSTER_TOP_LEVEL`; records 8..=11 sit at
`(204, 34)` / `(160, 66)` / `(248, 66)` / `(204, 98)` with width `48`, which is
`CLUSTER_COMMAND`'s four arms.

**The magic arm is not labelled `Magic`.** `0x801D8F30` reads the acting slot's
character id out of `DAT_8007BD10 + ctx[+0x13]` and indexes a 10-byte-stride run
at `0x801F4B9E`, so the word on the chip is the character's **Ra-Seru**: `Meta`
(Vahn), `Terra` (Noa), `Ozma` (Gala). Index `4` of the same run is a single `-`,
which the `ctx[+0x25F + slot]` gate above it selects for a character with no
Ra-Seru magic - the disc's own instance of the "an unavailable command keeps its
plate and draws a dash" law.

### The formation banner

`FUN_801D9D3C`'s arm at `0x801DA234` reads `ctx[+0x290]` and picks the line it
stores into placement record 67 before the intro timer runs:

| `ctx[+0x290]` | Line | Consequence |
|---|---|---|
| `0` | none - the draw at `0x801DA2E4` is skipped | ordinary round |
| `1` back attack | `0x801F4D10` | `0x0B` jumps to `0xFE`: the party enters **no** command that round |
| `2` pre-emptive | `0x801F4CD8` / `0x801F4CF8` | ordinary round; the monsters sit it out |

The singular / plural pick at `0x801DA274` tests the byte at `DAT_8007BD10 + 1`
(present-party slot 1), so a party with nobody there gets the shorter line. The
name is substituted into the `0xC1` token by `FUN_8003CBF8`, whose operand is
`DAT_8007BD10[0] - 1` - the party **leader**, not the acting member.

Record 67's role here is only to *hold* that pointer: the draw at `0x801DA2E4`
passes the line to `FUN_8003541C` with immediates, never through the record, and
the same string is re-raised from record 67 proper once the intro is over. The
whole intro surface - the enemy-name labels this line sits above, their seats
and their lifetime - is
[the battle-intro enemy-name banner](battle-hud.md#the-battle-intro-enemy-name-banner).

### Port

`engine-core::battle_input` carries the three phases (`CommandPhase::RoundPrompt`
/ `Menu` / `AttackMode`) and `engine-ui::battle_command_ui::ChipPhase` the
seating for each; `engine-core::battle_open` composes the banner and
`World::raise_battle_open_banner` queues it onto the shared battle message box
(retail's `ctx[+0x6B2]` surface). The round-scoped prompt is armed from
`World::arm_round_open_prompt`, keyed on the flow byte parking at
`BattleFlowState::TurnPrompt` - which the round boundary and battle entry both
set, and which a mid-round reopen does not. The ambush's lost round is already
the `ctx[+0x290]` side lockout in `World::reseed_initiative`.

The port follows retail's **direct-commit press**: a direction press takes the
chip drawn on that side of the screen in the same frame, no confirm. The map is
spatial, mirroring retail's own per-arm dispatch: on the ring Up commits
`Item`, Left `Attack`, Right the magic arm, Down `Spirit`
(`battle_input::ring_seat`), and on both two-chip prompts Left is always the
left chip and Right the right chip (`battle_input::pair_seat`). Cross
additionally commits whatever the cursor rests on - the route a scripted
harness that cannot aim a direction drives - and Circle keeps its back-out /
outright-`Run` roles. `engine-shell`'s
`direction_presses_land_on_the_chip_drawn_on_that_side` holds the map equal to
the drawn seating.

### The command-flow byte `ctx[+0x06]` - what the hook table indexes

The hook key is **not** the action SM's `ctx[+0x07]`. It is `ctx[+0x06]`, the
cursor of the *other* battle state machine: the menu half, `FUN_801D0748`. Both
are byte cursors over the same context struct, and their value spaces collide -
`ctx[7] == 0x64` is `RunBegin`, `ctx[6] == 0x64` is target confirm.

**They do not share a dispatch shape, and it is worth not carrying the opposite
forward.** `FUN_801D0748` has no jump table at all: it dispatches `ctx[+0x06]`
through a binary-search `beq`/`slti` comparison tree at
`0x801D0C84..0x801D0DC8`, and the only `jr` in its 2781 instructions is the
`jr ra` at `0x801D32B4`. The `jr`-table shape belongs to the tutorial hook
`FUN_801F6B70` (`jr v0` at `0x801F6BF8`) and to the action SM `FUN_801E295C`,
not to the menu SM. Reading the menu half as table-driven invents a dense index
space it does not have - its live cases are exactly the 22 constants below,
everything else falling to the default at `0x801D3290`.

Below `0x1E` the command flow is battle entry and turn setup: `0x00` init,
`0x0A`/`0x0B` the intro timer at `ctx[+0x6D6]`, `0x14` turn start (which opens
the top menu and falls into `0x1E`). From `0x1E` up it is the player's command
selection, and the states are regular decimal multiples of ten:

| `ctx[+0x06]` | Handler | On screen | Leaves to |
|---|---|---|---|
| `0x1E` = 30 | `0x801D102C` | `[Begin]` / `[Escape]` turn prompt | `0x28`, `0x32`, `0x6E` |
| `0x28` = 40 | `0x801D1188` | Action-category menu | `0x1E`, `0x3C`, `0x46`, `0x50`, `0x5A`, `0x6E`, `0x78` |
| `0x32` = 50 | `0x801D10F8` | Flee confirm | `0x1E`, `0xFE` |
| `0x3C` = 60 | `0x801D17DC` | Item window | `0x28`, `0x5B`, `0x5D`, `0x64` |
| `0x46` = 70 | `0x801D19F8` | Magic window | `0x28`, `0x5C`, `0x5E`, `0x65`, `0x67` |
| `0x50` = 80 | `0x801D1D84` | Arts command-entry screen | `0x28`, `0x5A`, `0x78` |
| `0x5A` = 90 | `0x801D21CC` | Target cursor | `0x28`, `0x50`, `0x6E`, `0x78` |
| `0x64` = 100 | `0x801D2A00` | Target confirm (item window's own) | `0x28`, `0x3C`, `0x6E` |
| `0x6E` = 110 | `0x801D3024` | All members committed - begin | `0x1E`, `0x28`, `0xFE` |
| `0x78` = 120 | `0x801D16E8` | Auto / Command attack-mode prompt | `0x28`, `0x50`, `0x5A` |

**How to read the "Leaves to" column.** It is the exhaustive set of
`sb <reg>,0x0(s3)` stores inside each handler's address range (`s3 = ctx+6`,
loaded at `0x801D0780`), resolved by constant propagation over the `li` / `move`
/ `clear` that feed the stored register - not a per-branch narration. Every
handler can also fall through without storing, which is the implicit "stay put".
One earlier reading does not survive that sweep: state `0x46` never stores
`0x6E` - that was a nested-`if` rendering, not a store.

A sweep of this kind has to read **branch delay slots**, or it under-counts. Two
edges live only there. State `0x28`'s Left/Attack arm does reach `0x50`
directly: `0x801D15F4 beq s0,v0,0x801D1650` with `0x801D15F8 _li v0,0x50` in the
slot, and `0x801D1650 sb v0,0x0(s3)` has that branch as its only predecessor -
so option `_DAT_800846C4 == 2` goes straight to the arts screen rather than
through the `0x78` prompt. State `0x46` reaches `0x5E` the same way
(`0x801D1C5C bne` / `_li v0,0x5e` in the slot / `0x801D1CDC sb`), which is what
supplies the `0x5E` the sub-cursor run `0x5B..0x5E` needs.

Above the selection band sit the per-window target sub-cursors. They are two
disjoint runs, `0x5B..0x5E` and `0x64..0x67` - there is no case for
`0x5F..0x63`, and treating the sub-cursors as one contiguous `0x5B..=0x67` range
invents five states. `0xFE` is a real dispatched case ("round armed - run the
action SM"). `0xFF` (idle) is **not**: no comparison tests for it, so it reaches
the default at `0x801D3290` like every other unlisted value - idle by falling
through rather than by being handled.

That band is what pins the tutorial's table. Its nine live slots are exactly
these ten states **minus the magic window** - the sparring fight teaches attacks,
items, spirit and hyper arts, and never magic. Engine mirror
[`engine-core::battle_flow`](../../crates/engine-core/src/battle_flow.rs), which
carries that cross-check as a test.

### The round loop - what re-arms `0x1E`

`0x14` is the round-start arm and the **only** writer of `0x1E`:

```text
801d0ec4  lw    v1,-0x42dc(s0)      ; ctx
801d0ecc  sw    v0,0x880(v1)        ; highlight cursor = 0x8000 (the Left arm)
801d0ed0  jal   0x801d88cc          ; per-round actor sweep
801d0ed4  _sb   s5,0x0(s3)          ; ctx[+0x06] = 0x1E   (s5 = 0x1E at 0x801D0C98)
801d0ee4  jal   0x801d388c          ; open the prompt window (a0 = a1 = 0)
801d0ef4  lbu   v0,0x28a(v0)        ; round index
801d0efc  beq   v0,zero,0x801d0f0c  ; round 0 only: the tutorial arm below
```

The store is unconditional - no arm of `0x14` skips it - so **every round the
player is given starts on `Begin` / `Run`**, and the ring `0x28` is only ever
entered from `0x1E`'s confirm at `0x801D108C`. A port that opens its command
surface on the ring is not one frame early, it is a different machine.

`0x14` is reached from two different state machines:

- **Battle open.** The intro timer `0x0B` runs down and branches on the
  back-attack byte: `ctx[+0x290] == 1` stores `0xFE` (the party loses its
  first round outright), anything else stores `0x14`
  (`0x801D0E68..0x801D0EB8`).
- **Every later round.** The *action* SM's `ctx[+0x07] == 0xFF` arm, jump-table
  slot `0xFF` of `0x801CED44`. Its whole body is two writes:

```text
801e67e8  lui   a0,0x8008
801e67ec  lw    v1,-0x42dc(a0)
801e67f0  li    v0,0x14
801e67f4  sb    v0,0x6(v1)          ; ctx[+0x06] = 0x14  -> next round's prompt
801e6800  lbu   v0,0x28a(v1)
801e6808  addiu v0,v0,0x1
801e680c  jal   0x801f45a4
801e6810  _sb   v0,0x28a(v1)        ; ctx[+0x28A] += 1   (the round index)
```

`ctx[+0x07] = 0xFF` is stored at `0x801E67E4`, on the arm where the per-round
action cursor has passed every living actor. So the two bytes hand the round
back and forth: the flow SM ends a round by arming `0xFE` -> `0xFF`, and the
action SM ends it by arming `0x14`.

**Read this one off the jump table, not off the decompiler's flow analysis.**
Nothing inside `FUN_801E295C` branches to `0x801E67E8` - it is reached only
through the `jr v0` at `0x801E2AAC` - so a pass that does not resolve the table
reports `Removing unreachable block (ram,0x801E67E8)` and drops the round bump
from the C entirely. That `+0x28A` is the round index rather than some other
counter is corroborated by `0x14`'s own second reader: under
`_DAT_8007BD0C == 0xB6` (the Muscle Dome match) `0x801D0F94..0x801D0FA4` draws
`4 - ctx[+0x28A]`, the rounds remaining.

### `s2` is not the pad, and how a command commits

Every handler in `FUN_801D0748` tests `s2`, and `s2` is built two different ways
before the state switch runs.

The masks are **packed** throughout - byte-swapped against the raw BIOS word
(`engine-core::world_map_panel_host::packed_pad`), so the four directions are
Left `0x8000`, Right `0x2000`, Down `0x4000`, Up `0x1000`. Read raw they look
like face buttons, which in turn makes the confirm and cancel masks look
unreachable; the same trap is catalogued in
[`arts-command-gauge.md`](arts-command-gauge.md).

**With a selection widget up** (`_DAT_800846C8 != 0` and `ctx[+0x275] != 0`), the
pre-dispatch block at `0x801D07FC..0x801D0AC0` walks a highlight rather than
handing the press down. A pressed direction stores that mask in `ctx[+0x880]`
and stamps `+0x1D = 2` on the matching widget actor - `ctx[+0x1114]` Left,
`+0x1118` Right, `+0x111C` Up, `+0x1120` Down - with every other actor set to
`1`; `ctx[+0x275]` is how many arms exist, and the Up and Down arms are skipped
below `3` and `4` (`sltiu` guards at `0x801D0A24` and `0x801D096C`). Then
`0x801D0AC4..0x801D0B08` **rewrites `s2` outright**: the confirm mask
`_DAT_800846D0` replaces it with the stored `ctx[+0x880]`, the cancel mask
`_DAT_800846D4` replaces it with itself, and anything else leaves zero. So a
handler below sees a direction bit only on the frame confirm is pressed, which
is what turns its direction tests into "take the highlighted chip".

**Without one**, `0x801D0B0C` builds `s2 = _DAT_8007B874 | _DAT_8007B938`, the
plain packed pad, and the direction tests are direct presses.

`0x14` seeds `ctx[+0x880] = 0x8000` (`0x801D0ECC`), so a freshly armed prompt is
highlighted on its Left arm.

Which `s2` bit routes where, in the three prompt states:

| State | Left `0x8000` | Right `0x2000` | confirm `_DAT_800846D0` | cancel `_DAT_800846D4` |
|---|---|---|---|---|
| `0x1E` | Begin | Run -> `0x32` | Begin | - |
| `0x32` | run confirmed -> `0xFE` | back to `0x1E` | - | back to `0x1E` |
| `0x6E` | begin the round -> `0xFE` | step back | begin the round | step back |

`0x32`'s confirm arm stamps `+0x1DE = 5` (the Run action category) on all three
party actors at `0x801D1174..0x801D1184` before storing `0xFE`.

The ring's four arms sit on the same four masks, and every one of them commits
through the same idiom - **advance to the next member that still owes a
command, or raise the [commit confirm](#the-commit-confirm-screen-0x6e)**:

```text
801d16ac  jal   0x801db81c          ; next member after ctx[+0x13] awaiting a command
801d16b4  lw    v1,-0x42dc(s6)
801d16bc  lbu   v1,0x0(v1)          ; ctx[+0x00] = seated party count
801d16c4  bne   v0,v1,0x801d16d8    ; someone still owes one -> stay in 0x28
801d16cc  li    v0,0x6e
801d16d0  sb    v0,0x0(s3)          ; nobody does -> 0x6E
```

Ten sites in the handler share it, one per commit path: Spirit at `0x801D16AC`,
the target-cursor confirm at `0x801D22C4`, and the per-window target
sub-cursors at `0x801D24B4`, `0x801D2698`, `0x801D2830`, `0x801D29C0`,
`0x801D2AAC`, `0x801D2D74`, `0x801D2E64` and `0x801D2FE4`. `FUN_801DB81C` scans
forward from `ctx[+0x13] + 1`; its sibling `FUN_801DBA04` scans from zero and is
what `0x1E`'s confirm and `0x6E`'s cancel call. Both skip a member whose
per-member state byte `_DAT_8007BD10[i]` is already `4` (committed), whose live
HP `+0x14C` is zero, or whose status word `+0x16E & 0xF84` is set, and both
return `ctx[+0x00]` when none is left.

**No command path leaves the flow parked.** All ten end in `0x28` or `0x6E`,
which is the invariant a port has to keep: a command that resolves without
arming the next surface is a soft-lock, and it does not have to be the command
itself that breaks - see the readout desync in
[`battle-action.md`](battle-action-exit-gates.md#the-0x51-exit-gate-and-the-hp-bar-settle-invariant).

### The commit-confirm screen (`0x6E`)

Every commit site above stores `0x6E` instead of `0x28` once `FUN_801DB81C`
comes back equal to the party count, so the screen is raised after the
**last** member that can act has committed, for every party size - a solo
party reaches it off its only member's command. No option word gates it: the
only option the review arm consults is `_DAT_800846C8` (the Battle Command
setting), and only to decide whether Up / Down count as a confirm. When no
member can act at all, the round prompt's `Begin` stores `0x6E` directly
(`0x801D10A0`, step `0x27`); the port takes the same arm
(`World::tick_battle_command`, after `World::begin_battle_round` opened the
prompt on member 0).

The arm at `0x801D3024` draws the D-pad glyph at `(152, 84)`
(`FUN_801DB8F4(0x98, 0x58)`) between two chips - placement record `0x10`
(content `(92, 88)`, width `48`, its word stamped from the overlay pool by the
round prompt's `Begin` arm at `0x801D1060`) and record `0x13` (`(180, 88)`,
payload `Reselect` at SCUS `0x800152D4`). The `party_basic_attack_vs_gobu_gobu`
capture's display list holds exactly those plates, `(84, 82)` and
`(172, 82)`, `64 x 20` each.

It splits the pad as the [`s2` table](#s2-is-not-the-pad-and-how-a-command-commits)
says: Left or the confirm mask stores `0xFE` and plays the round out;
Right or the cancel mask is `Reselect`. `Reselect` calls
`FUN_801D388C(0x21)`, whose tail is `FUN_801D32BC(1)` (`0x801D4750`) - a
**one-member** backward step - and stores `0x28`; the dispatcher then
refunds the landing member's item if it had committed one
(`FUN_800421D4(+0x1DF, 1)`, `0x801D30BC`). The step lands on the **last**
member that can act, not the first, because by `0x6E` the forward walk has
already moved the cursor past the party: the capture holds
`ctx[+0x13] = 1`, `ctx[+0x1F] = 1` on a one-member party. With nobody able
to act, `FUN_801DBA04` equals the count and the press returns to `0x1E`
(`0x801D30D0`).

Retail also keeps a **commit log** up through the whole command phase - see
[the commit log](#the-commit-log) below.

**Port.** `battle_input::CommandPhase::CommitConfirm` (step
`step_commit_confirm`); `World::open_commit_confirm` raises it where
`commit_party_command` used to begin the round, and
`World::reselect_battle_commands` is the `Reselect` step (the same
`FUN_801D32BC(1)` kernel the ring's cancel uses, started from the party
count). Both hosts draw it through `battle_hud::battle_command_chips`
(`CommandChipPhase::CommitConfirm`) and the shared chip cluster
`legaia_engine_ui::battle_command_ui::CLUSTER_COMMIT_CONFIRM`, with the chip
words read off the disc. The arts entry no longer carries a `Begin | Reselect`
of its own: that was this party-wide screen modelled per member, which put it
after every member's arts and after no one's Magic or Item.

### The commit log

Each committed member gets a row of three placement records at `0x2B + 3n`
(name, command, target), `n = ctx[+0x1F]` - the command cursor's step
depth. The commit arms of `FUN_801D388C` stage them: case `0x20` (Attack)
at `0x801D4444`, case `0x11` (Spirit, a member other than the last, with the
forward step) at `0x801D4020`, case `0x23` (Spirit, the last member) at
`0x801D4918`. Each arm:

1. lands the three elements with `FUN_801D5718(dst, src)` - name from the
   acting plaque (record `0x1A`), command from the chosen ring chip (record
   `0x0D` `Attack` on case `0x20`, record `0x0B` `Spirit` on the other two),
   target from the target plaque (record `0x29`); the Spirit arms point the
   target element at an empty string of width `0` instead;
2. seats the columns off the name's measured width: name at `x = 16`,
   command at `name_w + 0x20`, target at `name_w + 0x60`;
3. scrolls the log so the newest row sits lowest - one row rests at
   `y = 170`, a second moves the first to `146` and takes `170`, a third
   takes `194`.

`FUN_801D5718` copies `+0x02 <- src+0x0A`, `+0x04 <- src+0x0C`, `+0x06`,
`+0x0A` and `+0x14`, and never `+0x0C`: the element starts where its source
rests and the arm writes the landing row itself. When the target cursor
covers a whole side, `FUN_801D57E8` has already swapped record `0x29`'s
content for record `0x3D` (`"  All"`, width 36) or `0x3E` (`"All Allies"`,
width 48), so an all-target commit logs that label. The `0x6E` screen's
`Reselect` (case `0x21`, `0x801D45A8`) parks the landing member's row at
`x = 328`.

The log is **launched** - slid one display width off the left edge - when the
member leaves the ring for a sub-screen, and slid back when they return, not
when the round begins. `FUN_801D388C`'s shared tail dispatches through a
second jump table at `0x801CE948` (indexed by `step - 5`), and the steps that
land on its two launch loops (`0x801D50A0` / `0x801D50F8`) are `0x05` (item
window), `0x07` (magic window), `0x09` (arts entry under the `Command`
option), `0x2A` (the `Auto | Command` prompt), `0x30` (target cursor under the
`Automatic` option), `0x2B` (prompt cancelled), `0x31` (target cursor
cancelled) and `0x08` (magic window cancelled). Each loop runs
`FUN_801D5778` over `i` in `0..3*ctx[+0x1F]` - record `0x2B + i` into
`0x35 + i`, seat A the element's resting seat and seat B one display width
left - and opens every clone with `FUN_801D8DE8(0x35 + i, a1)`, where `a1` is
the step's own mode argument: `0` on the five outbound steps (spawn at A,
glide to B), `1` on the three returns (spawn at B, glide home). The step's
script has already reset the handle list (`FUN_801D99BC`), so the clones are
the only log on screen. The glide is `FUN_801D9BBC`'s linear step over
`ctx[+0x1C]` frames, which the round reset `FUN_801D88CC` seeds to `0x10`. The
Begin confirm's steps `0x24` / `0x29` take the tail table's plain exit - the
log leaves with the command phase, it does not slide.

The capture `party_basic_attack_vs_gobu_gobu` (solo Vahn at `0x6E`,
`ctx[+0x1F] = 1`) holds records `0x2B` / `0x2C` / `0x2D` at seat B
`(16, 170)` / `(59, 170)` / `(123, 170)` - `Vahn` (width 27), `Attack`,
`Gobu Gobu` - which is the one-row case of the layout above.

**Port.** `legaia_engine_vm::battle_commit_log` stages the rows over a
scratch placement array (`stage_commit_row`, with `FUN_801D5718` as
`battle_cursor_pose::element_placement_land` and `FUN_801D57E8` for the
all-target labels); `engine-core::battle_hud::battle_commit_log` lists the
members the cursor has walked past - all of them once `0x6E` is up - from
`RoundFlow::pending`; and `engine-ui`'s battle HUD builder draws each element
as a gold plate (kind `2`) with its text at the pen `(x, y - 2)`. Both hosts
pass the rows through `BattleHudFrame::commit_log`. A commit draws each
element at its resting seat (the landing glide is not modelled); the launch
is - `battle_commit_log::LogLaunch` builds the clone with `FUN_801D5778` and
steps `FUN_801D9BBC`'s glide, `engine-core::world::battle::commit_log_launch`
raises it on the engine's ring transitions (the prompt, the `Automatic` target
cursor, the arts entry, the item and magic windows out; a cancel or a
sub-screen backed out of in), and every row carries the offset as
`CommitLogRow::slide_x`. Which chip an Item or magic commit logs
(records `0x0C` / `0x0E`) and whether an Item row carries a target are
inferred from the ring's arm order, not read off a commit arm.

### How the engine raises the flow state

The engine splits what `FUN_801D0748` does in one machine across a
[`battle_input::BattleCommandSession`](../../crates/engine-menus/src/battle_input.rs)
plus host-owned Item / Magic / Arts submenus, so the flow byte is *recomposed*
each frame by `battle_flow::flow_state_for` (an open submenu wins over the
command phase). The round around them is retail's own two bands - every
member commits before anyone acts, and the commits execute in initiative
order - see [the two bands](battle-round-loop.md#auto-resolve-vs-player-driven). Three points
differ from retail and are deliberate:

- **Round prompt.** `World::open_battle_command` builds the session **already
  on** `CommandPhase::RoundPrompt` whenever the flow byte says the round is
  opening (battle entry leaves it `Idle`; the round boundary parks it on
  `TurnPrompt`), matching retail's unconditional `0x14 -> 0x1E` store. It has
  to be the phase the session is constructed in rather than one applied on a
  later tick: `battle_command.is_some()` is the only edge a host or a test
  has, so a prompt that lands one frame behind it is a prompt nothing sees -
  and `Run` lives on that prompt and on no other surface. A session reopened
  mid-round (a submenu backed out of) finds the flow on a window state and
  opens on the ring, which is where retail's own cancel arms land.
- **Target confirm.** `CommandPhase::Confirmed` is the Attack path, which retail
  routes `0x5A → 0x28` for the next member or `0x5A → 0x6E` after the last; state `100` is the item window's own target step and has
  no engine hook point yet.
- **Every commit meets the `110` validator.** The handler reads the category
  off the active actor (`lbu v1,0x1de(v0)` at `0x801F70E0`), so each of the
  four committing surfaces reaches it with its own byte: the Attack target
  confirm with `3`, the arts entry's target confirm with `3`, Spirit with
  `4`, and the item window's use with `1` (`World::tick_battle_item_menu`,
  checked before the copy is consumed). A surface that skipped the validator
  leaves its lesson unaccepted forever and the spar never ends - the item
  window once did, and so did the arts entry, which is the only way to
  perform the hyper-arts lesson's Somersault.
- **The arts entry raises its own two states.** The entry opens on `80`;
  leaving it (the confirm, or the press that exhausts the gauge) is retail's
  `0x50 -> 0x5A`, which `World::tick_battle_arts_input` raises as `90` with
  the entered arrows written into the hook's command buffer as the gauge's
  swing bytes; the review's cancel is `0x5A -> 0x50` and re-raises `80`.
- **Unresolved surfaces can rewind too.** The target cursor (`90`) and the
  attack-mode prompt (`120`) carry wrong-lesson rewinds; the engine honours
  them as it does a resolved commit's, by reopening the command menu.
- **Lesson counter.** Retail shares `ctx[+0x28A]` with the action SM, where the
  sparring fight's scripted `case 0xFF` bumps it. The engine has no script driver
  for that fight, so `BattleTutorial::pending_advance` bumps the lesson when the
  commit hook *accepts* the taught category - one lesson per successful player
  turn, which is the same observable cadence.

The recomposition has to run on the frame a window **opens**, not only while
the command session is unresolved: the resolution that hands off to a submenu
consumes the session, and a byte synced only from the session stayed on the
surface the player left (`0x28`, or `0x78` for the arts entry) for as long as
the window was up. Retail stores the window's own state as it opens - `0x50`
at `0x801D1738`, in the delay slot of the arts preseed `jal FUN_801DA34C` -
and the engine syncs it at the same point
(`World::tick_battle_command`).

A queued box parks the whole battle tick (`World::live_battle_tick` returns
early), which is the port of retail returning before it reads the flow state
while `FUN_801D9BBC` reports a box up (`ctx[+0x6B2]`). A hook that takes the
rewind exit discards the action and reopens the command menu.

**No host arms it.** `World::enter_battle` consumes the disc's own one-shot
system-flag arm (above), so the native window and the browser play page both
show the boxes in the fight retail shows them in, with no scene name, flag or
environment variable in the condition. `World::prime_battle_tutorial` is a
debug force, and `LEGAIA_BATTLE_TUTORIAL` (`0` suppress / `1` force / `now`
force and enter a fight) is `play-window`'s hand-testing knob on top of it -
neither is the port. Browser oracle:
`crates/web-viewer/tests/battle_tutorial_page.rs`.

The `asset-viewer battle-scene` subcommand drives the engine-side composite end-to-end: loads the same battle bundle TMDs, builds an `engine-core::World` in `SceneMode::Battle`, spawns 3 party + 5 monster actor slots, and ticks the [battle-action state machine](battle-action.md) per frame. HUD shows the current `ActionState` (decoded into the named variant), queued action, per-slot liveness, transition counts, and any `BattleEndCause` the SM emits. Triangle cycles `queued_action`; Cross re-seeds at `ActionState::Begin`.

## Battle target picker

Drives the post-action target cursor. Parameterised on a `TargetKind` enum constraining valid targets:

| TargetKind | Allowed targets |
|---|---|
| `SingleEnemy` | One alive monster slot. |
| `SingleAlly` | One alive party slot, **excluding** the actor. |
| `SingleAllyOrSelf` | Any alive party slot, including the actor. |
| `DeadAlly` | One fallen party slot (Revive / Resurrection). |
| `AnyAlly` | Any party slot, alive or dead. |
| `AllEnemies` / `AllAllies` | Sweep target - auto-confirm. |
| `Self_` | The actor itself - auto-confirm. |

Sweep kinds resolve in `init_cursor`; single-target picks walk valid candidates with cursor-wrap and auto-skip-dead. Implementation: [`crates/engine-core::target_picker`](../../crates/engine-battle/src/target_picker.rs).

The **enemy** row is not a slot-order walk. Each picker row carries the slot's battle-world seat (`actor[+0x34]` / `+0x38`, filled by `World::battle_target_rows` from the actor's `move_state`), and a `SingleEnemy` cursor steps through retail's attack-target ring - `FUN_801D8A88` builds the ring and `FUN_801D8D00` steps it, so Left/Right move to the *angularly* nearest live monster. Retail seats at most four monsters, so a fifth engine slot has no ring entry; that slot, an un-seated host (all seats at the origin), and a ring entry that is not a live monster each fall the cursor back to the plain scan. See [`battle-action.md`](battle-action-helpers.md#actor-pool-leaf-helpers) for the two kernels.

A sweep reaches the action SM as retail's target-group code, not a sentinel: the live command flow writes `+0x1DD = 8` for the party and `9` for the enemy row (absolute numbering, mirrored for a monster caster), and a self-target writes the caster's own slot - the values `FUN_801E295C`'s cast-begin split (`sltiu v0,t2,0x8` at `0x801E433C`) and self-skip (`beq v0,t2` at `0x801E4350`) decode.

### The sparring-tutorial prompt machine (overlay 967)

What overlay 967 *does* is emit the in-battle "how to fight" boxes of the Tetsu
sparring fight. The hook table and every prompt string address are resident in
967, and neither the battle-scene script, MES text, nor the battle overlay
`0898` carries them - which is why porting the battle SM alone never produces
the boxes.

**Who fights the spar.** Retail has no party override for it. Battle init
seats one actor per non-zero id in the present-party list `DAT_8007BD10`
(`FUN_80052FA0` counts them into `ctx[+0]`, `FUN_800513F0` loads each), and
neither routine reads the stage id while doing so; the spar is Vahn against
Tetsu only because the story's party is Vahn alone at that point. The port
makes it a rule: `World::enter_battle_from_formation` seats Vahn's record
alone whenever the fight is the spar (`World::sparring_fight_pending` - the
disc's arm flag or a forced tutorial), whatever the field party holds - a
`--party` debug party or a save with a fuller one - and
`World::finish_battle` hands the field composition back. The lessons walk
one member's command flow, so a fuller party would sit idle through prompts
that never address it. No disc patch is needed for the randomizer: no
`legaia-patcher` feature edits the party composition, so a patched disc
reaches the spar with the retail party.

**The machine's exclusivity is byte-anchored.** `FUN_801F6B70` is entry 967 file
`+0x198`, and `0x801F69D8 + 0x198` reproduces the printed VA exactly, so the
needle can be taken straight out of the image rather than hand-assembled.
Searching for it across all 1233 `PROT` entries, `SCUS_942.54`, `DMY.DAT` and
the extracted overlay images returns **one** physical copy - at five needle
lengths from 48 bytes to the whole 2316-byte body, and including a 116-byte
interior window that contains no `lui`, `j` or `jal` and would therefore still
match a copy relinked at a different base. (The other two hits are that copy
seen twice more: the static-overlay-pipeline duplicate, byte-identical to the
entry, and the entry's own bytes inside `PROT.DAT` at its LBA. PROT 0967 is
stored raw - the sector slice equals the extracted file - and every row in
`static-overlays.toml` is `form = "raw"`, so no code image on this disc hides
inside LZS.)

**The prompt pool is not exclusive, and its neighbour is why.** All 28 string
pointers the machine forms land inside 967's own `0x1800`-byte image, in a
30-string pool at file `0xCAC..0x1389`. But entry **0968** carries a
byte-identical 852-byte prefix of that pool at the same offset, inside a
`0x5D8`-byte run (`0xA28..0x1000`) it shares with 967 - a run that also covers
the machine's own 124-byte epilogue. 0968 is a sibling slot-B image at the same
base with its own, 7-entry dispatcher and no copy of the machine
(`sltiu v0,v1,0x5b` appears in 967 at file `0x200` and nowhere in 0968). So
"only in 967" is exact for the tick and its dispatch, and needs the 0968
qualification for the text.

Its tick `FUN_801F6B70` is a jump-table hook on the battle **flow-state byte**
`ctx[+0x06]` (`ctx = _DAT_8007BD24`), not a linear script:

```
ctx[0x6B0] = 0                           // sh zero,0x6b0(v1) @ 0x801F6BB8
if ctx[0x6B2] != 0  -> suppressed        // bnez @ 0x801F6BB4 - a box is up
if ctx[0x6AE] != 0  -> already emitted   // bnez @ 0x801F6BC4 - one-shot latch
idx = ctx[0x06] - 0x1E                   // 91-entry table at 0x801F69D8
if idx >= 0x5B      -> no-op             // sltiu 0x5b @ 0x801F6BD8
goto table[idx]                          // jr v0 @ 0x801F6BF8
```

The `ctx[0x6B0]` clear is written first here on purpose: it lives in the
**branch delay slot** of the suppression test, so it executes on both paths -
including the suppressed one. Ghidra's C prints it after the guard, which is the
reordered-store artifact.

Only **nine** of the 91 slots are live - flow states `30, 40, 50, 60, 80, 90,
100, 110, 120`; the other 82 point at the shared no-op tail `0x801F718C`. The
table decodes straight out of the disc image: it begins at overlay file offset
`0`, since its base `0x801F69D8` *is* the overlay load base.

Each live handler then switches on `ctx[+0x28A]`, the same byte the
battle-action SM's `case 0xFF` increments (ported as
`World::advance_battle_mode`), which the tutorial reads as the **lesson index**:
`0` attacks, `1` items, `2` spirit, `3` hyper arts, `4` → done. The script is
therefore a `(flow state × lesson)` cross-product, with a "you're learning about
X now! Try again!" rewind (`FUN_801F7628`) whenever the player picks the action
the current lesson is not teaching.

| Flow state | Handler | What it prompts |
|---|---|---|
| `30` | `0x801F6C00` | Turn start - the per-lesson intro, plus a first-visit vs repeat-visit input explainer selected by `_DAT_801D46C8`. |
| `40` | `0x801F6CB8` | `[Begin]` chosen - name the category to pick. Lesson 3 has no prompt here. |
| `50` | `0x801F6CAC` | Run selected - always rejected, always rewinds. |
| `60` | `0x801F6DCC` | Item window opened - the item lesson explains the two windows; every other lesson rewinds. |
| `80` | `0x801F6E4C` | Arts command-entry screen - combo hint (lesson 0) or the drill instruction (lesson 3). |
| `90` | `0x801F6EE4` | Target select; for lesson 3 it first validates the entered command buffer. Reads seat 0's Auto flag `ctx[+0x266]`. |
| `100` | `0x801F7060` | Target confirm - unconditional, lesson-independent. |
| `110` | `0x801F7088` | Validates the committed `actor[+0x1DE]` category against the lesson (`3` attack, `1` item, `4` spirit; hyper arts expects `3`, since it is reached through Attack). |
| `120` | `0x801F6D30` | The Auto / Command attack-mode prompt - free choice for lesson 0, forced `[Command]` for lesson 3. |

The hyper-arts drill at flow state `90` asks for `[High] [Low] [High]`
(`0x0F, 0x0E, 0x0F`) and accepts it at three alignments of the command buffer
`actor[+0x1DF..=+0x1E3]`, each a differently-masked load at `0x801F6FD8`. When
`_DAT_801D46C4 == 1` the buffer is auto-filled for the player at `0x801F6FB0`.
`[High] [Low] [High]` is the swing bytes `0F 0E 0F` - Vahn's Somersault,
`Up Down Up` - so a Somersault preceded by up to two other arrows passes too.

The handler reads `ctx[+0x266]` first (`0x801F6F48` / `0x801F6F78`). That byte
is seat 0's per-fighter **Auto** flag (written on the attack-mode prompt; see
[`battle-action.md`](battle-action.md)), not a "seen" latch: in lesson 0 it
moves the follow-up box from the `0xB0` anchor (style `5`) to the `0xCC` one
(style `3`), and in lesson 3 an Auto attack with no auto-fill is the
wrong-lesson rewind (`0x801F6F98`) - the drill is only ever checked on a
`Command` entry, the path a player reaches through the forced `[Command]`
prompt of state `120`.

The completion tail `0x801F7380` fires once `ctx[0x28A]` reaches `4`: it bumps
the lesson to `5`, writes `ctx[0x06] = 0xC8` (`0x801F73DC`) and `ctx[0x07] =
0xFF` (`0x801F73E8`) to close the command flow, emits the sign-off box and
calls `FUN_801F7628` (`0x801F7460`). Those stores park the fight; they do not
end it. What ends it is the countdown `FUN_801F7628` seeds.

The tail opens on an idempotence guard the C flattens away. At
`0x801F7390..0x801F73B4` an `sltiu ctx[0x28A], 5` skips ahead when the lesson is
still below `5`; a lesson **already** at or past `5` re-pins it to `5` and
re-issues the same `0xC8`/`0xFF` close writes before reaching the `== 4` arm. So
the close is safe to re-enter, and `5` is a terminal value rather than a
one-frame transient.

**The prompt is a sized window, not loose text.** The emitter
`FUN_801F747C(text, style)` measures its prompt before it places it -
`FUN_8003CBA8(str)` returns the rendered line count, `FUN_80035F04(str)` the
pixel width - and the shared tail at `0x801F75B8` passes both on to the SCUS
text-actor registrar as a full rect:

```
FUN_8003541C(1 + waits, 0xD, str, x, y, width, lines*14 - 4, 0x44 - waits)
             a0         a1   a2   a3 +0x10 +0x14  +0x18       +0x1C
```

`FUN_8003541C` links the node into a list sorted on its `+0x08` key and stores
the rect at `+0x0A..+0x10`, the kind byte at `+0x1C` and the frame style at
`+0x1D`, then calls `FUN_80030628` - a per-kind **content** builder, not the
draw: its table at `0x80010D38` sends kind `0x0D` straight to the epilogue
`0x80031978`, because a measured text box needs no build step. The drawing is
the per-frame list walk `FUN_80031D00`. So the box's *size* is measured, and
only its *corner* comes from the style table.

**Box placement.** The style index `0..=9` selects a jump table at
`0x801F6B48`. `x` is either the fixed left margin `0x10` or centred at
`0xA0 − width/2`; `y` is either the fixed top `0x0E` or bottom-anchored at
`base − (lines × 14 − 4)` for `base` in `{0x9A, 0xB0, 0xCC}` - the same height
expression the rect carries. Styles `0, 1, 8, 9` do not wait for
acknowledgement; `2..=7` do.

The wait is not a flag on one actor. The emitter initialises `s4 = 1` and only
the `0 / 1 / 8 / 9` arms clear it, because table slots `8` and `9` are the `2`
and `3` arms entered one instruction later - `0x801F7528` / `0x801F7538`, past
the `move s4, zero`. `s4` then picks the registered actor's sort key (`1 + s4`)
and priority (`0x44 − s4`), so a waiting prompt is a *different* text actor
from a self-dismissing one.

**What the frame looks like.** The frame is the class-0 frame the battle
message banner wears, not the dialog reading box's. The display list of
`v0_1_battle_command_menu` (the lesson intro over `Begin | Run`, centre rect
`(16, 14, 279, 10)`) draws it as ten opaque `POLY_GT4` fill tiles sampling
texel `(128, 0)` under CLUT `(32, 511)`, grey `0x40` at the top edge and `0x88`
at the bottom, over `(8, 6)..(303, 32)`, with tile-set 0's gold edge sprites
inside that rect - the left column at `(8, 10)`, `18` tall, the bottom run on
row `28`. So the footprint is the centre rect inflated 8 px on each side, the
fill is the opaque blue marble patch (`FUN_8002BDC4`) rather than the reading
box's translucent gradient, and the text rows sit at the rect origin on the
14-px pitch. Both hosts draw it through
`engine-ui::battle_hud_chrome::text_actor_frame_draws_for` at
`BoxStyle::box_rect` - see
[`engine-ui::battle_tutorial_box`](../../crates/engine-ui/src/battle_tutorial_box.rs) -
on the 320x240 stage transform (the rect is in retail framebuffer pixels, not
surface pixels). The timed-fight strip is the same kind of text actor and
wears the same frame.

**A waiting box wears the dark tile-set.** `s4` only reaches two of the
registrar's arguments. The emitter's tail passes `a1 = 0xD` unconditionally
(`li a1,0xd` at `0x801F75F0`), so both boxes register under the same widget
kind, and `FUN_80031D00` - the per-frame walker that actually draws a
registered node - dispatches on that kind byte alone (`lbu v0,0x1c(s4)` at
`0x80032170`, jump table `0x80010DC0`), never on the node's `+0x08` sort key.
The two arguments `s4` does move are the key itself (`1 + waits`:
`FUN_8003541C` compares it at `0x80035520` to decide whether to reuse an
existing node, and `FUN_800319A8` unregisters by it) and the `0x44 - waits`
byte at node `+0x1D`, the frame-style selector the draw tail hands to
`gp+0x14C`. That byte is visual: the same display list draws the waiting
explainer under the intro (centre rect `(23, 194, 275, 10)`, style byte
`0x43`) with its fill tiles from texel `(128, 32)` and every one of its
sprites under CLUT `(48, 511)` - the whole tile-set one skin row down and one
palette on, a near-black marble - where the intro's `0x44` samples `(128, 0)`
under `(32, 511)`. Retail adds no marker sprite for the wait; the skin is the
signal. The port's atlas carries tile-set 0 only, so its waiting box wears the
blue frame and the dialog pager's confirm hand in the dark skin's place.

The key decides how long a box stays. A self-dismissing prompt (key `1`) is
never timed out: the next key-`1` registration reuses its node, and the only
explicit removals are the hook's `FUN_800319A8(0)` / `FUN_800319A8(1)` pair on
the suppressed path at flow state `0x5A` (`0x801F71BC..0x801F71D8`) and the
teardown drain `FUN_800355F0`. `v0_1_battle_command_submenu` still shows the
`[Begin]` arm's prompt over the category ring it named. The port counts a
self-dismissing box down instead (`TUTORIAL_BOX_AUTO_FRAMES`), because its
battle loop parks on any box in the queue.

Two further consequences fall out of the registrar read: kind `0x0D` is one of the three kinds `FUN_800319A8` refuses to
free `+0x18` for (`0x80031A30..0x80031A44`, alongside kinds `< 2` and `0x11`),
because the string is the overlay's own, not heap; and kind `0x0D`'s slot in the
*registration*-time table at `0x80010D38` points at `0x80031978`, which is
`FUN_80030628`'s epilogue - so registering a prompt draws nothing that frame,
and the box first appears on the next walk.

Engine port: [`engine-core::battle_tutorial`](../../crates/engine-core/src/battle_tutorial.rs).
The prompt **text is Sony data living in the overlay**, so the port commits only
the string *addresses* and reads the text off the user's own disc at runtime
(`BattleTutorialScript::from_overlay` / `::from_prot`) - the same rule the item /
spell / dialog parsers follow. Disc-gated oracle
`crates/engine-core/tests/battle_tutorial_disc.rs`.

**One dispatch's boxes share the frame.** Each box the handler emits is its
own registered text actor, so a two-box hook - the lesson intro at the top
and the directional explainer at the bottom at `Begin | Run` - puts both on
screen at once. The engine's queue carries a dispatch **group** per box
(`ActiveTutorialBox::group`); both hosts draw the whole front group
(`World::battle_tutorial_boxes_on_screen`), a non-waiting member counts
itself down inside it, and the waiting member holds the group until Cross.

#### The opening caption - the SCUS side-band, not overlay 967

The first thing the sparring fight says is not a 967 prompt. It is the
caption at SCUS `0x80078CB4` (label `BattleUiLabel::SparringIntro`, read off
the executable at boot), raised by the battle **side-band tick**
`FUN_80056208` - the once-per-frame SCUS pass keyed on the stage id
`_DAT_8007B64A`, whose stage-`1` arm is a four-phase machine on
`ctx[+0x289]`:

```text
800562c8  lbu  v1,0x6(a2)         ; phase 0 waits for ctx[+0x06] == 0x14
800562e8  sb   v0,0x289(a2)       ; phase = 1
800562f8  sh   v0,0x6ae(a2)       ; hold timer 0xB40, drained 8 per frame
80056320  _sw  v0,0x7494(v1)      ; caption pointer _DAT_80077494 = 0x80078CB4
8005631c  jal  0x801d8de8         ; HUD element 0x5A
80056360  jal  0x801d829c         ; camera aimed at the first monster seat
80056370  ...                     ; phase 1: any packed-pad press zeroes the timer
80056400  sh   s0,0x6ae(v1)       ; expired -> phase 2
80056418  jal  0x801f6b70         ; phase 2: the overlay-967 hook, every frame
800565c4  sh   s0,0x6b0(v0)       ; ctx[+0x6B0] = 1 through phases 0 and 1
```

`ctx[+0x6B0]` is the hold: `FUN_801D0748` tests it at `0x801D0BDC` and returns
before its state switch, so `0x14` - the sweep, the seed, `Begin | Run` -
does not run until the caption has gone, and the prompt machine only ticks in
phase `2`. The retail frame (`v0_1_battle_start_tetsu`) is the caption
centred on the bottom anchor `0xCC`, the same corner as emitter style `9`.

Engine port: the side-band kernel is `engine-core::battle_sideband`, and
`World::tick_battle_sideband` (`world/battle/sideband.rs`) runs it at the top
of every live battle frame, for both play hosts. The round start asks it first
(`World::battle_sideband_holds_round`): on stage 1 it runs the kernel with the
flow byte at `0x14`, whose caption arm queues the caption on the tutorial box
queue and publishes the hold; the per-frame tick then decays the timer, and
its drain removes the caption and opens the round. The hold also selects the
battle camera's Dialogue close-up, which is the `FUN_801D829C` aim above. A
world with no caption text skips the hold.

#### How the sparring fight ends - the `ctx[+0x6B4]` countdown

`FUN_801F7628` stores `ctx[+0x6B4] = rate * 360` (`x3`, `x15`, `x8` of the
game-speed byte `DAT_1F80037D`, `0x801F7648..0x801F7660`), raises the hold
`ctx[+0x6B0] = 1`, clears the pad masks and captures the cancel mask
`_DAT_800846D4` into `ctx[+0x88C]`. Every hook call - dispatching, latched or
suppressed by a box - then reaches the countdown section at `0x801F71F0`:

```text
801f71fc  lh   v0,0x6b4(a0)       ; zero -> skip the section
801f7218  _sh  v0,0x6b0(a0)       ; hold = 1
801f721c  lh   v0,0x6b2(a0)       ; a new press with no box up ...
801f722c  sh   zero,0x6b4(a0)     ; ... zeroes the countdown
801f7258  sw   zero,-0x478c(a0)   ; pad masks cleared (B874 / B938 / B850)
801f7274  subu v0,v0,t2           ; countdown -= frame_step * rate
801f7280  bgtz v0,0x801f7380      ; still positive -> keep holding
801f7290  sltiu v0,v0,0x4         ; lesson < 4:
801f72a4  _sw  v0,-0x478c(a0)     ;   re-inject the cancel mask as a press
801f72c0  sb   v0,0x289(a2)       ; else: side-band phase + 1 (= 3)
801f72f4  jal  0x80024e80         ;   fade (kind 2, 0x40 frames, to black)
801f7318  sb   v1,-0x428f(t0)     ;   DAT_8007BD71 = 0xFE, the battle-end signal
801f735c  sb   v0,-0x42a0(t0)     ;   DAT_8007BD60 |= 0x80, the survived bit
801f7374  jal  0x800355f0         ;   drain the floating-element list
```

The rate cancels, so the hold is 360 vsyncs; a press skips it once the
sign-off box has gone. The same routine serves the wrong-lesson rewinds,
where the expiry's re-injected Cancel is what backs the player out of the
rejected menu. On the completion path the expiry raises side-band phase `3`,
whose arm ticks the teardown staging `FUN_80025358` and counts `ctx[+0x6CE]`
up by the frame step; the frame driver `FUN_80046A20` leaves the battle once
that halfword reaches `0x43` (`0x80046DAC`) with the staging's still-loading
byte `ctx[+0xB]` clear, storing mode word `2` and clearing the stage id
(`0x80046E74`). The results sequencer returns at once while the stage id is
non-zero (`0x8004E5B8`), so no spoils are shown or credited, and MAIN INIT
turns the survived bit into story flag 1.

Engine port. Phase `2`'s hook is split: the dispatch runs on each flow edge
(`World::set_battle_flow`; its one-shot latch makes an edge call and a
per-frame call equivalent), and the per-frame half - the completion tail and
the countdown section (`BattleTutorial::completion_tail` /
`BattleTutorial::tick_countdown`) - runs off the side-band's `SparringHook`
effect every battle frame. From the completion tail on, the side-band owns
the frame (the flow byte `0xC8` is no `FUN_801D0748` case), the sign-off box
is aged there, and the expiry raises phase `3`; `World::tick_battle_sideband`
applies the `0x43` exit gate after the side-band pass, as the frame driver
does, and exits through `World::finish_battle`. Only the completion tail arms
the countdown in the engine: its rewinds reopen the command menu directly, so
retail's 360-vsync hold after a wrong-lesson box and its synthetic Cancel are
not reproduced. Not staged either: the `FUN_801D829C` camera aim at party
seat 0 (`0x801F7368`).

### What the two boss-stage modules do (overlays 968 / 969)

Both modules are **one function over the whole of their own code**, and each is
a phase machine on the battle context's `ctx[+0x289]` byte driving the **first
monster seat** - the actor the eight-slot table hands back at `0x801C937C`,
i.e. `DAT_801C9370[3]`. Neither is a cast module: neither is named by any of the
three PROT 0898 entry tables that reach the `0903..0966` band
([`cast-module.md`](cast-module.md)), and the pager is the only thing that
brings them in.

`see ghidra/scripts/funcs/overlay_battle_slot_b_0968_0968_801f69f4.txt` and
`see ghidra/scripts/funcs/overlay_battle_slot_b_0969_0969_801f69d8.txt`.

**Entry 968 - `FUN_801F69F4`, seven phases.** Its head is a seven-word jump
table at the image base `0x801F69D8`, bounded by its own `sltiu a0, 7`, and the
body begins in the eighth word. Every tick re-seeds `ctx[+0x6D6] = 0x100`
before dispatching. The phases run on one countdown word in the image's own
data band (`0x801F73F8`, stepped by the scratchpad frame-delta byte
`0x1F800393`) and a frame counter beside it (`0x801F73FC`):

| Phase | What it does |
|---|---|
| `0` | Holds until the eye-space camera word `0x800840BC` passes `0xC00`, walking it and `0x800840C0` there by the frame delta; then raises cue `0x20A` through `FUN_8004FCC8`, spawns three in-image effect records (`0x801F71F0` / `0x7240` / `0x7290`) through `FUN_80050ED4`, stages a fade block at `0x801C9070` and arms the countdown at `0x80`. |
| `1` | Spawns three more records (`0x72D0` / `0x7320` / `0x7388`), then a `FUN_801D829C` camera move framed on the seat's live `+0x34` / `+0x38` / `+0x46`, and sets the seat's tint blend `+0x0C = 0x1000`, its anim rate `+0x21D = 1` and `+0x36 = 0x600`. |
| `2`..`4` | Each walks a different camera axis by the frame delta (`0x800840C0`, `0x800840BC`, the scroll trio `0x8007B790` / `0x92`) and spawns record `0x73A4` on every eighth frame, ending in its own `FUN_801D829C` framing. Phase `4` also sets `ctx[+0x243] = 1` and `ctx[+0x278] = 2`. |
| `5` | Measures a string with `FUN_80035F04` and draws it centred through `FUN_8003541C` at `(0xA0 - width/2, 0x96)` - the boss-name banner - then sets `ctx[+0x278] = 3`. |
| `6` | The hand-back: clears `ctx[+0x243]`, `ctx[+0x278]`, `ctx[+0x6D6]`, `ctx[+0x289]` **and the stage id `0x8007B64A`** that paged the module in, sets the seat's anim rate `+0x21D = 8`, writes flow state `ctx[+0x06] = 0x0B`, rebinds the two backdrop records at `ctx[+0x106C]` / `ctx[+0x1070]`, runs two move-VM effect trees through `FUN_80021B04`, and pushes one rect through `FUN_80058490` ([below](#the-arrival-re-dresses-the-arena)). |

**Entry 969 - `FUN_801F69D8`, four phases.** No head table: the image opens
straight on its prologue and branches four ways on the same `ctx[+0x289]`.
Phase `0` raises cue `0x20B`, forces the battle flow byte `ctx[+0x07] = 0xFC`,
and **writes the first monster seat's HP field `+0x14C = 1`** - the scripted
"survives the killing blow" beat - while parking the acting actor and the two
non-acting party seats at fixed `+0x34` / `+0x38` offsets and staging the
seat's `+0x21C` / `+0x38` / `+0x46` / `+0x1DC`. Phases `1` and `2` alternate
the camera word `0x800840BC` between `0x780` and `0x800` every other frame - a
two-position shake, not a ramp - spawn an in-image record every eighth frame at
an offset drawn from the battle RNG `FUN_80056798`, and run their own
countdown (`0x801F70DC`) with the fade block at `0x801C9070`; phase `2` then
blanks all four leading actor slots (`+0x04 = 0`, `+0x21C = 0xFF`) and sets
both model records' `+0x78 = 0x1000`. Phase `3` waits out the countdown and
calls `FUN_8003ED04(0)`.

#### The arrival re-dresses the arena

The evolved-Cort stage (`jouine`, variant 2, PROT 693) is a two-object shell,
and battle init's object edit leaves the two backdrop actors drawing object 0
only - a wall on texture page 12 through CLUT row 473, pink-brown on its idle
palette. Phase `6` changes what the arena is, in three steps:

- **The rebind** (`0x801F7148..0x801F7180`): for both records, `t = rec[+0x44]`
  (the object table `[count, slot 0, slot 1, ...]`) and `t[+4] = t[+8]` -
  slot 1 over slot 0, count untouched. Battle init's shift left slot 1
  holding object 1, so from here on the shell draws **object 1 alone**: the
  flesh shell on page 13 through CLUT `(32, 479)`, the dark-red veins.
- **The effect trees**: `FUN_80021B04` over `0x80078740` and `0x80078760`.
  The first seats a mode-4 [VRAM-rect scroller](field-ambient-fx.md#the-vram-rect-scroller-render-mode-4)
  on `(0x340, 0, 0x3F, 0xBF)`, the flesh texels, stepping up one row per
  period; the second animates the CLUT `(32, 479)` the flesh samples.
- **The rect push**: `MoveImage` of the empty `16 x 64` strip at
  `(0x340, 0xC0)` onto `(0x370, 0xC0)` - the ground grid's tile window, so the
  procedural floor samples only transparent texels and the flesh shell is the
  only ground.

The engine ports the rebind and the rect push
(`StageEffect::RebindBackdrop` / `StageEffect::MoveImage`); both hosts build
the shell from `SceneHost::battle_stage_object_indices` and apply the move
through `World::apply_battle_vram_moves`. The two effect trees are not staged,
so the engine's flesh neither scrolls nor pulses.

So 968 is the **arrival** staging (camera walk in, cue, banner, hand back to
flow state `0x0B`) and 969 is the **form transition** (drop the seat to 1 HP,
shake, blank the field). That is the same split the two writers of the stage id
imply, and it is why the Cort fight walks both.

Two details of the side-band's own stage arms matter to both modules. The
stage-2 arm tests the battle scene loader's step byte `DAT_8007BD71`
(`FUN_800520F0`, `gp[+0xA59]`), which the loader parks at `0x11` while it pages
the stage overlay in: below `0x12` the arm clears the pads and pulls the
camera back instead of ticking a module that is not resident yet. The stage-3
arm waits out `ctx[+0x6D8]` and then for the CD to go idle
(`FUN_8003DE7C(1)`). And the side-band never writes `ctx[+0x289]` for these
stages - each module walks its own phases.

Stage `3` is reached at the head of cleanup state `0x50`: the state opens with
the Final Heal sweep (`jal 0x801E6968` at `0x801E5C6C`), and the sweep's tail
parks the action SM at `0xFD`. The state's own advance to `0x51` is guarded on
`ctx[+0x07]` still reading `0x50` (`0x801E5F4C..0x801E5F5C`), so the park
stands and the end-of-action gate `0x5A` - whose survivor count would raise
the battle-end signal - never runs. The results sequencer `FUN_8004E568`
would return at once anyway while the stage id is non-zero
(`lbu v1,0x332(gp)`, `gp + 0x332 = 0x8007B64A`; `bne` at `0x8004E5B8`), so no
spoils are shown or credited. The form transition's phase 3 ends the
battle itself: mode word `2` (back to the field) and `DAT_8007BD60 = 0x80`,
the won bit MAIN INIT turns into story flag 1 (`0x8003B570..0x8003B590`).

**Engine port.** `engine-core::battle_stage_module` ports both phase
machines as pure kernels over the state they touch (`arrival_tick`,
`form_transition_tick`), and `World::tick_battle_sideband` hosts them: the
battle-init override writes stage `2` at entry, the round start holds until
the arrival's hand-back, `World::run_boss_transition_arm` writes stage `3`
at the head of cleanup state `0x50`, and the form transition's exit runs the engine's battle
teardown. While a module runs it owns the frame and the camera globals
(`World::battle_cam_pose` returns its camera to both hosts), and the arrival's
boss-name banner is drawn by both hosts through
`battle_hud::battle_stage_banner`. Not staged: the in-image spawn records
(every one is a meshless `model_sel = -1` part), the two SCUS move-VM effect
trees at the hand-back, the render-node words, the `FUN_80058490` rect push
and the CD-XA stop; the form transition still draws its battle-RNG values, so
the stream stays retail's.
