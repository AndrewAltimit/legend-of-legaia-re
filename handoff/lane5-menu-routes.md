# Lane 5 - menu routes off the op-`0x49` entry context

Three waivers in `scripts/ci/ui-host-drift-waivers.toml` were assigned. Two
are **deleted** (the screens are live on both hosts). One is **rewritten**,
because two of its stated premises were false and the true blocker is larger
than it claimed.

## What closed

### (a) The op-`0x49` park records its armed sub-op

`FieldHost::op49_arm` now takes the sub-op. That is not an added argument for
convenience - it is what retail's park *holds*. The Idle arm reads the
operand's first byte and then stores the operand **pointer**, so the byte
every consumer dereferences is the byte the arm just read:

```text
801e0984  lbu   v0,0x0(s6)        ; sub_op = *operand
801e098c  sltiu v0,v0,0xe         ; >= 0x0E never arms at all
801e09a0  jal   0x80020de0        ; spawn the driver
801e09a8  sw    s6,-0x4bb0(s0)    ; _DAT_8007B450 = operand
```

and the resume clears it (`sw zero,-0x4bb0(s0)` at `0x801e08d8`, on the `== 1`
Done sentinel). The port carries the byte itself on
`SubmodeScreen::park_sub_op`, still tagged with the `Op49ParkOwner` that armed
it - retail has one global, the port steps several field-VM contexts inside
one `World::tick`. `World::menu_entry_context_kind` reads it; the two legacy
derivations (armed shop = `0`, installed tile board = `5`) stay behind it as a
fallback for hosts that arm those directly without going through the VM.

**Only two routines write a dereferenceable pointer there.** A scan of every
`sw rt,0xb450(rs)` instruction across `SCUS_942.54` and every extracted
PROT entry finds ten sites, all in three images (SCUS, PROT 0897, PROT 0899): `0x801E09A8` (this arm), `0x801D0D04`
(`FUN_801D0B90`'s countdown expiry, pointing at the static record
`DAT_801F2278` whose kind byte is `0x0B`), and eight that store `0` or the `1`
Done sentinel. So any kind byte other than `0x0B` is a field script's own
operand - which makes reachability a disc question.

### The two `0x0D` screens, on both hosts

`FUN_801DC6B4`'s entry decode picks the *starting* sub-screen off the kind
byte, and `FUN_801D6B20`'s cancel arm picks its destination off the same byte:

| kind | sub-screen | routine |
|---|---|---|
| `0` | `0x1A` | `0x801dc8a0` |
| `1` | `0x19` | `0x801dc8b4` |
| `7` | `0x20` | `0x801dc8cc` - the casino prize exchange |
| `0x0D` | `4` | `0x801dc8e4` - the notice panel |
| `0x0D` (cancel) | `3` | `0x801d6d18` - the ready check |

`FieldMenuSession` grew the two phases: `Notice` (sub-screen `4`,
`FUN_801DD1B8` - one press on either button, then the root picker) and
`ReadyConfirm` (sub-screen `3`, `FUN_801D6D38` - horizontal two-row choice
seeded to row `1` = No; Yes routes to sub-screen `0` and ends the menu, No and
cancel return to the root). `open_entry_screen` is the entry decode; both
hosts call it right after `set_gate`.

Draws are paired: `window/menu_draws.rs::context_locked_screen_draws` and
`web-viewer/src/play_menu.rs::build_context_locked`, both resolving the window
id off the disc table through `painter_at`, both suppressing the root list for
the frame (retail's open scripts lead with `05 00` = close every window).

Labels are **read from the caller's own PROT 0899 image**, never committed:
`pause_screens::ContextLockedLabels` holds the VAs the two renderers load and
slices the strings, installed by the one `install_menu_overlay_tables` call
both hosts already made - so neither host can ship the panels text-less while
the other draws them.

### What is still short of retail on (a), stated plainly

The decode is complete and both hosts run it, but the **window in which a real
field script's `0x0D` park is visible to the menu is narrower than retail's**.
Retail's op-`0x49` arm spawns a driver actor that *opens the menu itself*, and
the park stays armed until that screen hands back. The port has no path from a
parked field script to opening the pause menu, because the session is host
state (`BootSession::field_menu`, the page's `PlayMenu`) and not world state:
`World::open_field_submode_screen` runs the close tick instead, which retires
within a few frames and lets `op49_clear` drop the park. So the byte is
produced, carried and acted on - but a player only sees these two screens if
the menu opens while the park is still armed.

This is disclosed on `FieldMenuSession::open_entry_screen` rather than left
for a reader to discover. Closing it is the shape the inline gold shop already
uses: a pending-request channel the hosts drain
(`World::take_pending_field_shop`) plus a host-called finish that flips the op
to Done - a `take_pending_field_menu` twin on both hosts. It is a different
gap from the one the waivers described, and naming it is the point.

## Two claims the old waivers had backwards

Both were corrected where they were written down (`ui_menu_window_painters.rs`,
`docs/subsystems/field-menu.md`):

- **Window 5 is not a "really leave?" gate.** `FUN_801D61B0` loads its two
  headings from `0x801CEC78` / `0x801CEC94`, and they are a **battle-start
  ready check**. Its Yes exits the menu *into the fight*. The routing (Yes ->
  sub-screen 0, No -> root) reads either way; the strings do not.
- **Window 6's six labels are not "content that record owns".** They are six
  static VAs in the menu overlay's own pool, loaded by `lui a0,0x801d` +
  `addiu` pairs at `0x801d636c..0x801d6448` - a multi-line pre-battle briefing
  plus a one-byte control string. The text was never the obstacle.

Together they say what the `0x0D` context *is*: a scripted pre-battle party
menu, briefed on entry and ready-checked on exit.

## The op-`0x49` sub-op census

`crates/engine-core/tests/op49_sub_op_census.rs`, disc-gated. **N = 124
CDNAME scene names, 99 of them carrying a field MAN.**

Two tallies, because neither is the truth alone.

**walk** = `engine-core::man_field_scripts::op49_window_census`, the repo's
own opcode-aware sweep, shared with `op49_window_census_disc.rs`. It walks
every MAN carrier a scene has (bundle *and* standalone variants) and every
record of every partition through `partition_record_span`, which knows that
partition 2 opens with a Shift-JIS name and three condition gates rather than
the `[u8 N][N*2][4]` prefix partitions 0/1 use. Reusing it was a correction:
the first cut of this census wrote its own walker, mis-framed partition 2, and
reported a corpus that did not match the pinned one. It does now - the tally
sums to **236 sites**, which is the invariant `op49_window_census_disc.rs`
already pins.

**bytes** = every `49 <n>` pair with `n <= 0x0D` in the decompressed MAN, a
strict upper bound.

| sub-op | walk sites | walk scenes | byte sites | byte scenes |
|---|---|---|---|---|
| `0x00` | 97 | 38 | 228 | 66 |
| `0x01` | 62 | 46 | 166 | 54 |
| `0x02` | 1 | 1 | 11 | 10 |
| `0x03` | 4 | 4 | 146 | 25 |
| `0x04` | 24 | 3 | 40 | 12 |
| `0x05` | 0 | - | 11 | 7 |
| `0x06` | 4 | 4 | 15 | 7 |
| `0x07` | 4 | 4 | 86 | 8 |
| `0x08` | 10 | 7 | 84 | 10 |
| `0x09` | 27 | 11 | 62 | 14 |
| `0x0A` | 0 | - | 47 | 8 |
| `0x0B` | 0 | - | 5 | 5 |
| `0x0C` | 2 | 2 | 9 | 3 |
| `0x0D` | 1 | 1 | 10 | 3 |

**The answer to the question the waivers posed: yes.** Sub-op `0x0D` is armed
by a real scene script - one decoded site, in `nilboa`; the byte bound puts it
in `kor` / `nilboa` / `other7`. Sub-op `7` is armed at four decoded sites
across `balden`, `balden2`, `koin1`, `other7`. Both screens are reachable, and
neither waiver was telling a future reader to do work that would not reach a
screen.

Two cautions the table encodes:

- A walk **ABSENT** row is not a disc absence. Sub-op `5` (the tile board) is
  the standing counter-example: the port resolves it through a dedicated host
  path and the byte bound finds eleven sites, yet the opcode walk decodes
  none. Read ABSENT as "not decoded here".
- The byte tally over-counts freely (`0x03`: 4 decoded vs 146 bytes), because
  a `49` inside an operand is not an opcode. A sub-op present in **both** is
  armed; a sub-op absent from **bytes** is armed nowhere.

Both are asserted: every sub-op the walk decodes must also be inside the byte
bound, and `7` / `0x0D` must appear in both.

## What did not close, and why

### (b) The casino prize exchange - waiver rewritten, not deleted

The brief suggested the browser was already close, citing
`web-viewer/src/minigames_fishing.rs`'s `prize_exchange(venue)`. **That is a
different subsystem.** It is the *fishing point exchange*
(`legaia_asset::fishing_exchange`), spending the pond session's points against
a per-venue rod table. Window 46 belongs to the **casino counter**, which
spends the coin bank `_DAT_800845A4` against the `0x801E4518` prize table.
Neither host has any part of that screen. The waiver's stated proximity was
itself an instance of the failure mode the file's header warns about.

What *is* now true, and is in the rewritten waiver:

- The trigger is no longer missing. Sub-screen `0x20` is written to
  `DAT_801E46A4` at exactly one site in PROT 0899 - `0x801dc8cc`, on kind `7`
  (a sweep of all 66 `sw rt,0x46a4(rs)` writers finds no other) - and kind `7`
  is now producible and disc-attested.
- The remaining blocker is the **window set**, not the confirm: script
  `0x801E4F18` opens windows 43 (tab) / 44 (list) / 45 (coin counter), and
  `0x801E4F2C` opens 46 on top. Wiring 46 alone would be a Yes/No box over
  nothing - a fake wire.
- The disc prize block at PROT 0899 file `0x15D00` is parsed nowhere outside
  `legaia_patcher::casino`; `engine-core::prize_exchange` takes records but
  has no reader.

Left as one coherent piece of work rather than half-done.

### (c) The remaining orphans - untouched

`key_rebind_draws_for`, `count_panel_draws_for` (24) and
`equip_compare_panel_fields` (25) are unchanged. (a) and (b) did not land
early enough to take on the Equip-screen rewrite that 24 and 25 share, and a
partial move onto the descriptor-table window set would leave the screen worse
than it is.

## Loose ends for the integrator

- **`save_subscreen.rs` labels sub-screen `0x20` "AutoSave"** and
  `SaveEntryContext::AutoSave` for kind byte `7`. The dispatch table settles
  it: PROT 0899 `0x16728 + 0x20*4` holds `0x801DC1CC`, the prize exchange -
  which `docs/subsystems/save-screen.md` already records, calling the
  auto-save reading falsified. The code did not follow. Out of this lane's
  scope; a rename plus a `PostSave` -> the real meaning pass is wanted.
- **`docs/subsystems/field-menu.md` had the Equip window set at sub-screen
  `0x13`.** Corrected to `0x14`: the script `0x801E4DC8` is loaded at
  `0x801d9d00`, inside the routine the pointer table lists at index `0x14`
  (`FUN_801D9C14`); index `0x13` is `0x801D99F0`.
- `FieldMenuInput` gained `left` / `right`. Three construction sites were
  updated (`engine-shell/src/boot.rs`, `window/boot_cutscene.rs`,
  `web-viewer/src/play_menu.rs`); `boot.rs` is outside the lane's stated scope
  but the workspace does not compile without it.
