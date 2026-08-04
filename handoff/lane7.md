# Lane 7 handoff - the menu / save / memory-card disclosed-inert slice

Work that belongs to this slice but lands in files another lane owns, plus
the threads a follow-up should pick up first.

## Out of scope, wanted by this slice

### `sub15_swap_rows` (`801da2a0`) is the one real wire left in the slice

`crates/engine-core/src/save_subscreen.rs` already records that its own
old reason was wrong: the backing array for a spell-list reorder **is**
the character record's `+0x13D` / `+0x161` pair, `build_spell_session`
lists it in record order, and `field_menu_subsession_e2e`'s
`spell_swap_permutes_the_magic_screen_order` pins that a swap moves the
Magic screen. What is missing is a screen that offers the exchange.

Building it needs three files this lane could not touch:

- `crates/engine-core/src/spell_menu.rs` - a reorder mode on the session
  (retail's step pair `2..4` / `5..7`, settle then browse, and the
  `0x1000` editing bit the commit raises).
- `crates/engine-core/src/field_menu_dispatch.rs` - the sub-session arm.
- `crates/engine-core/src/save_subscreen.rs` is in this lane's scope, so
  the kernel side is ready to be called as-is.

Both hosts already exist (`window/menu_draws.rs`, `web-viewer/play_menu.rs`),
so this is a session-side edit plus a pad binding, not a new screen.

### The op-`0x49` entry-context sub-op blocks three waived painters at once

`two_line_choice_panel_draws_for` (window 5), `label_list_draws_for`
(window 6) and - through the prize exchange - `choice_panel_draws_for`
(window 46) all wait on the same thing: retail's entry-context byte
reaching a value the port cannot produce. `World::menu_entry_context_kind`
resolves to `0`, `5` or `None` because the op-`0x49` park never records
its armed sub-op, so `0x0D` (locked menu) and `0x07` (prize counter) are
unreachable values.

The edit is in `crates/engine-core/src/world/vm_hosts.rs`
(`op49_menu_request`) plus `world/save.rs`, both sibling-owned. One change
unblocks three waivers; each waiver's prose already names it.

### The casino prize counter has a session, a coin bank, and no parser

`engine-core::prize_exchange` is complete and has **zero** references
outside its own tests. `World::casino_coins` exists and
`World::open_coin_counter` is live, so the hub side is real. The missing
piece is the prize table: `legaia_patcher::casino` knows it is at PROT
0899 file `0x15D00` (VA `0x801E4518`, `0x60`-byte blocks of
`[u16 id][u16 gate][u32 price]`, block picked by the entry-context byte at
`ptr+1`) and **`legaia_asset` has no parser for it**, so `engine-core`
cannot read it. Add `legaia_asset::prize_table` and the session becomes
mountable from both hosts; the confirm beat is window 46's painter.

Table bytes verified at file `0x15D00`: first record `id=0xD0 gate=0x3A
price=10000`, which matches the patcher's own reading.

### `card_flow::block_title_digits` (`801e1934`) is a `DELETE`

It is a byte-for-byte duplicate of `legaia_save::card::save_title_digits`,
which carries the **same** `PORT: FUN_801e1934` tag with the same address
range and is live (the browser card rack writes through it). Its only
non-test caller is its own file's `save_block_summary`, which keeps the
address anchor. Deleting it and delegating is the `symbol_pad_bit`
precedent applied verbatim.

Not done here only because the delete touches
`crates/engine-core/tests/save_block_checksum.rs`, which is outside this
lane's `tests/menu_*` + `tests/card_*` grant.
`docs/tooling/stale-not-wired-triage.md` already records the pair.

### The card cluster's ten anchors are one backend, and it is browser-shaped

`card_bu_io.rs` + `card_flow.rs`'s state machines + `save_select.rs`'s
`card_frame_tick` all wait on an asynchronous card backend behind
`CardIoMachine` - an issue, a per-frame poll, a completion. The bytes half
exists (`legaia_save::emu::CardView` + the block checksum); only the shape
is missing. The one host that could grow it is the browser rack, because
it is the only one with real card images; the native shell saves LGSF
files and never forms a `bu` path. That asymmetry is deliberate and should
be stated when the backend lands, or the host-drift gate's next reader
will read it as drift.

## Findings a sibling lane may care about

- **`FUN_801E2EE4` is a sprite-quad emitter, not a text drawer.** Its 4th
  argument indexes a 20-byte-stride descriptor table at `0x801E50A8`
  (PROT 0899 file `0x16890`) of pre-rendered VRAM strips. Anything else in
  the tree that reads a "message slot" through it is reading a sprite id.
  `docs/reference/functions/menus.md` had it right all along.
- **`0x801EF03C` / `0x801EF054`** hold the two save-filename prefixes and
  are inside PROT 0899's footprint but **past** the `clean_copy_bytes`
  prefix in `static-overlays.toml` - so a tool that stops at the verified
  prefix will not find them. They are still on the disc.
- `list_alloc` (`80030104`) resets a fourth global the port's doc did not
  list (`0x8007BB60 = -1`); the doc is corrected, the port is unaffected
  because the mirrors are caller-owned.
- **One `PRO_` spelling survives**, in `crates/save/src/emu.rs:394` - a
  `claim_block` test fixture, where the name is arbitrary so nothing is
  wrong. Worth respelling anyway: it is the same literal the matcher had
  wrong, and a future reader grepping for the prefix finds a spelling the
  disc does not carry. `crates/save` is outside this lane's grant.
