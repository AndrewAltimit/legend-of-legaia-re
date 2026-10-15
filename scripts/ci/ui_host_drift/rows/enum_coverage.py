"""Tier 9 rows: shared enums whose every variant each host must answer.

Analysis: `ui_host_drift.tiers.enum_coverage`.
"""

# --------------------------------------------------------------------------
# Tier 9 - enum coverage: does each host answer every variant of a shared enum?
# --------------------------------------------------------------------------
#
# The mode word is the case that motivated this. A `SceneMode` variant can be
# entered on both hosts - the shared scene host installs it from the scene's
# own door warp - and drawn on one, because the presentation of four of them
# is a text panel and a hand-rolled 3D scene rather than an `engine-ui`
# builder tier 1 enumerates. The browser landed in each with a frozen field
# and no screen, with every tier green.
#
# The variant list is **derived** from the enum's own source, so a variant
# added tomorrow joins the measurement by existing - which is the property
# tier 3's hand-named site pairs cannot have. What is declared is the enum and
# the per-variant waivers.
#
# A host "answers" a variant when its shipped sources name it *qualified*
# (`SceneMode::Fishing`). The qualified form is the point: an unqualified
# `Fishing` matches a session type, a module name and half the fishing HUD,
# and a rule that accepted it would report every host as covering everything.
#
# Scope:
#
# * it DOES prove each host's shipped code mentions every variant of the named
#   enum by its qualified name, and that no waiver names a variant that is
#   gone or now covered;
# * it does NOT prove the host's arm draws anything, that the two arms agree,
#   or that the variant is reachable at runtime.

ENUM_COVERAGE: list[dict[str, object]] = [
    {
        "enum": "SceneMode",
        "source": "crates/engine-mode/src/mode/game_mode.rs",
        "why": "a mode both hosts can ENTER (the shared scene host drains the "
        "mode-24 door warp for either) and only one can DRAW leaves the other "
        "host's player in a frozen field with no screen - the shape four "
        "minigame modes shipped in",
        # (variant, host) -> reason. The play page's fishing line pass
        # (`play_fishing.rs`) keys on `SceneMode::Fishing`.
        "waivers": {
            ("SlotMachine", "web"): {
                "reason": "the page answers its in-world minigame modes "
                "through `legaia_engine_minigame_scenes::minigame_status::"
                "StatusGame::of_mode`, the mapping its own copy used to spell; "
                "every other arm of the page's minigame screen keys on the "
                "`StatusGame` it returns"
            },
            ("BakaFighter", "web"): {
                "reason": "as `SlotMachine`: answered through `StatusGame::"
                "of_mode` (engine-minigame-scenes), not a page-side spelling"
            },
            ("Title", "native"): {
                "reason": "the window's title is its boot-UI screen "
                "(`BootUiState::Title`), opened by the launch and by the "
                "party-wipe hold; the world's `Title` word is the engine's own "
                "idle mode under it (`mode_tick`'s `Title` arm runs nothing), "
                "so the window never needs to spell it. The only native "
                "spelling was a mode-name table in the replay command, now "
                "`SceneMode::name`"
            },
            ("Cutscene", "native"): {
                "reason": "the window opens a movie off the world's FMV "
                "request (`World::active_fmv`, `try_start_windowed_cutscene`), "
                "which the engine raises together with the `Cutscene` word; "
                "keying the movie on the request rather than the word is what "
                "lets the window hold the whole frame for it"
            },
            ("Menu", "native"): {
                "reason": "entered by one engine call, "
                "`BootSession::open_field_menu`, and drawn off the session's "
                "own `field_menu` through the shared tick claim "
                "(`TickOwner::PauseMenuSpawned` raises the window's boot-UI "
                "pause screen) - the page's waiver below, on the other host"
            },
            ("Menu", "web"): {
                "reason": "entered on both hosts by one engine call, "
                "`BootSession::open_field_menu` (engine-session), and drawn on "
                "the page off the session's own `field_menu` "
                "(`play_menu_is_open` / `play_menu_draws_json`), so the page "
                "never spells the variant: the mode and the screen cannot "
                "come apart, which is the failure this row exists to catch"
            },
        },
    },
]
