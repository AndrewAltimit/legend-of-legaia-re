# Handoff from lane 9 (native-vs-web engine-platform drift)

## 1. What closed

`fishing_hud_draws_for`. The browser play page now has a fishing host
(`crates/web-viewer/src/play_fishing.rs`): a session lifted off PROT 0972, the
pad word the page already routes as the cast/reel path, `World::fishing_points`
as the point record, and the HUD projected through the shared
`fishing_hud_draws_for` consumer with the same blind sprite atlas the native
window passes. Its waiver is deleted.

Drift: **51 both / 4 native-only** -> **52 both / 3 native-only**.
Verified headless (Chromium, real disc, `town01`): 97 text quads and 30 483
painted overlay pixels while casting, `Z` walking the phase to `fighting`,
progress accruing while held, three resolved gauge frames, prize rows named off
SCUS with retail gating, and the field mode restored with the overlay cleared on
exit.

## 2. Nothing needed from another lane

No `engine-core` API was missing. `World::enter_fishing` / `exit_fishing` /
`fishing_points` / `open_fishing_exchange` / `fishing_exchange_buy` /
`tick_fishing` and the whole `fishing` rules module were already there, and
`engine-ui`'s `ui_fishing` already exported every builder the host needed. The
gap was purely on the web host side, i.e. inside lane 9's own scope. **No
`engine-core`, `engine-ui`, `engine-shell`, `engine-vm` or `engine-render` file
was touched.**

## 3. A measurement lesson for the waiver file

The deleted waiver said the browser had "no fishing host: no session, no
cast/reel input path and no point record". That was true of the **play page**
and false of the **crate**: `crates/web-viewer/src/minigames_fishing.rs` (the
site's standalone minigames page) already had a full `FishingSession`, a
cast/reel input path, a point record *and* a HUD draw list - it just serialized
the draw list itself instead of routing it through `fishing_hud_draws_for`.

That matters because the checker's `web` root is the whole crate, not the play
page. So a waiver phrased about one page can describe a gap the checker is not
measuring, and the work it implies can be far smaller than the wording suggests.
When writing a `web_missing` reason, say which *source root* lacks the thing,
not which page - the checker only ever sees the root.

Sibling trap already recorded in the file's own history: the `ap_gauge_sprites`
reason asserted two things that were both false because `engine-ui` ->
`engine-ui` composition is invisible to the checker. Same class of error, other
direction.

## 4. The three waivers left standing - all re-verified true

- **`battle_hud_draws_for`** - `grep` for `tick_encounter`,
  `arm_scripted_encounter`, `install_encounter_*`, `drain_encounter_formation`
  and `SceneMode::Battle` across `crates/web-viewer/src` returns **zero** hits;
  `site/js/play-app.js` mentions battle only in the comment about Start being
  inert. Reason stands verbatim.
- **`dev_menu_list_draws_for`** - `window/dev_menu.rs` exists and routes it
  behind `LEGAIA_DEV_MENU=1`; no `dev_menu` / `DevMenu` / `debug_menu` symbol
  anywhere in `crates/web-viewer/src`. Reason stands verbatim.
- **`game_over_draws_for`** - `BootUiState::GameOver` appears at exactly two
  sites, both `match` arms (`boot_cutscene.rs:364` input, `:621` draw), and the
  variant carries `#[allow(dead_code)]` in `window.rs` - the compiler itself
  agrees nothing constructs it. Left alone deliberately: the waiver's
  instruction ("close it with a runtime probe, not by picking a trigger") is the
  right call, and wiring either host would commit the port to a menu retail does
  not have.

## 5. Unmeasured drift the checker structurally cannot see

The gate counts references to `*_draws_for` builders from two source roots. That
makes it blind to everything below. None of these is fixed here; the list is the
deliverable.

### Native-only, no draw builder involved

1. **SFX are entirely absent from the browser play page.** `engine-shell/boot.rs`
   reads the SCUS sound-effect descriptor table into a `legaia_engine_audio::SfxBank`
   and stages its VAB into the director (`read_sfx_bank` / `set_sfx_bank` /
   `stage_sfx_vab`). `crates/web-viewer/src/audio.rs` and `audio_api.rs` contain
   no `sfx` symbol at all - the page has BGM and nothing else. This is a whole
   audio channel missing with zero signal in any gate.
2. **FMV / STR playback.** Native has `window/str_player.rs` and the `play-str`
   subcommand. The play page *auto-skips*: `tick_frame` calls
   `finish_cutscene()` the moment the field VM triggers an FMV, because the page
   has no MDEC path wired into the scene loop (the MDEC decoder exists in the
   crate - `audio.rs` uses `StrFrameAssembler` - but only for the viewer's STR
   page). So a scene whose script plays a movie silently plays no movie.
3. **The other four minigames in the live world.** Native `play-window` starts
   dance (`K`), slots (`O`), Baka Fighter (`B`) and Muscle Dome (`M`) as mode
   suspends on the running scene. The browser has all four, but only as
   standalone pages on `LegaiaMinigames` - they are not entries on the play
   page's world. Fishing is now the only one that is.
4. **Render toggles.** `--dynamic-lighting` and `Renderer::set_psx_mode` (vertex
   jitter + 15-bit dither) are native-only; no `dither` / `jitter` / lighting
   toggle exists in the page's WebGL path. The two hosts therefore cannot be
   asked for the same rasterisation.
5. **Battle, world-map dev toggle, record/replay.** `window/battle.rs`,
   `battle_cam.rs`, `record.rs` have no web counterpart. (Battle is waived at the
   HUD level; the *host* absence is broader than the one builder.)

### Web-only (the gate reports "web-ahead" as informational and would not fail)

6. **VR.** `site/js/vr-mode.js` presents the play page, world-overview and
   field-scene viewer through WebXR. Native has no VR path at all.
7. **Memory-card rack + browser save import/export.** `crates/web-viewer/src/cards.rs`
   models two card ports the in-canvas Load/Save screens read and write; the
   native window uses `saves/` on disk.

### Same screen, different plumbing (invisible to a reference count)

8. **Menu input edge vs held.** The native window feeds `MenuRuntime::tick` the
   *held* pad (`redraw.rs` builds `MenuInput` from held state); the page feeds
   *edges* (`play_menu_input(edge)`). `menu_runtime::step` does no edge
   detection, so the same builder is driven by two different cursor cadences.
   Documented as deliberate in the crate README, but it is drift a
   reference-count gate can never surface.
9. **Shop row labels.** Native draws placeholder `"Item"` labels; the page
   resolves real SCUS names. Same builder, different content.
10. **Key bindings.** Native reads `legaia-input.toml` and has
    `config set --binding`; the page's bindings are a hardcoded JS keycode table
    (`PAD` / `BOOT_PAD` in `play-app.js`). This is the substance behind the
    `key_rebind_draws_for` orphan waiver, but the waiver only covers the
    *screen*, not the capability split.

### Verified NOT drift (checked, and they agree)

- The 320x240 stage transform is byte-identical arithmetic on both hosts
  (`play_menu::stage_transform` vs `title_save_draws::save_select_stage`,
  same `BOOT_UI_STAGE_W/H`, same `clamp(1, 4)`), so overlay geometry lines up.
- The banner pens the page uses match the native window's (`(8, 60)` level-up,
  `(8, 40)` capture), and the fishing status rows here use the native `(8, 62)` /
  `(8, 80)`.

## 6. Known cosmetic defect, present on BOTH hosts

The fishing HUD's caption row renders as `Lu0000  left`: `FishingCaptions::placeholder()`
supplies engine-side English strings ("Lures", "left") at the retail pen
positions, and "Lures" is wider than whatever retail put there, so it overlaps
the lure count drawn beside it. This is not introduced by the web host - both
hosts pass the same placeholder set to the same builder. Fixing it needs the
retail caption strings (overlay rodata, not committed) or a
translation-pack-supplied `FishingCaptions`. Recorded in
`docs/guides/playing-and-viewing.md` so it does not get re-reported as a
rendering bug.

## 7. Files touched

- `crates/web-viewer/src/play_fishing.rs` (new), `runtime.rs` (4 fields +
  `tick_fishing_banners()` in `tick_frame`), `lib.rs` (module), `README.md`
- `crates/web-viewer/tests/play_fishing_host.rs` (new, disc-gated, 4 tests)
- `site/js/play-app.js`, `site/_content/play.html`
- `scripts/ci/ui-host-drift-waivers.toml` (one `[[waiver]]` block deleted, whole
  block, nothing else reflowed - safe to merge alongside lane 8)
- `docs/guides/playing-and-viewing.md`
