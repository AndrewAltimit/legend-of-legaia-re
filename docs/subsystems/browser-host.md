# The browser host, module by module

What the site's wasm bundle exports, one section per module family: the play runtime, the minigames page, the viewer pages and the ROM patcher. The architecture the browser host shares with the native window is in [`engine.md`](engine.md#the-browser-host); the gates that keep the two in step are in [`host-drift.md`](../tooling/host-drift.md).

## Four crates, one bundle

The browser side is four crates:

- [`legaia-web-play`](../../crates/web-play/README.md) - the browser hosts of the port: `runtime`, `play` and every `play_*` module, `boot_title`, `cards`, `resume`, `session_save`, `host_slot`, `pad_bindings`, the field actor layer (`field_actors`, `field_npc`, `field_scene_anim`). In-memory disc access is the format layer's: `legaia_iso::image` and `legaia_prot::toc_index`.
- [`legaia-web-minigames`](../../crates/web-minigames/README.md) - the minigames page's `LegaiaMinigames`; the play page holds one for the art it draws over an in-world minigame.
- [`legaia-web-patcher`](../../crates/web-patcher/README.md) - the ROM patcher, texture replacer and language-pack workbench (`rom_patcher`, `texture_registry`, `texture_pack`, `translate_workbench`); it names no engine crate.
- [`legaia-web-viewer`](../../crates/web-viewer/README.md) - the asset inspectors, and the `cdylib` the site loads.

`web-viewer` depends on `web-play` and `web-patcher`, and `web-play` on `web-minigames`, never the reverse. wasm-bindgen collects the `#[wasm_bindgen]` exports of every crate the `cdylib` links, so `LegaiaRuntime` and `LegaiaMinigames` reach the page from the same `legaia_web_viewer.js` as `LegaiaViewer`: which crate an export lives in is invisible to the site's JS. A module name below is unique across the crates, except `field_npc`, which each crate has - `web-play`'s holds the catalog and its builders, `web-viewer`'s the NPC page's exports over them.

## Playing the port in the browser (`runtime` + `play`)

`LegaiaRuntime` is the engine, not a re-implementation of it: it holds the
native window's own session, `legaia_engine_session::BootSession`, over the
page's audio output (`host_slot`), built with `BootSession::from_host` over
the `SceneHost` `load_disc` assembles in memory - so the browser runs the
ported field /
event VM, the free-movement controller against the per-scene walkability grid,
floor-height sampling, the NPC motion VMs, the interaction probe, and the
inline-script dialogue runner. Drives `site/play.html`.

The split is deliberate:

- **`runtime`** - the simulation. `load_disc` (user's own image, in memory, in
  their browser), `enter_field(name)`, `set_pad(mask)` (PSX pad word),
  `set_camera_azimuth(units)` (so the d-pad remaps camera-relative),
  `tick_frame()` (calls `BootSession::tick` - the native frame order - and
  returns the label of the scene a door just walked into, so the page rebuilds
  around a transition), and `state_json()` (frame / mode / player transform /
  live dialogue box). The page tells the session what it does itself (the
  pause menu, the per-tick queue drains; see the
  [`engine-session` README](../../crates/engine-session/README.md)). The session holds
  the port's seat at retail's **mode table** (`engine-mode::mode::ModeSeat`),
  reconciled once per frame, plus the two INIT modes a host enters by hand
  (`MAIN INIT` at field entry, `CARD INIT` on the pause-menu open). `mode_state_json()` reports the word, its table name,
  the front-end entry word and the seat's edge count; the cross-host oracle is
  `engine-shell/tests/mode_seat_host_parity.rs`, which drives **both** hosts
  over one ladder and compares the chains and the edge counts. The whole
  frame body - this page's ticks (`play_frame_ticks`) against the native
  window's - is
  compared state by state in `engine-shell/tests/host_frame_parity.rs`
  ([`host-drift.md`](../tooling/host-drift.md#output-level-the-frame-body-trace)).
- **`play`** - what the page draws, resolved against the **same**
  `SceneResources` the host already built at scene entry (nothing is decoded
  twice): the assembled map (`field_*` accessors), the lead's field mesh posed
  each frame from the world's live `pose_frame` (`player_mesh_*`,
  `player_transform`), and the scene's MAN-placed NPCs at their live world
  positions (`play_npc_*`). `play_npc_live_model(i)` is the scripted mesh
  re-bind (motion-VM op `0x0E`): it reports the model id the world recorded
  for that NPC, or `-1` while it still draws its spawn mesh, and the page
  re-uploads the mesh when the answer moves. The native window's twin reads
  the same world field and resolves through the same
  `SceneModelBank::tmd_bytes`.

The map + NPC layers make the **native play-window's exact resolver calls**,
pinned by the disc-gated parity test `crates/web-play/tests/play_parity.rs`:

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

The disc-gated `crates/web-play/tests/play_page_presentation_parity.rs` pins the hide-box contract, the
sim-tick clip cadence, and the opening-chain staging below.

## Scene BGM (`runtime`)

The play page plays its music and sound effects through the native window's
own director, `legaia_engine_session::AudioDirector`, over the browser's
audio output (`play_sfx::PageDirector`). `audio_init()` opens a
`legaia_engine_audio::WebAudioOut` (must run inside a user gesture - browser
autoplay policy), builds the director over it and stages the resident SFX
banks at once; every `tick_frame()` then routes the field VM's op-`0x35` BGM
events into it (`SceneHost::route_bgm_events`, the native call) and runs the
director's per-tick SFX routing. A track the script started before audio was
up is started on `audio_init()` from `World::audio.current_bgm`. Off wasm the
director runs over a headless `TestAudioSink`, so the tests exercise the same
staging and firing path. `audio_resume()` (browsers open the context
suspended), `audio_set_gain()`, and `audio_ready()` round out the JS surface.

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
sprites. The same channel carries the `4C E1` balloon, the map display's
labels, the tile-board prompt and the Incense notice; the whole layer is
`legaia_engine_screens::field_messages::field_message_frame`, the native
window's composition too. The page's DOM text box remains only as a fallback for a cached
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
`scene_depth_cue`; the page stages them as WebGL uniforms), the
narration input lock, and the retail intro-skip
(`play_take_prologue_handoff` - Cross skips the whole remaining opening to
`town01`). FMV beats play through `play_fmv` (below); the `town01`
establishing-sweep timeline runs through to the name-entry overlay it opens
(`play_name_entry` below).

## FMV beats (`play_fmv`)

An FMV the field VM triggers (or the title's attract countdown, `fmv_id 0`)
holds the world in `SceneMode::Cutscene` until the movie finishes, as the
native window does. The runtime names the segment
(`play_fmv_wanted_json`: the STR file plus the `fmv_segment_window` narrowing
shared with the native `cutscene_av` - `MV3.STR` carries several ids), the
page slices those raw sectors out of the disc bytes it still holds
(`window.__playDiscBytes` + `disc_file_extent_json`, so the runtime never
keeps a second copy of the image), `play_fmv_install` demuxes the video and
decodes the XA track through `legaia_mdec::str_av` - the native window's
kernel, so both hosts agree on the frame list, frame rate and soundtrack -
and
`site/js/play-fmv.js` puts the XA track on the engine mixer's XA lane
(`play_fmv_audio_start` - the native window's `play_xa`, so the movie sits
behind the same master trim and volume slider as every other sound), draws
the frames over the GL view paced by `play_fmv_due_frame` (the shared
`engine-system::movie::cutscene::MovieClock`), then
`play_fmv_finish` applies the same post-movie scene hand-off.
The retail skip rule is engine-side (`fmv_id 0` only, on the packed
face-button mask). A page that never declares support still auto-finishes the
beat with the hand-off applied, which is what a cached bundle does. Disc-gated
oracle: `crates/web-play/tests/play_fmv_real.rs`.

Two things the browser host has to do that the native one gets for free:

- **Seating.** Scene entry puts the player at the retail *cold-boot spawn*,
  which is only meaningful for `town01` - every other scene is normally entered
  through a door that supplies the arrival tile. Picking a scene from a list has
  no door, so `enter_field` arms the engine's picker seat
  (`SceneHost::arm_picker_entry_seat`, shared with the native `--scene` entry):
  the party lands where another scene's door or the overworld entrance would
  put it ([picker door arrivals](engine.md#picker-door-arrivals)). Only when no
  arrival qualifies does `LegaiaRuntime::seat_player` relocate the cold spawn:
  it keeps it when it has floor under it and otherwise seats the player on the
  walk-ground heightfield, never on a walk-on trigger tile (which would fire on
  the first tick and warp the scene out from under them).
- **Framing.** The page frames every field / overworld / cutscene shot with the
  ENGINE camera (`play_camera`, below), so the vantage is retail's rather than
  the page's own. The camera-occlusion fade dissolves whatever geometry sits
  between that lens and the character; nothing is culled. One export,
  `play_occlusion_fade`, runs the fade's whole frame (arming, gate, ramp); it
  folds the engine-owned screens (pause menu, a menu-runtime screen) into the
  arming term itself, so the page passes only its own vantage terms. The call
  sits inside the frame's `try`, so a throw there costs the fade silently -
  `tests/occlusion_fade_page.rs` pins that its arguments are declared.

## The camera (`play_camera`)

The engine's, not the page's. `play_camera_vp(w, h)` resolves which camera owns
the frame through the shared `engine-world-views::camera_view::resolve_field_camera` -
the retail field follow view, an op-`0x45` cutscene shot with its between-beat
glide, the overworld walk view, or the world map's top-view debug camera - and
returns the column-major view-projection the play page's script uploads.
`play_camera_eye` is the same frame's world-space lens, which is what the
occlusion fade ray-casts from; `play_camera_orbit_by` / `play_camera_tilt_by`
/ `play_camera_zoom_by` steer the user's three follow-camera knobs
(`Camera::manual_orbit` / `manual_tilt` / `manual_zoom`) through the engine's
cutscene-gated setters - the same fields the native window's left-mouse drag
and wheel write, so the movement compass tracks the view on both hosts and a
scripted shot is locked on both. `play_camera_reset_framing` is the Reset
camera button and the canvas double-click: it runs `Camera::recenter_follow`,
the kernel the native double-click calls, which zeroes the three knobs in any
mode and, in free roam, also releases a scripted shot that never handed the
focus back, so the follow camera re-pins to the lead. The page side
(`resetCamera`) also drops the `F3` vantage. `play_camera_set_orbit` remains
for the `F3` hand-off. A
direct entry through `enter_field` runs `Camera::reset_for_scene_entry` (the
native `enter_field_live` is the paired site), and the page's picker calls
`play_abandon_opening_chain` first so a scene picked mid-opening is that
scene's free-roam rather than the interrupted chain's next leg.

The page keeps exactly one camera of its own - the debug orbit vantage on
`F3`, which the native window has too, and which is an explicit override
rather than the default.

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
(`legaia_game_tables::menu_windows`) with the native window's pinned fallback.

The two atlases upload once (`play_menu_font_rgba` / `play_menu_chrome_rgba`);
the page's `AtlasBlitter` (an `image-rendering: pixelated` overlay `<canvas>`
over the GL view) blits the quads with a per-quad multiply tint.

The top-level command list plus the Items / Magic / Equip / Status / Options
sub-screens all run the real
`legaia_engine_menu_runtime::field_menu_dispatch::FieldMenuSubsession` the native
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
lifts that block into the live world and parks it; the page then lands it with
one call, `play_resume_save` (`resume.rs`, over the shared
`engine-scene-host::resume::land_save`: the saved scene, else the running scene,
else the opening town - never a New Game); Save raises the
overwrite prompt and writes the session into the card image. The block-grid
cursor is this crate's, not the session's - `SelectPhase::SlotPreview` ignores
directions by design (see `docs/subsystems/save-screen.md`).

The menu is **not purely input-driven**: its timers count 60 Hz frames, so it
steps on the sim tick, not the animation frame. The page's frame hands its
sim ticks to `play_frame_ticks` (`runtime/frame_ticks.rs`), which hands each
tick to the owner `BootSession::claim_tick` names - the order the native
window's `sim_tick` uses too: a Start edge (or a script's save point) opens the
menu and takes the tick (a Start open queues the confirm blip), the field keeps
ticking under the wipe until the menu spawns, the hand-over takes one tick,
and from then on each tick is one menu step (`play_menu_input`). A tick after the menu closes goes back to the field. Parity is asserted by the disc-gated `crates/web-play/tests/menu_parity.rs`
oracle, which walks the whole card flow.

## Live battles (`play_battle`)

`enter_field` arms the engine's live gameplay loop - the browser twin of the
native `--live-loop --player-battle` flags (`set_live_battles(false)` opts
out). Walking rolls the scene MAN's own step-driven encounter table, the world
flips `Field -> Battle`, and the whole fight runs in `engine-core` (turn SM,
player-driven command / arts / magic / item menus, damage formulas, loot).
This module is presentation only: it folds battle events and strike FX into
the shared `BattleHud` model via `engine_battle_hud::sync_battle_hud_rows`
(the same fold the native window uses), arms the ENCOUNTER! banner on the mode
edge, and mirrors the native window's battle HUD block - `battle_hud_draws_for`
rows (retail HP/MP colour law), `encounter_banner_draws_for`, and the submenu
text - into `play_overlay_draws_json`, in surface pixels. Disc-gated oracle:
`crates/web-play/tests/battle_overlay_parity.rs`.

It also arms the **field-to-battle intro emitter** - the same
`legaia_render_kernels::battle_intro::BattleIntro` the native window ticks, so all
five retail transition styles (tile shatter, both particle fields, curtain,
swirl) draw in the browser. `tick_battle_intro` tracks the encounter session's
`Transition` phase and caches the frame's ordered geometry for the page's
screen-prim pass (`play_screen_prim_*`); the one per-host step is the field
frame readback - the page answers `play_intro_wants_capture` with a
`gl.readPixels` of its finished frame (after the volumetric fog and both
screen-prim lists; the arming frame composes no transition layer once the page declares the
readback with `play_set_intro_readback`, so a host that never reads back -
a headless run - keeps the transition from its first frame) into
`play_intro_land_capture`, and
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
[`site-shell.md`](../tooling/site-shell.md#reaching-the-play-pages-battle-screen-headlessly).

## Battle 3D scene (`play_battle_render`)

The battle's 3D layer under that overlay - the browser twin of the native
window's `enter_battle_render` (`window/battle/scene_entry.rs`). Both read
the stage the session's battle-edge step resolved
(`engine-session::battle_stage::stage_battle_world`), which has already
written what the stage leaves on the world (its class, the fog's stage cap,
each monster's texture slot).
On the `Field -> Battle` edge it builds a **battle VRAM** (the scene rebuilt
with `SceneLoadKind::Battle` so the stage dome + its textures are resident,
plus the PROT 870 flame atlas, per-slot monster texture injection and the
party texture bands) and the meshes the page draws while the fight runs: the
stage **backdrop** (`legaia_scene_formats::battle_backdrop::drawn_objects_tmd`
object-list edit, drawn twice - the second copy pre-appended under the SCUS
`DAT_80078B50` mirror-table transform with `append_scaled`'s winding flip),
the **ground grid** (`build_ground_grid` + the `DAT_80078C1C` depth-cue far
colour, attached as a per-draw cue so the browser grid fogs like the native
one),
**monster meshes** (`monster_archive::battle_render_mesh`) and the
**assembled party battle forms** (`legaia_actor_formats::battle_char_assembly`, real
texture pools + battle palette; PROT 1204 mesh + PROT 1203 rest pose as the
fallback ladder). The engine builds and installs the party forms' idle /
action / swing / art-bank clips at battle entry
(`SceneHost::ensure_battle_party_forms`); the page replays the forms' VRAM
writes into its battle VRAM and installs only each monster's texture slot and
idle, so the engine's own battle SM poses every actor; the page reads
`play_battle_actor_pose` per frame and re-poses positions in place. The
camera runs the **shared** phase script (`engine-battle-vm::battle_cam_script`) that
the native window runs - dialogue / menu-with-orbit / submenu close-up /
action framing - and the page consumes a ready view-projection built by the
retail GTE model rather than re-deriving one. The per-tick VRAM re-stamps
(facial animation, the Stone CLUT recolour, the effect CLUT stage) run
through `play_battle_vram` against the page's battle VRAM copy - the same
three drains the native window runs - and the page re-uploads the texture on
`play_battle_vram_take_dirty` (a VRAM serial doubles as the residency
guard). Battle exit drops the state and the page restores the untouched
field VRAM. The damage numerals, the `N HIT` / `TOTAL` combo cluster and the
`<word> ARTS!!` banner draw in retail's 24x24 cells off the resident battle
effect atlas through the shared `legaia_engine_ui::battle_numerals` builder the
native window emits through; the font-atlas layout is only the fallback for
frames before the battle VRAM exists.

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
the one composition the native window calls,
`legaia_engine_screens::shop_overlay_frame`, driven by the real
`legaia_engine_menu_runtime::menu_runtime::MenuRuntime`; `play_overlay_draws_json`
serves the quads. The screen steps per sim tick, from the page's tick loop:
the shared tick claim lets a shop or the prize exchange take the tick (one
`play_shop_input` step) and holds the pad neutral under an inn, and
`play_shop_after_tick` steps an inn after the field's tick - except on the
tick the field VM opened the screen, whose first step the poll already took.

Retail's shop is a set of descriptor windows rather than one panel, so the
composition paints them - vendor plate, purse, item info, buy / sell
quantity, sell detail, the equipment-buy recipient list - through the same
`painter_at` renderer dispatch and the same disc-parsed rects on both hosts
(the window list is on the
[`engine-screens` README](../../crates/engine-screens/README.md)). They draw only when the menu-overlay window table
parsed: a `renderer_va` is not something the pinned-rect fallback can invent,
so without the real table the windows are absent rather than mislocated.
`crates/web-play/tests/shop_overlay_parity.rs` asserts content lands inside window 32's and
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

One deliberate divergence from the native window: input is **edge**-triggered
(`menu_runtime::step` does no edge detection, and the native window feeds it the
held pad). Both hosts label the buy/sell rows with the real item names off the
SCUS table.

## Developer menu (`play_dev_menu`)

The retail developer menu - the same `engine-menus`
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

The play page has no fishing button: a fishing spot's own script reaches the
minigame through the mode-24 door warp (`SceneHost::drain_minigame_warp`'s
fishing arm), exactly as natively. `LegaiaRuntime::play_fishing_start` is the
export-level launcher kept for drivers; it lifts the fishing overlay (PROT 0972)
through the static-overlay map and hands it to
`SceneHost::enter_fishing_from_overlay` - the door warp's own entry, which the
native window's `L` launcher calls too. That decodes the species, spawn and
cadence tables, runs the rod and lure ownership scans, and installs a
`legaia_engine_fishing::fishing::PondSession` seeded from the persistent fishing
words on `World::minigames` - the same mode-suspend contract the native window
uses, so the field scene stays intact underneath and comes back on exit with
the words banked. The two point-exchange venue pages decode alongside. `play_fishing_hud_json` serves the
retail HUD through the shared `fishing_hud_draws_for` consumer, and
`play_fishing_prizes_json` / `play_fishing_prize_buy` expose the prize rows with
retail availability gating.

Nothing here is an input path, and that is the design: the driver is
`World::tick_fishing`, which reads the pad word the page already routes each
frame. Circle casts and locks the power meter, Cross reels, Square reels
harder - and because the ported reel decoder classifies the two held bits,
holding both resolves the way retail does rather than the way a host `if` chain
would. The banner one-shots seed off `World::minigames.fishing_events`, the
same list the native window reads.

The one place the page draws more than the native window: with the fishing
sprite page undecoded, `fishing_hud_draws_for`'s atlas is blind on both hosts
and it drops every glyph and gauge fill, so native's fishing HUD is digits and
captions only. The gauges are the functional half of the tension tug-of-war, so
this host also emits their resolved frames (the ported cap/body/cap geometry) as
a `bars` channel the page fills as rects. Disc-gated oracle:
`crates/web-play/tests/play_fishing_host.rs`.

## Sound effects (`play_sfx`)

The play page ran with music and no sound effects at all: the native window
stages a descriptor bank from the disc executable plus a resident program bank
into its own SPU region, and this host staged neither. `play_sfx` is that
channel, assembled from what already existed rather than as a second audio path.

The chain is the retail one. `SCUS_942.54`'s static descriptor table
(`DAT_8006F198 + id*8`) is parsed at `load_disc` into a
`legaia_engine_audio::SfxBank` - pure data, so it lands whether or not the
visitor has enabled sound. The slot-0 system bank (PROT 0868) uploads into a
dedicated region at the **top** of SPU RAM the first time a cue fires, and the
rest of that region holds whichever bank the current mode's slot-2 / slot-6
residency names (PROT 0876 in the field, PROT 0869 in battle,
`World::sync_sfx_residency`), refilled on each mode change. Cues key through
`SfxBank::play_one_shot` into the **live** `WebAudioOut` SPU - the same mixer the
BGM sequencer feeds, so a cue and the music share one voice pool as they do on
hardware.

Sharing the mixer means sharing SPU RAM: both BGM upload sites are capped
below the SFX region, matching the native boot's split
(`legaia_engine_audio::spu_layout`), and a unit test asserts the two regions
stay disjoint.

### Cue provenance is reported, not assumed

Retail fires a cue by writing an id into the ring at `_DAT_8007B6D8`, and only a
handful of those writes are traced. So this host carries the same `disc` / `site`
split [`sfx_view`](../../crates/web-viewer/src/sfx_view.rs) established, and `play_sfx_events_json`
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
`crates/web-play/tests/play_sfx_channel.rs`.

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
| Seru-trade shop screens | Closed. Config + name table install at `load_disc`, and both hosts draw the trade screens through the one shared composition (`legaia_engine_screens`). |
| Options screen | Closed. The page persists `OptionsState` to `localStorage` (`legaia.options`, `runtime/options.rs`) through the same serde impl the native window writes to `legaia-options.toml`, and both save `OptionsPins::persistable` on an Options close. |
| Inn prompt | Closed. Neither host opens an inn session (the retail inn is an ordinary field-VM dialogue), and both draw the prompt when one is up: `play_shop`'s inn arm ports the native `window/hud.rs` arm. |
| Load / Save rows | Closed at the model. This host browses the console's two memory-card ports; the native window writes LGSF files to `saves/` - but both build the slot from one `legaia_save` summary (leader name, level, HP/MP, resume scene + location), and the card block carries the engine ext (play clock, saved chains) in the tail retail never reads. |
| Dance / Baka / Muscle / slots | Closed in-world. Both hosts enter them from the scene's own door warps; this host draws them through `play_minigames` (below) with the standalone page's renderers, and the standalone page stays as the free-play surface. |
| Field BGM / SFX / XA | Closed. `play_bgm` carries the native director's guard and volume policy, `play_sfx` the full u16 cue space with the duck and the reward bank, `play_xa` the shout and clip banks. The cast-cue band's voice (the item-use shout) resolves in the world onto the XA channel and plays through `play_xa` on both hosts. |

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
black. `site/js/play-atlas-blitter.js` exposes the `AtlasBlitter`; the page's small boot
controller runs before any scene exists, feeds the title edge-triggered pad
words, and on the New Game outcome seeds the retail defaults + enters the
opening prologue chain (`play_cutscene` above) - after `begin_new_game`
establishes the fresh slate the native `BootSession` does. The title theme
starts with the card and the attract countdown - which runs at the Press
Start prompt as well as on the menu, since retail has no prompt phase to
wait behind - plays `MV1.STR` through `play_fmv`.

The **publisher logos** run ahead of the card on this host too
(`boot_logos_start` / `_step` / `_draws_json`): `PublisherLogosSession` walks
retail's SCEA -> Contrail -> PROKION order off the PROT 0895 atlas, Start or
Cross skips, and the quads come out of the shared
`legaia_engine_ui::boot_logos::publisher_logo_sprite_draws` the native
window draws through, so the 640x480 boot stage letterboxes identically on
both. The title theme starts when the card comes up, not under the logos.

Neither stage is stepped here. Both exports step the session's one front end,
`legaia_engine_session::FrontEnd`, which the native window's boot UI steps
too: the logo skip and tick, the mode seat's `boot_handoff` to `CARD INIT`
with the Continue scan at that moment, the title tick, the row counter onto
the seat, and the attract hand-off with its skip edge. The page supplies the
movie lane (`PageAttractLane` over `play_fmv`, settled again in the arming
frame so an unplayable movie never outlives it), the rack scan
(`cards::rack_has_save`) and what each outcome does. Continue hands the
frame to the session's `ContinueScreen` (`boot_continue.rs`:
`boot_continue_step` reports `"loaded"` with the save parked for
`play_resume_save`, or `"back"`; `boot_continue_draws_json` draws it),
the native window's screen too. The page calls
`boot_title_start` before `boot_logos_start`; the logos replace that title, and
the front end raises a fresh one when they hand off.

One start owns the page at a time. Every user-initiated start - New Game, a
scene picked from the list, a save imported over a live title - bumps a page
start generation and calls `play_cancel_start` first, which drops the logo and
title sessions, abandons a live opening chain, ends an armed movie without
its post-movie hand-off and stops the score; the page side cancels the boot
controller's own loop and the movie overlay. A title loop or outcome that
captured an older generation bails, so a scene picked during the title card is
never replaced by the New Game it interrupted, and a New Game over a running
scene stops that scene's loop until its own outcome enters the opening.

## In-world minigames (`play_minigames`)

The casino slots, Muscle Dome, Baka Fighter and dance-hall sessions install
themselves on every host (the field-VM op-`0x3E` arm and the world-map
`MinigameDoor` walk-on both publish `World::minigames.pending_warp`, and the
shared scene host drains it). This module is their presentation: the runtime
synthesises a compact PROT image of just the minigame entries so the
standalone minigames page's decoders and renderers serve the in-world
session, live state reads off the world's own sessions, the game picture's own
text rides `minigame_overlay_draws`, input is the pad word through the engine's
own tick, and Start exits back to the field with coins and prizes settled.
The engine's status rows - the port's readout of the rules state plus the key
hints, fishing's included - are not in that draw list: `play_mg_caption_json`
exports them as strings (`engine-minigame-scenes::minigame_status::caption`) and the page
shows them in the strip under the canvas
([site-shell](../tooling/site-shell.md#the-play-pages-minigame-status-rows-sit-under-the-canvas)).
`site/js/play-minigames.js` also carries the fishing prize-exchange panel
over `play_fishing_prizes_json` / `play_fishing_prize_buy`. Disc-gated oracle:
`crates/web-play/tests/play_minigames_host.rs`. The overlays' own BGM tracks start through the
shared scene host's warp (`World::swap_to_minigame_bgm`), and the dance
count-in banner and how-to draw through `legaia_engine_ui::dance` on this host too.

## Field effects and script-spawned actors

Field frames draw the effect-pool billboards, `etmd` models, summon / move-FX
parts and the scene's own move-VM stager parts (`play_field_fx_sync`, built
against the page's field view-projection in page space), and actors a scene
script spawns with a TMD (`FieldEvent::ActorSpawned`) get their rest-pose
mesh uploaded and drawn from the live actor transform. The field party HUD
reads the lead's projected stage-Y off the same view-projection. The page
also drains every remaining field-VM event each tick through the one kernel
the native window runs, `World::drain_spawned_mesh_slots`, in a step named as
the window's (`drain_and_route_field_events`) - `World::pending_field_events`
is only ever emptied by a consumer.

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
`engine-menus::name_entry` - the page owns no name logic, only the pad-edge
bridge and the draw-list JSON (`name_entry_draws_json`, the same two-layer
chrome+font shape the pause menu uses). The bridge is per sim tick:
the shared tick claim (`BootSession::claim_tick`) steps the prompt through
the session's `step_name_entry_frame`, as it does for the native window (the
edge, then one frame of caret and camera); `name_entry_input` stays for
drivers that step the entry directly.

The screen lands **with its state**, which is the whole point: the
establishing timeline's pinned op-`0x49` opens it and stays suspended while
`name_entry_is_active()` holds, so the committed name has to reach
`World::party.party_names` for the script to resume at all. It is not an overlay
that could be skipped - skipping it would park the opening forever, the same
failure an unclosable field shop produces. Retail behaviours inherited from
the SM: the middle control button **restores the template default** (it is not
a space key), the confirm prompt opens on **No**, and Vahn's scripted walk-out
(scene-ANM records 47/48) plays post-confirm. Disc-gated oracle:
`crates/web-play/tests/new_game_flow_parity.rs`.

## Battle-stage backdrops in the entry viewer

A `scene_tmd_stream` entry is a battle-stage shell, and retail does not draw it
the way the file lays it out: object **1** never draws, and the shell is drawn
**twice**, the second copy under a per-stage transform that closes the
authored half. `build_current_vram_mesh` places it that way through
`legaia_scene_formats::battle_backdrop`, and `tmd_note` labels the entry with the
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
`engine-scene::field_env` kernel (env-pack vote + `.MAP` object-grid
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
`crates/web-viewer/tests/field_scene_blend.rs`.

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
`crates/web-viewer/tests/field_scene_assembly.rs` (incl. the colour-only prop recovery).

### The map is live

A baked map is not the scene the game shows. The scene's own scripts move
it on every world tick: the field VM animates the floor-height ladder (op
`0x4C` nibble 9 - `jouina`'s travelling path wave, `tunnela`'s pulsing
cells, Rim Elm's tide), and some entry scripts replace the shipped ladder
outright (`concnow`'s `4C 9E` raises its flesh mounds, so its baked heights
are never shown at all); the prop bank ticks every placed prop's clip (the
Rim Elm windmill); the ambient move-VM tree and scripted CLUT effects write
VRAM; VDF morphs move vertices.

So `field_scene_anim_init` enters the scene **live and headless** through
`engine-scene-host::scene_live::LiveScene` - an ordinary `SceneHost` entered the
way the play pages' scene picker enters it, ticked once per retail vsync
with no pad, never following the scene out (a door or scripted battle
re-enters it; repeated exits stop the ticking). The page reads that world
through the play page's own kernels, so there is no second animation path:

| accessor | kernel | what moves |
|---|---|---|
| `field_scene_floor_wave_offsets` | `field_env::FloorWave`, `World::placed_floor_offsets` | terrain cells on the live ladder; placed objects on the ladder they spawned on |
| `field_scene_ground_live_positions` | `field_ground::live_render_positions` | the walk ground, per vertex |
| `field_scene_placement_anim_ids` / `_frames`, `field_scene_mesh_posed` / `_posed_frame_positions` | `PropAnimBank::pose_key`, `field_env::posed_prop_offsets` | clip-bound props, posed (multi-object props assemble instead of heaping on the origin) and re-posed per frame |
| `field_scene_anim_tick` | `World::step_field_vram_effects`, `clut_walk_anim` | the VRAM effects and the CLUT walker |
| `field_scene_morph_slots` / `_positions` | `World::morphed_env_tmd` | VDF morphs |
| `field_scene_npc_*` | `web-play::field_actors::FieldActors` | the MAN-placed actors (NPCs, chests, story actors): posed off the world's clip cursors, placed at its live positions, re-bound and re-cut as the scripts direct |

A ladder is a property of the **room**, not of the map: `concnow`'s system
script tests the region-type mask (op `0x42` mode 0) and installs a different
ladder in each region block (`4C 9F` / `4C 9E` / `4C 90`, each block opened
by a `46 24` view-window write), and retail only ever draws the room the
player stands in. `deene`, `retona`, `vozz`, `uru`, `rikuroa2`, the `conc*`,
`jou*` and `tunnel*` scenes do the same. So `LiveScene` keeps one ladder per
region mask: at entry a throwaway probe host seats a standing player in each
room (from the entry room each time) and records what the scene's own script
installed and the oscillators it spawned, which then step on
`world::step_floor_ladder`, the world's own kernel. Terrain cells, the walk
ground (`field_ground::live_render_positions_by_cell`, per cell) and
window-owned placed objects take their room's ladder; the live player's room
reads the live world, which is what the play page shows.

The actor layer is the play page's own: `FieldActors` is the type
`LegaiaRuntime` holds for its `play_npc_*` exports, and `site/js/field-actors.js`
is the one JS draw path both pages run over the two export families. The
game-world page's **actors** button hides it.

The three kingdom overworld maps run live too, entered as the play page
enters them (`SceneHost::enter_world_map_scene`): their MAN actors play the
kingdom bundle's slot-4 clips
([`world-map-overlay.md`](../formats/world-map-overlay.md)) through
the same actor layer, and the ocean through the CLUT walker. Their ground
does not follow the field ladder, so they report no floor wave and no rooms.
The disc-gated `crates/web-viewer/tests/field_scene_anim.rs` runs the viewer and the play
runtime side by side on `concnow`, `jouina`, `town01`, `jou` and `map01` and
requires every one of these quantities in the live room - and every VRAM
texel the animation writes - to agree tick for tick.

## Field-NPC catalog (`field_npc`)

`LegaiaViewer::set_scene_npcs(name)` loads the field scene (for its TMD pool
+ VRAM) and catalogs every actor the scene's MAN places;
`field_npc_catalog_json()` returns the list. An NPC is not a separate asset
class - it's a **MAN partition-1 placement record**: a model byte indexing
the scene's TMD pool (`res.tmds[model_index]`, *not* the env-pack subset the
map placements use), an anim byte naming a record in the scene's ANM bundle,
and tile bytes for the spawn. `build_npc_catalog` is the pure builder behind
the binding (disc-gated test: `crates/web-viewer/tests/field_npc_catalog.rs`).

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

### Conversation trees (`npc_dialog`)

`field_npc_dialog_tree_json(catalog_idx)` is the actor's whole conversation,
not the one line the card shows. The walk is the field kernel's
`man_field_scripts::placement_dialog_tree` (built into each catalog entry as
`NpcEntry::dialog_tree`); it starts at the record's interaction cursor - one
instruction past the spawn section's `0x21` - and follows every arm of the
script the dialog SM `FUN_80039B7C` would run:

- consecutive `0x1F` lines are one **page**; the byte after a page is
  classified as `FUN_80038050` does (`inline_dialogue::talk_dispatch`):
  `0x21` ends the talk, `0x24` / `0x25` / `0x48` / `4C FF` / `4C FE xx`
  continue, a picker opens a **choice**, and any other byte ends the talk
  with the cursor parked on it - the tree's **next talk** arm, walked from
  that byte;
- every conditional jump in the [branch-encoding
  table](script-vm.md#branch-encodings) is a **branch** labelled by the
  condition its own operands name (story flag, gold / level / random
  compare, held or worn item, tile box, ...); waits stay inline;
- item / gold / party / flag writes, scene changes, battles, op-`0x49`
  menus and movies are **actions**, and an op-`0x44` spawn whose record
  carries text is an **event** with that record's own tree;
- a path that reaches an instruction already shown becomes a **goto** to
  the node that covers it, so the tree is finite and cycle-safe; an
  instruction the decoder cannot size ends its path as `undecoded`, never
  guessed past.

`npc_dialog::tree_json` renders it, decoding lines in the browser from the
loaded disc (`0xC1` default party names, `0xC2` / `0xC3` / `0xC4` item and
spell names from the disc's SCUS tables). Retail stores no speaker per line:
an actor's own pages are that actor's, and the page labels only balloons and
event lines. Across the field corpus nearly every talker's tree reaches its
text, about two-thirds reach several pages and almost half branch; disc-gated
test `crates/engine-scene/tests/npc_dialog_tree_disc.rs`.

## Shop vendors (`shop_vendors`)

`scene_shop_vendors_json()` ties each gold shop of the scene loaded by
`set_scene_npcs` to the actor that opens it. A shop is an op-`0x49` record
inline in the MAN's script bytes ([shop.md](shop.md)), and the MAN's three
record partitions tile its data region with contiguous script records, so
the record whose span holds the shop's `count` byte is the one that issues
it. A partition-1 record at index `>= 1` is a placement, and its index is the
catalog's `slot`; the JSON carries that catalog index as `npc`, or a `why`
when the owner is the scene controller (partition 1, record 0), a
partition-0 / partition-2 record, or an actor the curated catalog withholds.
Across the disc every gold shop the `asset shop-stock` scan finds ties to a
catalogued merchant except the Ancient Wind Cave's four, which sit in
partition-0 records (disc-gated test:
`crates/web-viewer/tests/shop_vendors_real.rs`). The shops page loads each
town's shop scenes, poses every vendor into one WebGL canvas through the NPC
page's poser (`site/js/field-npc-model.js`) and copies each out as a still
(`MeshView.snapshot`); its item renders reuse the characters page's cards
through `site/js/item-thumbs.js`.

The shops page's map pane is the game-world page's map view
(`FieldSceneView` over `set_scene_field` + `field_scene_anim_init`) on the
scene that holds the town's vendors. A vendor is placed through the same
record the model comes from: its shop's owning record is partition 1,
index `slot >= 1`, so the marker follows the live actor layer's entry with
that `slot` (`field_scene_npc_catalog_json`), reading its position each frame
from `field_scene_npc_draw_poses()` and projecting it through the map camera
(`buildWorldOrbitVp`). Many shopkeepers are flag-dispatched spawns: the
header tile is the off-map hide box or a staging square, and the spawn
prologue's story-flag arms park the actor in one state and run it to its
counter in another. So each vendor record also carries a `seat`
(`shop_vendors::vendor_seat`): the first non-parked target of
`placement_spawn_relocation` with every flag set, then with every flag
clear, else the header tile. The marker follows the live actor while the
running scene shows it at that seat, and sits on the seat otherwise (drawn
dashed: the map runs another story state). A shop owned by a partition-0
record (the Ancient Wind Cave's) has no actor and so no marker.

`shop_item_records_json()` (`shop_items`) is the open item's disc side: per
named item id, the item record's `kind` and price (sell is half), the
equipment stat-bonus record (slot, INT / ATK / UDF / LDF / SPD, equip mask,
Ra-Seru flag), the consumable's effect descriptor with its restore amount or
stat effect, and the passive an equipped piece grants with its stat boosts -
the `game-tables` parsers joined, decoded once at `load_disc` (disc-gated
test: `crates/web-viewer/tests/shop_items_real.rs`). An open weapon or armour
row draws the item-alone mesh (`set_equipped_character` with only that
section filled, then `equipped_item_only_*`) in a `MeshView`, posed at frame
0 of the character's first clip.

## Tactical Arts viewer (`arts_view`)

`LegaiaArts` drives `site/arts.html`'s interactive half: click an art card,
watch the character perform it. Per character it assembles the battle mesh
from the player battle file's equipment sections
(`legaia_actor_formats::battle_char_assembly`, PROT 863..866), textures it from the
same file's pools + decoded battle palette into a runtime VRAM image, and
decodes the record[0] idle loop plus the whole art-animation bank through
the `readef.DAT` `"ME"` archives (PROT 894) - every clip pre-expanded per
assembled object so the page's pose loop drives object `i` with channel
`i`. Arts-voice shouts are the RE'd retail cue: the character's own XA bank
demuxed off the disc, each art mapped to a real member of its
`FUN_8004C140` candidate channel pool.

`export_character_glb` bakes the current character + its **entire**
animation bank into one `.glb` (`legaia_actor_formats::character_gltf`): one node
per rigid TMD object, each decoded bank record a named glTF TRS animation
("battle idle" first; art names where the record carries one, else the
`anim_id` in hex), textured from the same VRAM via a baked tile atlas and
shaded by the same per-vertex packet colour the canvas modulates with
(`COLOR_0`; see `legaia_actor_formats::gltf_color`). The page's download button
hands it out client-side - nothing is uploaded.

The page's tinted after-image trail (the retail arts ghosting; per-character
tint) is a JS-side re-draw (`MeshView.setTrail` -> the renderer's
`ghostTrail` additive passes) with a persisted user toggle, default on.
Disc-gated oracle: `crates/web-viewer/tests/arts_view_real.rs` (bank coverage, arts-table
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
Clips come from the file's own `record[0]` action bank, the
equipment-spliced weapon swings (so they change with the weapon), and the
character's **Tactical Arts** (`art_clip_bank`): every curated art
resolved to its `readef.DAT` ME-archive record through the arts page's
ladder (action constant, then name + combo, combo, name), its consecutive
strike records concatenated into one clip, and curated rows that land on an
already-claimed record collapsed into it. The art decode is per character
and cached on the host; each loadout only re-expands the streams per
assembled object. The summary tags every clip `kind` (`action` / `swing` /
`art`) and carries the art's curated kind, AP, input and segment count, so
the page can group them. `equipped_character_glb` bakes the whole posed
character with that bank, named for the character and what it wears.

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
[`docs/formats/battle-data-pack.md`](../formats/battle-data-pack.md#the-item-is-still-separable---by-palette-not-by-geometry).
Disc-gated oracles: `crates/web-viewer/tests/equipment_view_real.rs` plus the 81-record sweep
in `crates/actor-formats/tests/equip_item_real.rs`.

`equipped_item_only_glb(section)` is the second download beside it: the
**item alone** - no host limb, no skin, no unchanged default geometry - the
opinionated cut of `battle_char_assembly::equip_isolate` under the section's
default reading (colour diff against the bare limb for held items and
headgear, geometry-and-colour identity for body and footwear) or the
record's committed rule in `crates/actor-formats/data/equip-isolation.toml`. The
summary's per-item `isolation` object and the glTF root name carry the mode,
the kept / dropped primitive counts and whether a rule hand-checked the
record, and the grip repair's `bridges` / `bridged_triangles`
(`equip_repair` lofts a tube between the two shaft rims a welded weapon
leaves where the fist hid the haft; the root name says `grip inferred`)
and `skin_masked_primitives` (kept primitives whose painted skin
`equip_skin_mask` made transparent: their `cba_tsb` points at a masked
15-bit copy inside `equipped_vram_bytes()`, so the preview needs no
extra upload).
The `equipped_item_only_{positions,uvs,cba_tsb,indices,object_ids,flat_rgba,bounds}(section)`
family is that same repaired geometry as mesh streams, parallel to the
`equipped_mesh_*` set, so the page's preview toggle draws exactly what the
file holds; `equipped_mesh_item_mask(section)` (per-vertex `0 / 1 / 2`:
outside the section / left behind / item) is the pre-repair view of the
same cut. Background:
[`docs/formats/battle-data-pack.md`](../formats/battle-data-pack.md#the-item-alone---an-opinionated-cut-with-a-committed-rule-table);
sweep + table integrity in `crates/actor-formats/tests/equip_isolate_real.rs`, the
grip sweep in `crates/actor-formats/tests/equip_repair_real.rs`.

The characters page's **equipment panel** - every slot, every piece as its
own card - runs on `equipment_item_card_json(slot, section, id)` /
`equipment_item_card_pixels(size)` / `equipment_item_card_glb(alone)`: one
single-item build per `(character, section, id)`, cached on the viewer so the
three calls share it. The JSON carries the name, the palette-cut class, the
item-alone decision (`mode` / `curated` / `note`), what it kept and what the
grip repair added; the pixels are the item alone at the character's rest
stance, drawn by `legaia_actor_formats::mesh_raster` (software, so forty cards do
not need forty GL contexts) - a stick-shaped piece stood on its long axis
with the rest-pose-higher end up, a compact one left as worn and yawed to
show its wide side - over a transparent background; the glb is the alone / with-limb
download without equipping the piece first. The page builds cards through a
queue that yields to the orbit view between items.

## Playable minigames (`legaia-web-minigames`)

`LegaiaMinigames` is a standalone `#[wasm_bindgen]` class (its own
`load_disc`, no canvas) that runs all five of the game's side-games in the
browser for `site/minigames.html`. It is a thin JSON shell over the
from-scratch rules engines in `legaia-engine-core` - the beat clock + judge
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
(`dance_press` / `baka_choose` / `slot_step`, one packed pad word a frame /
`fishing_pond_start` + `fishing_pond_tick` / `muscle_commit` +
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
`crates/web-minigames/tests/minigames_wasm_api.rs`.

`fishing_web.rs` and `muscle_web.rs` are the fishing / Muscle
Dome shells. Fishing drives [`legaia_engine_fishing::fishing::PondSession`] -
the one session all three hosts run - directly, with no `World`, seeded from
the persistent words the page reads off the visitor's memory card, and
resolves species names against the loaded PROT 0972 overlay image; the
cast-meter sweep rate and the flight / reel-down / snap glue are the module's
engine-side reconstruction (`docs/subsystems/minigame-fishing.md`). Muscle Dome drives
[`legaia_engine_battle_menus::muscle_dome_session::MuscleDomeSession`] on the disc's dealt
hand with the native launcher's flat favored per-card cost (the browser has
no player battle file for the per-command `+0x74` swing bytes) and a
battle-path damage stand-in matching the native `tick_muscle_dome`
constants, and [`legaia_engine_minigames::muscle_dome::DomeContest`] for the
ladder above the leg (`docs/subsystems/minigame-muscle-dome.md`). Having no
save file to read a flag bank out of, the page passes the course-unlock and
Master-gate flags it wants open and the shared kernel applies the same rule
to them.

`baka_presentation.rs` adds the Baka Fighter duel's **presentation** exports so
the page draws with the cabinet's own assets: per-side fighter mesh buffers
(player = PROT 1204 battle pack slot, opponent = its own PROT 1206..=1219
`[TIM][TMD][anim]` pack), pose-frame decodes from the real animation banks
(PROT 1203 bank records `char*9 + action` for the party, the pack's own bank
for the opponent), the 51-record HUD widget table + PROT 1203 art pages, the
stage TMD set, and a per-duel 1 MB VRAM build. Consumed by
`site/js/minigame-baka.js`; see
`docs/subsystems/minigame-baka-fighter.md` § HUD widget table.

`dance_presentation.rs` adds the dance's **presentation** exports: the PROT
1230 art pack's HUD page decoded through its row-500 CLUT strip
(`dance_hud_page_rgba`), the overlay's 34-record widget table + the traced
emitter geometry (`dance_widgets_json` / `dance_layout_json`, incl. the
capture-pinned `+4`-line draw-environment offset), the dancer face-stamp
windows with the pose blits replayed (`dance_face_rgba`; dancer 0 = Noa's
field atlas, PROT 0874 §2), the SFX cue bank (PROT 1228 descriptors +
the PROT 1231 sample VAB - a TOC-tail entry) plus the direct-keyed hit
stings (`dance_sting_pcm`), and the real BGM pair rendered through the
from-scratch SPU (`dance_bgm_pcm_i16`). Consumed by
`site/js/minigame-dance.js`; see `docs/subsystems/minigame-dance.md`.
Disc-gated oracle: `crates/web-minigames/tests/minigames_dance_api.rs`.

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
`scene_export_add_instance_euler(handle, tx, ty, tz, rot_x, rot_y, rot_z, scale)` /
`scene_export_finish() -> Vec<u8>`. The page feeds the same mesh buffers it
uploads to WebGL plus the same per-draw `(translation, rotY, scale)`
triples it builds model matrices from; a draw whose record carries a pitch
or roll goes through the `_euler` entry with its three raw angles, which
writes the same `Rx * Ry * Rz` node rotation the native `export-glb` writes
(`scene_assembly::placement_rotation_quat`), so a tilted record is not stood
upright in the file. The bake
(`legaia_actor_formats::scene_gltf::build_scene_glb`) renders every distinct
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
animations). Disc-gated smoke: `legaia-actor-formats`'s
`crates/actor-formats/tests/scene_gltf_real.rs`.

## In-browser ROM patcher (`rom_patcher`)

`rom_patcher::patch_rom(image, seed, lang_pack, …)` runs the Track-1
[`legaia-patcher`](../../crates/patcher/README.md) randomizer entirely client-side and
returns `{ data, summary, seed }` - the patched disc bytes for download, a
human-readable change report, and the resolved numeric seed. `resolve_seed`
exposes the seed-string hash so the page can display it. It drives the static
site's `tooling/rom-patcher.html` page: the user supplies their own disc, sets
the toggles, and downloads a patched image. The disc bytes never leave the
browser and nothing is uploaded - the same "user supplies the disc" model as
the CLI, so the site ships only code.

Every knob is a flat argument, one per CLI flag - the randomized pools (drops,
encounters, chests, shops, casino, steals, arts, doors), the tuning strings
(monster stats, move power, element affinity, spell costs, equip bonuses, XP
and catch-rate scales, the AP sliders), the softlock and quality-of-life
fixes, and the standalone content toggles. The function is the browser twin of
`legaia-patcher randomize`, so its argument list tracks that CLI's flags
rather than being a curated subset; the
[randomizer reference](../tooling/randomizer.md) is the per-feature
page for all of them. That includes the Delilas party swap and its two
sub-selects (arts-voice mode, move-set mode) - with one contract of its
own: a mid-apply Delilas error **fails the whole patch** ("no ROM was
produced") instead of the other options' skip-and-report, because the
swap's half-applied state spans too many carriers to ship.

`patch_rom` and `apply_texture_replacements` are async and take an optional
trailing `progress(stage_index, stage_count, label)` JS callback: each feature
stage reports once and the function then yields one `setTimeout(0)` macrotask,
so the page's progress bar paints between the synchronous patch stages instead
of the tab freezing for the whole run (a `Promise.resolve()` microtask would
not repaint). Yields happen only at stage boundaries, never in inner loops,
and without the callback nothing yields.

An optional `lang_pack` YAML argument (default `""` = English, strictly opt-in)
applies a [language pack](../tooling/patcher-modules.md#translation-packs) **before** any
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
art (PROT 863..866, `legaia_actor_formats::battle_texture_catalog`) is likewise
headerless and likewise invisible to a TIM scan, but does have an encoder, so
it is replaceable - against its record's own slot allocation rather than a
stream length, which is a budget that can genuinely be missed. The monster
battle skins (PROT 867, `legaia_actor_formats::monster_archive::MonsterPage`) are a
third headerless family, one page per enemy inside its own compressed archive
slot, and they carry the registry's only per-texel decode: a monster page has
no single colouring, so each texel renders through the palette of the polygon
that samples it. A `Tier` says which it is, and a family that declares itself
read-only cannot resolve a write.

A row carries derived metadata only: coordinates, dimensions, palette count,
byte length, VRAM placement, a label, and an FNV-1a-64 fingerprint of the
stored bytes. A label is either curated - [`tim_labels`](../../crates/asset/README.md),
keyed by fingerprint - or composed per row from disc data, which is why it is a
`Cow`: the battle-art tier joins each block to the equipment it dresses, so
`ScanCtx` carries the disc's `SCUS_942.54` item-name table alongside
`PROT.DAT`. That join is what makes the family searchable by the word a person
would actually type. The monster tier composes its label the same way and adds
the id - `"Songi #179"` - because retail reuses monster names freely, so the
name alone would produce rows a person cannot tell apart. The scan is a streaming pass into a caller sink -
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

## Other modules

| Module | Covers |
|---|---|
| [`nav_disc`](../../crates/web-viewer/src/nav_disc.rs) / [`inspect`](../../crates/web-viewer/src/inspect.rs) / [`viewer_render`](../../crates/web-viewer/src/viewer_render.rs) | `LegaiaViewer`'s disc load + entry navigation, the entry inspector's exports, and its internal render helpers. |
| [`scene_geom`](../../crates/web-viewer/src/scene_geom.rs) | Kingdom / walk-ground / continent / ocean / slot-4 geometry exports behind the world-overview pages. |
| [`shop_items`](../../crates/web-viewer/src/shop_items.rs) | `shop_item_records_json`: per-item SCUS records (item, equipment, effect, passive) for the shops page's open item. |
| [`world_mist`](../../crates/web-viewer/src/world_mist.rs) | The kingdom MAN's fog-region table and the fog sheet art, for the world overview's Mist overlay. |
| [`sentinel_placements`](../../crates/web-viewer/src/sentinel_placements.rs) | Cross-references a save state's live world-map actors against the disc's MAN placement records. |
| [`prot_locate`](../../crates/web-viewer/src/prot_locate.rs) | `PROT.DAT` offset to owning-entry lookup, the browser twin of `prot-extract locate`. |
| [`player_anm`](../../crates/web-viewer/src/player_anm.rs) | Player-ANM corpus + record decode exports. |
| [`packet_color`](../../crates/web-viewer/src/packet_color.rs) | Per-vertex packet-colour streams for the WebGL renderer, over `engine-field::packet_color`. |
| [`pad_bindings`](../../crates/web-play/src/pad_bindings.rs) | The keyboard-to-pad binding table, read from the engine rather than typed into each page. |
| [`summon_view`](../../crates/web-viewer/src/summon_view.rs) | `LegaiaSummons` - the Seru-magic summon viewer. |
| [`translate_workbench`](../../crates/web-patcher/src/translate_workbench.rs) | `Workbench` - the in-browser language-pack editor over the visitor's own disc. |
| [`fishing_scene_web`](../../crates/web-minigames/src/fishing_scene_web.rs) (`web-minigames`) | The fishing venue's 3D field scene and the player's body for the minigames page. |
| [`play_minigame_arena`](../../crates/web-play/src/play_minigame_arena.rs) / [`play_minigame_slots`](../../crates/web-play/src/play_minigame_slots.rs) / [`play_dance_art`](../../crates/web-play/src/play_dance_art.rs) | The play page's in-world Muscle Dome / Baka Fighter / dance and slot-machine read-outs and art. |
| [`play_battle_body_blend`](../../crates/web-play/src/play_battle_body_blend.rs) / [`play_battle_limb_dim`](../../crates/web-play/src/play_battle_limb_dim.rs) | Browser seats of the battle body's whole-mesh semi-transparency and the Rot limb dimming. |
| [`play_field_fx`](../../crates/web-play/src/play_field_fx.rs) | Field fog sheets, actor drop shadows and op `0x34` sub-1 attached lights. |
| [`play_field_hud`](../../crates/web-play/src/play_field_hud.rs) | The field party-status HUD (name / `LV` / `HP` / `MP`). |
| [`play_tile_board`](../../crates/web-play/src/play_tile_board.rs) | Tile-board (op `0x49`) rendering on the play page. |
| [`play_world_map_markers`](../../crates/web-play/src/play_world_map_markers.rs) | The overworld's kind-coded entity and player markers (a port marker, not a retail draw). |
| [`play_frame_step`](../../crates/web-play/src/play_frame_step.rs) | The shared wall-clock to sim-tick rule (`engine-field::frame_step::SimStepper`). |
| [`host_slot`](../../crates/web-play/src/host_slot.rs) | The page's slot for its `BootSession` over `play_sfx::PageSink`; every page module reaches the scene host through it. |
| [`play_hotkeys`](../../crates/web-play/src/play_hotkeys.rs) | `play_host_hotkey`: the native window's option / cheat / camera keys, resolved through the shared `engine-session::hotkeys` table; `play_host_hotkey_notice` / `play_host_hotkey_refusal` word the notice line; `host_hotkeys_json` lists the table. The page half is `window.__playHotkeyActions` in `play.html`. |
| [`play_lighting`](../../crates/web-play/src/play_lighting.rs) / [`play_fog_volume`](../../crates/web-play/src/play_fog_volume.rs) | The enhanced-lighting and volumetric-fog enhancements (twins of the native `I` / `F8` and `F9`), from the shared engine-side sources. |
| [`play_cheats`](../../crates/web-play/src/play_cheats.rs) | The play page's Cheats panel, thin bindings over `engine-cheats::cheats`. |
| [`play_toggles`](../../crates/web-play/src/play_toggles.rs) | Runtime twins of native launch switches: `--retail` (session-only `OptionsState::pin_retail`; the store is not written while it holds), `--set-flag`, `LEGAIA_SMART_MONSTERS`, `LEGAIA_WORLD_MAP_MARKERS`, and the [gameplay tweaks](gameplay-tweaks.md) (`set_shiny_seru`, `set_flee_exp`, `set_exp_rate_permille`, `set_fortress_encounter_fix`, `set_jewel_fix`, `set_hp_readout_guard`, `set_bonus_drop`, and the battle-tuning setters `set_enemy_stat_permille`, `set_enemy_attack_permille`, `set_spirit_ap`, `set_damage_ap`, `set_seru_catch_rate`). |
| [`audio_api`](../../crates/web-viewer/src/audio_api.rs) | `LegaiaAudio`, the media page's audio bindings. |
| [`play_host_parity`](../../crates/web-play/src/play_host_parity.rs) | Read-only probes over the live `World` for the tests pairing this host against the native window; not wasm exports. |
