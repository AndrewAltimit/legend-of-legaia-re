# Open reverse-engineering threads

An index of still-open hunts and the negative findings worth not re-walking. Rows are *questions*, not progress markers — each entry describes what is settled, what remains, and what would close it. Closing a thread removes or rewrites the row; nothing here counts ports, tests, or coverage.

Use this page to find what's worth digging into next. The detailed write-ups, captures, and decompiler dumps live in the per-topic memory files (`~/.claude/projects/.../memory/project_<slug>.md`) and in the linked docs.

Status conventions:

- **open** — active hunt; concrete next step exists.
- **partial** — main result pinned; a residual sub-question remains.
- **falsified** — hypothesis disproved; row kept so the path isn't re-walked.

---

## World map / kingdom bundles

| Thread | Status | What would close it | Memory |
|---|---|---|---|
| Kingdom slot 4 — per-record semantic | open | Identify which body bytes feed each per-kind handler in the slot-4 walker. Container layout + consumer (`FUN_80043390` + 22 kind handlers, kinds 8–19 across 3 banks) are already pinned. | `project_slot4_is_wireframe_not_terrain.md` |
| Slot-4 → cluster-A converter site | open | Find the function that walks the slot-4 outer pack and feeds cluster A. The converter does not run as a direct overlay read of `_DAT_8007B888`; the populating site is either an SCUS function-pointer table or an unscanned overlay. | `project_open_work_slot4_cluster_a.md` |
| `DAT_8007C018[45..53]` mid-load vertex-pool pointers | open | Single Lua write-watchpoint capture on `0x8007C018 + 45*4` during scene load to disambiguate stale-pointer vs. live-data. Steady-state model says reads past `DAT_8007BB38` are stale and never consumed; the mid-load snapshot deserves direct confirmation. | `project_dat_8007c018_global_tmd_table.md` |
| PROT 0874 section-0 outer producer | partial | Find the dispatch site that funnels PROT 0874 section 0 through the 3-section `parse_player_lzs` shape into `FUN_80020224` → `FUN_8001F05C` case 2 → `FUN_80026B4C`. Inner dispatch is pinned; outer producer is not. Likely lives in an overlay-resident scene loader (e.g. `FUN_801D6704` family). | `project_global_tmd_pool_source.md` + `project_next_session_backlog.md` § D |
| Drake uncapped cluster-A totals | open | Re-run `autorun_slot4_dispatcher_args.lua` with `LEGAIA_PC_CAP=50000` and a `timeout --kill-after=30s 1500s` wrapper. Drake saturated 7 of 9 PCs at PC_CAP=5000; raising the cap closes the cross-kingdom delta table. | `project_open_work_slot4_cluster_a.md` |
| Slot-4 freeze flag `_DAT_8007B824` | open | Write-breakpoint probe on `_DAT_8007B824` during retail play. Either an undumped overlay sets the freeze flag, or the BSS-init zero holds through retail and the "persistent slots" semantic is vestigial. | `project_open_work_slot4_cluster_a.md` |
| World-map outline / coastline reading | falsified | Visual inspection plus the slot-4 record-semantic work refuted the "world-map overlay outlines / coastline wireframe" interpretation. Bodies are most likely small object-local 3D meshes; treat any future "kingdom border lines" claim with suspicion. | `project_slot4_is_wireframe_not_terrain.md` |

## Battle / arts / level-up

| Thread | Status | What would close it | Memory |
|---|---|---|---|
| Encounter record carrier | resolved (no array) | Decompile of `FUN_801DE840` shows install handlers all use `pbVar43 = param_1 + param_2` (current opcode pointer in the field-VM script bytecode); each scripted encounter is its own dispatcher-op site (cases `0x37`/`0x41`, `0x38`, `0x43`, `0x47`, `0x4C`), with monster count/ids inline as the trailing operand bytes. No separate encounter-record table on disc. See `docs/formats/encounter.md` writer table. | `project_encounter_record_format.md` |
| Random-encounter trigger path | resolved | `FUN_801D9E1C` is the per-step roll function (rate counter `_DAT_8007B5FC`, scaled by config `_DAT_8007B5F8`). On counter underflow it picks a formation from the matching region's RNG range and installs `actor[+0x94] = formation_table_base + 1 + id * stride`, raises bit `0x80000`. Per-scene control block at `_DAT_801C6EA4 + 0x20/0x24/0x28` is populated by `FUN_8003A110` ("Mesworks set encount group table") from the MAN asset (type 0x03) buffered at `_DAT_8007B898`. See `docs/formats/encounter.md` § "Random-encounter trigger path". | `project_random_encounter_trigger_path.md` |
| Encounter MAN sub-section layout | open | `FUN_8003AEB0` walks the MAN multi-section header (sections at MAN offsets `+0x22, +0x24, +0x26, +0x28` read as signed 16-bit LE) before reaching the encounter section. Decode the full section layout so the encounter-section's in-MAN offset is known for every scene; this also pins the world-overview actor placement section that `FUN_8003A1E4` consumes. **Region-table section found:** the per-scene control block `_DAT_801c6ea4 + 0x4` points at a count-prefixed (`buffer[0]` = record count) array of 18-byte region records; `FUN_801dba20(tileX, tileZ)` returns the record whose tile-space bounding box `bytes[1..4] = [minX, minZ, maxX, maxZ]` contains the player tile (`tile = (player_pos - 0x40) >> 7`). `byte[0]` is a kind selector; `bytes[5..17]` are the region payload (still to be split). Consumed by the field camera arrival handler `FUN_801dbec4` and camera-config `FUN_801dbc20`. | `project_random_encounter_trigger_path.md` |
| Super / Miracle Arts trigger logic | open | Port the find/replace trigger matcher into `engine-vm` battle action. Tables and constants are in `legaia-art`; the trigger SM driving "find string in queue → replace" is not yet ported. | `project_arts_system.md` |
| Seru growth-rate extraction | open | Extract per-Seru stat-growth bytes (Seru struct `+0x74`) from a Seru-data PROT entry, surface as typed `legaia_gamedata::SeruGrowth`, and wire into `apply_battle_loot` for level-up stat application. | `project_shop_ui_and_levelup.md` + `project_next_session_backlog.md` § H |
| Terra slot-3 / story-flag overlap | open | Diff a real Terra-in-party memory-card save against the slot layout. `RETAIL_CHAR_RECORD_HEADER_SIZE + 3 * 0x414 = 0x14AB` collides with the `0x14C0` story-flag region — either Terra's record is shorter than 0x414, or the engine special-cases slot 3, or the header-size constant drifts. | `project_next_session_backlog.md` § G |
| Navmesh / per-scene navigation data | falsified | `0x80108EA4..0x80109550` is per-scene GPU primitive scratch, not a 24-byte stride navmesh. Pointer hunts find zero RAM cells pointing into the window. Real per-scene region / collision / event-trigger data lives in field-pack schema slots; the encounter-record path lives at `actor[+0x94]`. | `project_navmesh_negative_finding.md` |

## Field / locomotion

| Thread | Status | What would close it | Memory |
|---|---|---|---|
| Town/field free-movement locomotion | resolved | The player free-movement controller is `FUN_801d01b0` (field overlay 0897), pinned by a runtime write-watchpoint on `*(0x8007c364) + 0x14/0x18` (`autorun_player_pos_watch.lua`). It camera-remaps the held pad (`func_0x800467e8` + `FUN_80046494` → direction bits `& 0xf000`), computes a per-frame speed (`base_step * player[+0x72] >> 12 * DAT_1f800393`, with terrain-slow + diagonal modifiers), then steps the player position 2 units at a time with per-axis collision via `FUN_801cfe4c`. Sets facing `player[+0x26]`. Full write-up in [`subsystems/field-locomotion.md`](../subsystems/field-locomotion.md). The `801db81c..801dbf9c` cluster previously suspected here is the field *camera* system, not movement (see the camera notes in `project_field_camera_and_region_table.md`). | `project_field_locomotion_integrator.md` |
| Field collision-map source | resolved | The collision grid at `*(_DAT_1f8003ec) + 0x4000` (1 byte/128-unit tile, high nibble = 4 sub-cell wall bits) is **painted by the field-VM `0x4C` opcode, outer-nibble 7** (`op0` ∈ `0x70..0x7F`, handler `0x801e1c64`): a rectangular wall-paint with inline operands `[4C, 0x7s, col0, row0, col1, row1, mask]`, sub-op = clear-walkable / block-all / clear-mask / set-mask. So collision walls are authored in the scene event script (not a separate disc blob) — same inline-operand pattern as encounters / tile-board. The sibling terrain-flag grid at `+0x8000` is MAN-sourced (`FUN_8003aeb0` ← `_DAT_8007b898`). See [`subsystems/field-locomotion.md`](../subsystems/field-locomotion.md#where-the-collision-grid-comes-from). Residual sub-questions: the `+0x4000` zero-init site, and the low-nibble field (read by `FUN_8003a55c`). | `project_field_locomotion_integrator.md` |
| Tile-board grid mode | resolved (re-scoped) | The `_DAT_8007b450`/`DAT_801f35c0`/`801ef2b0` tile-grid walk is a puzzle / board minigame (procedural `rand`-filled board, per-cell drawn tiles), not town locomotion. Documented in `docs/subsystems/tile-board.md`. Open sub-questions: which minigames use it; whether any board is fixed (inline-script cells) vs. always procedural; the inline cell-array offset. | `project_tile_board_grid.md` |

## Text / fonts / dialog

| Thread | Status | What would close it | Memory |
|---|---|---|---|
| Dialog font extraction | done — kept for reference | Earlier "blocked on runtime trace" framing was wrong; tile-page lives at VRAM `(896, 0)..(960, 256)`, extracted by `legaia-font::font-extract` from any in-game save state. Listed here only so the older "open" framing doesn't get re-opened. | `project_dialog_font_hunt.md` |

## Title / boot / overlays

| Thread | Status | What would close it | Memory |
|---|---|---|---|
| `title.pak` PROT entry | open | Locate the dedicated title-screen PROT entry. `PROT 0895 = init.pak` is pinned (publisher logos + dev strings); a separate `title.pak` PROT entry has not yet been pinned. | `project_prot_0895_init_pak.md` |
| Title screen mode-table PROT | open | Pin the PROT entry referenced by the title-screen mode in the 14-entry mode table at `0x8007078C`. Adjacent entries are settled (PROT 973 = slot machine, 899 = options menu); title is still open. | `project_mode_table_structure.md` |
| Load-screen panel 9-slice geometry | open | Determine the 9-slice tile geometry the load-screen panel uses to draw its frame. TIM + CLUT source is pinned (`PROT.DAT[0x018E0]` CLUT row 2, byte-equal to retail VRAM). | `project_load_screen_panel_source_pinned.md` |
| Debug flags `0x8007B8C2` / `0x8007B98F` | open | Identify the overlay-resident writers of the two debug flags. Static analysis finds zero SCUS writers, but the dual-mode loader pattern at `FUN_8003E360` reads `_DAT_8007B8C2`, so at least one writer must exist in an unscanned overlay. | `project_debug_flags.md` |
| XP-table reader (LUI+ADDIU) | falsified | Zero LUI+ADDIU references to `0x8007123C` across SCUS plus every captured overlay. The XP table is still real (98 u16 increments, L1→2 = 50, mirrored in `retail_xp_table()`); the lookup is either through a `gp`-relative load or an indirection that LUI+ADDIU scans cannot catch. | `project_xp_split_static_negative.md` + `project_xp_table_and_cutscene_overlays.md` |

---

## When to add a row

A thread belongs here when:

1. There is something *specific* that would close it — a probe to run, a dump to read, a function to port. "Generally understand X better" is not closable; skip.
2. The next step is non-obvious from the code or git log. If `grep` would surface it, no row needed.
3. The detail lives elsewhere (a memory entry, a docs page, a Ghidra dump). The row is the pointer, not the analysis.

When the thread closes, rewrite the row to a `falsified` or `done — kept for reference` line if the path was instructive enough to warrant a "do not re-walk" marker; otherwise delete the row. Rotating the page is part of using it.

## Related pages

- [`docs/tooling/port-catalog.md`](../tooling/port-catalog.md) — per-function dumped × documented × ported × ignored axes. `port-catalog.py --missing-ports` is the function-level companion to this page's question-level index.
- [`docs/reference/functions.md`](functions.md) — canonical function directory; the place to learn what a `FUN_<addr>` mentioned in a row actually does.
- [`scripts/port-catalog-ignore.toml`](../../scripts/port-catalog-ignore.toml) — addresses explicitly *not* worth investigating (statically-linked PsyQ infra). Disjoint from this page.
