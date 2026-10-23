//! Which HUD phase and which command surface the battle is showing.

/// Which of retail's HUD phases the frame is in.
///
/// Retail's battle HUD is not drawn per frame - it is a list of retained
/// text actors (`ctx[+0x1074]`, forty handles) that the two battle state
/// machines rebuild at every transition, and both rebuilds are disc data:
///
/// * the **menu** SM `FUN_801D0748` runs `FUN_801D388C(step)` on every
///   `ctx[+0x06]` edge, and `step` indexes the sub-draw script table
///   `PTR_DAT_801F4D34` (overlay 0898 rodata). A record is
///   `[count][anim][panel]` + `count` x `(record, mode)`: `anim = 1` first
///   hard-resets the handle list (`FUN_801D99BC`), so after the step the
///   live elements are exactly the pairs listed. `record` indexes the
///   screen-element placement table at `0x80076C10` directly
///   (`FUN_801D8DE8`: `0x80076C10 + id * 0x18`), `mode` bit 0 picks which
///   of the record's two seats the actor spawns at (`+0x02/+0x04` for `0`,
///   `+0x0A/+0x0C` for `1`) before it glides to the other, and bit 1
///   suppresses the glide (`0x801D92E0..0x801D93DC`);
/// * the **action** SM `FUN_801E295C` opens the per-action elements in
///   its seed arms and closes every one of them in the `0x51` band
///   (`0x801E6170..0x801E6364`).
///
/// The phases below are the four distinct element sets those two tables
/// produce, cross-checked against the live handle lists of the catalogued
/// mednafen states (`scripts/scenarios.toml`; the walk is in
/// `docs/subsystems/battle.md`, "The per-phase rule"):
///
/// * **`RoundPrompt`** (`ctx[+0x06] = 0x1E`, step 0: `00/0 03/0 06/0
///   4E/0 4F/0`): the `Begin` / `Run` chips and one roster panel per
///   member - nothing else. No plaque, no bar, no AP plate.
/// * **`CommandEntry`** (`0x28` and every window below it): what shows
///   depends on the surface - [`battle_command_surface`].
/// * **`Action`** (the action SM's `0x0C..=0x52` bands): decided per
///   action by the seed arms - [`battle_readout_bar_slot`],
///   [`battle_move_name`], [`battle_target_plaque`].
/// * **`Idle`** (`0x0A` between actions, the open and the close): retail
///   holds no live handle at all (the `evil_medallion_rage_battle` state,
///   taken in `0x0A`, has an empty handle list).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattleHudPhase {
    Idle,
    RoundPrompt,
    CommandEntry,
    Action,
}

/// Is `state` (the action SM's `ctx[+0x07]`) inside an action's presentation,
/// from the seed through the Done band, plus the run / capture arms? The
/// pre-action holds (`0x00` / `0x0A` / `0x0B`) and the end-of-action gate
/// (`0x5A`) are outside: retail's `rage` capture, taken in `0x0A`, carries no
/// live widget handle.
pub fn battle_action_in_flight(state: u8) -> bool {
    matches!(state, 0x0C..=0x52 | 0x64..=0x6B | 0x6E..=0x71)
}

/// The HUD phase this frame is in ([`BattleHudPhase`]).
///
/// An open submenu or arts-entry session wins over the command session's
/// own phase (retail's windows are states below the ring), and the command
/// session wins over the action SM (it opens between actions).
pub fn battle_hud_phase(world: &legaia_engine_core::world::World) -> BattleHudPhase {
    use legaia_engine_battle_menus::battle_input::CommandPhase;
    if world.mode == legaia_engine_mode::mode::SceneMode::MuscleDome {
        // A dome leg is a battle whose only command surfaces are the
        // fighter's selection screens (`muscle_dome::DomeMenu`); the play-out
        // has no battle-action band behind it.
        // The resolved turn's play-out is that band's stand-in: while the
        // leg holds for it the frame is an action frame.
        let s = world.minigames.muscle_dome.as_ref();
        if s.is_some_and(|s| s.intro_up()) {
            // The battle-open hold: only the enemy-name banner is up.
            return BattleHudPhase::Idle;
        }
        if s.is_some_and(|s| {
            s.phase() == legaia_engine_battle_menus::muscle_dome_session::MusclePhase::Select
                && matches!(
                    s.menu(),
                    legaia_engine_battle_menus::muscle_dome_session::DomeMenu::Command(c)
                        if matches!(c.phase, CommandPhase::RoundPrompt { .. })
                )
        }) {
            // The turn's `Begin | Run` prompt is the battle's `0x1E`.
            return BattleHudPhase::RoundPrompt;
        }
        return match s.map(|s| s.phase()) {
            Some(legaia_engine_battle_menus::muscle_dome_session::MusclePhase::Select) => {
                BattleHudPhase::CommandEntry
            }
            Some(legaia_engine_battle_menus::muscle_dome_session::MusclePhase::TurnOver)
                if world.muscle_playback_tally().is_some() =>
            {
                BattleHudPhase::Action
            }
            _ => BattleHudPhase::Idle,
        };
    }
    if world.mode != legaia_engine_mode::mode::SceneMode::Battle {
        return BattleHudPhase::Idle;
    }
    if world.arts_input_active()
        || world.battle.arts_menu.is_some()
        || world.battle.spell_menu.is_some()
        || world.battle.item_menu.is_some()
    {
        return BattleHudPhase::CommandEntry;
    }
    if let Some(cmd) = world.battle.command.as_ref() {
        return match cmd.phase {
            CommandPhase::RoundPrompt { .. } => BattleHudPhase::RoundPrompt,
            _ => BattleHudPhase::CommandEntry,
        };
    }
    if battle_action_in_flight(world.battle_ctx.action_state) {
        BattleHudPhase::Action
    } else {
        BattleHudPhase::Idle
    }
}

/// The command surface that owns a [`BattleHudPhase::CommandEntry`] frame -
/// retail's `ctx[+0x06]` window, named by the sub-draw step that built it.
///
/// Each variant's doc names the step whose element list it stands for; the
/// pairs quoted are `(placement record / mode)` as the table carries them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandSurface {
    /// `0x28`, step 1: `09/0 0A/0 08/0 0B/0 01/0 05/0 06/1 4E/1 4F/1 07/0
    /// 1A/0 52/0` - the four ring chips unfold, the `Begin` chip becomes
    /// the `(16, 14)` tab, `Run` leaves, the panels park, the bar (7) rises
    /// for the acting member, the plaque (26) drops in behind the tab at
    /// `(68, 14)` and the AP plate (82) slides in to `(208, 174)`.
    Ring,
    /// `0x78`, step `0x2A`: `55/0 54/0 01/3 07/3 08/1 0A/1 0B/1 0D/0 1A/3
    /// 52/3` - the `Auto` / `Command` chips (records `0x54` / `0x55`) open
    /// on the ring's left and right arm seats, the other three arms leave,
    /// and the bar, tab, plaque and AP plate all stay. A target cursor
    /// backed out of reopens it with step `0x2E` (`55/0 54/0 01/3 07/0 0D/3
    /// 1A/3 29/1 52/0`), which slides the bar and the plate back in.
    AttackMode,
    /// `0x5A`, step `0x2D` from the prompt's `Auto`: `01/3 07/1 0D/3 1A/3
    /// 29/0 52/1 54/1 57/1` - the bar parks and the AP plate leaves; tab and
    /// plaque stay. Step `0x30` (the ring's Attack arm under the `Automatic`
    /// option) does the same from the ring.
    Targeting,
    /// `0x50`, step 9: `01/3 07/1 08/1 0A/1 0B/1 0D/0 0F/0 1A/3 1B/0 1C/0
    /// 1D/0 1E/0 52/3` - the bar parks and the AP **bar** (15) takes its
    /// seat; the AP plate stays.
    ArtsInput,
    /// The port's saved-chain list, which retail has no state for; treated
    /// as the arts-entry screen it stands in for.
    ArtsList,
    /// `0x3C`, step 5: `01/3 06/0 4E/0 4F/0 07/1 09/1 0A/1 0B/1 0C/0 16/0
    /// 17/0 1A/3 52/1` - the roster panels come **back up**, the bar parks,
    /// the plate leaves.
    ItemBrowse,
    /// `0x64`, step `0x12` then `0x18` per cursor move: `01/3 07/0 1A/3
    /// 29/0 2A/0 34/1 3B/3` - the panels park and the bar (7) rises for the
    /// member the cursor points at (`None` while it points at an enemy).
    ItemTarget(Option<u8>),
    /// `0x46`, step 7: `01/3 06/0 4E/0 4F/0 07/1 08/1 09/1 0B/1 0E/0 18/0
    /// 19/0 1A/3 1F/0 64/0 52/1` - panels up, bar parked, plate gone.
    SpellBrowse,
    /// `0x65..`, step `0x1B`: `01/3 06/1 4E/1 4F/1 07/0 18/1 19/1 1A/3 1F/1
    /// 64/1 29/0 32/0 3B/0` - panels park, bar up for the pointed member.
    SpellTarget(Option<u8>),
    /// A command-session phase with no window of its own (a confirmed
    /// command, the hand-offs, the escape roll). Retail's `0x6E` prompt
    /// (step `0x23`) carries the tab and the `Begin` / `Reselect` chips and
    /// no party surface.
    Other,
}

/// The surface owning this frame, or `None` outside
/// [`BattleHudPhase::CommandEntry`].
pub fn battle_command_surface(world: &legaia_engine_core::world::World) -> Option<CommandSurface> {
    use legaia_engine_battle::target_picker::{CursorRow, PickerState};
    use legaia_engine_battle_menus::battle_input::CommandPhase;
    if battle_hud_phase(world) != BattleHudPhase::CommandEntry {
        return None;
    }
    if world.mode == legaia_engine_mode::mode::SceneMode::MuscleDome {
        use legaia_engine_battle_menus::muscle_dome_session::DomeMenu;
        return Some(match world.minigames.muscle_dome.as_ref()?.menu() {
            DomeMenu::Command(c) => match c.phase {
                CommandPhase::Menu { .. } => CommandSurface::Ring,
                CommandPhase::AttackMode { .. } => CommandSurface::AttackMode,
                _ => CommandSurface::Other,
            },
            DomeMenu::Input(_) => CommandSurface::ArtsInput,
            DomeMenu::Magic => CommandSurface::SpellBrowse,
        });
    }
    if world.arts_input_active() {
        return Some(CommandSurface::ArtsInput);
    }
    if world.battle.arts_menu.is_some() {
        return Some(CommandSurface::ArtsList);
    }
    if let Some(m) = world.battle.spell_menu.as_ref() {
        return Some(match &m.phase {
            legaia_engine_battle::battle_magic::SpellPhase::Targeting { picker, .. } => {
                CommandSurface::SpellTarget(match picker.state() {
                    PickerState::Cursor {
                        row: CursorRow::Ally,
                        slot,
                    } => Some(slot),
                    _ => None,
                })
            }
            _ => CommandSurface::SpellBrowse,
        });
    }
    if let Some(menu) = world.battle.item_menu.as_ref() {
        let view = menu.menu_view();
        return Some(if view.target_select {
            CommandSurface::ItemTarget(
                menu.targets
                    .get(view.target_cursor)
                    .filter(|t| !t.is_enemy)
                    .map(|t| t.slot),
            )
        } else {
            CommandSurface::ItemBrowse
        });
    }
    let cmd = world.battle.command.as_ref()?;
    Some(match cmd.phase {
        CommandPhase::Menu { .. } => CommandSurface::Ring,
        CommandPhase::AttackMode { .. } => CommandSurface::AttackMode,
        CommandPhase::Targeting { .. } => CommandSurface::Targeting,
        _ => CommandSurface::Other,
    })
}

/// The party member entering a command while a command surface is up.
pub(super) fn command_entry_actor(world: &legaia_engine_core::world::World) -> Option<u8> {
    // The dome fields one fighter, seated at ordinal 0.
    if world.mode == legaia_engine_mode::mode::SceneMode::MuscleDome {
        return world.minigames.muscle_dome.as_ref().map(|_| 0);
    }
    if let Some(slot) = world.arts_input_actor() {
        return Some(slot);
    }
    if let Some(m) = world.battle.arts_menu.as_ref() {
        return Some(m.actor);
    }
    if let Some(m) = world.battle.spell_menu.as_ref() {
        return Some(m.actor);
    }
    if world.battle.item_menu.is_some() {
        return Some(world.battle_ctx.active_actor);
    }
    world.battle.command.as_ref().map(|c| c.actor)
}
