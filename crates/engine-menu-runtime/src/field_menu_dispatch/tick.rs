//! Per-frame stepping of the pause menu: the root list, the open sub-session, and the
//! handoff when a sub-session finishes.

use super::*;

// ---------------------------------------------------------------------------
// The pause-menu stack - the steps every host shares
// ---------------------------------------------------------------------------

/// Step the pause menu's **root list** one frame on a raw pad edge. Returns
/// the row the confirm suspended the list on, for the host to build that
/// row's sub-session ([`FieldMenuSubsession::build`]) with its own rack and
/// key table.
pub fn tick_root_list(
    menu: &mut legaia_engine_menus::field_menu::FieldMenuSession,
    edge: u16,
) -> Option<FieldMenuRow> {
    let _ = menu.tick(legaia_engine_menus::field_menu::FieldMenuInput::from_pad_edge(edge));
    match menu.phase() {
        legaia_engine_menus::field_menu::FieldMenuPhase::Suspended { row } => Some(row),
        _ => None,
    }
}

/// Step an open sub-session one frame on a raw pad edge (and the host's
/// latest key name, for the Options screen's Key Config screen). Returns the
/// rebound binding table when a rebind committed this frame, for the host
/// to adopt and persist.
///
/// Carries the engine extension every host offers: Triangle on the Status
/// screen swaps it for the Tactical Arts chain editor
/// ([`try_open_arts_editor`]), and the edge that did so drives nothing else.
/// A Load / Save sub-session's card flow (`SaveScreenFlow::before_tick`)
/// filters `edge` before this call; that half is the host's because the
/// card read is rack I/O.
pub fn tick_open_subsession(
    active: &mut FieldMenuSubsession,
    edge: u16,
    key_pressed: Option<&str>,
    world: &World,
) -> Option<Mapping> {
    if step_equip_character_picker(active, edge, world) {
        return None;
    }
    if !try_open_arts_editor(active, edge, world) {
        active.tick_pad_edge_with_key(edge, key_pressed);
    }
    active.take_rebound_mapping()
}

/// The Equip character picker's Up / Down: walk the present party
/// (`DAT_80084594` over `0x80084598`, [`World::present_party_list`]) and
/// rebuild the session on the hovered member, so the slot rows show that
/// member's equipment as retail's main window does. Returns `true` when the
/// edge was taken.
///
/// REF: FUN_801D98F0
fn step_equip_character_picker(active: &mut FieldMenuSubsession, edge: u16, world: &World) -> bool {
    let FieldMenuSubsession::Equip {
        session,
        char_slot,
        picking: true,
    } = active
    else {
        return false;
    };
    let up = edge & PadButton::Up.mask() != 0;
    let down = edge & PadButton::Down.mask() != 0;
    if !(up || down) {
        return false;
    }
    let list = world.present_party_list();
    let at = list.iter().position(|&s| s == *char_slot).unwrap_or(0);
    let next = if down {
        (at + 1).min(list.len().saturating_sub(1))
    } else {
        at.saturating_sub(1)
    };
    if let Some(&slot) = list.get(next)
        && slot != *char_slot
    {
        let mut rebuilt = build_equip_session(world, slot, &world.tables.equipment_table);
        rebuilt.set_slot_cursor_hidden(true);
        *session = rebuilt;
        *char_slot = slot;
    }
    true
}

/// What a finished sub-session leaves for its host once
/// [`finish_subsession`] has folded everything else into the world.
pub enum SubsessionHandoff {
    /// Folded into the world; nothing for the host to do.
    Applied,
    /// A Load / Save screen: the pick is committed against the host's save
    /// rack (the card image, the engine-save directory), which the engine
    /// does not own.
    Save(SaveSelectSession),
    /// The Options screen's closing state. Value edits commit inside its own
    /// popup (retail writes the config word at the popup's confirm and never
    /// reverts), so this is the player's; the host lifts and persists it.
    Options(OptionsState),
}

impl SubsessionHandoff {
    /// The host half of a finished sub-session when there is no world to
    /// fold the rest into - a menu opened from the title screen before any
    /// scene exists, whose only reachable screens are Load and Options.
    /// Every other row comes back [`Self::Applied`] with nothing applied.
    pub fn without_world(finished: FieldMenuSubsession) -> Self {
        match finished {
            FieldMenuSubsession::Save(s) => Self::Save(s),
            FieldMenuSubsession::Config(o) => Self::Options(o.state().clone()),
            _ => Self::Applied,
        }
    }
}

/// A finished sub-session after [`finish_subsession`].
pub struct FinishedSubsession {
    pub handoff: SubsessionHandoff,
    /// Window 7: a menu cast that leveled its spell, for
    /// [`crate::menu_runtime::MenuRuntime::arm_spell_level_notice`].
    pub spell_level_notice: Option<SpellLevelNotice>,
    /// Window 8: a Hyper-Art book taught an art, for
    /// [`crate::menu_runtime::MenuRuntime::arm_art_learned_notice`].
    pub art_learned_notice: Option<legaia_engine_menus::pause_screens::ArtLearnedNotice>,
    /// The sub-session left the menu an exit code (a Door of Light or Door of
    /// Wind use): the whole menu closes rather than resuming its root list,
    /// so the host passes `close` to `FieldMenuSession::resume`.
    ///
    /// Retail's menu SM (`FUN_801DC6B4`, jump table `0x801CEEB0` on
    /// `_DAT_8007B43C`) dispatches the screens only from state `2`; the Use
    /// screens store `4` / `5`, whose arm (`0x801DC9A4..0x801DC9E4`) waits
    /// for the brightness to reach `0xF2`, adds `3` and returns
    /// `counter >= 6` - the menu is over, and the field's session runs the
    /// travel art. Resumed instead, the menu stayed up while the art ran
    /// under it, and the party could walk off and open a cupboard before the
    /// lift.
    pub close_menu: bool,
}

/// Fold a finished pause-menu sub-session into the world - the one outcome
/// router every host calls when [`FieldMenuSubsession::is_done`] turns true,
/// before it resumes the root list.
///
/// Items run the full pause applier (bag decrements plus the escape / warp
/// hand-off) and surface a taught art's window-8 notice; Equip writes the
/// record back; Spells cast and surface window 7; Arts store the edited chain
/// library; the Status screen's reorder page replays its exchanges onto the
/// record. Save and Options come back as a [`SubsessionHandoff`], because
/// both end in host-owned storage. The native window, the headless
/// `BootSession` and the browser page each carried their own copy of this
/// match, and the headless one had dropped window 8.
pub fn finish_subsession(finished: FieldMenuSubsession, world: &mut World) -> FinishedSubsession {
    let mut out = FinishedSubsession {
        handoff: SubsessionHandoff::Applied,
        spell_level_notice: None,
        art_learned_notice: None,
        close_menu: false,
    };
    match finished {
        FieldMenuSubsession::Items(s) => {
            out.close_menu = apply_pause_items_outcome(&s, world).is_some();
            out.art_learned_notice = world.menu.pending_art_notice.take();
        }
        FieldMenuSubsession::Equip {
            session, char_slot, ..
        } => {
            let _ = apply_equip_outcome(&session, char_slot, world);
        }
        FieldMenuSubsession::Spells(s) => {
            out.spell_level_notice = apply_spell_outcome(&s, world);
        }
        FieldMenuSubsession::Arts(editor) => {
            // Lift the live library, apply the edit, store it back
            // (`World::chain_library` <-> `store_chain_library` over the
            // saved chains), so the next battle's Arts rows reflect it.
            let mut library = world.chain_library();
            if apply_arts_outcome(editor, &mut library).is_ok() {
                world.store_chain_library(&library);
            }
        }
        FieldMenuSubsession::ListOrder(s) => {
            // The page permuted its own copy; replay its exchanges onto the
            // live record through the ported swap.
            let _ = apply_list_order_outcome(&s, world);
        }
        FieldMenuSubsession::Status(_) => {}
        storage @ (FieldMenuSubsession::Save(_) | FieldMenuSubsession::Config(_)) => {
            out.handoff = SubsessionHandoff::without_world(storage);
        }
    }
    out
}
