"""Tier 3 rows: paired simulation injection sites, and the files the native
redraw's named steps live in.

Analysis: `ui_host_drift.tiers.sim_pairs`.
"""

from ui_host_drift.rows.host_files import (
    NATIVE_ASSETS_ACTORS,
    NATIVE_BOOT_CUTSCENE,
    NATIVE_BOOT_FRAME_TICK,
    NATIVE_WINDOW,
    SESSION_TICK_OWNER,
    WEB_RUNTIME_FRAME_TICKS,
    NATIVE_BOOT_SCENE_ENTRY,
    NATIVE_DEV_MENU,
    NATIVE_GEOMETRY,
    NATIVE_KEYBOARD_OPTION,
    NATIVE_RUN,
    NATIVE_MINIGAME_SESSIONS,
    NATIVE_REDRAW,
    NATIVE_REDRAW_PASSES,
    NATIVE_REDRAW_TICK,
    NATIVE_SAVE_HELPERS,
    NATIVE_TITLE_SAVE,
    NATIVE_XA_DISC,
    WEB_BOOT_CONTINUE,
    WEB_BOOT_TITLE,
    WEB_FIELD_SCENE,
    WEB_MINIGAMES_DANCE,
    WEB_MINIGAMES_MUSCLE,
    WEB_PLAY,
    WEB_PLAY_ARENA,
    WEB_PLAY_ARENA_SURFACE,
    WEB_PLAY_BATTLE,
    WEB_PLAY_BATTLE_SCREEN_PRIMS,
    WEB_PLAY_BATTLE_SESSION_EXPORTS,
    WEB_PLAY_CAMERA,
    WEB_PLAY_HOTKEYS,
    WEB_RUNTIME_OPTIONS,
    WEB_RUNTIME_FRAME,
    WEB_PLAY_DEV_MENU,
    WEB_PLAY_FISHING,
    WEB_PLAY_MENU,
    WEB_PLAY_MENU_INPUT,
    WEB_PLAY_MENU_OPEN_CLOSE,
    WEB_PLAY_MENU_SUB_SCREEN_DRAWS,
    WEB_PLAY_PLAYER_MESH,
    WEB_PLAY_SHOP,
    WEB_PLAY_TERRAIN_GROUND,
    WEB_RUNTIME_DISC_LOAD,
    WEB_RUNTIME_SCENE_SETUP,
)

# Simulation injection sites that must agree across hosts. See the module
# docstring's "third question" for the scope of the claim, and for what a
# `blocked_on` marker does and does not buy.
#
# A row's `sites` map a host label to `(repo-relative path, fn name or None)`.
# `None` means the whole file is the site, which is right when a host's
# injection is a call made from a place the pairing should not pin.
SIM_PAIRS: list[dict[str, object]] = [
    {
        "what": "session-switch world toggles, native vs play page - the "
        "native window read `LEGAIA_SMART_MONSTERS` and "
        "`LEGAIA_WORLD_MAP_MARKERS` inline in its launch code, and its "
        "`--retail` left the env-raised switches on while the page's retail "
        "mode masked only two of them. Both hosts now hand one "
        "`SessionSwitches` set (`engine-session::session_switches`) to its "
        "`apply`, so the retail mask has one definition; each site must "
        "build the full set",
        "sites": {
            "native": ("crates/engine-shell/src/window/run.rs", "apply_launch_toggles"),
            "web": ("crates/web-play/src/play_toggles.rs", "apply_page_toggles"),
        },
        "mode": "symbols_all",
        "symbols": [
            "SessionSwitches",
            "smart_monsters",
            "world_map_markers",
            "entry_pulse",
            "battle_frame_cadence",
            "arts_saved_list",
            "tweaks",
            "retail",
        ],
    },
    {
        "what": "field party readout on a claimed tick, native vs play page - "
        "the native window stepped `FUN_801D0D38`'s countdown on every tick "
        "whose owner is not the field (a host screen, the naming prompt, the "
        "prologue hand-off, the pause menu's spawn and press) while the page's "
        "tick loop skipped it there, so its countdown froze under the menu "
        "and the scene-change rearm landed a tick late. Both claim arms step "
        "it now; the frame-body trace compares the readout state per frame "
        "(`host_frame_parity`, `party_hud`)",
        "sites": {
            "native": ("crates/engine-shell/src/window/event_handler/redraw_tick.rs", "sim_tick"),
            "web": ("crates/web-play/src/runtime/frame_ticks.rs", "frame_ticks"),
        },
        "mode": "symbols_all",
        "symbols": ["tick_field_party_hud"],
    },
    {
        "what": "slot machine paylines, native vs play page - the page folded "
        "the paylines into its cabinet list ahead of the cash-out box, which "
        "shares their OT bucket, so the lines drew over the box there and "
        "under it natively. Both hosts take the cabinet and the paylines as "
        "two layers from `engine-screens::minigame_hud`",
        "sites": {
            "native": ("crates/engine-shell/src/window/minigames/slot_machine.rs", None),
            "web": ("crates/web-play/src/play_minigame_slots.rs", None),
        },
        "mode": "symbols_all",
        "symbols": ["slot_cabinet_prims", "slot_payline_prims"],
    },
    {
        "what": "battle screen FX, native vs play page - the weapon trail, "
        "the move-FX streak and ribbons and the Theeder beam were projected "
        "by each host through its own battle camera, and the two drifted "
        "(the native streak dropped the world scale and the packet's "
        "semi-transparency). Each host's screen-prim assembly now takes "
        "both layers from `engine-screens::battle_fx`",
        "sites": {
            "native": (
                "crates/engine-shell/src/window/event_handler/redraw_overlay.rs",
                "build_screen_prims",
            ),
            "web": ("crates/web-play/src/play_battle/screen_prims.rs", "tick_battle_intro"),
        },
        "mode": "symbols_all",
        "symbols": ["battle_fx_prims", "theeder_prims"],
    },
    {
        "what": "shop / prize-counter opening, native vs play page - each host "
        "drained the field VM's op-`0x49` shop and prize-counter requests "
        "itself and spelled the opening tick's no-edge rule (the Cross that "
        "closed the merchant's line must not also commit the picker's first "
        "row). The open and that first no-edge step are one kernel, "
        "`MenuRuntime::poll_field_overlay`, which each host's "
        "`poll_field_shop` step calls",
        "sites": {
            "native": ("crates/engine-shell/src/window/session_steps.rs", "poll_field_shop"),
            "web": ("crates/web-play/src/play_shop.rs", "poll_field_shop"),
        },
        "mode": "symbols_all",
        "symbols": ["poll_field_overlay"],
    },
    {
        "what": "dance tail, native vs play page - the auto-end (once the "
        "song-over arm restores the interrupted mode, the run is torn down) "
        "and the venue globals' stage / restore were two calls each host "
        "sequenced itself, in opposite orders. Both run the one session step "
        "`BootSession::step_dance_tail`",
        "sites": {
            "native": ("crates/engine-shell/src/window/minigames/dance.rs", "sync_dance_venue"),
            "web": ("crates/web-play/src/play_minigames.rs", "tick_minigame_ui"),
        },
        "mode": "symbols_all",
        "symbols": ["step_dance_tail"],
    },
    {
        "what": "battle presentation step, native vs play page - the battle "
        "audio routing (duck, strike / cast cues, cast-voice prestage, CD-XA "
        "clips, arts shouts), the strike popups, the per-hit drain, the "
        "effect-script spawn routing and the HUD row / status fold were one "
        "copy per host, and the copies drifted (the status CLUT pass ran a "
        "tick before the fold that arms its latch on the native window). "
        "Both run the one session step "
        "`BootSession::step_battle_presentation`",
        "sites": {
            "native": ("crates/engine-shell/src/window/battle.rs", "drain_and_log_battle_events"),
            "web": ("crates/web-play/src/play_battle.rs", "tick_battle_presentation"),
        },
        "mode": "symbols_all",
        "symbols": ["step_battle_presentation"],
    },
    {
        "what": "field-to-battle intro emitter, native vs play page - the "
        "Transition-phase gate, the arming off the formation's style inputs "
        "and the step to the transition entity's clock were one copy per "
        "host. Both run the one session step "
        "`battle_intro::step_battle_intro`; each keeps only its capture",
        "sites": {
            "native": ("crates/engine-shell/src/window/battle/intro.rs", "take_battle_intro_frame"),
            "web": ("crates/web-play/src/play_battle/screen_prims.rs", "tick_battle_intro_prims"),
        },
        "mode": "symbols_all",
        "symbols": ["step_battle_intro"],
    },
    {
        "what": "battle mode edge, native vs play page - the Field <-> Battle "
        "latch, the encounter banner it arms and the exit edge's reset of the "
        "popups and the status CLUT's palette copies and Stone latches "
        "(retail rebuilds that context per fight) were spelled per host and "
        "reset at different moments. Both latch through "
        "`battle_presentation::battle_mode_edge` and run its two edge steps",
        "sites": {
            "native": ("crates/engine-shell/src/window/battle.rs", "sync_battle_render"),
            "web": ("crates/web-play/src/play_battle.rs", "tick_battle_presentation"),
        },
        "mode": "symbols_all",
        "symbols": ["battle_mode_edge", "on_battle_entered", "on_battle_left"],
    },
    {
        "what": "the tick's audio frame, native vs play page - the footstep "
        "cadence ran on the page alone, beside a field CD-XA lane the "
        "session now routes for every host. Both `tick_sfx` steps are the "
        "same two session steps",
        "sites": {
            "native": ("crates/engine-shell/src/window/boot_cutscene.rs", "tick_sfx"),
            "web": ("crates/web-play/src/play_sfx.rs", "tick_sfx"),
        },
        "mode": "symbols_all",
        "symbols": ["step_footstep_cadence", "step_audio_frame"],
    },
    {
        "what": "Muscle Dome hub sounds, native vs play page - the first "
        "visit's CD-XA announcer lines and each tally lane's voice were "
        "drained and keyed once per host. Both run the session step "
        "`BootSession::sound_muscle_hub`",
        "sites": {
            "native": ("crates/engine-shell/src/window/minigames/muscle_dome.rs", "tick_muscle_hub"),
            "web": ("crates/web-play/src/play_minigame_arena.rs", "tick_muscle_hub"),
        },
        "mode": "symbols_all",
        "symbols": ["sound_muscle_hub"],
    },
    {
        "what": "Baka Fighter announcer line, native vs play page - the round "
        "chrome's CD-XA line was read off the chrome frame and played once "
        "per host. Both run the session step "
        "`BootSession::sound_baka_announcer`",
        "sites": {
            "native": ("crates/engine-shell/src/window/minigames/baka_fighter.rs", "tick_baka_chrome"),
            "web": ("crates/web-play/src/play_minigame_arena.rs", "tick_baka_ui"),
        },
        "mode": "symbols_all",
        "symbols": ["sound_baka_announcer"],
    },
    # --- lane H ---
    {
        "what": "Baka Fighter screen layer, native vs play page - both hosts "
        "drew only the cabinet's and round chrome's widget quads, and the duel "
        "HUD renderer `FUN_801d2afc` (VITAL bars and frames, combo, pips, "
        "attack icons, the STAGE strip) reached neither: a duel showed a lone "
        "font round digit. Both build the layer through "
        "`minigame_hud::baka_screen_prims`",
        "sites": {
            "native": ("crates/engine-shell/src/window/minigames/baka_fighter.rs", "baka_hud_prims"),
            "web": ("crates/web-play/src/play_minigame_arena.rs", "baka_hud_prims"),
        },
        "mode": "symbols_all",
        "symbols": ["baka_screen_prims"],
    },
    {
        "what": "Baka Fighter font digits, native vs play page - the round "
        "digit stands down once the duel HUD's own STAGE cells draw. Both "
        "place the digits through `minigame_hud::baka_digit_placements`",
        "sites": {
            "native": ("crates/engine-shell/src/window/hud/text_list.rs", "build_hud"),
            "web": ("crates/web-play/src/play_minigame_arena.rs", "baka_hud_text_draws"),
        },
        "mode": "symbols_all",
        "symbols": ["baka_digit_placements"],
    },
    # --- end lane H ---
    {
        "what": "scene-hidden lead, native vs play page - the opening "
        "prologue (`opdeene`) installs a free-roam lead at the cold spawn "
        "though its vignette shots have none; the native actor pass skipped "
        "slot 0 there and the page drew him as a stray mesh. Both ask "
        "`World::actor_hidden_by_scene`",
        "sites": {
            "native": (NATIVE_REDRAW, "handle_redraw"),
            "web": ("crates/web-play/src/play/player_mesh.rs", "play_player_drawn"),
        },
        "mode": "symbols_all",
        "symbols": ["actor_hidden_by_scene"],
    },
    {
        "what": "placed-object / overworld-landmark live model, native vs "
        "play page - a scripted model swap (op `0x0E`) re-skins only the "
        "record's first placement, and the rule was spelled three times: the "
        "native field pass through a cached bound mask, the native overworld "
        "pass inline over the records slice, the page with a `0xF0` sentinel "
        "filter the native passes lacked; the native turn-then-move "
        "composition was spelled twice inside the window. Both hosts' swaps "
        "come out of `field_env::placed_model_swaps`, and the native passes "
        "compose through `field_env::live_placed_model`",
        "sites": {
            "native": (NATIVE_REDRAW, "handle_redraw"),
            "web": ("crates/web-play/src/play/placements.rs", "field_placement_models"),
        },
        "mode": "symbols_all",
        "symbols": ["placed_model_swaps"],
    },
    {
        "what": "NPC posed-mesh cache key, native vs play page - a script's "
        "look rotation (`4C 45`) turns the head on top of the keyframe, so it "
        "is part of the pose each host memoises. The native window packed it "
        "into the key's high bits; the page folded the same inputs with "
        "overlapping XORs into its re-pose generation, under which two "
        "different looks could share a value and the head would not re-pose. "
        "Both key on `ActorLook::pose_key_bits`",
        "sites": {
            "native": (NATIVE_REDRAW, "handle_redraw"),
            "web": ("crates/web-play/src/field_actors.rs", "clip_states"),
        },
        "mode": "symbols_all",
        "symbols": ["pose_key_bits"],
    },
    {
        "what": "field NPC draw transform, native vs play page - the hide "
        "(off-map box, zero render scale), the heading composition and the "
        "tilt were spelled out natively and again in field-actors.js over two "
        "encodings of the yaw, and neither host folded retail's non-unit "
        "`actor+0x72` render scale in (`FUN_8001ADA4` `0x8001B240`). Both "
        "build the model from `World::field_npc_draw_pose`",
        "sites": {
            "native": (NATIVE_REDRAW, "handle_redraw"),
            "web": ("crates/web-play/src/field_actors.rs", "draw_poses"),
        },
        "mode": "symbols_all",
        "symbols": ["field_npc_draw_pose"],
    },
    {
        "what": "frame clear colour, native vs play page - both ran the "
        "shared `scene_clear` selector, but the page passed a hardcoded "
        "`false` for the screen term the native window built from its boot "
        "UI and `MenuRuntime::covers_field`, so a shop in a scene with a "
        "scripted clear colour sat on that colour on the page and on black "
        "natively. Both ask `field_frame::frame_clear_color`",
        "sites": {
            "native": (NATIVE_REDRAW, "handle_redraw"),
            "web": ("crates/web-play/src/play_battle_render/stage_exports.rs", "play_scene_clear_color"),
        },
        "mode": "symbols_all",
        "symbols": ["frame_clear_color"],
    },
    {
        "what": "prologue colour grade staging, native vs play page - each "
        "host mapped `World::scene_color_grade` / `scene_depth_cue` onto its "
        "renderer's colour-grade, palette-grade and depth-cue arms itself. "
        "Both take the one mapping `World::frame_grade`",
        "sites": {
            "native": (NATIVE_REDRAW, "handle_redraw"),
            "web": ("crates/web-play/src/play_cutscene.rs", "play_cutscene_state_json"),
        },
        "mode": "symbols_all",
        "symbols": ["frame_grade"],
    },
    {
        "what": "enhanced lighting's scene-light selection, native vs play "
        "page - each host spelled out the gate (field mode, not the world "
        "map, no screen owning the frame), the focus and the prop-light "
        "placement itself, and the page's gate lacked the screen term, so "
        "a shop's black backdrop carried the scene's candle halos there. "
        "Both ask `field_frame::field_scene_lights`",
        "sites": {
            "native": (NATIVE_REDRAW, "handle_redraw"),
            "web": ("crates/web-play/src/play_lighting.rs", "play_lighting_frame"),
        },
        "mode": "symbols_all",
        "symbols": ["field_scene_lights"],
    },
    {
        "what": "enhanced lighting's mood, native vs play page - each host "
        "looked up the running scene's name and its held scripted grade and "
        "graded the persisted time of day over them itself, and each tinted "
        "the fog bank by the mood behind its own toggle test. Both ask "
        "`field_frame::lighting_mood` and `field_frame::fog_volume_color`",
        "sites": {
            "native": (NATIVE_REDRAW_PASSES, "lighting_mood"),
            "web": ("crates/web-play/src/play_lighting.rs", "lighting_mood"),
        },
        "mode": "symbols_all",
        "symbols": ["field_frame::lighting_mood"],
    },
    {
        "what": "the fog bank's mood tint, native vs play page - see the "
        "mood row above. Both ask `field_frame::fog_volume_color`",
        "sites": {
            "native": (NATIVE_REDRAW_PASSES, "stage_fog_volume"),
            "web": ("crates/web-play/src/play_fog_volume.rs", "play_fog_volume_header"),
        },
        "mode": "symbols_all",
        "symbols": ["fog_volume_color"],
    },
    {
        "what": "move-FX spawn to sound, native vs play page - each host "
        "took the pending spawn, seated it, took the cue and classified it "
        "itself, and the two had drifted on the classification's arms (the "
        "native window logged a voice arm the page dropped; the cue is a "
        "byte, so that arm cannot arise). The spawn, the enqueue of its "
        "ring value and the scene-graph tick are one session step, "
        "`BootSession::step_effect_scene_graphs` (over "
        "`battle_fx::spawn_pending_move_fx`), which each host's "
        "`tick_world_effects` step calls",
        "sites": {
            "native": ("crates/engine-shell/src/window/session_steps.rs", "tick_world_effects"),
            "web": ("crates/web-play/src/runtime/frame.rs", "tick_world_effects"),
        },
        "mode": "symbols_all",
        "symbols": ["step_effect_scene_graphs"],
    },
    # The "dialog picker option labels" row (both snapshots must read
    # `OwnedDialogPanel::picker_labels`) is gone because the second snapshot
    # is: both hosts snapshot the pager through
    # `legaia_engine_screens::dialog_screens::dialog_snapshot`, which reads
    # `picker_labels` once. A re-duplicated snapshot needs the row back.
    {
        "what": "movie pacing, native in-window FMV vs play page - the page "
        "re-implemented the audio-cursor rule in play-fmv.js with a stall "
        "fallback the native window lacked (a stalled cursor held frame 0 "
        "there for the whole movie). Both pace through "
        "`cutscene::MovieClock::due_frame`",
        "sites": {
            "native": (
                "crates/engine-shell/src/window/boot_cutscene.rs",
                "render_windowed_cutscene",
            ),
            "web": ("crates/web-play/src/play_fmv.rs", "play_fmv_due_frame"),
        },
        "mode": "symbols_all",
        "symbols": ["due_frame"],
    },
    {
        "what": "overworld developer band on entry, native vs play page - the "
        "page armed the top-view debug chord (`_DAT_8007B98C`) on every "
        "world-map entry, a card Load included, while the native window armed "
        "it on its CLI picker entry only; arming it shadows retail's L1 map "
        "display. Both entries ask `World::arm_picker_world_map_debug`",
        "sites": {
            "native": (NATIVE_RUN, "enter_launch_scene"),
            "web": ("crates/web-play/src/runtime/scene_setup.rs", "enter_field_core"),
        },
        "mode": "symbols_all",
        "symbols": ["arm_picker_world_map_debug"],
    },
    {
        "what": "CD-XA shout staging, native boot vs play page - which channels "
        "of XA2 / XA4 / XA6 become shouts, how a shout is trimmed and which "
        "SCUS cue pools ride with it were written out once per host (the "
        "page's PlayXa restated the native reader). Both stage each shout "
        "file through `xa_banks::install_shout_file`",
        "sites": {
            "native": (NATIVE_XA_DISC, "read_arts_shout_bank"),
            "web": ("crates/web-play/src/play_xa.rs", "play_xa_install"),
        },
        "mode": "symbols_all",
        "symbols": ["install_shout_file"],
    },
    {
        "what": "CD-XA one-shot clip staging, native boot vs play page - the "
        "clip-slot file table, the per-channel decode and the interleave "
        "width the retail read span divides by. Both stage each clip file "
        "through `xa_banks::install_clip_file`",
        "sites": {
            "native": (NATIVE_XA_DISC, "read_battle_xa_clip_bank"),
            "web": ("crates/web-play/src/play_xa.rs", "play_xa_install"),
        },
        "mode": "symbols_all",
        "symbols": ["install_clip_file"],
    },
    {
        "what": "minigame status caption, native vs play page - the engine's "
        "status rows (rules readout + key hints for the slot machine, Baka "
        "Fighter, the Muscle Dome, the dance and fishing) are one row model, "
        "`minigame_status::caption`, shown outside the game picture: the "
        "window's title bar, the page's strip under the canvas. Each host "
        "used to draw them in the frame as glyph quads at fixed stage pens, "
        "under the game's own chrome, and the page then picked them back "
        "out of its draw list by pen and ink. Both sites must build the "
        "caption from a `StatusHost` - which is where a host says whether "
        "its slot machine draws itself and names all of its fishing keys "
        "(the page once printed only the menu key, so a player there never "
        "learned how to leave or reach the exchange)",
        "sites": {
            "native": ("crates/engine-shell/src/window/minigames.rs", "minigame_caption"),
            "web": ("crates/web-play/src/play_minigames.rs", "minigame_caption"),
        },
        "mode": "symbols_all",
        "symbols": ["StatusHost", "FishingKeys", "slot_machine_drawn", "prizes"],
    },
    {
        "what": "effect-pool billboards, native vs play page (battle and "
        "field FX) - each host selected and built the billboards itself "
        "(the battle centre flip, the view-scale half-extents, the UV "
        "mirror, the prim-ABE bit retail's GP0 0x2E carries, which the atlas "
        "page byte cannot: the page once pushed the bare page and every "
        "flame drew opaque), and the two disagreed on the overworld, where "
        "only the native window drew pool children although retail's walker "
        "runs from the battle draw tick alone. Every billboard builder takes "
        "the one list `effect_draws::effect_billboards`",
        "sites": {
            "native": (
                "crates/engine-shell/src/window/event_handler/redraw_passes.rs",
                "build_effect_billboards",
            ),
            "web_battle": ("crates/web-play/src/play_battle_fx.rs", "build_battle_fx"),
            "web_field": ("crates/web-play/src/play_battle_fx.rs", "build_field_fx"),
        },
        "mode": "symbols_all",
        "symbols": ["effect_billboards"],
    },
    {
        "what": "effect model draws, native vs play page (battle and field "
        "FX) - the `etmd` effect models, the summon / move-FX parts, the "
        "draw-kind-4 nodes, the battle ground shadows and the field stager "
        "parts were selected and placed once per host (about 250 lines "
        "apiece, glam on one side and hand-rolled matrices on the other): "
        "the overworld gate, the battle frame flip, the `+0x52` "
        "camera-relative prefix. Every FX model builder takes the one list "
        "`effect_draws::effect_model_draws`",
        "sites": {
            "native": (
                "crates/engine-shell/src/window/event_handler/redraw_passes.rs",
                "build_effect_draws",
            ),
            "web_battle": ("crates/web-play/src/play_battle_fx.rs", "build_battle_fx"),
            "web_field": ("crates/web-play/src/play_battle_fx.rs", "build_field_fx"),
        },
        "mode": "symbols_all",
        "symbols": ["effect_model_draws"],
    },
    {
        "what": "battle effect world scale, native vs play page - the 4x "
        "base matrix every battle FX layer composes (retail "
        "`DAT_8007BF10 = 16384 * I`, a battle-mode value the backdrop is "
        "only one more actor under): the native window took it only while "
        "a stage mesh was loaded and fell back to 1x on a stageless fight, "
        "the page always drew at 4x. Both take the one decision "
        "`battle_fx::battle_world_scale`",
        "sites": {
            "native": (
                "crates/engine-shell/src/window/event_handler/redraw_overlay.rs",
                "build_fx_frame",
            ),
            "web": ("crates/web-play/src/play_battle_fx.rs", "build_battle_fx"),
        },
        "mode": "symbols_all",
        "symbols": ["battle_world_scale"],
    },
    {
        "what": "battle stage world state, native vs play page - the stage "
        "class the backdrop ramp reads, the volumetric fog's stage cap (the "
        "bank may not outshine the stage it lies on) and each monster's "
        "battle texture slot are written on the battle edge by the one "
        "session step `battle_stage::stage_battle_world`, renderer or not, "
        "never by a host's mesh build",
        "sites": {
            "native": ("crates/engine-shell/src/window/battle.rs", "sync_battle_render"),
            "web": ("crates/web-play/src/play_battle.rs", "tick_battle_presentation"),
        },
        "mode": "symbols_all",
        "symbols": ["stage_battle_world"],
    },
    {
        "what": "volumetric ground fog, native vs play page - the bank is "
        "simulated once per tick inside `World::tick` "
        "(`render-kernels::fog_volume`); each host's draw site must take "
        "this frame's bank from `World::fog_volume_frame` rather than "
        "keep a bank (or a scene table) of its own",
        "sites": {
            "native": (
                "crates/engine-shell/src/window/event_handler/redraw_passes.rs",
                "stage_fog_volume",
            ),
            "web": ("crates/web-play/src/play_fog_volume.rs", "fog_volume_frame"),
        },
        "mode": "symbols_all",
        "symbols": ["fog_volume_frame"],
    },
    {
        "what": "shop blips, native vs play page - the shop's cursor / "
        "confirm / buzz / cancel cues and the quantity and recipient "
        "screens' purchase / sale / equip cues are the engine's decision "
        "(`MenuRuntime::take_ui_cue`, retail's list kernel `FUN_80032A44` "
        "and the shop sub-screens' own ring writes). Both hosts dropped "
        "them; each host's shop step must take and key the cue. The take "
        "now sits inside the one per-tick step both hosts run "
        "(`MenuRuntime::step_field_session`, which returns the cue), so the "
        "assertion is that each host's shop step routes through it",
        "sites": {
            "native": (NATIVE_REDRAW, "tick_menu_runtime_session"),
            "web": ("crates/web-play/src/play_shop.rs", "play_shop_input"),
        },
        "mode": "symbols_all",
        "symbols": ["step_field_session"],
    },
    {
        "what": "CLUT-walk shimmer install, native vs play page - the resolve "
        "(type-6 table on a field scene, slot 5 on an overworld), the strip "
        "park, the Drake complement and the ocean-head fallback were written "
        "out once per host and had drifted in which rows a field scene parked "
        "and which column the coverage test read. Both scene rebuilds must "
        "install through `ClutWalkAnim::install`",
        "sites": {
            "native": ("crates/engine-shell/src/window/field_render/terrain.rs", "resolve_ocean_anim"),
            "web": (WEB_RUNTIME_SCENE_SETUP, "rebuild_render_state"),
        },
        "mode": "symbols_all",
        "symbols": ["ClutWalkAnim::install"],
    },
    {
        "what": "battle target-cursor cue, native vs play page - the pulse "
        "toward white on the pointed-at monster and the dim on the rest are "
        "the port's readout of the stamped render flag, and their numbers "
        "were hand-copied into each host's draw pass. The read now sits "
        "inside the one precedence both draw passes take "
        "(`BattleActorDrawPlan::body_cue`), which this row pins",
        "sites": {
            "native": (NATIVE_REDRAW, "handle_redraw"),
            "web": ("crates/web-play/src/play_battle_fx.rs", "play_battle_actor_cursor"),
        },
        "mode": "symbols_all",
        "symbols": ["body_cue"],
    },
    {
        "what": "battle body whole-mesh blend, native vs play page - the "
        "capture / defeat fade and the near-camera ghost OR the colour word's "
        "ABE + ABR into every prim. The native window applied it to posed "
        "meshes only, so a body drawn off its rest mesh stayed opaque there. "
        "Both hosts' stream builders must go through "
        "`BattleActorDrawPlan::apply_body_blend`",
        "sites": {
            "native": (NATIVE_REDRAW_PASSES, "build_posed_actor_overrides"),
            "web": (
                "crates/web-play/src/play_battle_body_blend.rs",
                "play_battle_actor_blend_cba_tsb",
            ),
        },
        "mode": "symbols_all",
        "symbols": ["apply_body_blend"],
    },
    {
        "what": "battle body tint-cue gate, native vs play page - which "
        "render flags take the tint pass's cue (not the cursor's two; the "
        "fade only while its word raises ABE). The native gate also asked "
        "whether the body was posed. The gate is asked inside "
        "`BattleActorDrawPlan::body_cue`, which both draw passes take",
        "sites": {
            "native": (NATIVE_REDRAW, "handle_redraw"),
            "web": ("crates/web-play/src/play_battle_fx.rs", "play_battle_actor_cursor"),
        },
        "mode": "symbols_all",
        "symbols": ["body_cue"],
    },
    {
        "what": "Muscle Dome 3D arena, native vs play page - the page posed "
        "the arena, the fighter and the monster in its own script (clip picks "
        "off the turn edge, an orbit camera) while the native window drew no "
        "3D dome at all. Both hosts must drive the engine surface "
        "(`MuscleDomeSurface::frame`, held as `muscle_surface`)",
        "sites": {
            "native": (
                "crates/engine-shell/src/window/minigames/muscle_dome.rs",
                "refresh_muscle_dome_gpu",
            ),
            "web": (WEB_PLAY_ARENA, "play_mg_muscle_scene_frame"),
        },
        "mode": "symbols_all",
        "symbols": ["muscle_surface"],
    },
    {
        "what": "Muscle Dome camera, native vs play page - both hosts draw "
        "the dome under the surface camera's matrix (`DomeCamera::vp_raw`), "
        "never a host-side orbit framing",
        "sites": {
            "native": (
                "crates/engine-shell/src/window/minigames/muscle_dome.rs",
                "refresh_muscle_dome_gpu",
            ),
            "web": (WEB_PLAY_ARENA, "play_mg_muscle_scene_vp"),
        },
        "mode": "symbols_all",
        "symbols": ["vp_raw"],
    },
    # The "battle-intro arming" and "battle-intro style inputs" rows
    # (each host's `arm_battle_intro` had to build through
    # `BattleIntro::arm_for_battle` off `SceneHost::battle_intro_style_inputs`)
    # are gone because the per-host arming is: both hosts step the intro
    # through `legaia_engine_session::battle_intro::step_battle_intro`, whose
    # one `arm_battle_intro` reads both (the "field-to-battle intro emitter"
    # row). A re-duplicated arming needs the rows back.
    # The "Options screen model" row (both Options builders must read
    # `OptionsSession::screen_model`) is gone because the second builder is:
    # both hosts draw the Options screen through
    # `legaia_engine_screens::pause_screens::options_draws`, so the model is
    # read once. A re-duplicated Options projection needs the row back.
    {
        "what": "camera-occlusion fade gate, native vs play page - the gate "
        "ray-casts the player's body cross from the eye, and the cross is "
        "centred on `field_occlusion::player_body_centre` (the floor tier "
        "under the actor lifted half a character height). The page rebuilt "
        "that point inline with its own half-height literal, so a retune of "
        "the kernel's constant would have moved one host's gate alone. Both "
        "sites must read the kernel",
        "sites": {
            "native": (NATIVE_REDRAW, "handle_redraw"),
            "web": (WEB_PLAY_PLAYER_MESH, "field_player_occluded"),
        },
        "mode": "symbols_all",
        "symbols": ["player_body_centre"],
    },
    {
        "what": "camera-distance preset cycle, native vs play page - the "
        "stored option is what every options apply re-asserts onto the "
        "camera, so a cycle that steps the camera alone is undone by the next "
        "apply and never persisted. The page's export did exactly that (and "
        "had no caller). Both sites must step the option",
        "sites": {
            "native": (NATIVE_KEYBOARD_OPTION, "option_key"),
            "web": (WEB_PLAY_CAMERA, "play_camera_cycle_distance"),
        },
        "mode": "symbols_all",
        "symbols": ["apply_options_hotkey", "persist_and_apply_options"],
    },
    {
        "what": "host hotkeys, native vs play page - the option / cheat / "
        "camera keys (`F1`..`F9`, `I`, `Y`, `T`, `R`). The native window "
        "bound twelve in its keyboard arms and the page bound `F3` alone, in "
        "JavaScript, so the enhancement toggles the docs name had no key in "
        "the browser. Both resolve the key through the shared table "
        "(`HostHotkey::from_dom_code`), refuse the field-only keys under a "
        "front-end screen (`HostHotkey::refused` over a `FrontEndScreens` "
        "each host fills field by field - the page's fill is "
        "`hotkey_screens`, shared by the press and its refusal notice, so "
        "its site is the whole file - the page once tested `needs_field` "
        "against the title and logos alone, so `T` / `R` persisted options "
        "behind its pause menu), and make the options edit through "
        "`apply_options_hotkey` before persisting",
        "sites": {
            "native": (NATIVE_KEYBOARD_OPTION, "option_key"),
            "web": (WEB_PLAY_HOTKEYS, None),
        },
        "mode": "symbols_all",
        "symbols": [
            "from_dom_code",
            "refused",
            "boot_logos",
            "title",
            "save_select",
            "pause_menu",
            "game_over",
            "apply_options_hotkey",
            "persist_and_apply_options",
        ],
    },
    {
        "what": "host hotkey notices, native vs play page - the transient "
        "line after a hotkey press (`Ground fog: off`) and the refusal line "
        "for a field-only key under a front-end screen. The native window "
        "showed both; the page showed neither, so `F2`, `F7` and a refused "
        "`T` read as dead keys there. Both word them through the shared "
        "`toggle_notice` / `refused_notice` (the page's JS half is pinned by "
        "`play_hotkeys::tests::the_page_shows_hotkey_notices`)",
        "sites": {
            "native": (NATIVE_KEYBOARD_OPTION, None),
            "web": (WEB_PLAY_HOTKEYS, None),
        },
        "mode": "symbols_all",
        "symbols": ["toggle_notice", "refused_notice"],
    },
    {
        "what": "title Continue open, native vs play page - retail runs "
        "CONTINUE inside the title overlay's own card manager (`FUN_801DDCCC`: "
        "row 1 of sub-mode `0x10` stores `0x18` at `0x801DDC5C`, the card path "
        "runs to `LaunchFade`'s load-route master-mode write at `0x801DFAFC`), "
        "with no field under it. The page once opened the pause menu's Load "
        "row, whose field gates could bounce a Continue back to the title, and "
        "later a title-opened pause sub-screen beside the window's standalone "
        "screen. Both open the session's one `ContinueScreen`",
        "sites": {
            "native": (NATIVE_BOOT_CUTSCENE, "tick_boot_ui"),
            "web": (WEB_BOOT_CONTINUE, "open_boot_continue"),
        },
        "mode": "symbols_all",
        "symbols": ["ContinueScreen"],
    },
    {
        "what": "title Continue frame, native vs play page - the refusal "
        "pre-empt, the card read, the block-grid filter and the confirmed "
        "block's read are `ContinueScreen::step`'s; each host only sounds the "
        "step's blip and acts on `Loaded` / `BackedOut`. The window's "
        "standalone screen once ran the grid filter ahead of the refusal "
        "notice and fired no blips, where the page's sub-screen did both",
        "sites": {
            "native": (NATIVE_BOOT_CUTSCENE, "tick_boot_ui"),
            "web": (WEB_BOOT_CONTINUE, "boot_continue_step"),
        },
        "mode": "symbols_all",
        "symbols": ["ContinueStep", "blip", "Loaded", "BackedOut"],
    },
    {
        "what": "options persistence, native vs play page - what a save "
        "writes while a session pins engine-only knobs. The native window "
        "saved its `--no-volumetric-fog` override into the options file on the "
        "next options close; the page skipped the write outright under its "
        "retail box, dropping volumes, mute, time of day and the retail rows "
        "edited there. Both write `OptionsPins::persistable`",
        "sites": {
            "native": (NATIVE_WINDOW, "persist_and_apply_options"),
            "web": (WEB_RUNTIME_OPTIONS, "persist_and_apply_options"),
        },
        "mode": "symbols_all",
        "symbols": ["persistable", "options_pins"],
    },
    {
        "what": "options simulation knobs per tick, native vs play page - "
        "the native window re-asserted the persisted options onto the world "
        "on every tick while the page pushed them only at disc load and on a "
        "change, so a knob a transition reseeded stayed reseeded on one host. "
        "Both run the one session step ahead of the session tick",
        "sites": {
            "native": (NATIVE_REDRAW_TICK, "resolve_tick_pad"),
            "web": (WEB_RUNTIME_FRAME, "tick_frame"),
        },
        "mode": "symbols_all",
        "symbols": ["apply_option_knobs"],
    },
    {
        "what": "field screen-effect washes, native vs play page - the field "
        "VM's op `0x34` sub-0 arm spawns colour-tween actors whose per-frame "
        "`FUN_80024EE4(layer, blend, packed)` push IS the scene-entry "
        "fade-from-black and the door prologue's fade-to-black. Both hosts "
        "ticked the tween and NEITHER drew it: the pool had a producer and no "
        "consumer, so every scene entry simulated a fade in the clear. The "
        "three arguments are also three separate ways to get it backwards - "
        "`layer` is an ordering-table bucket and `blend` an ABR equation "
        "(two `i16`s a call site can swap), and `packed` is a GP0 colour word "
        "with red LOW, the opposite of every other kernel here. Both sites "
        "emit through `screen_effect_push_prims_split`, which also decides "
        "which pushes wash the text layer (OT bucket 0, in front of the "
        "glyphs at bucket 1); that call now sits inside the one composer "
        "both hosts run (`screen_layers::compose_screen_prims`), so the "
        "assertion is on the composer",
        "sites": {
            "native": (NATIVE_REDRAW, "handle_redraw"),
            "web": (WEB_PLAY_BATTLE_SCREEN_PRIMS, "rebuild_screen_geom"),
        },
        "mode": "symbols_all",
        "symbols": ["compose_screen_prims"],
    },
    {
        "what": "screen-primitive append order, native vs play page - the "
        "ordering-table walk breaks every bucket tie by append order (LIFO), "
        "and the two hosts appended in different orders: the PROT-0900 "
        "screen-effect widgets with the field's ordering-table effects "
        "natively and last on the page, the shop's opening fade and the "
        "pause wipe in opposite orders, and the page kept the field's "
        "effects under a shop that blacks the field out. Both fill "
        "`HostScreenPrims` and compose through `compose_screen_prims`; "
        "neither host may append a world-decided layer itself",
        "sites": {
            "native": (NATIVE_REDRAW, "handle_redraw"),
            "web": (WEB_PLAY_BATTLE_SCREEN_PRIMS, "tick_battle_intro"),
        },
        "mode": "symbols_same",
        "symbols": [
            "HostScreenPrims",
            "screen_fade_draws",
            "cross_beam_draw",
            "shop_fade_level",
            "cinematic_bar_prims",
            "arts_banner_prims",
            "draw_quads",
        ],
    },
    {
        "what": "dance HUD frame rows, native vs play page - which rows the "
        "frame carries, at which 320x240 seats, in which pen, is the engine's "
        "decision (`DanceGame::hud_frame_rows`, the presentation half of "
        "`FUN_801d231c`): digit suppression, the `Lv.` label and the rival "
        "track's chart sampling are all retail's, not a host's. The whole "
        "resolution was written out longhand inside the native window's dance "
        "block, so the play page - same run, same `DanceGame` - drew a plain "
        "status line and no frame at all. Both sites must read the rows",
        "sites": {
            "native": ("crates/engine-shell/src/window/hud/text_list.rs", "build_hud"),
            "web": (WEB_PLAY_ARENA, "dance_hud_text_draws"),
        },
        "mode": "symbols_all",
        "symbols": ["hud_frame_rows"],
    },
    {
        "what": "fishing prize one-time latch, native vs play page - "
        "`is_available` folds three independent refusals together (price, "
        "owned cap, one-time latch), so a host that reads it as the latch "
        "labels every unaffordable one-time prize on a fresh save as already "
        "taken. Each host answered the latch its own way - one re-tested "
        "availability with the other two gates forced open, one shifted the "
        "purchased mask by hand, and the play page never asked at all and "
        "exposed only the folded bool. Both sites must read "
        "`PrizeExchange::is_latched`. The native window's exchange screen is "
        "the shared `engine-screens` composition (`fishing_exchange_draws`, "
        "which the page's screen draws through too), so that is its site",
        "sites": {
            "native": ("crates/engine-screens/src/fishing.rs", "fishing_exchange_draws"),
            "web": (WEB_PLAY_FISHING, "play_fishing_prizes_json"),
        },
        "mode": "symbols_all",
        "symbols": ["is_latched"],
    },
    {
        "what": "camera-occlusion fade focus + arming, native vs play page - "
        "the fade has two halves that must name the SAME world point: the "
        "visibility gate ray-casts to it and the host stages it as the "
        "shader's focus. Each host spelled the point out locally, and the "
        "page's focus read the actor's own `world_y` while its gate read the "
        "floor tier under it, so on any tile where those differ the dissolve "
        "hole sat off the character. Its arming operands had drifted the same "
        "way - the native window excludes the boot UI, the world map, a "
        "scripted shot and the debug orbit, the page excluded only battle and "
        "the minigames, and the page carried the strength ramp's 0.25 ease "
        "and 0.01 snap in play-app.js. The arming terms combine in "
        "`field_occlusion::host_fade_armed` and the ramp is "
        "`field_occlusion::FadeRamp`; both sites must reach every kernel",
        "sites": {
            "native": (NATIVE_REDRAW, "handle_redraw"),
            "web": (WEB_PLAY_CAMERA, "play_occlusion_fade"),
        },
        "mode": "symbols_all",
        "symbols": [
            "player_body_centre",
            "player_feet",
            "host_fade_armed",
            "FadeHostTerms",
        ],
    },
    {
        "what": "`apply == 0` Camera Configure snap beats, native vs play "
        "page - retail's mover snaps the live camera globals to a snap beat "
        "immediately, and the field VM runs until yield, so a snap+glide pair "
        "committed in ONE tick must glide FROM the snapped pose. The beats "
        "are banked by `Camera::route_camera_events`, which is the only thing "
        "that drains `FieldEvent::CameraConfigure` off the world queue - a "
        "host watching its own later event drain for them sees none. Both "
        "hosts step the glide through the shared `frame_step::CutsceneGlide`, "
        "whose `advance` replays the bank before it glides and counts steps "
        "on the world's display-frame clock (zero on an idle redraw)",
        "sites": {
            "native": (NATIVE_REDRAW, "handle_redraw"),
            "web": (WEB_PLAY_CAMERA, "resolve_camera_frame"),
        },
        "mode": "symbols_all",
        "symbols": ["cutscene_glide"],
    },
    {
        "what": "save-select phase layout, native vs play page - which pills "
        "draw, whether the pill cursor shows, and whether the block grid and "
        "its info panel stay up are one decision per `SelectPhase`, and the "
        "two hosts answered it differently for exactly one phase pair: retail "
        "raises the overwrite / delete prompt FROM the preview (see "
        "docs/subsystems/save-screen.md), so a confirm is a `SlotPreview` "
        "wearing a messagebox. The native window drew neither the grid nor "
        "the panel under it. The whole overlay sequence is one model now "
        "(`SaveScreenFlow::overlay_model`, which reads `phase_layout`) and one "
        "composition (`save_select_overlay_draws`), whose text half draws "
        "without the chrome atlas - the page had returned before every phase "
        "overlay without it. Both sites route through the shared "
        "`engine-screens::pause_screens::save_select_overlay_draws`, which "
        "reads `overlay_model` once",
        "sites": {
            "native": (NATIVE_TITLE_SAVE, "save_select_overlay"),
            "web": (WEB_PLAY_MENU_SUB_SCREEN_DRAWS, "build_save_select"),
        },
        "mode": "symbols_all",
        "symbols": ["save_select_overlay_draws"],
    },
    {
        "what": "shop / inn / prize / coin overlay stage transform, native "
        "vs play page - every builder in that group places in the retail "
        "320x240 stage, so a host that composites its output has to scale it "
        "through the shared `pause_menu::stage_transform` before it reaches a "
        "surface-pixel draw list. The native window did not: it extended the "
        "group straight into `build_hud`'s list, so the whole shop UI drew at "
        "a third its size in a 960x720 window while the identical builders "
        "filled the browser tab. The pinned `SHOP_OVERLAY_PEN` / `SHOP_PEN` "
        "pair is blind to it by construction - the two pens ARE equal and the "
        "split is in the transform applied after them, which is the general "
        "lesson: a paired constant pins a value, not the space it lands in. "
        "Both composition sites must reach the transform and the scale pass",
        "sites": {
            "native": ("crates/engine-shell/src/window/hud/text_list.rs", "build_hud"),
            "web": (WEB_PLAY_SHOP, "play_overlay_draws_json"),
        },
        "mode": "symbols_all",
        "symbols": ["stage_transform", "scale_stage_text_draws"],
    },
    {
        "what": "shop / prize / inn / banner overlay composition, native vs "
        "play page - the screens' projection of engine-core state into "
        "engine-ui draw lists lived once per host, line for line, and the "
        "copies drifted (ink, toast, pictogram fallback, frame sizing, banner "
        "scale). It lives once now, in `legaia_engine_screens`, and each host "
        "frame must build the group through its entry. The native window "
        "builds it in the redraw (once, for the text pass and the sprite "
        "pass) and hands it to `build_hud`",
        "sites": {
            "native": (NATIVE_REDRAW, "handle_redraw"),
            "web": (WEB_PLAY_SHOP, "play_overlay_draws_json"),
        },
        "mode": "symbols_all",
        "symbols": ["shop_overlay_frame"],
    },
    {
        "what": "field / overworld / cutscene camera, native vs play page - "
        "which camera owns a frame and what its retail GTE inputs are is one "
        "engine question (`camera_view::resolve_field_camera`), and the "
        "matrix each host uploads is that resolution projected "
        "(`camera_view::frame_vp`). The play page shipped the opposite: it "
        "held no `engine_core::camera::Camera` at all, framed the field with "
        "its own spherical orbit projection and re-mapped the op-0x45 "
        "cutscene params onto it - so the two hosts' cameras, their cutscene "
        "shots and the azimuth each fed the locomotion compass all diverged "
        "with no shared symbol to notice it. Both sites must reach the "
        "resolver, not a camera of their own - through the host seam "
        "`engine-world-views::screen_passes::HostView::frame`, which is the "
        "resolver on the host's scene-box centre",
        "sites": {
            "native": (NATIVE_REDRAW_PASSES, "compute_scene_camera"),
            "web": (WEB_PLAY_CAMERA, "resolve_camera_frame"),
        },
        "mode": "symbols_all",
        "symbols": ["frame"],
    },
    {
        "what": "direct scene entry's camera reset, native vs play page - a "
        "scene picker / dev warp / load-from-save enters a scene by CALL, "
        "not through the in-world `SceneEntered` tick event both hosts "
        "already answer with `reset_globals_for_scene_entry`. The scene host "
        "clears the world's camera state on either path, but the follow view "
        "composes from the engine `Camera`'s globals, so a scene picked "
        "mid-cutscene kept the interrupted shot's pitch / yaw / eye trio and "
        "its focus latch and was framed by the old scene's camera. Both "
        "direct-entry sites must run `Camera::reset_for_scene_entry`",
        "sites": {
            "native": (NATIVE_BOOT_SCENE_ENTRY, "enter_field_live"),
            "web": (WEB_RUNTIME_SCENE_SETUP, "enter_field_core"),
        },
        "mode": "symbols_all",
        "symbols": ["reset_for_scene_entry"],
    },
    {
        "what": "resuming a loaded save, native vs play page - a Load or an "
        "import lands through ONE landing order (the saved scene, else the "
        "running scene, else the opening town; never a New Game). The page "
        "used to choreograph it in play.html callbacks gated on its own "
        "scene-picker list, and a title Continue naming a scene that list "
        "did not carry fell through to a New Game over the loaded save. Both "
        "hosts' resume entries must reach the shared kernel, which also "
        "owns the hydrate order (story flags before the entry, the whole "
        "save after it; the page used to apply the whole save at park and "
        "raised the scene's pre-bound actor slots)",
        "sites": {
            "native": (NATIVE_BOOT_SCENE_ENTRY, "resume_save"),
            "web": ("crates/web-play/src/resume.rs", "resume_parked_save"),
        },
        "mode": "symbols_all",
        "symbols": ["resume_card_load"],
    },
    {
        "what": "starting a New Game, native vs play page - the seeded slate "
        "(`begin_new_game_seeded`) and the opening order (opdeene, else "
        "town01) are one session function, `start_new_game_on`, each host "
        "handing in only its scene entry (`NewGameHost`). The page used to "
        "seed and enter from play.html callbacks, its post-wipe title "
        "skipped the seed entirely, and its entry later skipped the landing "
        "check the native one makes",
        "sites": {
            "native": (NATIVE_BOOT_SCENE_ENTRY, "start_new_game"),
            "web": ("crates/web-play/src/resume.rs", "start_new_game"),
        },
        "mode": "symbols_all",
        "symbols": ["start_new_game_on"],
    },
    {
        "what": "coplanar draw lifts, native vs play page - every host that "
        "assembles a field scene from EnvDraws must run the cross-draw "
        "coplanar kernel (`draw_plane_summaries` + `coplanar_draw_offsets`) "
        "and apply its lifts, or that host alone z-fights on every "
        "placement/terrain pair that meets on one world plane. The play page "
        "shipped exactly this gap: `build_field_render` resolved the same "
        "draws as the native shell and the field-scene viewer but never "
        "computed the lifts, so koin6's inn floor shimmered only in the "
        "browser play page (angle-dependently - invisible in a diff and in "
        "any single screenshot taken from the lucky angle)",
        "sites": {
            "native": ("crates/engine-shell/src/window/field_render/placements.rs", "compute_coplanar_env_offsets"),
            "web": (WEB_PLAY, "build_field_render"),
        },
        "mode": "symbols_all",
        "symbols": ["draw_plane_summaries", "coplanar_draw_offsets"],
    },
    {
        "what": "coplanar draw lifts, play page vs field-scene viewer - the "
        "two web surfaces assemble the same scene through the same resolver "
        "calls, so both must hand the combined draw list to the same "
        "coplanar kernel (see the native pairing above for the failure this "
        "catches). The field-scene viewer's whole assembly now lives in the "
        "shared kernel `engine-scene::scene_assembly::assemble_field_scene_with` "
        "(which the native export-glb path also reads), so the viewer side "
        "of this pair is checked at the kernel",
        "sites": {
            "web_play": (WEB_PLAY, "build_field_render"),
            "web_viewer": (
                "crates/engine-scene/src/scene_assembly.rs",
                "assemble_field_scene_with",
            ),
        },
        "mode": "symbols_all",
        "symbols": ["draw_plane_summaries", "coplanar_draw_offsets"],
    },
    {
        "what": "title card, native vs play page - the session's one front "
        "end (`engine-session::front_end::FrontEnd`) steps the title: the "
        "tick, the row counter onto the seat, retail's `AttractIdle` "
        "(`0x10`) hand-off to `fmv_id 0` and its skip edge, the return to "
        "the menu. Each host supplies only its movie lane and acts on the "
        "step; a host that ticked the `TitleSession` itself could arm a "
        "countdown it never returns from, or order the hand-off against the "
        "tick differently from the other host",
        "sites": {
            "native": (NATIVE_BOOT_CUTSCENE, "tick_boot_ui"),
            "web": (WEB_BOOT_TITLE, "boot_title_step"),
        },
        "mode": "symbols_all",
        "symbols": ["FrontEndStep", "AttractStart", "AttractEnded", "Picked"],
    },
    {
        "what": "publisher logos to title, native vs play page - `init.pak`'s "
        "phase-3 arm (`FUN_801CEFD4`) branches on the entry word to "
        "`CARD INIT` through `ModeSeat::boot_handoff`, and the title's "
        "Continue scan runs at that hand-off. The page used to keep a title "
        "it opened before the logos and never took the seat's hand-off, so "
        "its mode word stayed short of `CARD INIT`. Both step the logos "
        "through `FrontEnd::step` and raise the title on its `TitleOpened`",
        "sites": {
            "native": (NATIVE_BOOT_CUTSCENE, "tick_boot_ui"),
            "web": (WEB_BOOT_TITLE, "boot_logos_step"),
        },
        "mode": "symbols_all",
        "symbols": ["FrontEndStep", "TitleOpened"],
    },
    {
        "what": "ground-heightfield sink, native vs play page - the walk-ground "
        "grid shares its plane with the env pack's authored floor art (koin6: "
        "both at y=0 with different tessellations), so every render site must "
        "sink it by the shared GROUND_SINK or that host's floors z-fight as "
        "wedge streaks from steep cameras while every other host is clean. "
        "Both build it through `field_ground::render_positions`, which is "
        "where the sink lives",
        "sites": {
            "native": (NATIVE_GEOMETRY, "heightfield_to_vram_mesh"),
            "web": (WEB_PLAY_TERRAIN_GROUND, "field_ground_positions"),
        },
        "mode": "symbols_all",
        "symbols": ["render_positions"],
    },
    {
        "what": "ground-heightfield winding, native vs play page - the grid's "
        "builder winds opposite to the scene TMDs, and the cutscene camera's "
        "NCLIP pass (mode 2) discards exactly that facing, so a host that "
        "uploads the builder's order loses the whole floor under a scripted "
        "shot (the page did, in the opdeene prologue). Both reverse it through "
        "`field_ground::render_indices`",
        "sites": {
            "native": (NATIVE_GEOMETRY, "heightfield_to_vram_mesh"),
            "web": (WEB_PLAY_TERRAIN_GROUND, "field_ground_indices"),
        },
        "mode": "symbols_all",
        "symbols": ["render_indices"],
    },
    {
        "what": "visible-tile crop policy, native vs play page - retail draws "
        "only the cells the camera's tile window reaches (FUN_801F7088), and "
        "whether a frame crops at all is a policy (knob, retail framing, field "
        "mode, no cutscene, a focus the region box holds). Both hosts ask the "
        "one `field_view_window::field_view_cells` entry with "
        "`framing_is_retail`, so a host that cropped under its own rule would "
        "black out cells the other draws",
        "sites": {
            "native": (NATIVE_REDRAW, "handle_redraw"),
            "web": (WEB_PLAY_TERRAIN_GROUND, "field_view_cells_now"),
        },
        "mode": "symbols_all",
        "symbols": ["field_view_cells", "framing_is_retail"],
    },
    {
        "what": "visible-tile crop of the terrain list, native vs play page - "
        "the decoration pass's per-cell gate (widened by record +0x1E) is "
        "`field_view_window::terrain_draw_visible` on both hosts, keyed by "
        "`CellKey::of_draw`",
        "sites": {
            "native": (NATIVE_REDRAW, "handle_redraw"),
            "web": (WEB_PLAY_TERRAIN_GROUND, "field_terrain_live"),
        },
        "mode": "symbols_all",
        "symbols": ["terrain_draw_visible"],
    },
    {
        "what": "visible-tile crop of the ground, native vs play page - the "
        "ground emitters' cell loop (FUN_801F6D48) crops the heightfield's "
        "index list through `field_ground::crop_indices` on both hosts, "
        "re-issued on the rectangle's `stamp`",
        "sites": {
            "native": ("crates/engine-shell/src/window/field_render/floor_wave.rs", "sync_ground_crop"),
            "web": (WEB_PLAY_TERRAIN_GROUND, "field_ground_indices_cropped"),
        },
        "mode": "symbols_all",
        "symbols": ["crop_indices"],
    },
    {
        "what": "ground-heightfield sink, play page vs field-scene viewer - "
        "same property as the native pairing above, across the two web "
        "surfaces' ground exporters",
        "sites": {
            "web_play": (WEB_PLAY_TERRAIN_GROUND, "field_ground_positions"),
            "web_viewer": (WEB_FIELD_SCENE, "field_scene_ground_positions"),
        },
        "mode": "symbols_all",
        "symbols": ["render_positions"],
    },
    {
        "what": "dance-hall placement composition, native vs browser - every "
        "venue draw is placed through `EnvDraw::place_point` (Rx * Ry * Rz, "
        "FUN_80026988, then the world position and the coplanar lift) on "
        "both hosts. The browser bake once read the yaw alone, which stood the "
        "hall's tilted video walls inside the back wall: the page drew brick "
        "where the window drew the dancer screens, with every tier green",
        "sites": {
            "native": ("crates/engine-shell/src/window/minigames/dance.rs", "build_dance_venue_gpu"),
            "web": (WEB_MINIGAMES_DANCE, "append_draw"),
        },
        "mode": "symbols_all",
        "symbols": ["place_point"],
    },
    {
        "what": "Muscle Dome 3D surface cadence, native vs play page - the "
        "surface steps its choreography and battle camera script once per "
        "call, so a world host must reach it through `frame_at`, which gates "
        "the step on the world's sim tick. The page once called the per-call "
        "`frame` from a draw that also runs on idle frames, and its round "
        "intro framed a different fighter than the window's",
        "sites": {
            "native": ("crates/engine-shell/src/window/minigames/muscle_dome.rs", "refresh_muscle_dome_gpu"),
            "web": (WEB_PLAY_ARENA_SURFACE, "play_mg_muscle_scene_frame"),
        },
        "mode": "symbols_all",
        "symbols": ["frame_at", "sim_ticks"],
    },
    {
        "what": "Muscle Dome damage - the arena's per-exchange damage must come "
        "off the same battle-formula kernel on both hosts, or the same command "
        "deals different numbers in the window and in the browser. Both hosts "
        "install a `DomeDamageModel` and resolve through it, so the assertion "
        "is on the shared entry point, not on the formula leaf underneath it. "
        "It is `pattern_same` over the whole `resolve_turn*` family, not a "
        "name check on one of them: both hosts once named `resolve_turn_retail` "
        "and passed, while only the native side handled the kernel-absent "
        "return - so the browser contest hung in `Resolve` forever with the "
        "gate green. Naming the same resolvers is the property that matters, "
        "and it does not need the right set stated up front",
        "sites": {
            "native": (NATIVE_MINIGAME_SESSIONS, "tick_muscle_dome"),
            "web": (WEB_MINIGAMES_MUSCLE, "muscle_resolve"),
        },
        "mode": "pattern_same",
        "pattern": r"(resolve_turn\w*)",
    },
    {
        "what": "scripted mesh re-bind (motion-VM op `0x0E`) - the world holds "
        "the actor's new model id per placement slot and each host resolves it "
        "to bytes through the scene's model bank. A host that resolves it "
        "anywhere else is resolving against `SceneResources::tmds`, which is a "
        "magic scan blind to a TMD inside an LZS bundle descriptor - which is "
        "where the four scenes that script a re-bind keep all of their models",
        "sites": {
            "native": (NATIVE_ASSETS_ACTORS, "upload_field_npc"),
            # The page's actor layer (`web-play::field_actors`, shared
            # with the map viewer) is the body that reads the world field;
            # `play_npc_live_model` and the mesh build both go through it.
            "web": ("crates/web-play/src/field_actors.rs", "live_model"),
        },
        "mode": "symbols_all",
        "symbols": ["field_npc_live_model"],
    },
    {
        "what": "save-select model - which rack a host declares decides how "
        "many pills the screen shows and what each one addresses, so the two "
        "hosts must declare the same kind. No host sets the card-slots flag "
        "any more: `SaveSelectSession::for_rack` derives it from the "
        "`SaveRack` variant, and the driver around the second stage is the "
        "shared `save_screen::SaveScreenFlow`. No host builds the rack "
        "either: the pause menu's Load / Save rows take the session's "
        "`card_port_rack` over the two ports (the title's Continue screen "
        "builds it inside `ContinueScreen::open`)",
        "sites": {
            # The pause menu's sub-session build, over the window's two
            # cards (port 1 its own `memcard1.mcr`, port 2 `--card`'s image).
            "native": (NATIVE_BOOT_CUTSCENE, "tick_boot_ui"),
            "web": (WEB_PLAY_MENU_INPUT, "play_menu_input"),
        },
        "mode": "symbols_all",
        "symbols": ["card_port_rack"],
    },
    {
        "what": "enemy Steal table - PROT 0941's `0x51` arm resolves a MONSTER "
        "victim against the static `DAT_80077828` table, so a host that boots "
        "a world without installing it draws nothing where the other host "
        "draws an item, and the two RNG cursors diverge from that battle on. "
        "Neither install is inside `World::arm_live_loop` (they are disc-table "
        "installs, which each host does for itself from its own disc source), "
        "so the live-loop pairing below cannot see them; they sit in different "
        "crates' boot paths and neither is reached by the other's tests",
        "sites": {
            "native": (NATIVE_BOOT_SCENE_ENTRY, "enter_field_live"),
            "web": (WEB_RUNTIME_DISC_LOAD, "load_disc"),
        },
        "mode": "symbols_all",
        "symbols": ["set_steal_table"],
    },
    {
        "what": "live-loop arming - the browser twin of `enter_field_live`. "
        "Every `World::set_*` one host installs before running the live "
        "gameplay loop and the other does not is a table the two simulations "
        "disagree about (drops, prices, spells, battle BGM). Both hosts now "
        "delegate to the shared `World::arm_live_loop`, so the assertion is "
        "that each still routes through it - scanning for `set_*` in the host "
        "bodies would pass trivially once the calls moved into the kernel",
        "sites": {
            "native": (NATIVE_BOOT_SCENE_ENTRY, "enter_field_live"),
            "web": (WEB_PLAY_BATTLE, "arm_live_battles"),
        },
        "mode": "symbols_all",
        "symbols": ["arm_live_loop"],
    },
    {
        "what": "pause-menu press - one engine rule answers every menu-button "
        "press: `BootSession::press_field_menu` (the scripted op-`0x49` press, "
        "the Start edge through `World::field_menu_open_allowed`, the deny "
        "buzz) over `BootSession::open_field_menu` (the `FieldMenuGate` "
        "sample, the switch into `SceneMode::Menu`, the open through the mode "
        "seat). Each of those used to be paired across two open sites, "
        "because the page kept a private root picker and re-spelled the open "
        "beside the session's: three hosts once wrote their own `mode == "
        "Field` copy of the precondition, which is how the OVERWORLD lost the "
        "pause menu, and the page once opened without sampling the gate, "
        "which let a player Save where the scene's header forbids it. The "
        "page's menu is the session's `field_menu` now, so the property left "
        "to assert is that every host routes the press through the kernel "
        "rather than around it",
        "sites": {
            "play_hosts": (SESSION_TICK_OWNER, "claim_tick"),
            "native_boot": (NATIVE_BOOT_FRAME_TICK, "tick"),
            "web": (WEB_PLAY_MENU_OPEN_CLOSE, "play_menu_open"),
        },
        "mode": "symbols_all",
        "symbols": ["press_field_menu"],
    },
    {
        "what": "tick order, native vs play page - which screen owns each "
        "sim tick. The window's `sim_tick` and the page's `frame_ticks` each "
        "spelt the order in their own words, and the two orders differed "
        "(the page asked the pause menu before the naming prompt). Both hand "
        "every tick through `BootSession::claim_tick` and act only on the "
        "`TickOwner` it returns",
        "sites": {
            "native": (NATIVE_REDRAW_TICK, "sim_tick"),
            "web": (WEB_RUNTIME_FRAME_TICKS, "frame_ticks"),
        },
        "mode": "symbols_all",
        "symbols": ["claim_tick", "TickClaims"],
    },
    {
        "what": "pause-menu open with no press - the title's Continue / "
        "Options rows land on a pause-menu sub-screen, so they open the menu "
        "through the session's own builder (`BootSession::open_field_menu`) "
        "rather than through a Start press; a host that built its own root "
        "picker here would be back to two menus",
        "sites": {
            "native": (NATIVE_BOOT_CUTSCENE, "open_menu_row_from_title"),
            "web": (WEB_PLAY_MENU_OPEN_CLOSE, "open_play_menu_unpressed"),
        },
        "mode": "symbols_all",
        "symbols": ["open_field_menu"],
    },
    {
        "what": "party wipe - both hosts must route it to the title screen "
        "and nowhere else. Retail's wipe arm has exactly one exit store "
        "(`game_mode = 0x16` + `_DAT_8007BB00 = 1`), so a host that offers "
        "the player a row here has invented one. The panel that used to sit "
        "in this slot was exactly that, and the browser drew it from a pinned "
        "`(1, false)` while the native window drew it from a live cursor - "
        "two pictures of a menu that never existed. Pairing the routing "
        "sites, not the draw sites, is what keeps a second destination from "
        "reappearing on one host only. The hold's resolve is one session "
        "step (`BootSession::step_party_wipe_hold`: the deferred field "
        "restore and the battle track stopped), so neither host spells the "
        "outcome itself",
        "sites": {
            "native": (NATIVE_BOOT_CUTSCENE, "tick_boot_ui"),
            "web": (WEB_PLAY_BATTLE_SESSION_EXPORTS, "game_over_input"),
        },
        "mode": "symbols_all",
        "symbols": ["step_party_wipe_hold"],
    },
    {
        "what": "dev-menu tick - both hosts drive the shared `DevMenuSession` "
        "off their world's pad pump, and three pieces are each a silent "
        "cross-wire if one host drops them: the raw-to-packed pad conversion "
        "(`retail_packed` - without it Up arrives as PACK_TRIANGLE and Cross "
        "as PACK_DOWN), the EQUIP row's bag commit (inside `tick_host` - "
        "without it the row steps an id and never equips), and the Square "
        "records-page swap (`RECORDS_TOGGLE`)",
        "sites": {
            "native": (NATIVE_DEV_MENU, "tick_dev_menu"),
            "web": (WEB_PLAY_DEV_MENU, "tick_dev_menu"),
        },
        "mode": "symbols_all",
        "symbols": ["retail_packed", "tick_host", "RECORDS_TOGGLE"],
    },
    {
        "what": "dev-records model - both hosts draw the records page off "
        "the one model in `legaia_engine_screens::dev_menu` "
        "(`dev_records_model`: the record-relative counter reads, "
        "`record_counters`, through the retail clamp / decompose model, "
        "`records_screen`) and its one view projection (`records_view`). "
        "Each host used to carry its own copy of both; a host that reads the "
        "record fields itself, or skips the clamp, shows different numbers "
        "for the same save. Both now take the whole screen from one builder, "
        "`records_draws_for_world` (the roster walk, the model, the view)",
        "sites": {
            "native": (NATIVE_DEV_MENU, "records_draws_at"),
            "web": (WEB_PLAY_DEV_MENU, "records_draws_at"),
        },
        "mode": "symbols_all",
        "symbols": ["records_draws_for_world"],
    },
    {
        "what": "play clock - the H:MM:SS box the pause menu draws reads "
        "`World::play_time_seconds`, and that counter only moves if a host "
        "drives `advance_play_time`. The browser substituted `world.frame / 60` "
        "at the draw site instead, which reset on every page load, ignored a "
        "loaded save's hours, and - because the *save* writes the world's "
        "counter, not the drawn proxy - recorded the LOADED play time in every "
        "save taken from the browser. Asserted across the two "
        "`field_menu_draws_for` call sites, so the reader and the writer are "
        "pinned together",
        "sites": {
            "native": (NATIVE_BOOT_CUTSCENE, None),
            "web": (WEB_PLAY_MENU, None),
        },
        "mode": "symbols_same",
        "symbols": ["advance_play_time"],
    },
    {
        "what": "light-source rows, native vs play page - TMD rows with group "
        "flags 0x10..=0x17 carry no colour word; retail's NCCS / NCCT handlers "
        "colour them from the live field light, whose scene-load back colour "
        "(0x202020) is the dim ambient every scene starts under. Both hosts "
        "shade through `field_lit_mesh` under `World::presentation.field_light`",
        "sites": {
            "native": (
                "crates/engine-shell/src/window/field_render.rs",
                "rebuild_field_lit_meshes",
            ),
            "web": ("crates/web-play/src/play/field_mesh.rs", "field_mesh_posed_lit"),
        },
        "mode": "symbols_all",
        "symbols": ["field_lit_mesh", "field_light"],
    },
    {
        "what": "light-source rows in a graded scene, native vs play page - the "
        "native mesh upload used to replace the light kernel with a flat 0x20 "
        "on the three prologue legs (keyed on `scene_color_grade`) while the "
        "page shaded them like any scene. Neither mesh build may key its lit "
        "rows on the grade: the grade is the palette's and the baked words'",
        "sites": {
            "native": ("crates/engine-shell/src/window/assets/scene_geometry.rs", None),
            "web": ("crates/web-play/src/play/field_mesh.rs", None),
        },
        "mode": "symbols_same",
        "symbols": ["scene_color_grade", "frame_grade"],
    },
    {
        "what": "placed-object live model, native vs play page - retail's "
        "case-5 draw reads the actor, so a placed object a script has turned "
        "(op 0x38, `4C 48`) or moved (`A3`, `4C 42`) draws turned about its own "
        "origin and then displaced. The page used to take the two tables apart "
        "and redo the composition in JavaScript, conjugated into its Y-up "
        "frame; both hosts now hand `World::object_draw_turn_matrices` and "
        "`World::object_draw_displacements` to `field_env::live_placed_model`, "
        "and the page flips the finished model once",
        "sites": {
            "native": (NATIVE_REDRAW, "handle_redraw"),
            "web": (
                "crates/web-play/src/play/placements.rs",
                "field_placement_live_models",
            ),
        },
        "mode": "symbols_all",
        "symbols": [
            "live_placed_model",
            "object_draw_turn_matrices",
            "object_draw_displacements",
        ],
    },
    {
        "what": "fog pool render step, native vs play page - the step ages "
        "the pool and writes the live count and depth view the next tick's "
        "spawner reads, and a spawn draws the world `rand()` stream, so it "
        "is simulation. The window ran it per redraw from its draw prep and "
        "the page per sim tick, which parted the two streams on any display "
        "not at 60 Hz (`host_frame_parity`). Both hosts' per-tick step must "
        "call the one session step",
        "sites": {
            "native": (
                "crates/engine-shell/src/window/event_handler/redraw_tick.rs",
                "tick_frame_tail_effects",
            ),
            "web": ("crates/web-play/src/play_field_fx.rs", "tick_field_fog_prims"),
        },
        "mode": "symbols_all",
        "symbols": ["step_field_fog"],
    },
]


# The native redraw is one frame split into named steps across several
# files: `handle_redraw` (`NATIVE_REDRAW`) calls `run_frame_ticks` and the
# render steps, the per-tick body `sim_tick` calls its own step helpers. Every
# check that reads "what the native frame does" reads the whole frame, so a
# step helper defined in one of these files is spliced back in at its call
# site (recursively) - which reproduces the text the steps were split out of.
# The scene draw lists are built by `DrawCtx` methods (`draw_cx.` / `self.`
# inside them) rather than window methods; the splice follows those too.
# A helper in any other window file (`tick_field_party_hud`, the draw-pass
# builders) stays a call: those are kernels in their own right.
NATIVE_REDRAW_STEP_FILES = (
    "crates/engine-shell/src/window/event_handler/redraw_tick.rs",
    "crates/engine-shell/src/window/event_handler/redraw_draws.rs",
    "crates/engine-shell/src/window/event_handler/redraw_draws/actors.rs",
    "crates/engine-shell/src/window/event_handler/redraw_draws/battle.rs",
    "crates/engine-shell/src/window/event_handler/redraw_draws/field_env.rs",
    "crates/engine-shell/src/window/event_handler/redraw_draws/world_map.rs",
    "crates/engine-shell/src/window/event_handler/redraw_overlay.rs",
    "crates/engine-shell/src/window/event_handler/redraw_prep.rs",
    "crates/engine-shell/src/window/event_handler/redraw_present.rs",
    "crates/engine-shell/src/window/event_handler/redraw_render.rs",
    "crates/engine-shell/src/window/event_handler/redraw_stage.rs",
)
