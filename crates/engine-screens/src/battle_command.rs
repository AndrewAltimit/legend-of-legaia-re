//! The battle command menu's chip cluster for one frame, projected off the
//! world onto the leaf UI crate's types.

use legaia_engine_battle_hud::{battle_chip_highlight, battle_command_chips};
use legaia_engine_battle_menus::battle_input::CommandChipPhase;
use legaia_engine_core::world::World;
use legaia_engine_ui::battle_command_ui::ChipPhase;

/// The chip cluster a host draws for one frame: the owned `(label, enabled)`
/// chips in seat order, the highlighted chip (`battle_chip_highlight`:
/// `None` under the directional-buttons command entry), and the cluster's
/// phase mapped onto `engine-ui`'s own enum.
pub type CommandChips = (Vec<(String, bool)>, Option<usize>, ChipPhase);

/// The command menu's chips this frame, or `None` when no command menu is up.
/// Both play hosts draw the ring through it; each used to carry a copy.
pub fn battle_command_menu_chips(world: &World) -> Option<CommandChips> {
    let chips = battle_command_chips(world)?;
    // Two enums because `engine-ui` is a leaf that does not link
    // `engine-core`.
    let phase = match chips.phase {
        CommandChipPhase::RoundPrompt => ChipPhase::RoundPrompt,
        CommandChipPhase::CommandRing => ChipPhase::CommandRing,
        CommandChipPhase::AttackMode => ChipPhase::AttackMode,
        CommandChipPhase::CommitConfirm => ChipPhase::CommitConfirm,
    };
    let highlight = battle_chip_highlight(world, chips.cursor);
    Some((chips.chips, highlight, phase))
}
