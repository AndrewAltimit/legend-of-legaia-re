# Lane E handoff - host-wiring drain in `engine-ui` / `engine-shell`

Scope was the two host crates and the disclosed-inert rows whose named
prerequisite is a host-side call inside them.

## Numbers (two of them, as asked)

Run the audit from **this worktree** - see "the worktree can run the audit"
below.

| Measure | Before | After |
|---|---|---|
| `ported + live` (addresses) | 529 | 530 |
| PORT anchors, live | 787 | 795 |
| PORT anchors, disclosed inert | 460 | 453 |
| `engine-ui` + `engine-shell` disclosed-inert anchors | 37 | 32 |
| stale `NOT WIRED` (tagged inert, analysed live) | 12 | 14 |
| ui-host-drift `unused` / `native-only` | 15 / 8 | 14 / 9 |

**Wired: 2 addresses / 7 anchors.** `801ED710` (records-screen layout) and
`80034E4C` (zero-padded decimal field) in `engine-ui`, plus the two
`engine-vm` model anchors they now reach. Address-level `live` moves by only
`+1` because `80034E4C` was already live through `engine-render`'s own port of
the same primitive - the `engine-ui` anchors were the inert half.

**Still disclosed: 16 addresses / 32 anchors** in the two crates. Verdicts
below; two of them are corrected rather than closed.

## The worktree CAN run the audit

`port-catalog.py --live-audit` reports `dumped: 0` from a worktree only because
`ghidra/scripts/funcs/` is missing there. Symlinking it in
(`ln -s <repo>/ghidra/scripts/funcs ghidra/scripts/funcs`) makes the audit
identical to the main checkout's, so a lane can and should self-check its own
disclosure gaps. That is what caught the two stale rows below before hand-off
rather than at integration.

## What landed

### 1. Records page, wired from the developer-menu host

`engine-ui::ui_menu::records_screen` gains `SAVE_BLOCK_TO_RECORD`,
`record_offset::*` and `record_counters()` - the rebase that turns retail's
save-block displacements into offsets a host holding a bare `0x414` character
record can serve. `window/dev_menu.rs` draws the page from the existing
`LEGAIA_DEV_MENU` host (Square swaps the row list for it), feeding the six
per-character counters from `world.roster.members[0..3].raw` and the clock from
`World::play_time_seconds`, through the ported model
`engine-vm::world_map_overlay::records_screen`.

**Caller chain from a real host root:** `impl ApplicationHandler for
PlayWindowApp` -> `window_event` -> `redraw` -> `tick_dev_menu` ->
`build_dev_records_draws` -> `records_screen_draws_for` ->
`zero_number_draws`. No `#[cfg(test)]` site is involved; the audit confirms it
independently.

The old disclosure claimed two blockers and both were wrong in part: there
**is** a dev-menu host now, and `World::play_time_seconds` **does** exist. What
is genuinely missing is narrower and is stated in the module: no lifetime
battle / escape tally and no treasure census, so those read zero and the
treasure line stays hidden - which is also the page retail draws off a save
that never incremented them.

### 2. The dev menu was reading a bit-scrambled pad word (real bug, fixed)

`World::set_pad` forwards its argument straight into the retail pad pump
(`RetailPadState::pump_packed`), but the native host passes the **raw** PSX pad
word (`PadButton`, `Start = 0x0008`) while the pump's own module doc says the
packed layout is *not* that word (`Start = 0x0800`). So
`InputState::retail_pad()` republishes the raw word under packed field names,
and `window/dev_menu.rs` - its only consumer outside `engine-core` - read every
button through the wrong bit: Up arrived as `PACK_TRIANGLE`, Cross as
`PACK_DOWN`, Square as `PACK_LEFT`. The dev menu has been unusable.

Fixed host-side with `retail_packed()` (the two layouts are one byte swap;
unit-tested against both documented tables plus `retail_pad`'s own decode
test). **The root cause is in `engine-core::input::set_pad`, which is out of
this lane's scope** - it should re-pack before calling `pump_packed`, or the
hosts should feed `set_pad_reports`. Until then any future `retail_pad()`
consumer inherits the same defect.

I only found this because the first screenshot of the wired page came back
showing the *row list* - the Square press never landed. A call-site check would
have passed.

### 3. `World::play_time_seconds` never advanced (real bug, fixed)

`World::advance_play_time` had **zero callers in the workspace**, despite its
own doc saying "Engines drive this from the frame loop's wall-clock delta". The
play clock therefore only ever changed when a save was loaded, so the save
screen's play-time column, the seru-trade gate
(`World::open_seru_trade`) and the new Records page all read a frozen value.
`PlayWindowApp::tick_play_clock` now advances it once per redraw, by **delta**
against a host-side high-water mark so a loaded save keeps its accumulated
total. Verified live: at `t 11.8s` on the HUD the page prints `0:00:11`.

This is not a `// PORT:` anchor, so it moves no count.

### 4. Two disclosures corrected, not closed

- **`guarded_box_rect` (`FUN_801E4140`)** - I nearly mis-wired this one. The
  host does draw window frames, so gating them on this guard looked free. It is
  not the same primitive: past the `y < 0xF1` test the routine calls
  `FUN_80034B6C` then `FUN_8002C69C(x, y, w, h)`, and the decompiled C's
  `func_0x80034b6c()` is the dropped-register-argument artifact - `a0`/`a1` are
  untouched from the prologue, and a live menu caller passes
  `(0x44, 0x02202020, …)`, i.e. a mode selector and a packed RGB word. It is a
  **shaded colour fill**, and `FUN_8002C69C` inflates its own rect by 8px per
  side, so the guard tests the *content* y while the hosts' atlas chrome is
  drawn at the already-inflated frame rect. Stays disclosed, with that recorded
  in the module and in `field-menu.md`.
- **`char_prompt_draws_for` (window 7, `FUN_801DCCB4`)** - the substituted byte
  `record[0x13D + sel]` is the first entry of the character's **learned-magic id
  list**, whose length byte is `+0x13C` - the same field the records page prints
  under "Magic". So `DAT_8007BB78` is a list index and the glyph is a spell id;
  window 7 is a magic-side prompt. Narrows the flow but does not name it, so it
  stays disclosed.

## Requests for the integrator

### A. `scripts/ci/ui-host-drift-waivers.toml` (not edited, per instructions)

`records_screen_draws_for` moved orphan -> native-only, so the gate now fails
with `waiver kind is 'orphan' but the builder is native-only`. Change that
waiver's `kind` to `"web_missing"` and replace its reason. Proposed text,
matching the shape of the `dev_menu_list_draws_for` waiver directly below it
(same root, same scope argument):

```toml
[[waiver]]
builder = "records_screen_draws_for"
kind = "web_missing"
reason = """
The retail world-map Records screen renderer (`FUN_801ED710`). The native
window draws it from the developer-menu host (`window/dev_menu.rs`, behind the
same `LEGAIA_DEV_MENU` opt-in as its list sibling `dev_menu_list_draws_for`
above); the browser play page exposes no debug-menu route - a debug surface is
deliberately out of the web page's scope rather than a wiring gap.
"""
```

Note the reason is phrased as a fact about the **whole native root**, not about
one page, which is the direction the file's own precedents fail in.

### B. `crates/engine-vm/src/world_map_overlay.rs` (out of scope)

Two `NOT WIRED:` notes are now stale - both are reached from the native host:

- `decompose_play_time` (line ~399): "reached only from `records_screen`, which
  has no host" - `records_screen` now has one.
- `records_screen` (line ~418): "the engine keeps none of the lifetime counters
  this reads and has no records page" - it has a page; the accurate remainder is
  "battles / escapes / treasure have no engine counter, so the host passes
  zero".

Also worth fixing while there: `CharRecordStats`'s doc says the block base is
`0x80088140`; it is `0x80084140`.

### C. `crates/engine-core/src/input.rs` (out of scope)

`set_pad` should convert the raw `PadButton` word to the retail packed layout
before `pump_packed` (a `u16::swap_bytes`), or the hosts should be moved onto
`set_pad_reports`. Today `retail_pad()` is a trap for any new consumer. The
host-side `retail_packed()` in `window/dev_menu.rs` becomes redundant the
moment that lands and should be deleted with it.

## Per-row verdicts for what stays disclosed

Fold into `docs/tooling/live-audit-triage.md` at integration if useful.

| Addr | Symbol | Verdict |
|---|---|---|
| `801ED710` / `80034E4C` | records screen | **WIRED** - dev-menu host page |
| `801E4140` | `guarded_box_rect` | blocked: a colour-fill primitive no host emits; not the atlas chrome - see above |
| `801DCCB4` | `char_prompt_draws_for` | blocked: owning flow unknown, now narrowed to the magic side |
| `801DCE20` | `amount_prompt_draws_for` | blocked in `engine-core`: no Point Card counter |
| `801DCC20` | `count_panel_draws_for` | blocked: which screen opens 24 rather than 17 |
| `801D603C` / `801D61B0` | choice panels 46 / 5 | blocked: owning flow unknown. The standing note calls this an options-screen layout decision; the options screen is windows **48 + 47** (`FUN_801DCEF0` / `FUN_801D2B44`), so these are not it and adopting them there would invent a screen |
| `801D6360` | `label_list_draws_for` | blocked: no host opens window 6 |
| `801D56FC` | `equip_target_list_draws_for` | blocked: driver `FUN_801D8308` not ported |
| `801CF5D0` / `801D1290` / `801D4C28` | equip stat block + compare panels | blocked in `engine-core`: `EquipSession` is single-character, so nothing produces window 41's party-wide preview |
| `801D050C` / `801D08EC` / `801D1308` | `other_game_hud` | blocked: needs the arena HUD screen **and** a `legaia-asset` parser for the 0977 sprite-descriptor table |
| `801E0418` | `card_message_rows` | blocked: needs a raw PSX card backend and the overlay message-string table |
| `8004C650` | `battle_name_banner` | blocked: the id-space bridge (ActionConstant -> arts-name-table `(row, display index)`) and the banner's Y. Lane 8's correction stands; nothing in the engine consumes `legaia_art::arts_table` at all yet, so the bridge is a new port, not a lookup |
| `8003D53C` / `8004FCC8` | `xa_clip` | blocked: needs a streaming CD device **and** a parser for the per-clip start-LBA table at `0x801C6ED8`, which nothing decodes - the second half is a `legaia-asset` job and is not in the standing note |

## Verification

- `cargo test -p legaia-engine-ui` (139 pass), `cargo test -p
  legaia-engine-shell --release` (all suites pass, disc-gated ones ran with
  `LEGAIA_DISC_BIN` set).
- `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings` on
  both crates.
- **Live**, not at the call site: `LEGAIA_DEV_MENU=1 legaia-engine play-window
  --disc … --seed-party --no-audio --pad-script "60:Square" --screenshot …
  --screenshot-tick 700`. The captured frame shows the page with retail's
  headings, the per-column ink staging, `0/15` and `0/22` maxima, the treasure
  line correctly absent, and the play clock reading `0:00:11` against the HUD's
  independent `t 11.8s`.
