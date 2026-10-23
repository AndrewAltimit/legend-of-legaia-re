"""The canonical ladders: every `(test, package)` whose coverage export the
report unions. A renamed test or a moved test file is one edit here;
`replay_port_coverage.ladder_check` stops the run when a row names a test that
is not on disk.
"""

from __future__ import annotations

# The pad-only ladders that make up the canonical denominator, in the order
# the usage block exports them, as `(test_name, cargo_package)`. Membership is
# "drives the engine with `set_pad` and nothing else" - a ladder that seats the
# player is measuring a different thing (see `critical_path_replay`'s module
# docs on the spine oracle). `play_compose_ladder` additionally *composes* a
# frame per tick from the browser play page's builders, which is what makes
# the union see a rendering host at all (see the module docstring).
#
# This list exists so a *partial* union is visible. A union missing a ladder
# is not a conservative version of the full number; it is a number about the
# ladders that ran, and without naming the absentees it reads exactly like the
# complete one. `--list-ladders` prints the current membership - prefer it to
# any count written down elsewhere, which is what goes stale as ladders land.
CANONICAL_LADDERS = [
    ("critical_path_replay", "legaia-engine-shell"),
    ("menu_replay", "legaia-engine-shell"),
    ("minigame_replay", "legaia-engine-shell"),
    ("v0_1_playthrough", "legaia-engine-shell"),
    ("play_compose_ladder", "legaia-web-play"),
    # --- lane W1-A ---
    # Two ladders for content no pad-only *engine-shell* ladder can reach.
    #
    # `fmv_ladder` plays every retail `fmv_id` through the retail STR chain
    # (dispatch slot -> file seek -> `StrPlayer` -> `MdecDecoder` -> pad skip).
    # No other union member plays a movie at all: the headless ladders have no
    # cutscene rung and the browser play page's FMV arm auto-skips, so the whole
    # `mdec` crate reported zero executed regions - a fact about the harness,
    # not the port. Its fourth rung additionally *spawns* `CARGO_BIN_EXE_mdec`,
    # which is how the MDEC-hardware half (whose only host is the `mdec
    # str-plan` subcommand, in a `bin/` target) runs under coverage at all.
    #
    # That rung also settles a structural claim this report has been read
    # against: "a `bin/` target is unreachable from any `#[test]`" is true of
    # *calls* and false of *coverage*. `LLVM_PROFILE_FILE` is inherited, so a
    # spawned `CARGO_BIN_EXE_*` writes its own profile and it is merged in -
    # measured: `mdec::cmd_str_plan` reports 40 executions from a test that
    # only spawned it. Every `bin/`-resident row on this report's never-entered
    # list is therefore reachable by a ladder that runs the subcommand.
    #
    # `narration_ladder` drives the two text presenters a pad run reaches
    # and no ladder did: the opening-prologue subtitle roller and the inline
    # field-VM conversation path (as opposed to the pre-decoded dialog panel
    # every other ladder drives).
    #
    # Both are disc-gated and skip-pass without `LEGAIA_DISC_BIN`, exactly like
    # the ladders above; a disc-free export therefore contributes no executed
    # regions rather than failing, which is the same shape the rest of the union
    # already has.
    #
    # Both are exported WITHOUT `--release`, and that is not a detail: see the
    # module docstring's "a release export loses executed code". These two
    # ladders are where that was measured.
    ("fmv_ladder", "legaia-mdec"),
    ("narration_ladder", "legaia-engine-scene-host"),
    # --- lane W1-C ---
    ("battle_render_ladder", "legaia-web-play"),
    ("arts_swing_ladder", "legaia-engine-shell"),
    # --- lane W1-B ---
    # Three pad-driven ladders that live in `engine-core` because what they
    # drive is the world tick rather than a host: the op-0x49 submode screens
    # opened from a field-VM instruction, a Baka duel played through its
    # cabinet intro to the end-of-match tally, and a Muscle Dome leg played to
    # its between-leg tally. Each reaches content no engine-shell or
    # web-play ladder does, so leaving them out is a different number rather
    # than a smaller one.
    ("submode_screen_ladder", "legaia-engine-core"),
    ("baka_duel_ladder", "legaia-engine-scene-host"),
    ("dome_leg_ladder", "legaia-engine-scene-host"),
    # --- lanes W1-D / W1-E / W1-F, plus four older ladders ---
    # These existed and ran green while being absent from this list, which is
    # the export *recipe* - so nobody exported them, no `cov-*.json` was ever
    # written for them, and every row they reach kept reading *live but never
    # entered*. A ladder written to convert a row cannot convert it until it is
    # named here. When a lane adds a ladder, it belongs in this list in the same
    # commit.
    ("world_map_render_ladder", "legaia-engine-world-views"),
    ("dev_menu_equip_ladder", "legaia-web-play"),
    ("audio_session_ladder", "legaia-engine-audio"),
    ("scene_bgm_transition_ladder", "legaia-engine-scene-host"),
    ("battle_tutorial_ladder", "legaia-engine-core"),
    ("fishing_pond_ladder", "legaia-engine-fishing"),
    ("fishing_banner_ladder", "legaia-web-play"),
    ("pause_special_use_ladder", "legaia-engine-menu-runtime"),
    ("menu_depth_ladder", "legaia-web-play"),
    ("battle_depth_replay", "legaia-engine-shell"),
    ("battle_flee_ladder", "legaia-engine-core"),
    ("battle_full_playthrough", "legaia-engine-core"),
    ("seru_cast_magic_xp_ladder", "legaia-engine-core"),
    # --- lane L5 ---
    # The native window's own composition layer, reached by SPAWNING it.
    #
    # `native_minigame_ladder` runs `CARGO_BIN_EXE_legaia-engine
    # play-window` once per minigame and lets the inherited `LLVM_PROFILE_FILE`
    # merge each child's profile into this export. That is the same route the
    # `mdec` ladder's fourth rung takes: the native window
    # (`crates/engine-shell/src/window/`) - the HUD builders, the minigame
    # side-channel tick, the field render pass - is library code a test could
    # link, but it needs a wgpu surface, so it executes here, through the
    # spawned binary, and nowhere else in the union.
    #
    # What made the cluster reachable at all is a CLI channel rather than a
    # file move: `--pad-script` writes a pad word and the window's keyboard
    # handler never runs, while every native minigame entry lives in that
    # handler, so no pad-only run could open one. `--key-script` delivers keys
    # through the same arms a player's keyboard does, and the two scripts
    # compose - keys open the surface, the pad plays it.
    #
    # Two gates beyond the disc: the rungs need a display (`play-window` needs
    # a real wgpu surface even for its offscreen capture) and they assert on
    # the captured FRAME, not on the exit status - a HUD builder that emits an
    # empty draw list would pass "did it run" and fails here.
    #
    # EXPORT THIS ONE IN TWO STEPS. `-p <pkg>` scopes the *report* to that
    # package's sources, not only the build, and this ladder's whole yield is
    # in other crates: a one-shot `-p legaia-engine-shell --json` export of it
    # carries 42 files, all under `crates/engine-shell/`, while the dance HUD,
    # the fishing chrome, the Baka number drawers and the casino counter are
    # `engine-core` and the builders under them are `engine-ui`. Reporting
    # without `-p` over the same profiles carries 652 files across fifteen
    # crates - including `engine-render`, which this page records as
    # structurally unreachable because the browser composition ladder cannot
    # carry a wgpu link. The native window is that link.
    #
    #     cargo llvm-cov clean --workspace
    #     cargo llvm-cov -p legaia-engine-shell \
    #         --test integration native_minigame_ladder:: --no-report -- --test-threads=1
    #     cargo llvm-cov report --json \
    #         --output-path target/cov-native_minigame_ladder.json
    #
    # The same scoping applies to every `-p`-exported ladder above, so a row
    # that reads never-entered may be a row whose only driver exported it out
    # of scope. That is worth re-measuring before spending a fixture.
    ("native_minigame_ladder", "legaia-engine-shell"),
    # --- lane L4 ---
    # Three ladders for content the union is blind to *by construction* rather
    # than by depth.
    #
    # `page_compose_ladder` drives the browser surfaces `play_compose_ladder`
    # opens but cannot populate. Its Load rung already reaches the card rack -
    # with no card inserted, so `card_block_snapshots` takes its `None` early
    # return and the retail directory walk behind it never runs. This one inserts
    # real card images, prices their free blocks, commits a save through the
    # filename classifier, and walks the info panel's three modes. It also drives
    # the two standalone minigame pages and the Items target panel, whose
    # preview word needs a bag the page has no affordance to fill (it arrives on
    # a resumed save, as a player's would).
    #
    # `slot_bonus_marquee_ladder` plays the casino machine into a **bonus
    # round** and composes the dot-matrix marquee off that round's own live
    # state. The schedule is solved rather than rolled for: the machine is
    # deterministic and `SlotMachine` is `Clone`, so each candidate stop frame is
    # probed on a copy and the RNG is untouched by the search.
    #
    # `slot_bonus_marquee` is the disc-data half of the same gate, and the
    # one that can spawn `CARGO_BIN_EXE_asset` - `asset slot-scene` is the only
    # non-test caller the marquee kernels have, and a spawned bin inherits
    # `LLVM_PROFILE_FILE` (the same mechanism `fmv_ladder` uses for `mdec`).
    #
    # All three are disc-gated and skip-pass without `LEGAIA_DISC_BIN`, and all
    # three are exported WITHOUT `--release` - see the module docstring's "a
    # release export loses executed code".
    ("page_compose_ladder", "legaia-web-play"),
    ("slot_bonus_marquee_ladder", "legaia-web-minigames"),
    ("slot_bonus_marquee", "legaia-asset"),
    # --- lane L1 ---
    # The browser play page driven into a battle with retail's own one-shot
    # arm flag raised, so the tutorial box the page composes actually exists.
    ("battle_tutorial_page", "legaia-web-play"),
    # --- lane L2 ---
    # The pause menu's three data-driven kernels, driven to their OUTPUT (bag
    # count, picked weapon id, destination rows) rather than to their call.
    ("menu_data_wiring", "legaia-engine-menu-runtime"),
    ("menu_data_wiring_disc", "legaia-engine-menu-runtime"),
    # --- lane L3 ---
    # Five fixtures for the GATED-(b) rows: content behind a story flag, a
    # scene, or a battle state no pad stream reaches from a cold boot.
    #
    # These are **gate-seeded**, not pad-only, and the distinction has to stay
    # visible in this list or the denominator quietly changes meaning. Each one
    # writes exactly the one piece of state the gate is (a system flag, a
    # status effect, a visited-map record) and then drives the ordinary engine
    # path - `World::tick`, `World::step_field`, the scene loader - from there.
    # Three take their bytecode from the disc corpus rather than inventing it,
    # so they skip-pass without `LEGAIA_DISC_BIN` like the rest of the union.
    #
    # `field_ledge_hop_disc` is not new: it already drove the ledge-hop rows
    # (`801d2404` / `801d2298`) off a real `town01` ledge and was simply never
    # in the union, which is why those rows read as never-entered.
    #
    # Export them WITHOUT `--release` (see "a release export loses executed
    # code" above) - several of the kernels here are small enough to inline.
    ("scripted_scene_program_gate", "legaia-engine-scene-host"),
    ("gated_field_arms_disc", "legaia-engine-core"),
    ("confused_monster_target_gate", "legaia-engine-core"),
    ("travel_art_visited_gate", "legaia-engine-core"),
    ("field_ledge_hop_disc", "legaia-engine-scene-host"),
    # --- lane W2C ---
    # Five existing tests that were already the honest drivers for
    # never-entered rows and were simply absent from this list (the W1-D/E/F
    # shape: a ladder converts nothing until it is named here), plus two new
    # ladders.
    #
    # `pause_menu_compose` composes every pause screen through the shared
    # session-path builders on both chrome passes: the Items special-Use
    # confirm (801d1dac / 801d1f10), its hand quad (801e3ff0), and the three
    # painter-gated windows 5 / 6 / 7 (801d61b0 / 801d6360 / 801dccb4).
    # Disc-free.
    #
    # `dome_ladder_and_hub_real` builds the Muscle Dome hub screens off the
    # disc's recovered draw lists - the score tally's label strips are the
    # corner-anchored emitter (801d08ec). `character_pack_real` applies the
    # equipment group-swap patch to every active-party slot (8001ebec).
    # `boot_overlay_disc` resolves the boot-time slot-B overlay choice and,
    # with this lane's rungs, the effect-data side-band branch and the
    # sector-count rounding against the real TOC (80025ba0 / 8003e360 /
    # 8001eef0). `cdname_retail_parse_disc` runs the lossy retail CDNAME
    # reader over the real file (8001d8fc). All four disc-gated skip-pass.
    #
    # `battle_fx_ladder` walks the move-FX streak counter through the
    # production dispatcher so the chained ribbon (801e1d98) is reached the
    # way a host reaches it - by the counter falling below 0x201, not by a
    # direct call. Disc-free.
    #
    # `card_inventory_ladder` runs the retail item-window accessor family
    # over a real memory card's SC block (select / find / consume / normalize
    # / add, 8004313c and friends). Keys on ~/.mednafen/sav like
    # real_card_roundtrip, not on the disc; skip-passes without a card.
    ("pause_menu_compose", "legaia-engine-ui"),
    ("battle_fx_ladder", "legaia-engine-ui"),
    ("dome_ladder_and_hub_real", "legaia-web-minigames"),
    ("character_pack_real", "legaia-actor-formats"),
    ("boot_overlay_disc", "legaia-overlay-images"),
    ("cdname_retail_parse_disc", "legaia-prot"),
    ("card_inventory_ladder", "legaia-save"),
    # --- lane W2B ---
    # The two remaining NO-LADDER rows whose blocker was scene content.
    #
    # `dialog_picker_ladder` drives a real multi-choice conversation (the
    # `koin4` merchant's two-price offer) through the session's field-interact
    # path into `OwnedDialogPanel::confirm_menu` (`80038050`) - the native
    # keyboard handler's own contract, previously reached by no ladder because
    # a menu needs a picker-bearing NPC record, not a harness entry point.
    #
    # `fmv_handoff_ladder` executes each of the eight trigger scenes' own
    # `4C E2` op through the session field VM (record bytes sliced at the op -
    # the surrounding story choreography spins on actor-motion waits no
    # headless world satisfies; see the ladder's module doc) and runs the
    # post-play hand-off map (`fmv_post_play_handoff`, `801cea3c`) to the
    # hand-off scene, completing one full Field -> Cutscene -> hand-off-scene
    # transfer.
    #
    # Both are disc-gated and skip-pass without `LEGAIA_DISC_BIN`; export both
    # WITHOUT `--release` (see "a release export loses executed code" above).
    ("dialog_picker_ladder", "legaia-engine-scene-host"),
    ("fmv_handoff_ladder", "legaia-engine-scene-host"),
    # --- lane L4 (playable-ch1) ---------------------------------------- <L4>
    # `chapter1_frontier_ladder` is the only union member denominated in
    # SCENES rather than in a route: it takes the BFS closure of `town01` over
    # the disc's own `0x3F` destination tables (27 scenes, bounded at the
    # `jiji -> map02` kingdom handoff) and drives every member through load ->
    # enter -> settle -> pad-walk -> exit.
    #
    # What that reaches and no route ladder does: the entry script, walk-on
    # dispatch, collision grid and exit-record path of twenty-two interiors
    # the pad ladders never visit - the Drake Castle chain (`jouina` ..
    # `jouine`), the Uru Mais rooms, `garmel`, `bylon`, `izumi`, `town0b`.
    # `critical_path_replay` walks five scenes well; this one walks
    # twenty-seven once each, so the two are different numbers rather than
    # different sizes of the same one.
    #
    # Disc-gated and skip-passes without `LEGAIA_DISC_BIN`; export WITHOUT
    # `--release` (see "a release export loses executed code" above).
    ("chapter1_frontier_ladder", "legaia-parity"),
    # ------------------------------------------------------------------ </L4>
    # --- lane W4-D -----------------------------------------------------
    # `cast_band_ladder` is denominated in **spell ids**, which is the axis
    # every other member is blind to. `cast_module_ticks`' bodies are gated on
    # `World::cast_module_for`, so a route ladder enters whichever one or two
    # casts its route happens to reach and the rest of the PROT 0903..0966
    # band reads *live but never entered* no matter how far the route goes.
    # This one asks the engine's own two dispatchers for a representative id
    # per band entry and steps each entry's module code, so the gap it closes
    # is an id gap rather than a reach gap.
    #
    # Disc-gated, and specifically on `SCUS_942.54` rather than only on
    # `PROT.DAT`: the capture half of the band is keyed on the spell record's
    # class byte, so without the executable every id falls to the action-id
    # dispatcher and half the band cannot be seated.
    ("cast_band_ladder", "legaia-engine-core"),
    # --- lanes W1-B / W1-C / W1-D / W2-C: the cast band by DEPTH -------
    # `cast_band_ladder` above is denominated in spell ids and steps two
    # arms per band entry, which answers "was this body reached at all" and
    # nothing about what runs inside it. These four are the depth axis, and
    # each is a different denominator:
    #
    # `seru_ticks_0x81_0x86_ladder` / `seru_ticks_0x87_0x8b_ladder` walk the player-Seru
    # tick bodies of ids 0x81..=0x86 and 0x87..=0x8B through their whole phase
    # chain. Those bodies are `beq` chains fifteen arms deep whose simulation
    # writes live in the late arms, so an id-denominated ladder enters the
    # body and exits it without executing the half that does the work.
    #
    # `trampoline_arms_ladder` is denominated in `(PROT entry, action id)`
    # PAIRS, because those fourteen bodies are reached only through their
    # module's trampoline: an id the trampoline does not name ticks nothing,
    # and one cell can hold two bodies, so an entry-keyed ladder runs one of
    # them and reports the cell covered.
    #
    # `cast_band_body_ladder` takes its denominator from the SOURCES
    # rather than from a hand-written table: it scrapes every `// PORT:`
    # address out of the three cast-band modules at compile time and fails if
    # a body exists that its row table does not account for. That is the one
    # member of this union whose coverage claim cannot silently go stale when
    # a later lane adds a body.
    #
    # All four are disc-gated (`w1c` and `w1d` additionally on SCUS, since the
    # resolver consults the spell table's class byte) and skip-pass without
    # `LEGAIA_DISC_BIN`; export WITHOUT `--release`.
    ("seru_ticks_0x81_0x86_ladder", "legaia-engine-core"),
    ("seru_ticks_0x87_0x8b_ladder", "legaia-engine-core"),
    ("trampoline_arms_ladder", "legaia-engine-core"),
    ("cast_band_body_ladder", "legaia-engine-core"),
    # `cast_band_body_ladder` above scrapes its denominator out of THREE
    # band modules and `cast_module_ticks.rs` is not one of them, so the
    # module holding the band's largest never-entered cluster had no
    # source-denominated ladder at all. `cast_module_bodies_ladder` is
    # that ladder: it scrapes every `// PORT:` address out of
    # `cast_module_ticks.rs` and drives each dispatch site through the seam
    # its own dispatch uses - the trampoline (keyed on `(entry, action id)`,
    # which is what `cast_band_ladder`'s one-representative-id-per-entry
    # walk is blind to), the no-trampoline band arm, the inline staging call,
    # and `World::run_cast_module_aoe` for the two modules whose damage lands
    # in their own sweep stager.
    #
    # Disc-gated on `PROT.DAT` + `SCUS_942.54` (the capture half of the band
    # is keyed on the spell record's class byte); export WITHOUT `--release`.
    ("cast_module_bodies_ladder", "legaia-engine-core"),
    # The capture-class boss cast, driven through the ordinary live battle
    # loop. Its gate had a seeded oracle already
    # (`world/tests/battle_capture_class_disc.rs`) that can never be a union
    # member - a `#[cfg(test)]` module is not a `--test` binary - and the
    # oracle also calls `World::cast_spell_on_slots` directly, while every
    # seam from a host to the fold is `pub(in crate::world)`. So this ladder
    # seeds one disc monster record's magic list and lets the monster AI pick
    # the cast, which makes it a reach measurement of
    # `capture_bypass_predamage` / `capture_respect_predamage` rather than a
    # kernel test. Disc-gated; export WITHOUT `--release`.
    ("capture_class_cast_ladder", "legaia-engine-core"),
    # --- lane W1-G: the three gates behind the (b) rows --------------------
    #
    # `seru_capture_ladder` is the Seru-capture gate's *production* route.
    # Its address (`801e92dc`, `magic_xp::learn_spell_prepend`) already read as
    # entered because `battle_depth_replay` calls the function from its own
    # test body to seat a caster - the shape this page warns about, where the
    # only executor is a ladder's setup code. This one casts the capture spell
    # in a live fight, lands the roll on a weakened monster, and lets
    # `World::finish_battle` -> `World::resolve_captures` reach the learn
    # commit. Disc-free: the vanilla formation / monster / Seru / spell tables.
    #
    # `save_subscreen_ladder` opens the five save-UI sub-screen bodies no
    # other ladder does. The gate is the machine's **entry context**: the card
    # rack constructs only the card contexts, so the shop chain
    # (mode select -> quantity spinner -> terminal screen) is reached here from
    # `SaveEntryContext::ShopEntry`, which is the op-`0x49` record's own kind
    # byte, and the two orphan screens from the screen-id write that is retail's
    # own transition. Disc-free.
    #
    # `fishing_tackle_pick_ladder` is denominated in the player's PICKS.
    # `fishing_pond_ladder` sets the lure and rod as constructor arguments
    # and therefore cannot enter either picker screen; this one drives both by
    # pad over a tackle bag and opens the pond on what they returned, so the
    # band-4 preconditions arrive rather than being seeded. Disc-free.
    ("seru_capture_ladder", "legaia-engine-core"),
    ("save_subscreen_ladder", "legaia-engine-menus"),
    ("fishing_tackle_pick_ladder", "legaia-engine-fishing"),
    # --- lane W3-C: the boot chain ----------------------------------------
    #
    # `boot_logos_ladder` opens the stage BEFORE the title card. Every
    # other member starts at a scene, a battle, or the title - the composition
    # ladder's own first rung calls `boot_title_start`, which is one stage
    # late - so the publisher-logo sequencer (`FUN_801CEFD4`), its retail play
    # order and the per-logo quad tables had no driver at all. It is pad-only:
    # `boot_logos_start` is the browser play page's own boot entry, and the
    # only other input is the pad word `boot_logos_step` takes.
    #
    # Disc-gated (PROT 0895 `init.pak`); export WITHOUT `--release`.
    ("boot_logos_ladder", "legaia-web-play"),
    #
    # `field_vm_op_arms_disc` is the other exit this page records: an
    # existing honest driver promoted into the set rather than a new fixture.
    # It is the op census's two rows' own fixture - the scripted game over
    # (`4C EA`, `8003c7ec`) and the take-item unequip fallback (`4C 52`,
    # `800430ac`) - and it takes both arms' bytecode from the disc corpus at
    # DECODED instruction boundaries, then steps the record in a real `World`
    # and asserts on world state. Like the L3 members above it is
    # record-seated rather than pad-driven, and the take-item arm is driven
    # twice (bag hit and bag miss) because one byte sequence has two host
    # paths. Disc-gated on `LEGAIA_DISC_BIN` + `extracted/`.
    ("field_vm_op_arms_disc", "legaia-engine-core"),
    # --- lane W3-B: the composing host, and three field arms off the disc --
    #
    # `fog_page_prims` is the **composing** ladder the fog rows always
    # needed and nobody had joined. `World::fog_render_step` has exactly two
    # callers in the workspace and both are render passes, so no headless walk
    # reaches it however deep it goes; this one drives the browser play page's
    # per-tick screen-prim assembly over the first scene whose entry script
    # raises the gate unconditionally, and asserts the pass carries the quads.
    # It was written, green and outside the union, which is the "promote the
    # oracle that already drove it" exit one more time.
    ("fog_page_prims", "legaia-web-play"),
    #
    # `play_page_host_parity_ladder` is the page half of the shared camera kernels -
    # the NCLIP cull mode, the occlusion fade's arming gate and body centre,
    # and the field HUD projection. Each replaced a decision the page had been
    # making in JS, so each is engine code whose only browser caller is the
    # page's own per-frame read surface; the ladder drives a real
    # `LegaiaRuntime` over two disc scenes and reads the exports back.
    ("play_page_host_parity_ladder", "legaia-web-play"),
    #
    # `clone_reflection_effect_arms_disc` is `field_vm_op_arms_disc`'s sibling for
    # three more arms: the actor clone (`4C 14`), the reflection controller
    # install (`4C 86`) and the screen-effect colour tween (`34 0x`). Same
    # denominator - sites taken from the disc corpus at decoded instruction
    # boundaries, one per scene, then stepped in a real `World` - and each arm
    # is driven past its spawn into the pool tick that consumes it, because a
    # seat nothing steps is indistinguishable from a seat that never ran.
    ("clone_reflection_effect_arms_disc", "legaia-engine-core"),
    # --- lane W6-C ---
    # Two new ladders and two promotions, each for a wire the union reached
    # only halfway.
    #
    # `arts_banner_ladder` types a Super Art through the live battle and
    # reads `World::battle_arts_banner_quads` once per frame, where both hosts
    # read it. Every battle ladder already stepped the banner tick; none
    # committed a SpecialStarter, so the quad emitter `flash_quads`
    # (`801e2650`) had never run. `morph_page_ladder` enters the three
    # `4C D8` carriers on the play page and stages the blended mesh through
    # the page's own export - the spawner was entered, the page draw was not.
    #
    # The promotions were green and unlisted, and neither adds an anchor the
    # union lacked - what each adds is an untagged host draw or a state flip,
    # which this report cannot key on and the triage page records by name:
    # `play_minigames_host` drives the four door-warp minigames on the page
    # (the Baka digit strips are its only driver of `baka_hud_text_draws`), and
    # `conc_flag_6de_position_latch_disc` walks the player across `conc`'s
    # `CD F8` box both ways, which makes `sync_field_ctx_player_anchor`
    # observable as a flip rather than as a call.
    ("arts_banner_ladder", "legaia-engine-core"),
    ("morph_page_ladder", "legaia-web-play"),
    ("play_minigames_host", "legaia-web-play"),
    ("conc_flag_6de_position_latch_disc", "legaia-engine-scene-host"),
    # --- lane W7-C (reach burn-down) ---
    # One new ladder, nine promotions and four whole-game instruments.
    #
    # `pause_learned_content_ladder` opens the pause menu by pad over a
    # party that has learned something - one Seru spell on the record, one
    # art book in the bag - which is the content `menu_replay`'s cold party
    # lacks: the Magic list's broadcast (`8003053c`), the Status screen's
    # reorder page (`801da2a0`) and the art notice's operand patch
    # (`801dcd58`).
    #
    # The nine promotions were found by running every integration test of the
    # engine crates under coverage and joining each against the never-entered
    # set; each was read before it was listed, and each drives its row through
    # the path a host calls rather than calling the kernel from its own body.
    # Disc-free ones seat a synthetic party (the L3 disclosure applies):
    # `steal_attack_round_disc` (the death-spoils caption through the live
    # round, `8003cb54`), `battle_always_resolves` (the results frame's panel
    # read, `801d84c0`), `battle_member_step_back` (the ring cancel's cursor
    # step, `801d32bc`), `shop_quantity_stepper` + the engine-ui
    # `shop_quantity_compose` (the stepper session and window 35,
    # `801db7f4` / `801d5510`), `fishing_hub_and_floor_window_disc` (the venue
    # hub's help pages, `801d72a0`), `play_ringside_still_disc` (a dome leg won
    # by pad on the page, `801d1288`), `attached_light_page_prims` (the page's
    # screen-prim cache over `dolk`'s attached light, the light-pool quartet
    # and `801e4470`) and `code_lock_doman_disc` (the shipped `49 02`,
    # `801eed58`).
    #
    # The four whole-game instruments are session-shaped rather than
    # pad-only (segments seeded from retail anchors, seeded approaches, a
    # seeded-random soak); they are in the union because they execute
    # content no other member reaches, and `soak_harness` contributes only
    # its default `soak_smoke_no_panics` (the other tests are opt-in).
    ("pause_learned_content_ladder", "legaia-engine-shell"),
    ("steal_attack_round_disc", "legaia-engine-core"),
    ("battle_always_resolves", "legaia-engine-core"),
    ("battle_member_step_back", "legaia-engine-core"),
    ("shop_quantity_stepper", "legaia-engine-menu-runtime"),
    ("shop_quantity_compose", "legaia-engine-ui"),
    ("fishing_hub_and_floor_window_disc", "legaia-engine-scene-host"),
    ("play_ringside_still_disc", "legaia-web-play"),
    ("attached_light_page_prims", "legaia-web-play"),
    ("code_lock_doman_disc", "legaia-engine-scene-host"),
    ("full_game_ladder", "legaia-engine-shell"),
    ("boss_approach_disc", "legaia-engine-shell"),
    ("monster_approach_sweep_disc", "legaia-engine-core"),
    ("soak_harness", "legaia-engine-shell"),
    # Two page ladders for the boot save-select's title backdrop (the title
    # strips, `801e0418`) and the command ring's Ra-Seru cross-out in Tetsu's
    # fight (`801dbc30`); each has a native twin outside the union (a bin
    # unit test, a shared-builder unit test).
    ("title_backdrop_parity", "legaia-web-play"),
    ("raseru_cross_out_page", "legaia-web-play"),
    # Three routines reached only through the world's frame tail: the
    # fishing strike splash (`801d7a5c`, a cadence match in the world's
    # fishing tick), a shipped op-`0x43` sub-`0x12` rect copy drained by the
    # VRAM tail (`80057914`), and a shipped Bezier stager advanced by
    # `World::step_world_frame_tail` (`801e45bc`); plus shipped `4C D4` /
    # `4C D5` mask-bit runs through the same VRAM tail (`8005842c` /
    # `800583c8`).
    ("world_frame_tail_ladder", "legaia-engine-scene-host"),
    # An all-target spell committed on the play page from a played-through
    # card, so the commit log copies the whole-row label (`801d57e8`).
    ("commit_log_all_target_page", "legaia-web-play"),
    # The SFX ring's replace-last (`80035bd0`): every headless member runs
    # its producers, but the ring lives in a host's SFX scheduler and the
    # play page's is the only one that exists without an audio device. The
    # overworld sub-list's confirm brings its cue due there.
    ("world_map_sublist_sfx_page", "legaia-web-play"),
    # The two ailment marks over the command chips - the Curse plate on the
    # ring (`801dbec4`) and the Rot stamp on the arts entry (`801dbddc`) -
    # with the ailment seeded the way an inflicting strike writes it.
    ("status_marks_page", "legaia-web-play"),
    # Retail's visible-tile crop (`801f7088` / `801f6d48`), which the shared
    # kernel answers only at the retail camera preset - and both hosts start
    # on a wider one. The page's distance control takes it there.
    ("view_window_crop_page", "legaia-web-play"),
    # The passive-ability badge column (`801d095c`), behind a party that
    # wears one of six passive bits: an accessory equipped by pad.
    ("passive_badge_page", "legaia-web-play"),
    # The end of the game: `edlast`'s credits, their press poll, and `49 0C`'s
    # return-to-title soft reset (slot `0x33`, `FUN_801EDF00`) - the records
    # screen over the field and the title hand-off.
    ("soft_reset_records_page", "legaia-web-play"),
    # The casino prize counter's Yes/No confirm (window 46, `801d603c`):
    # coins and the clerk's `49 07` arm seeded, the pad walks a prize to it.
    ("prize_confirm_page", "legaia-web-play"),
    # The Point Card toast (window 31, `801dce20`): gated on holding item
    # `0xFE`, which a cold bag never does. The card is seeded through the
    # page's item cheat and the pad buys into the bag off the equipment
    # shop's recipient picker; scored against the same walk with no card.
    ("point_card_toast_page", "legaia-web-play"),
    # A promoted oracle (record-seated, not pad-driven - the L3 members'
    # disclosure): the effect ribbon (`801cfa48`, `effect_ribbon.rs`) and its
    # RNG (`801d0290`) are built only for a summon whose move program issues
    # op `0x42`, and no pad ladder casts one. This test stages each shipped
    # carrier (PROT 0923 / 0934 / 0957 / 0964) through `World::spawn_summon`,
    # ticks it through `World::tick_summon` and reads the ribbons off
    # `World::active_effect_ribbons`, the list both battle hosts draw.
    # Disc-gated; export WITHOUT `--release`.
    ("effect_ribbon_carriers_real", "legaia-engine-core"),
    # The camera yaw factor of a camera-relative part (`8004629c`,
    # `GteMat3::rot_y` under `camera_view_rotation`), which runs only for a
    # `+0x52` word that skips pitch or roll and keeps yaw. One word on the
    # disc does: `urudre1` stager record 14's `0x0080`. The page walks onto
    # the dream's walk-on band, pages cutscene record `P2[1]` to its
    # `34 30 05`, and composes the spawned sprite-arm quads through
    # `play_field_fx_sync`.
    ("dream_yaw_part_page_ladder", "legaia-web-play"),
    # The map display's location stepper (`801cf5e8`), reached only by a
    # Square press on the open L1 display (Circle's own step arm sits behind
    # the exit test). Enters each kingdom map through
    # `SceneHost::enter_world_map_scene`, opens the display by pad, and
    # requires Square to seat the survey on the kingdom's last record.
    # Disc-gated; export WITHOUT `--release`.
    ("map_display_disc", "legaia-engine-screens"),
    # The sparring tutorial's countdown arm (`801f7628`), which only the
    # completion tail calls - after the fourth lesson. `battle_tutorial_ladder`
    # walks the hooks with Cross alone and never finishes the drill;
    # `the_sparring_fight_is_won_by_playing_all_four_lessons` plays each
    # lesson's command by pad through a real encounter until the fight hands
    # back to the field. Disc-free (synthetic prompt script); the file's
    # flag-bank halves are save-library-gated.
    ("sparring_exit_retail_capture", "legaia-parity"),
]
CANONICAL_LADDER_NAMES = [name for name, _pkg in CANONICAL_LADDERS]
