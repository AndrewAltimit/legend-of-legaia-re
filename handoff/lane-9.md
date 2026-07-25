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

## 3. Rule for waiver authors: scope every reason to the CHECKER's units

**The rule.** `check-ui-host-drift.py` measures exactly one thing: whether a
builder's name appears in a non-test file under a *host source root*
(`crates/engine-shell/src` + `crates/engine-render/src` = native,
`crates/web-viewer/src` = web). A `reason` is only checkable against that, so
every reason must be written in the checker's own units - **name the source
root, and phrase the claim as a reference fact about it**. A reason scoped to
anything finer or coarser than a root can be completely true and still describe
a gap the gate is not measuring.

Two failure shapes, both already in this file's history:

- **Scoped too fine (a page, a screen, a binary).** The deleted
  `fishing_hud_draws_for` reason said the browser had "no fishing host: no
  session, no cast/reel input path and no point record". True of the **play
  page**; false of the **root**. `crates/web-viewer/src/minigames_fishing.rs` -
  the site's standalone minigames page, same root - already had a full
  `FishingSession`, a cast/reel input path, a point record *and* a HUD draw
  list; it just serialized that list itself instead of routing it through the
  shared consumer. The waiver read as a greenfield blocker; the real gap was one
  call.
- **Scoped too coarse (a whole crate, when composition is what matters).** The
  old `ap_gauge_sprites` reason claimed no host drew the gauge and named the
  wrong intended caller. Both halves were false: two `native,web` builders
  already fold it in one call deep, and `engine-ui` -> `engine-ui` composition
  is invisible to a root-reference scan, so "unused" never means "unreached".

**Checks before writing a reason.** Say which root lacks the reference. Grep
that *whole root*, not the file you have in mind, and quote the symbols that
came back empty. If the builder composes into another `engine-ui` builder, say
so and treat the `orphan` bucket as a measurement artifact rather than a gap.
And never describe the *work* as larger than the reference fact supports - the
next lane sizes its effort off your wording.

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

1. ~~**SFX are entirely absent from the browser play page.**~~ **CLOSED** - see
   section 6. Left in the list because it is the worked example of how far a
   gate-invisible gap can run: a whole audio channel, missing with zero signal
   in any check.
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

## 6. The SFX channel (the follow-up the coordinator authorised)

`crates/web-viewer/src/play_sfx.rs`. The chain is the retail one and every link
comes from something that already existed - `legaia_asset::sfx_table` for the
descriptors, `legaia_engine_audio`'s `SfxBank` / `SfxScheduler` /
`FootstepCadence`, and the live `WebAudioOut` SPU. **No `engine-audio` or
`engine-core` file was touched**, and no API turned out to be missing.

Descriptors parse at `load_disc` off `SCUS_942.54` (`DAT_8006F198`, so a
`PROT.DAT`-only load leaves the bank empty and every cue is a silent no-op
rather than an error). The class-2 program bank (PROT 0869, documented 0875
alternate as fallback) uploads lazily on the first cue into a dedicated region
at the top of SPU RAM. Cues key through `SfxBank::play_one_shot` into the
**live** SPU, so they share the voice pool with the music.

### The bug this surfaced

The page's BGM allocator claimed `SpuAllocator::new(0x1000, SPU_RAM - 0x1000)`
at **both** upload sites - i.e. all of SPU RAM above the reserved floor,
including the region the SFX bank needs. Native does not do this: its
`stage_scene_vab` caps the BGM span at
`SPU_RAM - SPU_RESERVED - SFX_BANK_SPU_BYTES` precisely so a scene change cannot
stomp the resident SFX samples. Both web sites are now capped the same way, and
a unit test asserts the two regions stay disjoint. Worth knowing: this was
latent-harmless while the page had no SFX, and would have become an
intermittent "some cues go silent after a door" the moment it did.

### Provenance discipline, and where the honest boundary is

`sfx_view.rs` already had a `disc` / `site` convention for exactly this problem;
this host reuses it rather than inventing a second one.
`play_sfx_events_json` reports per event, and `play-app.js` names events, never
cue numbers.

All four advertised events are currently `site`, and that is the honest
boundary of this lane:

- **menu cursor / confirm / cancel** use cue ids `0x21` / `0x20` / `0x37`, which
  *are* traced ring writes - but traced from the **Baka Fighter menu SM**
  (`FUN_801CF388` family), not from retail's pause-menu SM. Real blips at a
  place retail may use different ones. Pinning the pause menu's own ring writes
  would upgrade these to `disc`; that is an RE task, not a wiring one.
- **footstep** has **two** unpinned inputs, not one, and the second only showed
  up because the headless run measured it. See below.

### Finding: the footstep cadence's speed input is not a world-unit delta

The first wiring fed `FootstepCadence::tick` the player's per-tick XZ
displacement, which is the obvious reading of "movement magnitude". Headless,
the player walked 274 world units and **zero** footsteps fired. The kernel's own
constants say why: `interval = 0xF - (min(speed + 0x20, 0xFA) >> 4)` with the
`interval < 0xB` gate needs `speed >= 0x30` (48). The port's controller steps
**2 units per tick**, so the raw delta pins `interval` at `0xD` - permanently
below the gate. Retail's speed word is therefore a different quantity at a
different scale, not a world-space delta.

The port has no analogue: `World` carries a walking *flag* and a fixed step, so
a single-speed walker has to be placed somewhere in retail's moving band.
`WALK_SPEED_UNITS = 0x30` is the deliberately conservative end - the slowest
speed retail treats as moving, so the cadence cannot overstate the step rate.
Both the cue id and this scale are declared `site` in the event table.

**Pinning retail's actual speed word (the one `FUN_80018db0` reads) is the
single highest-value follow-up for this channel**, and it would upgrade the
cadence from "right shape, port-chosen input" to fully retail. `FUN_801d01b0`
in the field overlay computes the per-frame speed
(`docs/subsystems/field-locomotion.md`) and is the place to look.

This is also a general warning for wiring any ported kernel: **a kernel that
runs and produces nothing looks identical to one that is not wired.** Only
driving it with real input and measuring the output separates them - the unit
tests for `FootstepCadence` all passed the whole time, because they feed it
retail-scale speeds directly.

### `engine-audio/src/footstep.rs` now has a host caller - its doc says otherwise

That module's header says **"NOT WIRED. This port is not on the engine's frame
path - nothing calls `FootstepCadence::tick` outside this module's unit
tests"**, and names "the field-mode per-frame audio update in `engine-shell`" as
the caller that would fix it. `crates/web-viewer/src/play_sfx.rs::tick_sfx` is
now that caller, from the other host. **The doc comment needs updating by
whoever owns `crates/engine-audio`** - out of lane 9's scope. This is the
`stale-not-wired-triage` shape (tagged NOT WIRED, analysed live); it is
warn-level and not in `main-ci.yml`, so nothing goes red in the meantime.

### Observation: cues drop under voice contention (shared with native)

Headless, with music playing, `idle_voices` hit **0** and three of four
back-to-back cues returned no voice. That is `play_one_shot`'s documented retail
behaviour ("no free voice -> skip") and it is **not** web-specific - the native
director calls the same function with the same idle-search. The reason both
hosts search rather than reserve: `SfxBank::from_descriptors` takes
`(id, program, tone, note, voices)` and never sets `voice_pref`, so every
descriptor installed from the SCUS table competes for the pool, even though the
table has a mixer-channel field (`sfx-table.md`) that presumably pins one.
Threading that field through would be an `engine-audio` change and is a real
follow-up for both hosts. In normal play it self-corrects - the walk fired 3
cues across its 4 seconds - but a burst is lossy.

### What is NOT in this channel

No battle strike cues (the page has no battle host), no fishing / minigame cues
(their rules engines emit no cue events - adding those is an `engine-core`
change, i.e. lane 4/5), no dialog-advance blip (retail's id there is unpinned
and I would rather ship three honest events than four with one invented). No
`FieldEvent::Sfx` exists in `engine-core` at all, so there is no field-VM cue
stream to drain on either host - native's cues come only from battle strikes and
the dev menu. That is the ceiling on how much of this channel any host wiring
can reach today.

## 7. Known cosmetic defect, present on BOTH hosts

The fishing HUD's caption row renders as `Lu0000  left`: `FishingCaptions::placeholder()`
supplies engine-side English strings ("Lures", "left") at the retail pen
positions, and "Lures" is wider than whatever retail put there, so it overlaps
the lure count drawn beside it. This is not introduced by the web host - both
hosts pass the same placeholder set to the same builder. Fixing it needs the
retail caption strings (overlay rodata, not committed) or a
translation-pack-supplied `FishingCaptions`. Recorded in
`docs/guides/playing-and-viewing.md` so it does not get re-reported as a
rendering bug.

## 8. Files touched

- `crates/web-viewer/src/play_fishing.rs` + `play_sfx.rs` (new), `runtime.rs`
  (fields, the two per-tick hooks, the SCUS SFX parse at `load_disc`, and the
  capped BGM SPU allocators), `lib.rs` (modules), `README.md`
- `crates/web-viewer/tests/play_fishing_host.rs` (4 tests) +
  `play_sfx_channel.rs` (5 tests), both new and disc-gated
- `site/js/play-app.js`, `site/_content/play.html`
- `scripts/ci/ui-host-drift-waivers.toml` (one `[[waiver]]` block deleted, whole
  block, nothing else reflowed - safe to merge alongside lane 8)
- `docs/guides/playing-and-viewing.md`
- `site/wasm/*` rebuilt once at the end; per the coordinator, resolve any
  conflict there by rebuilding rather than by hand.
