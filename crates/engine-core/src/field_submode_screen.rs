//! Host for the field overlay's **op-`0x49` submode screens**: the missing
//! caller that walks the submode driver actor and invokes its `+0x50` handler.
//!
//! [`legaia_engine_field::submode_hub_actors`] carries the dispatcher `FUN_801F159C`,
//! its `PTR_FUN_801F33B4` state machines and the `PANEL_WINDOW_TABLE` painters.
//! None of them can run without three things this module supplies: an actor
//! with a `+0x50` handler id, the submode cursor context `DAT_801C6EA4` the
//! dispatcher polls, and a per-frame caller. The engine already had the first
//! ingredient and did not use it - [`World::man_load_actor_reset`] spawns an
//! [`ActorHandler::SubmodeDriver`] actor on every MAN load (retail's
//! `FUN_801D9C3C` at `0x8003B444`) and
//! [`legaia_engine_field::actor_handler::HandlerKernel`] classed it `Unported`, so the actor
//! sat in the pool doing nothing.
//!
//! ## Why this is load-bearing rather than decoration
//!
//! Field-VM opcode `0x49` (`STATE_RESUME`) is a **tristate park**: the script
//! arms `_DAT_8007B450` with its operand pointer and then re-enters the same PC
//! every frame until something writes `1` there. `docs/subsystems/script-vm.md`
//! names the writer: "the Done writer is field-overlay `FUN_801F159C`-class" -
//! that is [`hub_dispatch`]'s retire arm. The engine recognised exactly three
//! sub-ops (`0` inline shop, `3` name entry, `5` tile board) and had no Done
//! writer for the rest, so a script reaching any other sub-op re-armed and
//! halted on the same PC forever. Running the dispatcher supplies the writer.
//!
//! Slot `0` of `PTR_FUN_801F33B4` is `FUN_801F2134` (the close tick), and a
//! freshly spawned driver carries `+0x50 = 0`, so a sub-screen the engine
//! cannot draw yet still closes itself the retail way instead of parking.
//!
//! Two properties of that park are load-bearing and easy to lose:
//!
//! - **A driver per arm.** The dispatcher retires the driver when a screen
//!   hands back, and retail's Idle arm allocates a fresh one every time it
//!   arms - so [`World::open_field_submode_screen`] spawns one when none is
//!   live. Relying on the single MAN-load driver leaves every screen after
//!   the first with no dispatcher, hence Armed for the rest of the scene.
//! - **A park per context.** Retail's `_DAT_8007B450` is one global; the port
//!   steps several field-VM contexts inside one `World::tick` and resolves
//!   three sub-ops through host paths that bypass the global. The park is
//!   therefore tagged with its [`Op49ParkOwner`] and read only by that owner.
//!
//! REF: FUN_8002519C (the `jalr node[+0x0C]` walk this hangs off),
//! FUN_801D9C3C (the spawn), FUN_801E9B3C (the panel install this records
//! rather than performs)

use legaia_engine_dev_menu::dev_menu_leaves::{EquipProps, ItemProps};
use legaia_engine_field::submode_hub_actors::{
    self as hub, ACTOR_RETIRE, CoinCounter, HubAction, HubActor, HubDraw, HubEnv, HubEquipEnv,
    HubEquipRecord, HubFrame, HubGrid, HubPainter, slot,
};

use crate::world::World;
use legaia_engine_field::actor_handler::ActorHandler;
use legaia_engine_menus::equipment::{DiscEquipEntry, DiscEquipInfo, EquipSlot};

mod coin_counter;
mod hub_env;
mod op49_park;
mod panels;
#[cfg(test)]
mod tests;
mod tick;

pub use coin_counter::*;
pub use panels::*;

/// Which field-VM context armed the op-`0x49` park.
///
/// Retail's park is a single global (`_DAT_8007B450`) and every context shares
/// it. The port cannot: it runs the per-tick field script
/// ([`World::step_field`]), the per-actor channels, and the spawned
/// partition-2 record contexts (the modal cutscene timeline and its concurrent
/// helpers) inside the *same* `World::tick`, and it resolves three sub-ops
/// ([`OP49_DEDICATED_SUB_OPS`]) through dedicated host paths that bypass the
/// global entirely. A park armed by one context and read by another therefore
/// answers a question that was never asked - which is exactly how the town01
/// opening's name-entry hand-off gets swallowed: the field script arms a
/// screen, and the modal timeline's own op-`0x49` reads that screen's Armed
/// and halts before `op49_invoke_setup` can open name entry.
///
/// Tagging the park with its owner keeps every context's Done its own.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Op49ParkOwner {
    /// The per-tick field script / interaction + inline-dialogue runners -
    /// everything stepped outside a spawned record slice.
    #[default]
    FieldScript,
    /// The modal cutscene timeline (`in_cutscene_timeline`).
    CutsceneTimeline,
    /// A concurrent helper context (a spawned partition-2 record that is not
    /// the modal timeline).
    HelperContext,
}

/// The live state of the one submode screen a field frame can have up.
///
/// Mirrors the retail globals rather than inventing a shape: [`Self::actor`]
/// is the driver actor's `+0x0A..+0x54` view, [`Self::cursor`] is
/// `DAT_801C6EA4`, [`Self::counter`] the coin counter's own cells.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubmodeScreen {
    /// The driver actor's fields the dispatcher reads and writes.
    pub actor: HubActor,
    /// `DAT_801C6EA4` - the submode cursor context.
    pub cursor: HubGrid,
    /// The coin counter's digit cells, cursors and hold timer.
    pub counter: CoinCounter,
    /// Panel-window record index a **host** pins for this screen, independent
    /// of what the state machine installs. `None` is the retail shape - the
    /// descriptor is what names the window (see [`Self::installed_windows`]).
    pub window: Option<usize>,
    /// The panel-window records the last installed descriptor names, which is
    /// how retail decides which painter draws: a state machine calls
    /// `FUN_801E9B3C` with a descriptor, and the descriptor's entries carry
    /// the window indices ([`legaia_engine_field::submode_hub_actors::panel_windows`]).
    ///
    /// The engine previously took the window from the *caller* at open time
    /// and never read the installs at all, so the coin counter drew the
    /// two-option panel where retail draws the three-line one, and the
    /// sub-menu drew nothing.
    pub installed_windows: Vec<usize>,
    /// A screen is up: the op-`0x49` park is Armed.
    pub open: bool,
    /// The dispatcher retired the actor - retail's `_DAT_8007B450 = 1`, which
    /// is the op-`0x49` Done signal.
    pub done: bool,
    /// Whatever the last tick drew and did, for a renderer to consume.
    pub frame: HubFrame,
    /// `FUN_801E9DC8`'s return for the confirm panel, supplied by the host
    /// that owns the two-option picker. `0` while nothing is picking.
    pub picker_result: i32,
    /// Pad edges seen since the dispatcher last ran.
    ///
    /// The actor pool advances on the **game-tick** clock - every second vsync
    /// under the field cadence floor of `2` - while the engine's hosts publish
    /// a pad word every vsync, so an edge that lands on a skipped tick is
    /// gone by the time the dispatcher looks. The parity is stable, so this is
    /// not a flake: with the pad set once per `World::tick`, *every* press
    /// released on the next tick fell in the gap, and no directional input
    /// ever reached a submode screen. A digit could not be entered on the pad
    /// at all.
    ///
    /// Retail does not have the gap because it samples the pad **once per game
    /// tick** (the master driver `FUN_80016444` runs one pass per
    /// `DAT_1F800393` vsyncs and the pad pump is inside it), so its edge word
    /// `_DAT_8007B874` is produced at exactly the rate the pool consumes it.
    /// The latch restores that relationship from the other side: it unions the
    /// per-vsync edges and hands the union to the pass that runs.
    pub pad_edge_latch: u32,
    /// The three bytes at `_DAT_8007B450 + 1..=3` - the op-`0x49` operand's
    /// payload, which is what the start menu counts to size its panel.
    ///
    /// Retail parks the operand *pointer* and the start menu dereferences it
    /// (`lw a0,-0x4bb0(v0)` then `lbu v0,0x1(a0)` .. `0x3(a0)` at
    /// `0x801F1168..0x801F11A4`); the port has no pointer, so the arm edge
    /// copies the three bytes off the instruction itself.
    pub board_entries: [u8; 3],
    /// The field-VM context that armed this park - see [`Op49ParkOwner`].
    /// Only that context's op-`0x49` reads [`Self::open`] / [`Self::done`].
    pub owner: Op49ParkOwner,
    /// The armed park's **kind byte**: retail's `*_DAT_8007B450`.
    ///
    /// Retail stores the op-`0x49` *operand pointer* in the park
    /// (`sw s6,-0x4bb0(s0)` at `0x801e09a8`) and every consumer
    /// dereferences its first byte, which is the sub-op the arm read one
    /// instruction earlier (`lbu v0,0x0(s6)` at `0x801e0984`). The port has
    /// no RAM pointer, so it keeps the byte itself - the whole of the
    /// pointer any consumer actually uses.
    ///
    /// `None` is retail's null park (nothing armed). It is set for **every**
    /// sub-op, including the three the port resolves through dedicated host
    /// paths ([`OP49_DEDICATED_SUB_OPS`]), because retail's single global is
    /// written before the port's paths diverge.
    ///
    /// Cleared on resume, which retail also does: the Done arm reads the
    /// sentinel and zeroes the slot before advancing.
    ///
    /// ```text
    /// 801e08c8  lw   v0,-0x4bb0(s0)     ; the park
    /// 801e08d0  bne  v0,s1,0x801e097c   ; s1 = 1, the Done sentinel
    /// 801e08d8  sw   zero,-0x4bb0(s0)   ; resume clears it
    /// ```
    ///
    /// REF: FUN_801DE840 (op `0x49` arm + Done arms)
    pub park_sub_op: Option<u8>,
    /// `_DAT_8007BB9C` - the selected menu-list row's **class nibble**, which
    /// the entry list's equipment sub-panel reads to decide where its
    /// comparison candidate comes from (see [`World::set_hub_equip_mode`]).
    ///
    /// Retail's own value is a menu-list global, not hub state: the list
    /// machinery publishes the highlighted row's class there
    /// ([`legaia_engine_menus::menu_list_rows`]) and the sub-panel reads it. `0` is the
    /// no-candidate arm, which is what a hub screen opened without a list up
    /// sees.
    pub equip_mode: u32,
    /// The `+6` character mask and `+7` slot byte of the equipment table
    /// `DAT_80074F68`, per item id - the two bytes of that row the engine's
    /// world-resident tables do not carry (see
    /// [`World::install_hub_equip_restrictions`]). Empty until a host installs
    /// them, which also holds [`Self::equip_mode`] at the no-candidate arm.
    pub equip_restrictions: std::collections::BTreeMap<u8, (u8, u8)>,
    /// `_DAT_8007B450[0..7]` - the parked op-`0x49` operand from the sub-op
    /// byte on, for the slots that read past [`Self::board_entries`] (slot
    /// `0x23` reads its base flag at `+4..+6`).
    pub op49_operand: [u8; 7],
    /// Slot `0x23`'s picker state ([`crate::field_submode_flag_window`]).
    pub flag_window: legaia_engine_field::field_submode_flag_window::FlagWindowState,
    /// Slot `0x21`'s lock state ([`crate::field_submode_code_lock`]).
    pub code_lock: legaia_engine_field::code_lock_actor::CodeLockActor,
    /// The coin counter's entry panel this frame - the painter of record
    /// [`legaia_engine_minigames::slot_machine::COIN_ENTRY_WINDOW`], which lives in
    /// `slot_machine` rather than beside the other painters because it reads
    /// the counter's cells. Empty while that record is not installed.
    pub coin_panel: Vec<legaia_engine_minigames::slot_machine::CoinPanelDraw>,
    /// A host has opened the pause menu for the current scripted
    /// menu-button press ([`op49_sub_op_is_menu_press`]), so the request
    /// is spent until the park is re-armed. See
    /// [`World::scripted_menu_open_pending`].
    pub scripted_menu_opened: bool,
    /// Slot `0x33`'s return-to-title state ([`SoftResetScreen`]).
    pub soft_reset: SoftResetScreen,
}

/// Handler slot op `49 0C` installs: `FUN_801EDF00`, the return-to-title soft
/// reset (`OP49_SUBOP_SLOTS[0x0C]`).
pub const SOFT_RESET_SLOT: u16 = 0x33;

/// Retail pen x of the records screen the soft reset draws:
/// `FUN_801ED710(0x20, _DAT_801F35B8)` (`li a0,0x20` at `0x801EDF48` /
/// `0x801EE004`).
pub const SOFT_RESET_RECORDS_X: i32 = 0x20;

/// Slot `0x33` - the screen `edlast`'s credits end on (`49 0C` after the
/// press-to-continue poll). It slides the play records in, waits for a face
/// button, fades to white and reloads the executable, so it never hands the
/// frame back: the op stays parked until the title takes over.
///
/// The phase machine is `legaia_engine_field::world_map_panel_actors::soft_reset_tick`
/// (`FUN_801EDF00`); this is the per-screen state it runs over.
// REF: FUN_801EDF00 (soft_reset_tick), FUN_801ED710 (the records screen),
// FUN_801D58F0 (the white fade), FUN_80017714 (the executable reload)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SoftResetScreen {
    /// The actor's `+0x54` phase.
    pub phase: i16,
    /// `_DAT_801F35B8` - the records slide, then the reload counter.
    pub slide: i32,
    /// The records screen's y this frame (`FUN_801ED710`'s `a1`), `None`
    /// before the first draw.
    pub records_y: Option<i32>,
    /// The reload was reached and the title hand-off raised.
    pub reloaded: bool,
}

impl SubmodeScreen {
    /// Is a screen up (the op-`0x49` park should read Armed)?
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Did the last tick hand the frame back (the park should read Done)?
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Is a screen up *and* armed by `owner` (the park this context reads)?
    pub fn is_open_for(&self, owner: Op49ParkOwner) -> bool {
        self.open && self.owner == owner
    }

    /// Did a screen armed by `owner` hand the frame back?
    pub fn is_done_for(&self, owner: Op49ParkOwner) -> bool {
        self.done && self.owner == owner
    }

    /// Every text/sprite draw the last tick produced.
    pub fn draws(&self) -> &[HubDraw] {
        &self.frame.draws
    }
}
