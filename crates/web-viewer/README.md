# legaia-web-viewer

WebAssembly bindings for browsing a Legend of Legaia disc image in the
browser.

Auto-detects the input form: a full Mode2/2352 `.bin` disc, a raw
`PROT.DAT`, or a single `.tim`. After loading a disc, classifies every
PROT entry via `legaia_asset::categorize` and pre-scans for embedded
TIMs, so the UI shows a filtered, browsable list of viewable entries
instead of every raw entry.

## What's wrapped

- `legaia-iso` - disc reader.
- `legaia-prot` - TOC + CDNAME.
- `legaia-asset` - categorize + tim_scan + `monster_archive`.
- `legaia-lzs` - LZS decoder.
- `legaia-tmd` - mesh parser.
- `legaia-patcher` + `legaia-iso` - the randomizer / disc patcher (see `rom_patcher` below).

## Playing the port in the browser (`runtime` + `play`)

`LegaiaRuntime` is the engine, not a re-implementation of it: it owns a real
`legaia_engine_core::scene::SceneHost` - the same host the native
`legaia-engine play-window` drives - so the browser runs the ported field /
event VM, the free-movement controller against the per-scene walkability grid,
floor-height sampling, the NPC motion VMs, the interaction probe, and the
inline-script dialogue runner. Drives `site/play.html`.

The split is deliberate:

- **`runtime`** - the simulation. `load_disc` (user's own image, in memory, in
  their browser), `enter_field(name)`, `set_pad(mask)` (PSX pad word),
  `set_camera_azimuth(units)` (so the d-pad remaps camera-relative),
  `tick_frame()` (returns the label of the scene a door just walked into, so the
  page rebuilds around a transition), and `state_json()` (frame / mode / player
  transform / live dialogue box).
- **`play`** - what the page draws, resolved against the **same**
  `SceneResources` the host already built at scene entry (nothing is decoded
  twice): the assembled map (`field_*` accessors), the lead's field mesh posed
  each frame from the world's live `pose_frame` (`player_mesh_*`,
  `player_transform`), and the scene's MAN-placed NPCs at their live world
  positions (`play_npc_*`).

The map + NPC layers make the **native play-window's exact resolver calls**,
pinned by the disc-gated parity test `tests/play_parity.rs`:

- the placed-object layer goes through
  `field_env::resolve_placed_env_draws` **with the scene's object binds**, so
  a placed prop whose bind names a clip carries its `anim_id`
  (`field_placement_anim_ids`) and the page draws it through
  `field_mesh_posed(slot, anim)` - the frame-0 rest pose of scene ANM record
  `anim_id - 1` (cupboard doors closed on the cabinet's front face) with the
  native fallback to the raw mesh under the bone-count contract;
- the terrain sweep excludes `FLAG_PLACED` records (already drawn - posed -
  by the placement layer; the second copy would be the unposed one);
- the NPC catalog (`field_npc::build_npc_catalog_play`) lists **everything
  the native window draws**: the `model >= 0xF0` global-pool specials (save
  crystal / party heads, meshed from the world's pool and posed from the
  PROT 0874 locomotion bundle) and the clipless multi-object actors retail
  draws raw (draw kind 5), which the curated NPC-browser catalog withholds;
- a catalogued NPC's mesh truncates its TMD object table to its clip's bone
  count (the objects past it are equipment-swap templates), and a slot with
  no seeded heading renders at identity, both as the native draw pass does;
- NPC visibility follows the native hide-box contract: a header-parked
  placement uploads only when the scene-entry spawn-prologue pre-run seated it
  into the town (story-placed NPCs appear from frame one), and any slot whose
  **live** position is the off-map hide box (`field_offmap_hide_xz`) is
  skipped at draw time (story-parked actors never render);
- NPC clip playback runs in **sim-tick time**: `LegaiaRuntime` hosts one
  `FieldClipPlayer` per placed slot (the native `npc_clip_players` twin),
  advances it once per `tick_frame` (60 Hz ticks, 2 per clip frame - the
  retail cadence), re-targets it on channel op-`0x4B` ANIMATE cues, and
  serves the current frame (`play_npc_clip_states` / `play_npc_live_bones`)
  without moving the playhead - so clip cadence is refresh-rate-independent
  and scripted actors perform their cued beats.

The disc-gated `tests/play_parity_wave.rs` pins the hide-box contract, the
sim-tick clip cadence, and the opening-chain staging below.

## Scene BGM (`runtime`)

The play page plays the scene's music through the clean-room SPU + sequencer,
the browser twin of the native `AudioBgmDirector`. `audio_init()` opens a
`legaia_engine_audio::WebAudioOut` (must run inside a user gesture - browser
autoplay policy) and stages the scene's VAB bank; every `tick_frame()` then
routes the field VM's op-`0x35` BGM events through a `WebBgmDirector` that
implements `legaia_engine_core::scene::BgmDirector`. Scene-local starts
(`bgm_id < 2000`) play their SEQ through the pre-staged scene bank; global-pool
tracks (`>= 2000`, the path most real Legaia BGM takes) carry their own
`[chunk][pBAV VAB][pQES SEQ]` and upload it before playing - the same
`SceneHost::route_bgm_events` / `bgm_seq_bytes` / `music_bank_entry_bytes`
selection the native window uses, no new mapping. Tracks loop to the start and
cross-fade in; a redundant op-`0x35` re-emit of the same id is suppressed so
the playhead survives. `audio_resume()` (browsers open the context suspended),
`audio_set_gain()`, and `audio_ready()` round out the JS surface; routing and
staging no-op until audio is live, so the field VM's events stay queued until
the user enables sound. Not built into a disc-gated parity test - the SPU
render path is exercised by the media page (`audio_api`) and the native
`audio-trace` oracle.

### Output level

`BGM_DEFAULT_GAIN` is the gain the runtime parks on `WebAudioOut`'s post-mixer
`GainNode` at `audio_init`, and the play page's volume slider is a live handle
on the same node through `audio_set_gain`. It is **unity** - the level the
native cpal path runs at, and the same default the media page's BGM auditioner
hands its listener. An earlier revision baked in `5.0` on the claim that the
mixer is near-inaudible at unity; listening said the opposite (unity is
already on the loud side), and the hot default is how the page came to clip on
peaks. Any further lift belongs to the listener: the slider spans 0x (mute) to
10x, and its HTML `value` stays equal to the constant so the control starts
where the audio actually is.

## Retail dialog reading box (`play_dialog`)

The field NPC / event message box, served as the same `{ sprites, texts }`
quad lists the pause menu ships (`play_dialog_draws_json`) and blitted by the
page over the live, still-running field. Geometry is the traced pager's
(`FUN_801D84D0`): main centre rect `(0x26, 0x10, 0xF4, lines*0xF - 3)` at the
top of the 320x240 stage, picker `(0x26, 0x94 + ((4-n)*0xF)/2, ...)`, text
pen at the box origin with 15-px row pitch in the staged CLUT-7 white, the
chrome via `legaia_engine_ui::dialog_window_chrome_draws_for` (fill at
centre+4, border ring at centre+8) and the option / page-advance hand
sprites. The page's DOM text box remains only as a fallback for a cached
WASM without this API.

## Opening prologue chain (`play_cutscene`)

New Game runs the retail opening: entering `opdeene` through the engine arms
the chain (`opening_chain_active`), installs the cutscene timeline whose
SceneChange ops walk the remaining legs, and the page renders the
presentation layer from per-frame reads: the narration crawl + title card
(`play_cutscene_text_draws_json`, font quads at the roller's PSX 240-line
Ys), the "It was the Seru." caption (`cutscene_caption_*`, faded by the
engine's alpha), the prologue sepia grade + gold depth-cue ramp
(`play_cutscene_state_json` mirrors `World::scene_color_grade` /
`scene_depth_cue`; the page stages them as WebGL uniforms), the op-`0x45`
cutscene camera decode (`play_cutscene_camera_json`, mapped onto the page's
orbit projection - an approximation of the native PSX GTE camera), the
narration input lock, and the retail intro-skip
(`play_take_prologue_handoff` - Cross skips the whole remaining opening to
`town01`). One browser deviation, deliberate: FMV beats are auto-finished (no STR/MDEC
playback on the play path). The `town01` establishing-sweep timeline **does**
run, through to the name-entry overlay it opens (`play_name_entry` below).

Two things the browser host has to do that the native one gets for free:

- **Seating.** Scene entry puts the player at the retail *cold-boot spawn*,
  which is only meaningful for `town01` - every other scene is normally entered
  through a door that supplies the arrival tile. Picking a scene from a list has
  no door, so `LegaiaRuntime::seat_player` keeps the cold spawn when it has floor
  under it and otherwise seats the player on the walk-ground heightfield, never
  on a walk-on trigger tile (which would fire on the first tick and warp the
  scene out from under them).
- **Framing.** Retail authors a camera per scene. The page has one follow camera,
  so `site/js/play-app.js` culls any mesh straddling the camera-to-player line -
  without it a cave roof or a house's upper storey fills the screen.

## Retail pause menu (`play_menu`)

Start (Enter) opens the real retail field menu, drawn from the same wgpu-free
`legaia-engine-ui` builders the native `play-window` uses - **not a DOM
stand-in**. The site is just a different framebuffer over the same menu.

`LegaiaRuntime::play_menu_*` owns the state + navigation and serves two draw
lists (`play_menu_draws_json`):

- **Window chrome** - the gold 9-slice + navy filigree as sprite quads off the
  disc's menu-UI atlas (`save_menu_atlas::build_atlas` over PROT 0899 + the
  PROT.DAT system-UI sheet).
- **Labels** - font-glyph quads sampling the **real retail proportional dialog
  font**, decoded straight from the disc
  (`legaia_font::Font::from_disc_tim_and_scus` over the PROT.DAT font TIM at
  `0x7F40` + the SCUS width table). Byte-identical to the save-state
  extraction, with no `extracted/` artifacts needed.

Window geometry is the disc-parsed descriptor table
(`legaia_asset::menu_windows`) with the native window's pinned fallback.

The two atlases upload once (`play_menu_font_rgba` / `play_menu_chrome_rgba`);
the page's `AtlasBlitter` (an `image-rendering: pixelated` overlay `<canvas>`
over the GL view) blits the quads with a per-quad multiply tint.

The top-level command list plus the Items / Magic / Equip / Status / Options
sub-screens all run the real
`legaia_engine_core::field_menu_dispatch::FieldMenuSubsession` the native
`play-window` builds - the disc equipment / spell / item catalogs are installed
on the host world at `load_disc` - and render through the exact same
`legaia-engine-ui` draw builders (`inventory_use_draws_for` /
`spell_menu_draws_for` / `equip_screen_draws_for` / `status_screen_draws_for` /
`options_draws_for`).

**Load / Save** run the retail save-select screen against the memory-card rack
(`cards`, below): the `SLOT 1` / `SLOT 2` pills are the console's two card
ports, confirming one plays the "Now checking. Do not remove MEMORY CARD"
card-read beat, and the card's fifteen blocks then come up as retail's 5x3
portrait grid with the focused block's info panel sliding up beneath it. Load
lifts that block into the live world (`play_menu_take_load_scene` hands the page
the save's scene so it can resume where it was written); Save raises the
overwrite prompt and writes the session into the card image. The block-grid
cursor is this crate's, not the session's - `SelectPhase::SlotPreview` ignores
directions by design (see `docs/subsystems/save-screen.md`).

The menu is **not purely input-driven**: its timers count 60 Hz frames, so the
page clocks `play_menu_input` on its own fixed step rather than once per
animation frame. Parity is asserted by the disc-gated `tests/menu_parity.rs`
oracle, which walks the whole card flow.

## Live battles (`play_battle`)

`enter_field` arms the engine's live gameplay loop - the browser twin of the
native `--live-loop --player-battle` flags (`set_live_battles(false)` opts
out). Walking rolls the scene MAN's own step-driven encounter table, the world
flips `Field -> Battle`, and the whole fight runs in `engine-core` (turn SM,
player-driven command / arts / magic / item menus, damage formulas, loot).
This module is presentation only: it folds battle events and strike FX into
the shared `BattleHud` model via `engine_core::battle_hud::sync_battle_hud_rows`
(the same fold the native window uses), arms the ENCOUNTER! banner on the mode
edge, and mirrors the native window's battle HUD block - `battle_hud_draws_for`
rows (retail HP/MP colour law), `encounter_banner_draws_for`, and the submenu
text - into `play_overlay_draws_json`, in surface pixels. Disc-gated oracle:
`tests/battle_overlay_parity.rs`.

It also arms the **field-to-battle intro emitter** - the same
`legaia_engine_ui::battle_intro::BattleIntro` the native window ticks, so all
five retail transition styles (tile shatter, both particle fields, curtain,
swirl) draw in the browser. `tick_battle_intro` tracks the encounter session's
`Transition` phase and caches the frame's ordered geometry for the page's
screen-prim pass (`play_screen_prim_*`); the one per-host step is the field
frame readback - the page answers `play_intro_wants_capture` with a
`gl.readPixels` of its own drawn frame into `play_intro_land_capture`, and
`field_vram_bytes` serves the emitter's captured VRAM clone for the length of
the window.

`debug_force_battle(row)` is the exported twin of the native
`--battle <ROW|first>`: it resolves a formation row against the scene's own MAN
table, turns the live loop on and hands the row to `World::force_encounter`, so
the intro, the BGM swap and the battle load are the ordinary ones.
`debug_formation_rows()` lists the rows the current scene registered. Both are
on the ordinary `#[wasm_bindgen]` surface, not behind a feature: every retail
town is rate-0 by design and the native `debug_start_test_battle` is
`#[cfg(not(target_arch = "wasm32"))]`, so without these the battle screen is
unreachable from a headless driver - which is how it came to be verified on the
native window alone. Driver recipe:
[`site-shell.md`](../../docs/tooling/site-shell.md#reaching-the-play-pages-battle-screen-headlessly).

## Battle 3D scene (`play_battle_render`)

The battle's 3D layer under that overlay - the browser twin of the native
window's `enter_battle_render` / `build_battle_stage` (`window/battle.rs`).
On the `Field -> Battle` edge it builds a **battle VRAM** (the scene rebuilt
with `SceneLoadKind::Battle` so the stage dome + its textures are resident,
plus the PROT 870 flame atlas, per-slot monster texture injection and the
party texture bands) and the meshes the page draws while the fight runs: the
stage **backdrop** (`legaia_asset::battle_backdrop::drawn_objects_tmd`
object-list edit, drawn twice - the second copy pre-appended under the SCUS
`DAT_80078B50` mirror-table transform with `append_scaled`'s winding flip),
the **ground grid** (`build_ground_grid` + the `DAT_80078C1C` depth-cue far
colour, attached as a per-draw cue so the browser grid fogs like the native
one),
**monster meshes** (`monster_archive::battle_render_mesh`) and the
**assembled party battle forms** (`legaia_asset::battle_char_assembly`, real
texture pools + battle palette; PROT 1204 mesh + PROT 1203 rest pose as the
fallback ladder). Idle / action / swing / art-bank clips are installed on
the world so the engine's own battle SM poses every actor; the page reads
`play_battle_actor_pose` per frame and re-poses positions in place. The
camera runs the **shared** phase script (`engine-vm::battle_cam_script`) that
the native window runs - dialogue / menu-with-orbit / submenu close-up /
action framing - and the page consumes a ready view-projection built by the
retail GTE model rather than re-deriving one. Facial-animation VRAM stamps
and the battle-intro emitter remain native-only (disclosed in
`docs/subsystems/battle.md`). Battle exit drops the state and the page
restores the untouched field VRAM.

## Battle effects (`play_battle_fx`)

The browser twin of the native window's per-frame FX block: it drains the
effect-script spawns the battle tick queues (`World::drain_battle_effect_spawns`,
both the direct `0x80`-flag form and the action-table form), then builds the
draw work for the pools they feed - camera-facing **billboards** over the 2D
`efect.dat` pool, the `etmd.dat` **effect models**, the move-VM **scene-graph
parts** (summon + move FX), and the **summon creature**. Transforms are
composed engine-side into ready 4x4 model matrices under the same
`battle_vp * scale(4)` the native FX layers ride, so no page-side transform
model exists to drift. The target-select **cursor tint** (`FUN_801DA6B4`'s
three render words) rides along as a per-draw cue, and move-FX **sound cues**
are classified through the shared `classify_cue` into the page's scheduler.

## Field merchant + banners (`play_shop`)

A field-VM op-`0x49` sub-0 merchant record opens the retail gold shop, and the
post-battle level-up / Seru-capture banners draw over the live field. Both are
the shared `legaia-engine-ui` builders (`shop_draws_for`, `level_up_draws_for`,
`capture_banner_draws_for`) driven by the real
`legaia_engine_core::menu_runtime::MenuRuntime`; `play_shop_input` forwards pad
edges and `play_overlay_draws_json` serves the quads.

Retail's shop is five windows rather than one panel, so alongside the engine's
interactive list the page paints the four **descriptor windows** that have
painters - 33 vendor plate, 32 purse, 34 item info, 37 sell quantity - through
the same `painter_at` renderer dispatch and the same disc-parsed rects the
native `play-window` uses. They draw only when the menu-overlay window table
parsed: a `renderer_va` is not something the pinned-rect fallback can invent,
so without the real table the windows are absent rather than mislocated.
`tests/shop_overlay_parity.rs` asserts content lands inside window 32's and
34's rects, keyed off the table re-parsed from the disc in the test rather than
off any constant this crate carries.

The shop and its **catalog** have to ship together, and the reason is worth
knowing before touching either. `World::try_arm_field_shop` sets both
`field_shop_armed` and `field_shop_open`, and the op-`0x49` tristate then
reports `Armed`, which *suspends the field VM* until the host calls
`finish_field_shop`. A host that installs `item_shop_data` without a shop
screen therefore does not merely lack a screen - it parks the script at the
first merchant. Before both landed, the page had no catalog at all, so the
priced-record validation failed and every merchant was silently inert.

Two deliberate divergences from the native window: input is **edge**-triggered
(`menu_runtime::step` does no edge detection, and the native window feeds it the
held pad), and the buy/sell rows carry **real item names** off the SCUS table
the page already parses, where the native rows are placeholder labels.

## Developer menu (`play_dev_menu`)

The retail developer menu - the same `engine-core`
`dev_menu_host::DevMenuSession` the native window mounts behind
`LEGAIA_DEV_MENU` - behind the play page's session-only **Dev menu**
checkbox: unchecked on every load, never persisted, never read from the URL,
so only a deliberate click by the visitor (who *is* the person running a
client-side program) enables it. Ticked from `tick_frame` off the world's own
pad words through the shared `dev_menu::retail_packed` conversion; the row
list draws through `dev_menu_list_draws_for` and Square swaps in the
battle-records page (`records_screen_draws_for`), both at the same pens as
the native window. `check-ui-host-drift.py` pins the pens, the records
headings and the tick/records-model injection sites to the native host's.

## Fishing minigame (`play_fishing`)

`LegaiaRuntime::play_fishing_start` lifts the fishing overlay (PROT 0972)
through the static-overlay map, decodes its per-species table plus the two
point-exchange venue pages off the visitor's own disc, and installs a
`legaia_engine_core::fishing::FishingSession` with `World::enter_fishing` - the
same mode-suspend contract the native window uses, so the field scene stays
intact underneath and comes back on exit. `play_fishing_hud_json` serves the
retail HUD through the shared `fishing_hud_draws_for` consumer, and
`play_fishing_prizes_json` / `play_fishing_prize_buy` expose the prize rows with
retail availability gating.

Nothing here is an input path, and that is the design: the driver is
`World::tick_fishing`, which reads the pad word the page already routes each
frame. Cross casts and reels, Square reels harder, Cross recasts - and because
the ported reel decoder classifies the two held bits, holding both resolves the
way retail does rather than the way a host `if` chain would.

The one place the page draws more than the native window: with the fishing
sprite page undecoded, `fishing_hud_draws_for`'s atlas is blind on both hosts
and it drops every glyph and gauge fill, so native's fishing HUD is digits and
captions only. The gauges are the functional half of the tension tug-of-war, so
this host also emits their resolved frames (the ported cap/body/cap geometry) as
a `bars` channel the page fills as rects. Disc-gated oracle:
`tests/play_fishing_host.rs`.

## Sound effects (`play_sfx`)

The play page ran with music and no sound effects at all: the native window
stages a descriptor bank from the disc executable plus a resident program bank
into its own SPU region, and this host staged neither. `play_sfx` is that
channel, assembled from what already existed rather than as a second audio path.

The chain is the retail one. `SCUS_942.54`'s static descriptor table
(`DAT_8006F198 + id*8`) is parsed at `load_disc` into a
`legaia_engine_audio::SfxBank` - pure data, so it lands whether or not the
visitor has enabled sound. The resident class-2 program bank (PROT 0869, with
the documented 0875 alternate as a fallback) uploads into a dedicated region at
the **top** of SPU RAM the first time a cue fires, and cues key through
`SfxBank::play_one_shot` into the **live** `WebAudioOut` SPU - the same mixer the
BGM sequencer feeds, so a cue and the music share one voice pool as they do on
hardware.

That last part forced a fix worth knowing about: the page's BGM allocator
previously claimed all of SPU RAM above `0x1000`, so a scene-BGM upload could
have overwritten the SFX region. Both BGM upload sites are now capped below it,
matching the native boot's split, and a unit test asserts the two regions stay
disjoint.

### Cue provenance is reported, not assumed

Retail fires a cue by writing an id into the ring at `_DAT_8007B6D8`, and only a
handful of those writes are traced. So this host carries the same `disc` / `site`
split [`sfx_view`](src/sfx_view.rs) established, and `play_sfx_events_json`
reports it per event with a note explaining the choice - the page names
behaviour (`menu_confirm`) and never a cue number, and can label which sounds
are the game's.

The footstep row is the instructive one. Its *cadence* is the ported retail
kernel `FUN_80018db0` (interval derived from movement magnitude, the `0xB`
stationary gate, the `0x4B0` ambient period) getting its first host caller - but
**two** of its inputs are port picks, and the second is worth knowing before
touching this code. Retail's "movement magnitude" is a controller speed word,
not a world-space delta: the kernel's own arithmetic requires `speed >= 0x30`
before a step can fire, and the port's controller moves 2 units per tick, so
feeding the raw displacement in leaves the interval permanently below the gate
and no footstep ever sounds. `WALK_SPEED_UNITS` places a single-speed walker at
the conservative end of retail's moving band instead. Pinning the real speed
word is what would retire the pick.

`play_sfx_probe_peak` is the measurement that keeps this honest: it renders a cue
through a throwaway SPU and reports its peak sample, so "wired but every cue is
inaudible" fails a test instead of shipping. Disc-gated oracle:
`tests/play_sfx_channel.rs`.

## Keeping the two hosts in step

The browser play page and `legaia-engine play-window` are two framebuffers over
one engine, and the failure mode that costs the most is quiet: a wave adds a
screen, wires it natively, and the web host drifts a release behind with nothing
in any diff to say so.

`crates/engine-ui` is what makes that checkable. Every screen's geometry is a
`pub fn ..._draws_for(...) -> Vec<TextDraw>` there, and a host *has* that screen
exactly when it calls the builder - so the set of engine-ui draw builders is a
feature surface that can be derived from source rather than maintained by hand.

`scripts/ci/check-ui-host-drift.py` derives it and fails when a builder reaches
the native window but not this crate. Genuine one-host cases live in
`scripts/ci/ui-host-drift-waivers.toml`, each with a reason, and the checker
validates those in both directions: a waiver naming a builder that was renamed
away, or one whose gap has since been closed, is itself a failure. The file
cannot decay into a stale list of intentions - it only passes while it describes
what the two hosts actually do.

The practical consequence for anyone adding a screen: wire it in both hosts, or
write down why not.

### Platform drift against the native window

The gate above answers "does this host call the builder". It cannot answer "does
this host feed the builder the same model", and that is where the drift it
misses lives - both hosts call `shop_draws_for`, both call `options_draws_for`,
and for a while both looked fine while one of them drew a blank screen. What is
known today, found by reading the two hosts side by side rather than by any
gate:

| Area | State |
|---|---|
| Seru-trade shop screens | Closed. Config + name table install at `load_disc`, and `play_shop::shop_trade_draws` is the twin of the native `draw_shop_trade`. |
| Options screen | Half closed. Edits persist for the session; a page reload still starts from defaults, where the native window reloads `legaia-options.toml`. |
| Inn prompt | Open, and host-symmetric in the sense that matters least: neither host opens an inn session, but only the native window would draw one if something did. |
| Load / Save rows | Deliberate. This host browses the console's two memory-card ports; the native window writes LGSF files to `saves/`. A browser has no filesystem to be the other thing. |
| Dance / Baka / Muscle | Deliberate. The native window starts them from developer keybinds; here they are their own site pages driven by `LegaiaMinigames`. Neither host reaches them from a field trigger yet. |

The Options row deserves the sharpest statement, because it is the shape that
recurs: the screen was *drawn* on both hosts and the gate was green, while one
of them rebuilt the session from `OptionsState::default()` on every open and
dropped the result on close. A screen can be fully wired and still be connected
to nothing.

## Boot title screen (`boot_title`)

The front of the native `--boot-ui` chain (publisher logos -> title ->
save-select -> field). "New game" boots the retail title card off the disc's own
art: `LegaiaRuntime::boot_title_*` drives the engine's `TitleSession` (FadeIn ->
PressStart -> MainMenu) and serves the title-TIM bands (wordmark, Press Start,
NEW GAME / CONTINUE, copyright) as sprite quads off `title_screen_atlas`
(PROT 0888), blitted onto the same overlay canvas the pause menu uses, over
black. `site/js/play-app.js` exposes the `AtlasBlitter`; the page's small boot
controller runs before any scene exists, feeds the title edge-triggered pad
words, and on the New Game outcome seeds the retail defaults + enters the
opening prologue chain (`play_cutscene` above). Publisher logos and the
Continue save-slot grid are not yet wired.

The menu rows have the same two sources the native window has, and exactly one
draws at a time: with PROT 0888 resolved the title TIM's own NEW GAME /
CONTINUE bands carry them; without it they fall back to the shared
`title_menu_draws_for` builder sampling the menu-glyph atlas
(`boot_title_glyph_atlas_*`), and only if that is missing too does the dialog
font stand in. Drawing two of them would double-render the rows.

## Name entry (`play_name_entry`)

The last screen of the New Game flow: the opening `town01` naming prompt, at
the retail-traced geometry, through the shared `name_entry_draws_for` +
`name_entry_chrome_sprite_draws_for` builders. The state machine is
`engine-core::name_entry` - the page owns no name logic, only the pad-edge
bridge (`name_entry_input`) and the draw-list JSON (`name_entry_draws_json`,
the same two-layer chrome+font shape the pause menu uses).

The screen lands **with its state**, which is the whole point: the
establishing timeline's pinned op-`0x49` opens it and stays suspended while
`name_entry_is_active()` holds, so the committed name has to reach
`World::party_names` for the script to resume at all. It is not an overlay
that could be skipped - skipping it would park the opening forever, the same
failure an unclosable field shop produces. Retail behaviours inherited from
the SM: the middle control button **restores the template default** (it is not
a space key), the confirm prompt opens on **No**, and Vahn's scripted walk-out
(scene-ANM records 47/48) plays post-confirm. Disc-gated oracle:
`tests/new_game_flow_parity.rs`.

## Battle-stage backdrops in the entry viewer

A `scene_tmd_stream` entry is a battle-stage shell, and retail does not draw it
the way the file lays it out: object **1** never draws, and the shell is drawn
**twice**, the second copy under a per-stage transform that closes the
authored half. `build_current_vram_mesh` places it that way through
`legaia_asset::battle_backdrop`, and `tmd_note` labels the entry with the
transform it resolved.

The transform comes from the stage table in `SCUS_942.54`, decoded during
`load_disc` alongside the item / spell / steal tables and kept as
`backdrop_mirror`. A raw `PROT.DAT` load has no executable, so the preview
falls back to retail's default (a half turn) and the note says the transform
is unresolved rather than asserting one.

## Assembled full-scene maps (`field_scene`)

`LegaiaViewer::set_scene_field(name)` loads a CDNAME field/town scene
through the **real engine loaders** and surfaces the whole assembled map -
the answer to "a `scene_asset_table` entry viewed alone shows one
object-local mesh at the origin". The build is the engine-parity path:
`SceneResources::build_targeted_with_options` (field-mode VRAM pre-pass +
the LZS-packed environment TMD pack), the shared
`engine-core::field_env` kernel (env-pack vote + `.MAP` object-grid
placement resolution + floor-height-LUT world Y), the terrain-tile layer,
and the walk-ground heightfield. Accessors mirror the kingdom `pack_*`
family: `field_scene_mesh(slot)` + `field_scene_mesh_*` per-mesh arrays,
`field_scene_vram_bytes`, `field_scene_placement_{slots,positions}`,
`field_scene_terrain_{slots,positions}`, `field_scene_ground_*`.

Each `field_scene_mesh` is a **hybrid** (`build_hybrid_env_mesh`): the
VRAM-filtered textured prims plus the untextured flat/gouraud
vertex-colour prims the textured builder drops - the browser sibling of
the native engine's colour-mesh pipeline, so colour-only props (benches /
fences / small furniture) render instead of vanishing.
`field_scene_mesh_flat_rgba` returns the parallel per-vertex
`[r, g, b, flag]` array (flag `255` = textured / sample VRAM, `0` =
untextured / use the colour; empty for pure-textured meshes), consumed by
the WebGL shader's `u_use_flat_colors` / `a_flat_rgba` hybrid path.

The per-vertex `cba_tsb` stream carries each prim's PSX **semi-transparency**
state (ABE enable in TSB bit 15 - `legaia_tmd::mesh::TSB_SEMI_TRANSPARENT_BIT`
- and the ABR blend mode in bits 5..=6) for textured prims and, via the
hybrid merge, for the untextured colour-half verts (their `ColorMesh::blend`
word lands in the TSB slot; the flat path never samples VRAM, so it's blend
metadata only). The WebGL renderer (`site/js/webgl-tmd.js`) draws these prims
in a deferred **blend pass**: the opaque pass discards blending texels
(prim ABE + texel STP, `u_semi_pass = 0`), then per-ABR-mode index tails
(`buildSemiTail`, the browser mirror of engine-render's
`psx_blend::append_semi_tail`) re-draw them depth-tested but not
depth-written with the matching GL blend state (`0.5B+0.5F` / `B+F` / `B-F`
/ `B+0.25F`). This is what makes fountain water (Hunter's Spring) and house
window light read translucent instead of opaque grey. Disc-gated pin:
`tests/field_scene_blend.rs`.

Two site pages drive it through the shared `site/js/field-scene-view.js`
(`window.FieldSceneView`): the **game-world** page's town navigator, which
swaps locations in place, and the **asset viewer**'s "full map" button. The
view streams the draws through the WebGL renderer's instanced scene-mesh
path (the same plumbing as the world-overview kingdom continents) and
classifies sky-dome shells and horizon-backdrop planes (huge-footprint /
zero-depth-sheet AABB heuristic, `FieldSceneView.isSkyMesh`), hiding their
draws - under the assembled camera they'd paint over the map they surround
in retail. `load()` is re-entrant, so a navigator swap releases the previous
scene's GL meshes rather than leaking them. Disc-gated parity test:
`tests/field_scene_assembly.rs` (incl. the colour-only prop recovery).

## Field-NPC catalog (`field_npc`)

`LegaiaViewer::set_scene_npcs(name)` loads the field scene (for its TMD pool
+ VRAM) and catalogs every actor the scene's MAN places;
`field_npc_catalog_json()` returns the list. An NPC is not a separate asset
class - it's a **MAN partition-1 placement record**: a model byte indexing
the scene's TMD pool (`res.tmds[model_index]`, *not* the env-pack subset the
map placements use), an anim byte naming a record in the scene's ANM bundle,
and tile bytes for the spawn. `build_npc_catalog` is the pure builder behind
the binding (disc-gated test: `tests/field_npc_catalog.rs`).

Per-actor mesh accessors mirror the character family:
`field_npc_mesh(catalog_idx)` + `field_npc_mesh_{positions,uvs,cba_tsb,
indices,object_ids,flat_rgba,bounds}`. The mesh is the field-hybrid build
(`tmd_to_vram_mesh_field_hybrid`), so it carries per-vertex object ids *and*
flat colours in one stream.

**The pose is load-bearing.** A multi-object character TMD ships its
vertices in object-local space, so drawn raw its parts collapse onto the
origin; the figure only assembles as `v_world = R_bone . v_object_local +
T_bone` from frame 0 of the placement's clip (the page composes this via the
existing `player_anm_record_pose_frames`, keyed on the catalog's `anm_prot`).
The catalog therefore lists only actors it can assemble: multi-object actors
with no clip, or in a scene shipping no ANM bundle at all (`rikuroa`), are
withheld and reported as `unposable_count` rather than shown as a heap.
Party / save-point heads (`model_index >= 0xF0`) draw from the global pool
instead of the scene's and are routed to the characters page
(`special_count`). Off-map "hidden" spawns are script-gated story actors,
fully resolvable, so they *are* listed - flagged `conditional`.

## Tactical Arts viewer (`arts_view`)

`LegaiaArts` drives `site/arts.html`'s interactive half: click an art card,
watch the character perform it. Per character it assembles the battle mesh
from the player battle file's equipment sections
(`legaia_asset::battle_char_assembly`, PROT 863..866), textures it from the
same file's pools + decoded battle palette into a runtime VRAM image, and
decodes the record[0] idle loop plus the whole art-animation bank through
the `readef.DAT` `"ME"` archives (PROT 894) - every clip pre-expanded per
assembled object so the page's pose loop drives object `i` with channel
`i`. Arts-voice shouts are the RE'd retail cue: the character's own XA bank
demuxed off the disc, each art mapped to a real member of its
`FUN_8004C140` candidate channel pool.

`export_character_glb` bakes the current character + its **entire**
animation bank into one `.glb` (`legaia_asset::character_gltf`): one node
per rigid TMD object, each decoded bank record a named glTF TRS animation
("battle idle" first; art names where the record carries one, else the
`anim_id` in hex), textured from the same VRAM via a baked tile atlas and
shaded by the same per-vertex packet colour the canvas modulates with
(`COLOR_0`; see `legaia_asset::gltf_color`). The page's download button
hands it out client-side - nothing is uploaded.

The page's tinted after-image trail (the retail arts ghosting; per-character
tint) is a JS-side re-draw (`MeshView.setTrail` -> the renderer's
`ghostTrail` additive passes) with a persisted user toggle, default on.
Disc-gated oracle: `tests/arts_view_real.rs` (bank coverage, arts-table
resolution, voice pools, and the `.glb` export's animation bank).

## Equipment loadout viewer (`equipment_view`)

The `equipment loadout` form on `site/characters.html`, and the only surface
in the project that assembles a party member's battle model from a
**non-default** loadout - every other call site passes all-zero equip ids.
`equipment_pack_json` enumerates each player file's five descriptor sections
(labelled from the SCUS equipment stat table, because the section order
differs per character: Vahn's weapons are section 2, Noa's are section 3);
`set_equipped_character(slot, ids, diff)` runs `assemble_character` +
`relocate_tsb_cba` on the chosen ids and paints it from
`character_texture_uploads` for the *same* ids. That upload set is the whole
band - both `record[0]` blocks plus every flagged section pool - which
matters because two blocks ship `clut_n == 0` and sample a palette a sibling
block put on the shared row.

Objects tagged `200+` are dropped: they are duplicates of the bone they
attach to, and retail only reaches them through the actor's `+0xA4` window.
Clips come from the file's own `record[0]` action bank plus the
equipment-spliced weapon swings, so they change with the weapon.
`equipped_character_glb` bakes the whole posed character with that bank,
named for the character and what it wears.

Objects tagged `200+` are dropped **when they are byte-copies of their
attach bone** (`AssembledCharacter::duplicate_objects`) - drawing a copy
alongside its host z-fights a limb, but sixteen of the disc's assemblies
carry a non-copy surplus that is real geometry, so the tag alone is not the
test.

`diff = true` adds the **diff highlight**: a per-vertex tint stream that dims
geometry shared with the unequipped part, brightens what reaches beyond its
radius envelope, and draws the bare geometry the section replaced alongside
in a third colour. It is a viewing aid over an approximate boundary
(`battle_char_assembly::equip_diff`), and deliberately *not* the item cut.

`equipped_item_glb(section)` exports **every** equipped section. It ships one
node per *source object* on each side of the cut, and passes the character's
clip bank through so the builder can write each node's rest transform: a
battle pose is flat (absolute `R.v + T` per object, nothing parented), so a
node with no transform draws at the model origin and several of them stack.
That is what made a multi-object export - Vahn's weapon spans forearm and
hand, a fused armour spans the torso chain - come out as two limbs on top of
one another. Synthetic item ids inherit their host object's pose and channels
via `character_gltf::CharacterGlbLayout::pose_source`. For the
two weapon-bearing sections it is the exact cut: the held item is a primitive
subset of the bone object selected by palette column
(`battle_char_assembly::equip_item`), shipped beside the limb it came from as
two named nodes. Anything with no material boundary - armour, headgear,
footwear, one single-palette Ra-Seru - comes back `fused`: the section's
whole contribution, item and host together. The class (`own-object` /
`separate` / `welded` / `fused`), its one-line `describe`, and `complete` /
`pure` flags ride in the summary and the glTF root name - a `welded` item's
grip is open, a `fused` one carries its limb. Completeness over purity: no
equipped section yields nothing. Background:
[`docs/formats/battle-data-pack.md`](../../docs/formats/battle-data-pack.md#the-item-is-still-separable---by-palette-not-by-geometry).
Disc-gated oracles: `tests/equipment_view_real.rs` plus the 81-record sweep
in `crates/asset/tests/equip_item_real.rs`.

`equipped_item_only_glb(section)` is the second download beside it: the
**item alone** - no host limb, no skin, no unchanged default geometry - the
opinionated cut of `battle_char_assembly::equip_isolate` under the section's
default reading (colour diff against the bare limb for held items and
headgear, geometry-and-colour identity for body and footwear) or the
record's committed rule in `crates/asset/data/equip-isolation.toml`. The
summary's per-item `isolation` object and the glTF root name carry the mode,
the kept / dropped primitive counts and whether a rule hand-checked the
record, and the grip repair's `bridges` / `bridged_triangles`
(`equip_repair` lofts a tube between the two shaft rims a welded weapon
leaves where the fist hid the haft; the root name says `grip inferred`).
The `equipped_item_only_{positions,uvs,cba_tsb,indices,object_ids,flat_rgba,bounds}(section)`
family is that same repaired geometry as mesh streams, parallel to the
`equipped_mesh_*` set, so the page's preview toggle draws exactly what the
file holds; `equipped_mesh_item_mask(section)` (per-vertex `0 / 1 / 2`:
outside the section / left behind / item) is the pre-repair view of the
same cut. Background:
[`docs/formats/battle-data-pack.md`](../../docs/formats/battle-data-pack.md#the-item-alone---an-opinionated-cut-with-a-committed-rule-table);
sweep + table integrity in `crates/asset/tests/equip_isolate_real.rs`, the
grip sweep in `crates/asset/tests/equip_repair_real.rs`.

The characters page's **equipment panel** - every slot, every piece as its
own card - runs on `equipment_item_card_json(slot, section, id)` /
`equipment_item_card_pixels(size)` / `equipment_item_card_glb(alone)`: one
single-item build per `(character, section, id)`, cached on the viewer so the
three calls share it. The JSON carries the name, the palette-cut class, the
item-alone decision (`mode` / `curated` / `note`), what it kept and what the
grip repair added; the pixels are the item alone at the character's rest
stance, drawn by `legaia_asset::mesh_raster` (software, so forty cards do
not need forty GL contexts) re-framed on the item's principal axes - blade
up, flat-on - over a transparent background; the glb is the alone / with-limb
download without equipping the piece first. The page builds cards through a
queue that yields to the orbit view between items.

## Playable minigames (`minigames`)

`LegaiaMinigames` is a standalone `#[wasm_bindgen]` class (its own
`load_disc`, no canvas) that runs all five of the game's side-games in the
browser for `site/minigames.html`. It is a thin JSON shell over the
clean-room rules engines in `legaia-engine-core` - the beat clock + judge
(`dance`), the rock-paper-scissors duel (`baka_fighter`), the reel state
machine + payout eval (`slot_machine`), the cast/tension/catch loop
(`fishing`), and the dome's four-turn deal/commit/resolve (`muscle_dome`). It
carries no rules of its own.

Every table each game plays with is decoded from the visitor's own disc via
the same path the play-window uses (raw PROT entry ->
`static_overlay::as_loaded` -> table parser): the step chart out of PROT
0980, the roster + action tables out of 0976, the payout table out of 0975,
the fishing species table out of 0972, and the Muscle Dome hand command-id
table out of the battle overlay 0898. Nothing is shipped with the site.

Per game: `<g>_start` / a step or input method
(`dance_press` / `baka_choose` / `slot_spin` + `slot_stop` +
`slot_collect` / `fishing_advance_cast` + `fishing_lock_cast` +
`fishing_reel` + `fishing_recast` / `muscle_commit` +
`muscle_end_selection` + `muscle_resolve` + `muscle_next_turn`) /
`<g>_state_json`. The dome adds a second tier for the contest above a leg
(`muscle_contest_start` / `muscle_report_leg` / `muscle_contest_settle` /
`muscle_contest_json`), so the browser walks the same ladder the native
window does off the same `DomeContest` kernel. `load_disc` returns a status object naming which games'
overlays resolved, so a disc that can't feed one game still plays the
others. `dance_state_json` deliberately surfaces **both** halves of retail's
split chart lookup - `judged` (what the hit judge matches, the step to
press) and `displayed` (the display half's held-sequence substitution); see
`docs/subsystems/minigame-dance.md`. Disc-gated oracle:
`tests/minigames_wasm_api.rs`.

`minigames_fishing.rs` and `minigames_muscle.rs` are the fishing / Muscle
Dome shells. Fishing drives [`legaia_engine_core::fishing::FishingSession`]
from a default rod stat (no save-block record on the web entry point) and
resolves species names against the loaded PROT 0972 overlay image; the
cast-meter sweep rate and the land/snap glue are the module's engine-side
reconstruction (`docs/subsystems/minigame-fishing.md`). Muscle Dome drives
[`legaia_engine_core::muscle_dome::MuscleDomeSession`] on the disc's dealt
hand with the native launcher's flat favored per-card cost (the browser has
no player battle file for the per-command `+0x74` swing bytes) and a
battle-path damage stand-in matching the native `tick_muscle_dome`
constants, and [`legaia_engine_core::muscle_dome::DomeContest`] for the
ladder above the leg (`docs/subsystems/minigame-muscle-dome.md`). Having no
save file to read a flag bank out of, the page passes the course-unlock and
Master-gate flags it wants open and the shared kernel applies the same rule
to them.

`minigames_baka.rs` adds the Baka Fighter duel's **presentation** exports so
the page draws with the cabinet's own assets: per-side fighter mesh buffers
(player = PROT 1204 battle pack slot, opponent = its own PROT 1206..=1219
`[TIM][TMD][anim]` pack), pose-frame decodes from the real animation banks
(PROT 1203 bank records `char*9 + action` for the party, the pack's own bank
for the opponent), the 51-record HUD widget table + PROT 1203 art pages, the
stage TMD set, and a per-duel 1 MB VRAM build. Consumed by
`site/js/minigame-baka.js`; see
`docs/subsystems/minigame-baka-fighter.md` § HUD widget table.

`minigames_dance.rs` adds the dance's **presentation** exports: the PROT
1230 art pack's HUD page decoded through its row-500 CLUT strip
(`dance_hud_page_rgba`), the overlay's 34-record widget table + the traced
emitter geometry (`dance_widgets_json` / `dance_layout_json`, incl. the
capture-pinned `+4`-line draw-environment offset), the dancer face-stamp
windows with the pose blits replayed (`dance_face_rgba`; dancer 0 = Noa's
field atlas, PROT 0874 §2), the SFX cue bank (PROT 1228 descriptors +
the PROT 1231 sample VAB - a TOC-tail entry) plus the direct-keyed hit
stings (`dance_sting_pcm`), and the real BGM pair rendered through the
clean-room SPU (`dance_bgm_pcm_i16`). Consumed by
`site/js/minigame-dance.js`; see `docs/subsystems/minigame-dance.md`.
Disc-gated oracle: `tests/minigames_dance_api.rs`.

## Session saves + retail cards (`session_save`)

The play page's save boundary. This module is **serialization only** -
persistence (localStorage, base64) lives in `site/js/legaia-saves.js`, and the
save bar itself is `site/js/minigame-saves.js`.

**Engine sessions** round-trip as **LGSF**: `LegaiaRuntime.export_save` /
`import_save` are `World::save_full` / `load_full` with magic + version
validation, so a corrupt upload throws a readable message and leaves the
session untouched.

**Retail emulator saves are first-class.** `card_saves_json(bytes)` lists the
Legaia saves inside a raw `.mcr`/`.mcd` card image, DexDrive `.gme`, or
single-save `.mcs` (party names, gold, coins, location, the CDNAME scene
label). `LegaiaRuntime.import_card_save(bytes, block)` lifts one into the live
world via `legaia_save::SaveFile::from_retail_sc_block`. PS3 `.psv` is rejected
(signed container).

`card_patch_coins(bytes, block, coins)` banks browser-minigame coin winnings
into the pinned retail coin slot (SC `+0x464`, RAM `0x800845A4`) **in place**:
the container comes back in the format it arrived in with only those 4 bytes
changed, so an untouched export is byte-identical and the patched save still
loads in the emulator.

The minigames page's **save bar** draws on two more exports:

- `card_icon_rgba(bytes, block)` decodes the SC block's own 16x16 memory-card
  icon (palette `+0x60`, 4bpp pixels `+0x80`) - for Legaia that is the lead
  character's baked portrait.
- `LegaiaMinigames.save_portrait_rgba(char_id)` decodes the three 16x16
  load-screen portrait TIMs (Vahn / Noa / Gala) from the pre-`init_data` gap of
  `PROT.DAT` - the faces the bar's tiles show.

Save summaries carry the lead's displayed level (record `+0x130`).

## Memory-card rack (`cards`)

The two card ports the in-canvas Load / Save screens read and write - the
console has two, which is why retail's save screen has exactly two SLOT pills.
The page fills them from the card images the player already imported
(`insert_card(slot, bytes, label)` / `eject_card` / `card_slots_json`), and
`export_card(slot)` hands the image back for download.

Container bytes are kept **verbatim** in whatever container they arrived in
(`.mcr` / `.mcd` / `.gme` / `.mcs`, normalised by `legaia_save::emu`) and saves
are stamped in place through `SaveFile::write_into_retail_sc_block`, so a card
that was never saved into exports byte-identical, a card that was keeps every
other save and its container header untouched, and either still loads in the
player's emulator. Saving into a block that was free also claims its directory
frame (state + product code + XOR checksum) so the emulator's card browser sees
the new save. A `dirty` flag per port tells the page there are unexported
writes.

`card_block_snapshots` lifts each block through `SaveFile::from_retail_sc_block`
to feed the preview grid's portraits and the info panel's name / level / HP /
MP / location rows - the same fields retail reads into its per-slot buffer.

## Scene `.glb` export (`scene_export`)

Builder-style session on `LegaiaViewer` so the site pages can download
**exactly what they render** as a binary glTF: `scene_export_begin(name)` /
`scene_export_set_vram(bytes)` / `scene_export_add_mesh(name, positions,
uvs, cba_tsb, indices, flat_rgba) -> handle` /
`scene_export_add_instance(handle, tx, ty, tz, rot_y, scale)` /
`scene_export_finish() -> Vec<u8>`. The page feeds the same mesh buffers it
uploads to WebGL plus the same per-draw `(translation, rotY, scale)`
triples it builds model matrices from; the bake
(`legaia_asset::scene_gltf::build_scene_glb`) renders every distinct
`(cba, tsb-page)` pair the vertices sample into a 256x256 tile of one RGBA
atlas (the PSX VRAM+CLUT indirection has no glTF equivalent), remaps UVs,
and carries the `flat_rgba` packet stream into `COLOR_0` - untextured
vertices as a fill over a white atlas tile, textured ones as the
`texel * colour / 128` modulation the page's shader applies, so a caller
that passes the stream gets the shading it draws (pass an empty array only
when there is genuinely no packet colour). Consumers: the world-overview page (assembled continent), the
game-world page's town navigator and the viewer page's full-map mode (both
via `FieldSceneView.exportGlb`), the viewer's single-TMD inspector, and the
characters + NPC pages (via `MeshView.exportGlb`, which bakes the **posed**
vertices - the object-local parts would otherwise arrive in the file as a
heap at the origin). The monster page's enemy export stays on the sibling
`monster_gltf::export_glb` (it additionally carries the action
animations). Disc-gated smoke: `legaia-asset`'s
`tests/scene_gltf_real.rs`.

## In-browser ROM patcher (`rom_patcher`)

`rom_patcher::patch_rom(image, seed, drops, encounters, chests)` runs the
Track-1 [`legaia-patcher`](../patcher/README.md) randomizer entirely client-side and
returns `{ data, summary, seed }` - the patched disc bytes for download, a
human-readable change report, and the resolved numeric seed. `resolve_seed`
exposes the seed-string hash so the page can display it. It drives the static
site's `tooling/rom-patcher.html` page: the user supplies their own disc, toggles
the drop / encounter / chest settings, and downloads a patched image. The disc
bytes never leave the browser and nothing is uploaded - the same "user supplies
the disc" model as the CLI, so the site ships only code.

An optional `lang_pack` YAML argument (default `""` = English, strictly opt-in)
applies a [language pack](../patcher/README.md#translation-packs) **before** any
randomizer pass (translate-then-randomize composes; the reverse loses relocated
scenes' lines). The page offers the shipped `site/lang/*.yaml` packs by dropdown,
plus an import path (user-supplied YAML) and `export_lang_pack` (dump a
source-bearing working pack from the user's own disc to author one) and
`validate_lang_pack` (disc-measured dry run before patching). The packs are
static assets fetched from `site/lang/`, never bundled into the WASM.

### Texture replacement (`texture_registry`, `texture_pack`)

The same page swaps textures for the user's own PNGs. Which *families* of
texture exist is data, not control flow: `texture_registry` declares one
`Tier` per family - its id, how it enumerates rows, how a row decodes to
RGBA, and how a row resolves to a write - and `scan_textures` /
`preview_texture_replace` / `apply_texture_replacements` iterate the registry
rather than branching on a family. Adding a family is adding a `Tier`.

Families are not all the same shape, which is why the registry exists. Two are
standard PSX TIMs (raw in a PROT entry, or inside an LZS section) and share one
writer. Save-slot portraits are tiles of a shared sheet addressed by slot, with
their own writer. The summon / readef pages (PROT 893/894) are not TIMs at all -
no TIM scan reaches them - and are listed and exportable but read-only, because
this build has a decoder for that format and no encoder. The battle character
art (PROT 863..866, `legaia_asset::battle_texture_catalog`) is likewise
headerless and likewise invisible to a TIM scan, but does have an encoder, so
it is replaceable - against its record's own slot allocation rather than a
stream length, which is a budget that can genuinely be missed. A `Tier` says
which it is, and a family that declares itself read-only cannot resolve a
write.

A row carries derived metadata only: coordinates, dimensions, palette count,
byte length, VRAM placement, a label, and an FNV-1a-64 fingerprint of the
stored bytes. A label is either curated - [`tim_labels`](../asset/README.md),
keyed by fingerprint - or composed per row from disc data, which is why it is a
`Cow`: the battle-art tier joins each block to the equipment it dresses, so
`ScanCtx` carries the disc's `SCUS_942.54` item-name table alongside
`PROT.DAT`. That join is what makes the family searchable by the word a person
would actually type. The scan is a streaming pass into a caller sink -
full-size pixels for every texture on the disc would not fit in 32-bit WASM
memory - and `ScanCtx` keeps exactly one decompressed entry, which is enough
because the compressed tier's rows arrive grouped by entry.

`texture_pack` is the shareable form: one pretty-printed JSON file holding the
user's replacement PNGs plus, per entry, the coordinates and fingerprint of the
retail texture each replaces. It never contains retail pixels, so a pack is
publishable. Import resolves each entry against the user's *current* image and
grades it (`ok` / `unknown-family` / `not-found` / `hash-mismatch` /
`size-mismatch`), so a different disc revision - or a texture the user already
patched - is reported instead of silently overwritten. The format is versioned
from v1 and a reader refuses a newer one rather than half-applying it. The page
also keeps the queue in `localStorage` in exactly this format, so persistence
and sharing are the same code path.

`LegaiaViewer::monster_archive_json` decodes the global monster stat
archive (PROT entry 867, extended footprint) into a JSON array of every
populated record (id / name / HP / MP / stats). It drives the static
site's `monsters.html` enemy-table page entirely client-side - the disc
bytes never leave the browser. Per row, the page also renders the enemy's
3D battle model: `monster_mesh_{positions,normals,indices,bounds,uvs,palette_index}`
plus `monster_texture_{indices,palette_rgba,dims}` feed a textured WebGL2
viewer (the embedded TMD at record `+0x04`, coloured from the decoded
texture pool at `+0x08` via the prim-CBA palette lookup).

Rendering targets the canvas's 2D context (`CanvasRenderingContext2d` +
`ImageData`) for TIM blits and the canvas's WebGL2 context for textured
3D TMDs - no `wgpu` dependency on the WASM target. The 3D path
(`tmd3d` module + `site/js/webgl-tmd.js`) does either software
painter's-algorithm rasterisation or a paletted GPU shader matching
the engine-render VRAM-mesh pipeline, which is enough for "browse the
asset library from a phone" but not enough to drive a real game scene.

A canvas can only ever bind one rendering context type for its lifetime
(once `getContext("webgl2")` succeeds, `getContext("2d")` returns null
forever on that element). The host page swaps in a fresh `<canvas>`
between entry switches and `LegaiaViewer` re-resolves it by id on every
2D draw, so flipping back to a TIM entry after viewing a TMD entry
keeps working. When a disc is loaded, primitives whose texture pages
weren't supplied are dropped from the mesh before upload, which
prevents the "solid green / cyan tint" symptom for entries that
reference TIMs sitting in other PROT entries.

## Build

`wasm-bindgen` for the JS bindings; `wasm-pack` for packaging.
`wasm-opt` is disabled in `Cargo.toml` to keep the build reproducible
across environments without an emscripten install.

```bash
# Direct invocation:
wasm-pack build crates/web-viewer --target web

# Or via the convenience script that also syncs into site/wasm/ for
# local previewing:
scripts/ci/build-wasm.sh
```

The generated `pkg/` is consumed by the static site under
[`site/`](../../site/). `site/wasm/` is gitignored - the build script
regenerates it from `pkg/` on demand.

## Serve locally

```bash
scripts/ci/build-wasm.sh
python3 -m http.server -d site 8000
# then open http://localhost:8000/viewer.html
```

The viewer instantiates `mod.LegaiaViewer('viewer-canvas')` against the
canvas in `site/viewer.html`. Drop a `.bin`, `.dat`, or `.tim` onto the
page; nothing leaves the browser.

## Crate type

`crate-type = ["cdylib", "rlib"]` - `cdylib` for the WASM build,
`rlib` so the host renderer in `site/` can also link against it for
ahead-of-time bundling experiments.

## See also

- [`site/`](../../site/) - the landing site that hosts this viewer.
