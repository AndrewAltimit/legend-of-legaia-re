# Lane C - `check-port-provenance.py` `dual-label`, all 49 findings

`dual-label` fires when one address carries a defining description (a `##`/`###`
heading, or a row on a `docs/reference/functions/` page) on two docs pages whose
**filename tokens** share nothing. Page relatedness is decided on the stem, so
`menus.md` + `save-screen.md` is "unrelated" and `battle.md` + `battle-action.md`
is not. That is the whole matching rule, and it explains most of the list.

Every row below was read against the disassembly in
`/home/mikunpc/Documents/repos/legend-of-legaia-re/ghidra/scripts/funcs/` (the
worktree has no dumps; the corpus was reached read-only, never copied or
staged). "Evidence" is `disassembly` when a verdict rests on a fresh read of the
instructions and `doc` when it rests on the two texts already agreeing.

## Counts

| Verdict | N |
|---|---|
| `RESOLVED` (one label was wrong; the losing carrier is corrected) | 6 |
| `NOT-DUAL` - per-image VA alias, both labels true of their own image | 4 |
| `NOT-DUAL` - one claim filed twice, gate matching on page names | 39 |
| `BOTH-WRONG` | 0 |
| `UNRESOLVED` | 0 |

Gate after the work: **`dual-label` 49 unwaived → 0 unwaived, 47 waived.**
Whole-report unwaived 180 → 131 (`module-orphan` 66 + `doc-citation` 65, both
other lanes'). Three of the 49 stopped firing outright when five pure draw
routines moved off the audio directory page; the rest are waived with per-address
reasons in `scripts/ci/port-provenance-waivers.toml`.

## Three-or-more-label addresses (the `FUN_801D5DE0` shape)

Five addresses carried a defining description on **three** pages. Two of them
were genuinely wrong on one carrier:

| Address | Carriers | Outcome |
|---|---|---|
| `801D0748` | `functions/battle.md` (x2 rows) · `functions/minigames-debug.md` · `subsystems/battle.md` | **RESOLVED** - one body, three mode labels |
| `801D8DE8` | `functions/battle.md` (x2 rows) · `subsystems/battle.md` · `minigame-muscle-dome.md` | **RESOLVED** - `battle.md`'s section was still guessing |
| `80021934` | `scene-v12-table.md` · `functions/game-modes.md` · `asset-loader.md` | NOT-DUAL, all three agree |
| `80021DF4` | `functions/game-modes.md` · `actor-vm.md` · `move-vm.md` | NOT-DUAL, all three agree |
| `801D1344` | `functions/asset-loading.md` · `boot.md` · `world-map.md` | NOT-DUAL, all three agree |

## The six `RESOLVED` rows

| Address | The two labels | Winner | Evidence |
|---|---|---|---|
| `80034A6C` | new-game data-init (`game-modes.md`) vs menu/HUD globals reset (`menus.md`) | game-modes | disassembly |
| `801CFC40` | actor-collision box probe (`script-vms.md`) vs world-map sprite batcher (`world-map.md`) | script-vms | disassembly |
| `801EAD98` | world-map debug-menu renderer (`world-map.md`) vs "field subsystem hub" (`script-vms.md`) | world-map | disassembly |
| `801D0748` | battle round driver (`functions/battle.md`) vs Muscle Dome "card" controller in a distinct overlay (`minigames-debug.md`) | battle | disassembly |
| `801D8DE8` | HUD element renderer (`functions/battle.md`) vs "likely a per-actor utility" (`subsystems/battle.md`) | HUD renderer | disassembly |
| `801DA51C` | world-map entity SM - agreed, but `world-map.md` had a wrong global and a truncated size | corrected in place | disassembly |

What each read showed:

- **`80034A6C`.** With `s0 = 0x80084140`: `li v0,0x1f4` / `sw v0,0x45c(s0)` is
  party gold `0x8008459C = 500`; the descending loop at `0x80034B1C` stores
  through `0x1618(v1)` with `v1` starting at `s0+0x1FF`, so it clears
  `0x80085758..0x80085957` - the **fourth flag bank**, not the
  `0x80084340..0x8008453F` "save-data scratch slot" `menus.md` named (that is
  the range the *register* sweeps). Tail call `FUN_800560B4` expands the
  starting-party template. `crates/asset/src/new_game.rs` independently models
  the same fifteen `SC` offsets, which corroborates it without being cited as
  proof.
- **`801CFC40`.** The 131-instruction body stores only the probe point into
  scratchpad `0x1F800020/22/24`, the mutual `+0x98` partner links on contact,
  and the result accumulator; it box-tests every entry of `DAT_801C93C8`
  (count `_DAT_8007B6B8`) at `tile*128 + (i8)sub*16` with half-extent `0x40`
  widened by the caller's `(ex, ez)`, then calls `FUN_8003D038`. No packet, no
  OT link, no texture page. `0x1F800020` is scratchpad RAM; the GPU ports are
  `0x1F801810`/`0x1F801814`. The 110-instruction PROT 0897 print has an
  identical prologue - it is a truncation, not a second occupant, so
  "present only in `world_map_top`" is false too.
- **`801EAD98`.** `lui v0,0x801d; addiu v0,v0,-0xb94` forms the 24-entry jump
  table `0x801CF46C`; `lui a0,0x801d; addiu a0,a0,-0xcbc` forms the label
  strings `0x801CF344`; the callees are the window emitter `FUN_8002C69C` and
  the glyph blitter `FUN_80036888`. 1820 instructions / 7280 bytes, not the
  "5.9 KB" the stub row claimed.
- **`801D0748` / `801D8DE8`.** Every dump of each hashes identically across the
  battle-action, magic-capture, magic-level-up, Muscle Dome and static `0898`
  images (2781 and 757 instructions). `801D0748` opens by loading
  `_DAT_8007BD24` and reading `ctx+6`; `801D8DE8` bounds with
  `sltiu v0,v1,0x50` over `0x801CEB68`.
- **`801DA51C`.** 181 instructions, `sltiu v1,0x5` over `0x801CEC28`, gated on
  `lui 0x8008 / lw -0x4798` = **`_DAT_8007B868`**. `world-map.md` said
  `_DAT_80083808` and "260 bytes"; 260 bytes is the truncated minigame-image
  print.

## The four per-image aliases

Both labels true, of different overlay images. Each pair now cross-references
the other so neither reads as absolute.

| Address | Image A | Image B |
|---|---|---|
| `801F71E0` | PROT 0967 @ `0x801F69D8`: a `bne` target mid-routine, `lui`/`lw` halves straddling it - not an entry | overlay 0897: `addiu sp,sp,-0x40`, 1070 instructions, forms the actor band `0x801C9370` |
| `801F69D8` | `overlay_muscle_dome.bin`: PROT 0900's slot-B link base, 18 table words | `world_map_top_ext.bin`: `addiu sp,sp,-0x70`, 643 instructions |
| `801D9D3C` | overlay 0897: 4-instruction stub | battle family: 388 instructions, the enemy target menu builder (already stated on `script-vm.md`) |
| `801D84D0` | dialog / cutscene / world_map: 1499 instructions | field_battle_intro / world_map_top: 544 instructions; neither page claims the other's image |

## The 39 filed-twice rows

All `NOT-DUAL`, `doc`-grounded except where a dump hash was checked to confirm
one body (noted in each waiver). The recurring pairs:

- `functions/menus.md` row + `save-screen.md` section - `801D688C`, `801D6E18`,
  `801D9C14`, `801DA2A0`, `801E13B8`, `801E3294`. The menu overlay hosts the
  save UI, so the coarse directory page is the right home and the fine page is
  the write-up. `801D9C14`'s `save-screen.md` section literally opens "Not a
  save-screen function at all".
- `functions/script-vms.md` row + the owning subsystem page - `8003CE9C`,
  `80046494`, `801CFE4C`, `801D4A60`, `801DD35C`, `801DE840`, `801E1C1C`.
- `functions/minigames-debug.md` row + the per-minigame page - `801CF388`,
  `801D0750`, `801D1288`, `801D3380`, `801D7BB8`.
- `functions/battle.md` row + a `re-*.md` thread page - `8005126C`, `801DBC30`,
  `801F12D0`. A falsification or settled-thread entry naming an address is not
  a rival definition; two of these are titled "X is **not** Y".
- Format page + directory row - `800198E0`, `8005567C`, `801D84D0`, `801F3990`.
- Remainder: `80021934`, `80021DF4`, `80035274`, `800508DC`, `801CF5BC`,
  `801CFE98`, `801D1344`, `801D1EC4`, `801D362C`, `801D5780`, `801D5AE8`,
  `801D9D3C`, `801DA51C`, `801E1D98`*, `801E2524`*, `801E2650`*, `801EAD98`,
  `801F17F8`, `801F69D8`, `801F71E0`.
  (\* cleared by the audio.md move, no waiver needed.)

## Side finding: `audio.md` was defining five draw routines

`docs/reference/functions/audio.md` carried `801E1AB0`, `801E1D98`, `801D3380`,
`801E2524` and `801E2650` - a move-FX afterimage quad, a streak ribbon, the
casino payline draw and the Arts-banner pair. None touches the SPU. Moved to
`functions/battle.md` and `functions/minigames-debug.md`; the Arts-banner pair
was **already** a row on `battle.md`, so `audio.md` held a duplicate definition.
That alone cleared three `dual-label` rows.

## Side finding: the Ra-Seru capture table claimed copies that do not exist

`functions/battle.md`'s "Ra-Seru capture overlay" table presented
`801D0748` / `801D388C` / `801D5854` / `801D8DE8` / `801E295C` as distinct
capture-overlay functions, and said of `801E295C` "Distinct from
`overlay_battle_action_801e295c.txt` despite sharing the same entry address".
Every one of the five is **byte-identical** to its battle-action dump -
`801E295C` at 4099 instructions. Rewritten as capture-mode roles of the routines
documented above.

## Nothing here needs a Rust change

Checked every `// PORT:` / `// REF:` tag for the six relabelled addresses. All
already agree with the winning label:

- `801CFC40` - tagged five times in `engine-core/src/world/field_movement.rs`
  (collision), never as a renderer. The Rust side was right and `world-map.md`
  was the outlier.
- `801EAD98` - tagged in `engine-vm/src/world_map_overlay.rs` and
  `engine-ui/src/ui_menu/dev_menu_list.rs` (dev-menu list), not as a field hub.
- `80034A6C` - tagged in `asset/src/new_game.rs`.
- `801D0748` / `801D8DE8` / `801DA51C` - tag prose already matches.

No tag prose was edited, so this lane's commits are `docs/` +
`scripts/ci/port-provenance-waivers.toml` only and touch no file another lane
owns.

## Proposed gate refinement (not implemented - the gate is not this lane's)

39 of 49 rows are the same false shape: a `docs/reference/functions/*.md` **row**
plus the subsystem page that owns the write-up. The checker already excludes
"a `###` section plus its directory row" when the stems share a token; the
exclusion under-fires because the directory pages are named for coarse topics
(`menus`, `script-vms`, `battle`) while the write-ups are named for fine ones
(`save-screen`, `field-locomotion`, `battle-action`).

Two candidate sharpenings, in order of preference:

1. **Treat a directory row as a *pointer*, not a definition, when the row links
   to the other page.** Most of these rows already carry a `[…](../../subsystems/
   X.md#anchor)` link to the page they are said to contradict. A row that cites
   its counterpart cannot be an independent claim. This is cheap, needs no
   table, and would clear the great majority of the 39 while leaving
   `FUN_801D5DE0` (whose three carriers cited nobody) firing.
2. A `directory_page_topics` map from each `functions/*.md` stem to the
   subsystem stems it legitimately files. More precise, but it is a hand-kept
   table that will rot.

The residue after either would be the rows worth a human: the per-image aliases,
which need an image-tag comparison rather than a name comparison, and which the
page-token rule can never settle.

## Reproducing

    ln -s <main-checkout>/ghidra/scripts/funcs ghidra/scripts/funcs   # gitignored
    python3 scripts/ci/check-port-provenance.py --signal dual-label
    python3 scripts/ci/check-port-provenance.py --addr 801cfc40
