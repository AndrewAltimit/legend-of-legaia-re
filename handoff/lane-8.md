# Lane 8 handoff - menu-window `renderer_va` dispatch

What landed, what it can and cannot close, and what other owners would need
to do to finish the block.

## What landed

- `engine-ui::ui_menu_window_dispatch` - `painter_for_renderer_va` /
  `painter_for` / `painter_at` / `painter_rect` / `menu_window_painters`.
  Maps a parsed `legaia_asset::menu_windows` descriptor to the painter that
  draws its content, the port of the retail walker's indirect call
  (`FUN_80031D00` `0x80031E30..0x80031E44`, `jalr` on the live window `+0x28`
  that `FUN_800326AC` copied from the descriptor `+0xC`).
- The native window routes through it: pause-screen tabs
  (`window/menu_draws.rs`) and the shop's vendor plate / purse / item-info /
  sell-quantity windows (`window/shop_windows.rs`, new).
- Disc oracle `engine-shell/tests/menu_window_dispatch_real.rs` - asserts the
  dispatch resolves exactly the retail table's own renderers, at the retail
  ids, with the descriptor's rect. Skips + passes without a disc.

## The thing the next lane needs to know first

**No waiver in this block can be *deleted* from a native-only lane.** The
drift checker's buckets are: both hosts -> closed (delete), native-only ->
DRIFT (needs a `web_missing` waiver), neither -> ORPHAN. Wiring a painter
natively therefore moves it `orphan` -> `web_missing`; only a matching
`crates/web-viewer` call can delete the waiver, and web-viewer belongs to
Lane 9. Five painters made that move here; the file records why for each.

### The drift line reads like a regression and is not one

`75 builders: 51 both / 4 native-only / 0 web-only / 20 unused` became
`75 builders: 51 both / 9 native-only / 0 web-only / 15 unused`.

Five builders left the `unused` bucket - *nobody draws this* - and landed in
`native-only` - *native draws it, web does not*. The only bucket that can
absorb them from a scope excluding `crates/web-viewer` is the one the gate
calls DRIFT, so **progress here raises the drift count by construction**.
Read the pair of numbers, never `native-only` alone: `unused` falling is the
work landing, and `native-only` rising by the same five is the arithmetic
consequence, not new drift. The number goes back down when the web half
lands, and only then.

### What the web half needs

`crates/web-viewer/src/play_menu.rs` (or a sibling) needs three things, none
of them new geometry:

1. the parsed table - the page never calls `legaia_asset::menu_windows`; it
   needs the boot-time parse the native host keeps in
   `PlayWindowApp::menu_window_table`;
2. `painter_at(table, id, expected)` + `painter_rect(d)` at the tab draw site,
   which today calls `tab_label_draws` with a pen (that alias now delegates to
   `title_tab_draws_for`, so the geometry is already identical);
3. for the four shop windows, a shop host at all - nothing in
   `crates/web-viewer/src` opens a `ShopSession`.

Doing (1) + (2) alone deletes the `title_tab_draws_for` waiver.

## Requests for other owners

**`engine-core` (Lanes 4/5).** Three gaps block three more painters, and each
is a data gap rather than a draw gap:

- `ShopSession` / `ShopInventory` drop the vendor name that
  `legaia_asset::shop_stock` decodes (`SceneShop::name` keeps it, but the
  live session does not carry which `SceneShop` it came from). The native
  host currently recovers it by matching stock lists, which is a workaround -
  a `vendor_name: String` (or a `scene_shop_index`) on `ShopSession` would
  make window 33 exact.
- There is **no Point Card counter**. Retail's window 31 prints
  `_DAT_800845B4`, credited by the buy handler before the gold debit (see
  `docs/subsystems/shop.md`); `World` has `money` and `casino_coins` and
  nothing else that window could be printing. That is the whole blocker for
  `amount_prompt_draws_for`.
- `EquipSession` is single-character by construction, so nothing produces the
  party-wide preview window 41 prints (per member: "Equipped" / "Cannot
  Equip" / the ATK-UDF-LDF triple under a trial equip). That plus a host
  screen that opens windows 25 / 41 is what `compare_panel_draws_for` needs.

**`scripts/ci/check-ui-host-drift.py` (measurement instrument, not edited).**
One proposal: the gate currently cannot express "reached one call deep", which
is why `ap_gauge_sprites` and `sprite_draws_for` need prose waivers and why
`tab_label_draws` reads as wired while `title_tab_draws_for` - the function it
delegates to - would read as unused if the host stopped naming it. A cheap fix
is to seed the used-set transitively: after collecting host references, also
mark any builder referenced from inside a builder that is itself used. That
would close two standing waivers by measurement rather than by prose, and
would stop rewarding a host for naming the shallowest wrapper.

## Left open (waived, with the real blocker recorded per builder)

| Builder | Window | Remaining blocker |
|---|---|---|
| `char_prompt_draws_for` | 7 | which flow opens it, and what `record[0x13D + sel]` holds |
| `amount_prompt_draws_for` | 31 | no Point Card counter in `World` |
| `count_panel_draws_for` | 24 | painter is only the delta over the shared item-info panel `FUN_801D0F1C`; which screen opens 24 rather than 17 (identical rect) is unknown |
| `choice_panel_draws_for` / `two_line_choice_panel_draws_for` | 46 / 5 | adopting them replaces the engine options screen's layout - a host decision |
| `label_list_draws_for` | 6 | no host opens window 6; the pause command list is window 50 |
| `equip_target_list_draws_for` | 36 | its driver is the party-target panel `FUN_801D8308`, not ported; retail's shop open script does not list window 36, so wiring it there would invent a screen |
| `compare_panel_draws_for` | 25 + 41 | no host screen opens either window; see the `engine-core` request above |

## Host-wiring worklist: every `engine-ui` / `engine-shell` disclosed-inert row

Triaged against `port-catalog.py --live-audit`. The verdict column is what a
future lane needs; the rule applied throughout is that a `NOT WIRED:` note
comes off **only** when the caller chain from a host root can be named, and
that no screen or trigger gets invented to manufacture one. Fold this into
`docs/tooling/live-audit-triage.md` at integration (out of this lane's scope).

| Addr(s) | Site | Verdict |
|---|---|---|
| `801dcfe4` + the five pause-tab VAs | `title_tab_draws_for` | **WIRED** - `window/menu_draws.rs` dispatches on `renderer_va` |
| `801dcf14` | `record_title_tab_draws_for` | **WIRED** - shop vendor plate |
| `801dcf84` / `801dd028` | `counter_panel_draws_for` | **WIRED** - shop purse (window 45's screen still unopened) |
| `801d4a80` | `item_description_draws_for` | **WIRED** - shop item info |
| `801d5944` | `sell_quantity_draws_for` | **WIRED** - shop sell quantity |
| `8003c1f8` | separator glyph | **WIRED** - extracted as a shared primitive and called from the sell row; the address was already live through `engine-render`'s own port of the same routine, so this adds a second, precise site rather than a new live address |
| `801d1290` / `801d4c28` | compare panels | blocked: no host opens windows 25 / 41; needs a party-wide preview in `engine-core` |
| `801d56fc` / `801d603c` / `801d61b0` / `801d6360` / `801dcc20` / `801dccb4` / `801dce20` / `801e4140` | painter block | blocked: unknown owning screen, a host layout decision, or missing world state - per-builder table above |
| `801ed710` / `80034e4c` | records screen | blocked in `engine-core`: `World` tracks none of the lifetime counters it prints (verified - no battle / escape / knockout / max-damage / treasure fields exist), so a wired host would draw a readout of state the engine never produces |
| `801d050c` / `801d08ec` / `801d1308` | `other_game_hud` | blocked: needs the arena HUD screen **and** a parser for the 0977 overlay's sprite-descriptor table (a `legaia-asset` job) |
| `8004c650` | `battle_name_banner` | blocked, but its note was **wrong** and is now corrected - see below |
| `8003d53c` / `8004fcc8` | `xa_clip` | blocked: needs a streaming CD/voice output; the engine's two XA consumers read whole files up front |
| `801e0418` | `card_message_rows` | blocked: needs a raw PSX memory-card backend plus the overlay message-string table |
| `801cf5d0` | equip stat-block seeder | **newly ported** here, and honestly disclosed inert: its only consumers are the two compare panels above |

### The one stale disclosure found

`battle_name_banner`'s note claimed the driving state - which actor committed
which Art - does not exist. It does: `World::drain_battle_shout_cues` yields
`(cslot, action)` per executed party art and the native battle host already
drains it every frame to fire the CD-XA shout. A future lane would have
started by building a trigger that is already there. The real blockers, now
recorded in the module, are an **id-space bridge** (the cue carries an
ActionConstant; the name table is keyed by a per-character display index, and
nothing maps between them - the Learned-Art-Constant slot is a third index its
own docs warn is not 1:1) and a **Y coordinate** (`FUN_8004C650` writes only
X, into four sub-primitive fields nothing in the engine emits).

## RE findings worth keeping

- **Window 33 is "the armed op-`0x49` record's trailing string"**, not a
  fixed prompt. `_DAT_8007B450` points at the opcode's sub-op byte
  (opcode `+1`), so `FUN_801DCF14`'s `record + record[2] + 3` lands at
  `opcode + 4 + record[2]`. For a shop record (`[count][ids][name]`) that is
  exactly one past the last item id - the vendor name. This reconciles
  `field-menu.md` (prize-exchange "prompt line") with `shop.md` (window
  `0x21` = vendor plate): same renderer, different armed record.
- The six plain title tabs (`FUN_801DCA0C` / `CA50` / `CA94` / `CAD8` /
  `CB1C` / `CFE4`) are byte-identical bar the string pointer, so they are one
  painter, not six.
- The two counter windows differ only in a pictogram literal and which global
  they load: 32 = party gold `_DAT_8008459C` + `0x62`, 45 = casino coin bank
  `_DAT_800845A4` + `0x66`.
- Three painters stage the accent pen (`_DAT_8007B454 = 6`) for one field:
  window 34's name **and** owned count (staged once before the name, restored
  only after the count), window 24's count, window 31's number. The port drew
  all of them white; now fixed.
- `FUN_801DCC20` (window 24) opens its gated body with `jal 0x801D0F1C` - the
  same shared item-info panel window 17 draws - so window 24 is that panel
  plus a count, not a standalone count window.
- Window 37's row draws a **separator glyph** between the quantity and the
  held count (`FUN_8003C1F8(6, WX+0x20, row)`) that the port was dropping.
  Glyph `6` is `/` in the records screen's own glyph table, which is what
  that row reads as - two independent uses pinning one id space.
- `FUN_801CF5D0` is a frameless leaf: `((slot<<6 + slot)<<2 + slot)<<2` is the
  `0x414` record stride, and every field is `lhu`-read (zero-extended) before
  being stored as a word. An `lh` reading would flip a high-bit HP maximum
  negative; the port's test pins that direction.
