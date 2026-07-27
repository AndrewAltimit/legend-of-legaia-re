//! Host for the world-map band's **panel windows and panel actors**.
//!
//! [`legaia_engine_vm::world_map_panel`] ports the three shared leaves of the
//! field overlay's world-map band - the panel command-script interpreter
//! (`FUN_801E9B3C`), the shared list cursor (`FUN_801E9DC8`) and the dev-menu
//! row-action dispatcher (`FUN_801EA9B0`) - and
//! [`legaia_engine_vm::world_map_panel_actors`] ports the six `ctx[+0x54]`
//! phase machines that sit on top of them. Both were hostless: they modelled a
//! screen the engine did not have. This module is that screen's simulation
//! half, hung off [`crate::world_map::WorldMapController`] and driven once a
//! frame by `World::tick_world_map`.
//!
//! # What is retail here and what is the port's
//!
//! **Retail** - every phase machine, every effect ordering, the window-script
//! opcode set, the cursor kernel, the panel geometry arithmetic, and the
//! record fields the restore arm writes. None of that is re-derived here; this
//! module only supplies inputs and applies outputs.
//!
//! **The port's** - four things retail keeps in overlay data, in a dispatcher
//! this crate does not own, or outside the actor entirely:
//!
//! 1. **The panel scripts.** Retail's window scripts live at overlay VAs
//!    (`0x801F3274`, `0x801F3284`, `0x801F32B4`, `0x801F32DC`, `0x801F2A88`,
//!    `0x801F3304`) that the engine never loads. [`PanelScripts::default`]
//!    ships a minimal stand-in keyed by the same VAs, so the interpreter runs
//!    on real records; a host with the overlay image can replace the table.
//!    This is the same seam `crate::dev_menu_host::DevMenuSession::flag_tags`
//!    has for the debug flag table.
//! 2. **The handler-id table.** Retail retires an actor by writing a new
//!    handler id into `ctx[+0x50]`, which `FUN_801F159C` turns back into a
//!    function pointer through the 52-entry `PTR_FUN_801F33B4`. The
//!    dispatcher itself is ported
//!    ([`legaia_engine_vm::baka_hub_actors::hub_dispatch`]) but takes the
//!    resolved handler as a closure, and only seven of the table's slots are
//!    read out ([`legaia_engine_vm::baka_hub_actors::slot`]) - `0x1A`, which
//!    the sub-list / text-box / flag-window exits hand back to, and not the
//!    `0x29` / `0x2B` the fade/flash exits pick. So [`ActorExit::apply`] makes
//!    all four stores here and [`PanelActorHost::retire`] drops the actor,
//!    recording the pair in [`PanelFrame::exits`] rather than following it.
//! 3. **Which pad chord installs which actor.** Retail reaches these from
//!    debug branches in the world-map controller. The engine's bindings live
//!    on `World::tick_world_map_panels` and are tabulated there.
//! 4. **Entry phase and dismissal.** Several of these machines park rather
//!    than exit, and retail releases them from outside: the scene manager
//!    watching `scene[+0x3E]`, whoever armed the flash counter, the executable
//!    reload. [`PanelActorKind::entry_phase`] picks the arm the host wants to
//!    run and [`PanelActorHost::dismiss`] is the escape hatch, without which
//!    the first parking arm wedges the screen.
//!
//! # The pad layout trap
//!
//! Every kernel in this band reads the **packed** pad words
//! (`_DAT_8007BB84` newly-pressed, `_DAT_8007B874` held) that `FUN_8001822C`
//! builds, not the raw BIOS layout [`crate::input::PadButton`] carries. The
//! two hold the same 16 buttons with the byte halves swapped, so feeding a raw
//! word straight in cross-wires the whole screen: raw Cross (`0x4000`) arrives
//! as packed D-pad Down, and raw Up (`0x0010`) arrives as nothing at all.
//! [`packed_pad`] does the conversion and every entry point here takes packed
//! words only.

use legaia_engine_vm::travel_art_actor::{
    TravelArt, TravelArtActor, TravelDestination, destination_for, find_visited_map,
};
use legaia_engine_vm::world_map_panel::{
    CursorPad, PanelCommand, PanelDescriptor, PanelEffect, run_panel_script,
};
use legaia_engine_vm::world_map_panel_actors::{
    ActorExit, FadeFlashEffect, FadeFlashInput, FillFadeEffect, FillFadeInput,
    FlagWindowDescriptor, FlagWindowEffect, FlagWindowInput, HudDecision, HudInput, SubListEffect,
    SubListInput, TextBoxEffect, TextBoxInput, fade_flash_tick, field_hud_tick, fill_fade_tick,
    flag_window_tick, soft_reset_tick, sub_list_tick, text_box_tick,
};
use std::collections::HashMap;

/// Convert a raw BIOS pad word into the packed layout every kernel in this
/// module reads.
///
/// The two words hold the same 16 buttons with the byte halves swapped
/// (`FUN_8001822C` builds `~((b2 << 8) | b3)`), so the conversion is a byte
/// swap. Two fixed points pin the direction: Cross is `0x4000` raw and `0x40`
/// packed, Up is `0x0010` raw and `0x1000` packed.
pub fn packed_pad(raw: u16) -> u16 {
    raw.swap_bytes()
}

/// Packed-pad mask for action button A (`_DAT_800846D0`), the confirm the
/// shared list cursor tests against the **held** word.
pub const PANEL_ACTION_A: u32 = 0x0040;
/// Packed-pad mask for action button B (`_DAT_800846D4`), the cancel.
pub const PANEL_ACTION_B: u32 = 0x0020;

/// Panel-descriptor slots the host allocates.
///
/// Retail's array is overlay data at `0x801F2B98`; the engine sizes it to
/// cover every index the ported arms address - the flag window's `14`
/// ([`legaia_engine_vm::world_map_panel_actors::FLAG_WINDOW_PANEL_INDEX`])
/// and the party panel's `7`
/// ([`legaia_engine_vm::world_map_panel::PARTY_PANEL_INDEX`]).
pub const PANEL_SLOTS: usize = 16;

/// Panel index the port's stand-in sub-list script opens.
pub const SUBLIST_PANEL_INDEX: i16 = 3;

/// Value the host arms the flash counter `_DAT_8007B43C` with to release a
/// parked brightness flash.
///
/// `FUN_801ED308` parks at phase 3 until an external writer raises the counter
/// to at least [`legaia_engine_vm::world_map_panel_actors::FLASH_COUNTER_RESTORE`];
/// the ramp-down arm then returns to phase `counter - 1`, so `7` is the value
/// that lands on the phase-6 terminal arm instead of the phase-5 dead end.
pub const FLASH_RELEASE_COUNTER: i32 = 7;

/// A live window object - retail's per-panel actor, allocated by the window
/// spawner the `op 1` / `op 2` arms call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PanelWindow {
    /// `obj[+0x0A]` - the live x the respawn arm reads back.
    pub x: i16,
    /// `obj[+0x0C]` - the live y.
    pub y: i16,
    /// `obj[+0x1D]` - the style byte the `op 3` arm writes.
    pub style: u8,
    /// `obj[+0x20]` - the counter halfword the `op 6` arm zeroes.
    pub counter: i16,
    /// Set by the `op 9` slide arm; a renderer eases toward `(x, y)`.
    pub sliding: bool,
}

/// The panel-script table, keyed by the retail overlay VA the actors name.
///
/// The default table is the **port's** stand-in, not retail's bytes - see the
/// module docs. Replacing an entry replaces what that script does; an unknown
/// VA runs nothing.
#[derive(Debug, Clone, Default)]
pub struct PanelScripts {
    by_va: HashMap<u32, Vec<PanelCommand>>,
}

impl PanelScripts {
    /// A table with no scripts at all. Every `RunPanelScript` becomes a no-op.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Install (or replace) the script at a retail VA.
    pub fn set(&mut self, va: u32, script: Vec<PanelCommand>) {
        self.by_va.insert(va, script);
    }

    /// The script at a VA, or an empty slice.
    pub fn get(&self, va: u32) -> &[PanelCommand] {
        self.by_va.get(&va).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// The port's minimal stand-in table.
    ///
    /// Each script is the smallest command sequence that makes the actor above
    /// it observable: the sub-list opens its panel and resizes the party panel
    /// (which is what exercises
    /// [`legaia_engine_vm::world_map_panel::party_panel_geometry`]), the close
    /// script closes it, the fill-fade script clears the screen, and the
    /// flag-window script opens descriptor `14` at the geometry the picker
    /// just wrote into it.
    pub fn stand_in() -> Self {
        use legaia_engine_vm::world_map_panel_actors as actors;
        let mut s = Self::default();
        let cmd = |op: u16, panel: i16, operand: u32| PanelCommand { op, panel, operand };
        // Open at the descriptor position, then run the party-panel resize.
        s.set(
            actors::SUBLIST_OPEN_SCRIPT,
            vec![
                cmd(1, SUBLIST_PANEL_INDEX, 0),
                cmd(12, SUBLIST_PANEL_INDEX, 0),
                cmd(0, 0, 0),
            ],
        );
        s.set(
            actors::SUBLIST_CLOSE_SCRIPT,
            vec![cmd(4, SUBLIST_PANEL_INDEX, 0), cmd(0, 0, 0)],
        );
        s.set(actors::FILL_FADE_SCRIPT, vec![cmd(5, 0, 0), cmd(0, 0, 0)]);
        s.set(
            actors::TEXT_BOX_CONFIRM_SCRIPT,
            vec![cmd(3, SUBLIST_PANEL_INDEX, 1), cmd(0, 0, 0)],
        );
        s.set(
            actors::TEXT_BOX_DECLINE_SCRIPT,
            vec![cmd(4, SUBLIST_PANEL_INDEX, 0), cmd(0, 0, 0)],
        );
        s.set(
            actors::FLAG_WINDOW_SCRIPT,
            vec![
                cmd(1, actors::FLAG_WINDOW_PANEL_INDEX as i16, 0),
                cmd(0, 0, 0),
            ],
        );
        s
    }
}

/// The `0x801F2B98` descriptor array plus the live window objects the script
/// interpreter opens and closes.
#[derive(Debug, Clone)]
pub struct PanelWindowHost {
    /// The descriptor array. Retail's is overlay data; the two computed arms
    /// ([`FlagWindowEffect::SizePanel`] and [`PanelEffect::PartyPanel`]) write
    /// their own entries, so those two are real rather than seeded.
    pub descriptors: Vec<PanelDescriptor>,
    /// Live window objects, one slot per descriptor.
    pub windows: Vec<Option<PanelWindow>>,
    /// Live party size (`0x80084594`), the `op 12` arm's input.
    pub party_members: u8,
    /// The script table.
    pub scripts: PanelScripts,
}

impl Default for PanelWindowHost {
    fn default() -> Self {
        Self {
            descriptors: vec![PanelDescriptor::default(); PANEL_SLOTS],
            windows: vec![None; PANEL_SLOTS],
            party_members: 1,
            scripts: PanelScripts::stand_in(),
        }
    }
}

impl PanelWindowHost {
    /// A host with the port's stand-in script table.
    pub fn new() -> Self {
        Self::default()
    }

    fn slot(panel: i16) -> Option<usize> {
        usize::try_from(panel).ok().filter(|i| *i < PANEL_SLOTS)
    }

    /// The descriptor for a panel index, or the zero descriptor.
    pub fn descriptor(&self, panel: i16) -> PanelDescriptor {
        Self::slot(panel)
            .and_then(|i| self.descriptors.get(i).copied())
            .unwrap_or_default()
    }

    /// Whether a panel currently has a live window object.
    pub fn is_open(&self, panel: i16) -> bool {
        Self::slot(panel).is_some_and(|i| self.windows[i].is_some())
    }

    /// How many panels are open.
    pub fn open_count(&self) -> usize {
        self.windows.iter().filter(|w| w.is_some()).count()
    }

    /// Run the script installed at `va` and apply every effect it decodes.
    ///
    /// The decode is [`run_panel_script`]; the application is the host's, and
    /// it is the only place window objects are created or destroyed.
    pub fn run_script(&mut self, va: u32) -> Vec<PanelEffect> {
        let script = self.scripts.get(va).to_vec();
        if script.is_empty() {
            return Vec::new();
        }
        // Snapshot the descriptors so the decode's `descriptor_of` closure can
        // borrow while `self` is still needed mutably for the apply pass.
        let descs = self.descriptors.clone();
        let members = self.party_members;
        let effects = run_panel_script(&script, members, |p| {
            Self::slot(p)
                .and_then(|i| descs.get(i).copied())
                .unwrap_or_default()
        });
        for e in &effects {
            self.apply(*e);
        }
        effects
    }

    fn apply(&mut self, effect: PanelEffect) {
        match effect {
            PanelEffect::OpenAtDescriptor { panel, x, y } | PanelEffect::OpenAt { panel, x, y } => {
                if let Some(i) = Self::slot(panel) {
                    let w = self.windows[i].get_or_insert_with(PanelWindow::default);
                    w.x = x;
                    w.y = y;
                    w.sliding = false;
                }
            }
            PanelEffect::SetStyleByte { panel, value } => {
                if let Some(w) = Self::slot(panel).and_then(|i| self.windows[i].as_mut()) {
                    w.style = value;
                }
            }
            PanelEffect::Close { panel } | PanelEffect::Retire { panel } => {
                if let Some(i) = Self::slot(panel) {
                    self.windows[i] = None;
                }
            }
            PanelEffect::CloseAll => {
                for w in self.windows.iter_mut() {
                    *w = None;
                }
            }
            PanelEffect::ClearCounter { panel } => {
                if let Some(w) = Self::slot(panel).and_then(|i| self.windows[i].as_mut()) {
                    w.counter = 0;
                }
            }
            PanelEffect::SlideTo { panel, x, y } => {
                if let Some(i) = Self::slot(panel) {
                    let w = self.windows[i].get_or_insert_with(PanelWindow::default);
                    w.x = x;
                    w.y = y;
                    w.sliding = true;
                }
            }
            PanelEffect::Respawn { panel } => {
                // Retail retires and respawns, sliding the fresh object back to
                // where the live one already was - so the observable result is
                // the object's own position re-applied as a slide.
                if let Some(w) = Self::slot(panel).and_then(|i| self.windows[i].as_mut()) {
                    w.sliding = true;
                }
            }
            PanelEffect::PartyPanel { panel, geometry } => {
                if let Some(i) = Self::slot(panel) {
                    self.descriptors[i] = geometry;
                }
            }
            PanelEffect::Nop => {}
        }
    }
}

/// Which panel actor is installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelActorKind {
    /// `FUN_801ED308` - screen brightness fade / flash.
    FadeFlash,
    /// `FUN_801ED590` - the sub-list open / close picker.
    SubList,
    /// `FUN_801EDF00` - the return-to-title soft reset.
    SoftReset,
    /// `FUN_801EE5D4` - the screen-fill fade transition.
    FillFade,
    /// `FUN_801EE90C` - the yes/no text-box dispatcher.
    TextBox,
    /// `FUN_801EF014` - the story-flag window picker.
    FlagWindow,
    /// `FUN_801EE094` / `FUN_801EE328` - a travel art.
    TravelArt(TravelArt),
}

impl PanelActorKind {
    /// The phase [`PanelActorHost::install`] seeds this actor at.
    ///
    /// Everything starts at `0` except the text box. `FUN_801EE90C`'s phase 0
    /// is not its prompt - it jumps straight to the fill-fade block at phase
    /// 10, which walks 11..13 and parks at **14**, an arm that only clears
    /// `scene[+0x3E]` and has no exit. That is retail: phase 0 is the arrival
    /// path for a caller that installs the actor *mid*-transition, and the
    /// release comes from the scene manager watching `scene[+0x3E]`, which
    /// this engine does not model. Seeding the prompt phase directly is what
    /// makes the yes/no box the thing the actor runs.
    pub fn entry_phase(self) -> i16 {
        match self {
            PanelActorKind::TextBox => 1,
            _ => 0,
        }
    }
}

/// A world map the party has stood on, and the tile they left it at.
///
/// Retail keeps these as `0x10`-byte records in the buffer `FUN_80019788`
/// returns, keyed by the map name at `+0xC`; the travel-art resolve phase
/// scans them for the party's current map and warps to the stored tile. The
/// engine keeps the same `(map id, tile)` pair, recorded by the world-map tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisitedMap {
    /// The map id the scan compares against `0x80084628`.
    pub map_id: u32,
    /// Stored tile X (`0x80084624`).
    pub tile_x: i32,
    /// Stored tile Z (`0x8008462C`).
    pub tile_z: i32,
}

/// Story-flag access the flag-window picker needs (`FUN_8003CE64` test,
/// `FUN_8003CE08` set, `FUN_8003CE34` range clear).
pub trait PanelFlagStore {
    fn flag_test(&self, id: i32) -> bool;
    fn flag_set(&mut self, id: i32);
    fn flag_clear(&mut self, id: i32);
}

/// What one host frame produced, for the caller to apply and a renderer to
/// draw.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PanelFrame {
    /// SFX cues raised this frame, in order.
    pub sfx: Vec<u32>,
    /// Every [`ActorExit`] a terminal arm asked for. Recorded, not dispatched -
    /// see the module docs.
    pub exits: Vec<ActorExit>,
    /// Brightness level the fade/flash actor wants pushed to the display.
    pub brightness: Option<i32>,
    /// The records screen's slide y, on frames the soft-reset actor draws it.
    pub records_y: Option<i32>,
    /// The soft-reset actor asked for the white fade, in frames.
    pub white_fade: Option<i32>,
    /// The soft-reset actor reached its executable reload. The engine records
    /// it and does not act on it.
    pub reload_executable: bool,
    /// The text-box confirm arm asked for the party HP/MP restore.
    pub restore_party: bool,
    /// Story flags the picker set / cleared this frame.
    pub flags_set: Vec<i32>,
    /// The travel art resolved a destination; the caller warps the player.
    pub warp: Option<TravelDestination>,
    /// The travel-art scan missed and the actor parked in its diagnostic phase.
    pub travel_unfound: bool,
    /// The actor retired this frame.
    pub retired: bool,
    /// The sub-list took its state-3 hand-off (row 1 confirm).
    pub hand_off: bool,
}

/// The panel-actor screen: one installed actor, the window system under it,
/// and the globals its phases read.
#[derive(Debug, Clone)]
pub struct PanelActorHost {
    /// The window system.
    pub windows: PanelWindowHost,
    /// The installed actor, if any.
    pub kind: Option<PanelActorKind>,
    /// `ctx[+0x54]` - the phase halfword.
    pub phase: i16,
    /// `ctx[+0x9E]` - the per-actor frame counter.
    pub timer: i16,
    /// `ctx[+0x50]` - the handler id the terminal arms save.
    pub handler_id: u16,
    /// `_DAT_8007B440` - the brightness accumulator.
    pub brightness: i32,
    /// `_DAT_8007B43C` - the flash counter.
    pub flash_counter: i32,
    /// `_DAT_8007B910` - the live **audio** level the sub-list halves and
    /// doubles. Seeded `0xD7` beside its persistent reference `_DAT_8008457C`
    /// by the cold reset `FUN_8001FFA4`, and halved into libsnd's `0..0x7F`
    /// range by every reader. Not screen brightness - that is `brightness`
    /// above.
    pub audio_level: i32,
    /// `_DAT_8007BB88` - the shared list cursor / picker selection.
    pub cursor: i32,
    /// `_DAT_8007BB9C` - the row the flag scan remembered on entry.
    pub remembered_row: i32,
    /// `_DAT_8007BB80` - the global input lock.
    pub input_locked: bool,
    /// `_DAT_801F35B8` - the records-screen slide counter.
    pub slide: i32,
    /// `_DAT_8007B450` - the op-`0x49` descriptor the flag window reads.
    pub flag_desc: FlagWindowDescriptor,
    /// `FUN_8003CF04` - true while a staged load is still running.
    pub load_pending: bool,
    /// The live tint triple `0x8007BF5D..5F` and its saved copy.
    pub tint: [u8; 3],
    /// The saved tint at `0x8007B634..636`.
    pub saved_tint: [u8; 3],
    /// `scene[+0x2E]` - the hand-back sentinel every [`ActorExit`] clears.
    /// Distinct from [`Self::scene_field_3e`]; the exit arms write this one
    /// and the `ClearSceneField3E` arms write that one.
    pub scene_field_2e: i16,
    /// `scene[+0x40]` - where an [`ActorExit`] parks the retiring actor's old
    /// handler id, for the dispatcher that would pick it back up.
    pub scene_field_40: u16,
    /// `scene[+0x3E]`, zeroed by two of the actors' own `case 5` arms.
    pub scene_field_3e: i16,
    /// `scene_obj[+0x10]`, whose bit `0x0008_0000` the fill-fade sets.
    pub scene_obj_flags: u32,
    /// How many times `FUN_80031D00` (the text-actor list tick) was asked for.
    pub text_actor_ticks: u64,
    /// The travel-art actor, when one is installed.
    pub travel: Option<TravelArtActor>,
    /// The visited-map table the travel art scans.
    pub visited: Vec<VisitedMap>,
    /// The party HUD's idle countdown (`_DAT_801F348C`).
    pub hud_timer: i16,
    /// The HUD's cached player position (`_DAT_801F3488` / `_DAT_801F348A`).
    pub hud_cached_pos: Option<(i16, i16)>,
    /// The last party-HUD decision, for a renderer.
    pub hud: Option<HudDecision>,
}

impl Default for PanelActorHost {
    fn default() -> Self {
        Self {
            windows: PanelWindowHost::new(),
            kind: None,
            phase: 0,
            timer: 0,
            handler_id: 0,
            brightness: 0,
            flash_counter: 0,
            audio_level: 0xD7,
            cursor: 0,
            remembered_row: 0,
            input_locked: false,
            slide: 0,
            flag_desc: FlagWindowDescriptor {
                count: 16,
                first_visible: 0,
                rows: 8,
                base_flag: 0,
            },
            load_pending: false,
            tint: [0; 3],
            saved_tint: [0; 3],
            scene_field_2e: 0,
            scene_field_40: 0,
            scene_field_3e: 0,
            scene_obj_flags: 0,
            text_actor_ticks: 0,
            travel: None,
            visited: Vec::new(),
            hud_timer: 0,
            hud_cached_pos: None,
            hud: None,
        }
    }
}

impl PanelActorHost {
    /// A fresh host with no actor installed.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether an actor currently owns the frame.
    pub fn is_active(&self) -> bool {
        self.kind.is_some()
    }

    /// Install an actor at phase 0. Replaces whatever was running.
    ///
    /// `handler_id` is what the terminal arms save into `scene[+0x40]`; hosts
    /// that do not model the retail handler table can pass anything.
    pub fn install(&mut self, kind: PanelActorKind, handler_id: u16) {
        self.kind = Some(kind);
        self.phase = kind.entry_phase();
        self.timer = 0;
        self.handler_id = handler_id;
        self.cursor = 0;
        if let PanelActorKind::TravelArt(art) = kind {
            self.travel = Some(TravelArtActor::new(art));
        } else {
            self.travel = None;
        }
    }

    /// Force the installed actor off the screen.
    ///
    /// The **port's** escape hatch, not retail's. Several of these machines
    /// have arms that park rather than exit - the text box's phase 14, the
    /// fade/flash's phase 3, the soft reset's phase 3 - because retail's
    /// release comes from the scene manager, which the engine does not model,
    /// or from a `PTR_FUN_801F33B4` slot the table read does not reach (see
    /// the module docs). Without this a parked actor would wedge the screen
    /// for the rest of the session.
    ///
    /// Returns whether anything was dismissed.
    pub fn dismiss(&mut self) -> bool {
        if self.kind.is_none() {
            return false;
        }
        self.kind = None;
        self.travel = None;
        self.phase = 0;
        self.timer = 0;
        true
    }

    /// Release a brightness flash parked at phase 3 (see
    /// [`FLASH_RELEASE_COUNTER`]). No-op unless the fade/flash actor is up.
    pub fn release_flash(&mut self) {
        if self.kind == Some(PanelActorKind::FadeFlash) {
            self.flash_counter = FLASH_RELEASE_COUNTER;
        }
    }

    /// Record that the party is standing on `map_id` at `(tile_x, tile_z)`.
    ///
    /// One record per map id - a revisit updates the stored tile, which is
    /// what makes the travel art return the party where it left.
    pub fn note_visit(&mut self, map_id: u32, tile_x: i32, tile_z: i32) {
        match self.visited.iter_mut().find(|v| v.map_id == map_id) {
            Some(v) => {
                v.tile_x = tile_x;
                v.tile_z = tile_z;
            }
            None => self.visited.push(VisitedMap {
                map_id,
                tile_x,
                tile_z,
            }),
        }
    }

    /// Retire the installed actor on a terminal arm's [`ActorExit`].
    ///
    /// The four stores are retail's and are made by [`ActorExit::apply`] - the
    /// `scene[+0x2E]` sentinel, the old handler id parked in `scene[+0x40]`,
    /// the new id into `ctx[+0x50]` and the phase reset. What is the port's is
    /// only what happens *after*: retail leaves the actor installed for the
    /// handler dispatcher to pick back up off the new id, and this host drops
    /// it instead, recording the pair in [`PanelFrame::exits`].
    fn retire(&mut self, frame: &mut PanelFrame, exit: ActorExit) {
        frame.exits.push(exit);
        frame.retired = true;
        exit.apply(
            &mut self.scene_field_2e,
            &mut self.scene_field_40,
            &mut self.handler_id,
            &mut self.phase,
        );
        self.kind = None;
        self.travel = None;
        self.timer = 0;
    }

    fn cursor_pad(pad_held: u16, pad_edge: u16) -> CursorPad {
        CursorPad {
            held: u32::from(pad_held),
            pressed: u32::from(pad_edge),
            action_a_mask: PANEL_ACTION_A,
            action_b_mask: PANEL_ACTION_B,
        }
    }

    /// Advance the installed actor one frame.
    ///
    /// `pad_edge` / `pad_held` are the **packed** words ([`packed_pad`]);
    /// `frame_delta` is the adaptive frame-delta byte `_DAT_1F800393`;
    /// `flags` is the story-flag bank the picker reads and writes.
    ///
    /// Returns what the frame produced. An idle host returns the default.
    pub fn tick(
        &mut self,
        pad_edge: u16,
        pad_held: u16,
        frame_delta: u8,
        flags: &mut dyn PanelFlagStore,
    ) -> PanelFrame {
        let mut frame = PanelFrame::default();
        let Some(kind) = self.kind else {
            return frame;
        };
        let pad = Self::cursor_pad(pad_held, pad_edge);
        let delta = i32::from(frame_delta);
        match kind {
            PanelActorKind::FadeFlash => self.tick_fade_flash(delta, &mut frame),
            PanelActorKind::SubList => self.tick_sub_list(pad, &mut frame),
            PanelActorKind::SoftReset => self.tick_soft_reset(delta, pad_held, &mut frame),
            PanelActorKind::FillFade => self.tick_fill_fade(frame_delta, &mut frame),
            PanelActorKind::TextBox => self.tick_text_box(pad, pad_edge, frame_delta, &mut frame),
            PanelActorKind::FlagWindow => self.tick_flag_window(pad, flags, &mut frame),
            PanelActorKind::TravelArt(_) => self.tick_travel_art(frame_delta, &mut frame),
        }
        frame
    }

    fn tick_fade_flash(&mut self, frame_delta: i32, frame: &mut PanelFrame) {
        let (phase, level, counter, out) = fade_flash_tick(
            self.phase,
            FadeFlashInput {
                frame_delta,
                level: self.brightness,
                flash_counter: self.flash_counter,
                handler_id: self.handler_id,
            },
        );
        self.phase = phase;
        self.brightness = level;
        self.flash_counter = counter;
        for e in out {
            match e {
                FadeFlashEffect::ApplyBrightness(v) => frame.brightness = Some(v),
                FadeFlashEffect::CaptureAndClearTint => {
                    self.saved_tint = self.tint;
                    self.tint = [0; 3];
                }
                FadeFlashEffect::RestoreTint => self.tint = self.saved_tint,
                FadeFlashEffect::ClearSceneField3E => self.scene_field_3e = 0,
                FadeFlashEffect::Exit(x) => self.retire(frame, x),
            }
        }
    }

    fn tick_sub_list(&mut self, pad: CursorPad, frame: &mut PanelFrame) {
        let (phase, cursor, out) = sub_list_tick(
            self.phase,
            SubListInput {
                input_locked: self.input_locked,
                cursor: self.cursor,
                pad,
                handler_id: self.handler_id,
            },
        );
        self.phase = phase;
        self.cursor = cursor;
        for e in out {
            match e {
                SubListEffect::RunPanelScript(va) => {
                    self.windows.run_script(va);
                }
                SubListEffect::ScaleAudioLevel { shift_right } => {
                    self.audio_level = if shift_right {
                        self.audio_level >> 1
                    } else {
                        self.audio_level << 1
                    };
                }
                SubListEffect::PlaySfx(s) => frame.sfx.push(s),
                SubListEffect::ClearWindowDescriptor => {
                    self.flag_desc = FlagWindowDescriptor::default()
                }
                SubListEffect::HandOff => frame.hand_off = true,
                SubListEffect::Exit(x) => self.retire(frame, x),
                SubListEffect::TickTextActors => self.text_actor_ticks += 1,
            }
        }
    }

    fn tick_soft_reset(&mut self, frame_delta: i32, pad_held: u16, frame: &mut PanelFrame) {
        use legaia_engine_vm::world_map_panel_actors::{SoftResetEffect, SoftResetInput};
        let (phase, slide, out) = soft_reset_tick(
            self.phase,
            SoftResetInput {
                frame_delta,
                slide: self.slide,
                pad: u32::from(pad_held),
            },
        );
        self.phase = phase;
        self.slide = slide;
        for e in out {
            match e {
                SoftResetEffect::ArmReset => {}
                SoftResetEffect::DrawRecords { y } => frame.records_y = Some(y),
                SoftResetEffect::WhiteFade { frames } => frame.white_fade = Some(frames),
                SoftResetEffect::ReloadExecutable => frame.reload_executable = true,
                SoftResetEffect::TickTextActors => self.text_actor_ticks += 1,
            }
        }
    }

    fn tick_fill_fade(&mut self, frame_delta: u8, frame: &mut PanelFrame) {
        let (phase, timer, out) = fill_fade_tick(
            self.phase,
            FillFadeInput {
                frame_delta: i16::from(frame_delta),
                timer: self.timer,
                input_locked: self.input_locked,
                load_pending: self.load_pending,
                handler_id: self.handler_id,
            },
        );
        self.phase = phase;
        self.timer = timer;
        for e in out {
            self.apply_fill_fade(e, frame);
        }
    }

    fn apply_fill_fade(&mut self, e: FillFadeEffect, frame: &mut PanelFrame) {
        match e {
            FillFadeEffect::RunPanelScript(va) => {
                self.windows.run_script(va);
            }
            FillFadeEffect::PostFillPrim => {}
            FillFadeEffect::SpawnSubActorAndCaptureTint => {
                self.saved_tint = self.tint;
                self.tint = [0; 3];
            }
            FillFadeEffect::QueueDmaAndRestoreTint => self.tint = self.saved_tint,
            FillFadeEffect::SetSceneFlagBit => self.scene_obj_flags |= 0x0008_0000,
            FillFadeEffect::TickTextActors => self.text_actor_ticks += 1,
            FillFadeEffect::Exit(x) => self.retire(frame, x),
            FillFadeEffect::ClearSceneField3E => self.scene_field_3e = 0,
        }
    }

    fn tick_text_box(
        &mut self,
        pad: CursorPad,
        pad_edge: u16,
        frame_delta: u8,
        frame: &mut PanelFrame,
    ) {
        // Retail's dismiss test is the newly-pressed word masked with the two
        // configured action buttons; the caller pre-reduces it to a bool.
        let dismiss = u32::from(pad_edge) & (PANEL_ACTION_A | PANEL_ACTION_B) != 0;
        let (phase, cursor, timer, out) = text_box_tick(
            self.phase,
            TextBoxInput {
                input_locked: self.input_locked,
                cursor: self.cursor,
                pad,
                dismiss_pressed: dismiss,
                fade: FillFadeInput {
                    frame_delta: i16::from(frame_delta),
                    timer: self.timer,
                    input_locked: self.input_locked,
                    load_pending: self.load_pending,
                    handler_id: self.handler_id,
                },
                handler_id: self.handler_id,
            },
        );
        self.phase = phase;
        self.cursor = cursor;
        self.timer = timer;
        for e in out {
            match e {
                TextBoxEffect::PlaySfx(s) => frame.sfx.push(s),
                TextBoxEffect::RestoreParty => frame.restore_party = true,
                TextBoxEffect::RunPanelScript(va) => {
                    self.windows.run_script(va);
                }
                TextBoxEffect::Exit(x) => self.retire(frame, x),
                TextBoxEffect::Fade(f) => self.apply_fill_fade(f, frame),
                TextBoxEffect::ClearSceneField3E => self.scene_field_3e = 0,
                TextBoxEffect::TickTextActors => self.text_actor_ticks += 1,
            }
        }
    }

    fn tick_flag_window(
        &mut self,
        pad: CursorPad,
        flags: &mut dyn PanelFlagStore,
        frame: &mut PanelFrame,
    ) {
        let input = FlagWindowInput {
            desc: self.flag_desc,
            input_locked: self.input_locked,
            selection: self.cursor,
            remembered: self.remembered_row,
            pad,
            handler_id: self.handler_id,
        };
        // The scan reads the bank; the writes below take it mutably, so the
        // immutable reborrow is scoped to the call.
        let (phase, selection, remembered, out) = {
            let read: &dyn PanelFlagStore = &*flags;
            flag_window_tick(self.phase, input, |id| read.flag_test(id))
        };
        self.phase = phase;
        self.cursor = selection;
        self.remembered_row = remembered;
        for e in out {
            match e {
                FlagWindowEffect::ClearRange { base, count } => {
                    for i in 0..i32::from(count) {
                        flags.flag_clear(base + i);
                    }
                }
                FlagWindowEffect::SizePanel { y, height } => {
                    let i = legaia_engine_vm::world_map_panel_actors::FLAG_WINDOW_PANEL_INDEX;
                    if let Some(d) = self.windows.descriptors.get_mut(i) {
                        d.y = y as i16;
                        d.height = height as i16;
                    }
                }
                FlagWindowEffect::RunPanelScript(va) => {
                    self.windows.run_script(va);
                }
                FlagWindowEffect::SetFlag(id) => {
                    flags.flag_set(id);
                    frame.flags_set.push(id);
                }
                FlagWindowEffect::Exit(x) => self.retire(frame, x),
                FlagWindowEffect::TickTextActors => self.text_actor_ticks += 1,
            }
        }
    }

    fn tick_travel_art(&mut self, frame_delta: u8, frame: &mut PanelFrame) {
        let Some(mut actor) = self.travel else {
            self.kind = None;
            return;
        };
        // The current map is the last one recorded, which is where the party
        // is standing - the same `0x80084628` read the retail scan compares.
        let current = self.visited.last().map(|v| v.map_id).unwrap_or(0);
        let table: Vec<VisitedMap> = self.visited.clone();
        let out = actor.tick(false, i16::from(frame_delta), || {
            let idx = find_visited_map(table.len(), current, |i| table[i].map_id)?;
            let v = table[idx];
            Some(destination_for(idx, v.tile_x, v.tile_z))
        });
        self.travel = Some(actor);
        if out.spawn_flash {
            frame.brightness = Some(legaia_engine_vm::world_map_panel_actors::BRIGHTNESS_MAX);
        }
        if out.unfound {
            frame.travel_unfound = true;
            self.kind = None;
            self.travel = None;
        }
        if let Some(dest) = out.destination {
            frame.warp = Some(dest);
            frame.retired = true;
            self.kind = None;
            self.travel = None;
        }
    }

    /// One frame of the field party HUD (`FUN_801D0D38`).
    ///
    /// Retail installs this in the field band's per-frame panel builder; the
    /// engine drives it from the overworld tick, where the party panel and the
    /// player marker both live. `pad_held` is the **packed** held word - the
    /// suppress mask is the packed D-pad, so a raw word suppresses nothing.
    ///
    /// `projected_y` is `None` when the host has no screen projection for the
    /// player, which is retail's own staged-load path and forces the low panel.
    pub fn tick_party_hud(
        &mut self,
        hud_disabled: bool,
        view_mode: i32,
        pad_held: u16,
        player_pos: Option<(i16, i16)>,
        timer_delta: i16,
        projected_y: Option<i16>,
    ) -> HudDecision {
        let rearm = self.hud_cached_pos.is_none();
        let stationary = match (self.hud_cached_pos, player_pos) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        };
        let (timer, decision) = field_hud_tick(HudInput {
            hud_disabled,
            view_mode,
            pad: u32::from(pad_held),
            scratch_suppress: false,
            rearm,
            short_idle: false,
            timer: self.hud_timer,
            timer_delta,
            player_stationary: stationary,
            projected_y,
        });
        self.hud_timer = timer;
        self.hud_cached_pos = player_pos;
        self.hud = Some(decision);
        decision
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_vm::world_map_panel_actors::{
        HUD_SUPPRESS_PAD_MASK, HUD_Y_BOTTOM, SOFT_RESET_PAD_MASK, SOFT_RESET_SLIDE_REST,
        SOFT_RESET_SLIDE_START,
    };

    #[derive(Default)]
    struct Flags(std::collections::HashSet<i32>);
    impl PanelFlagStore for Flags {
        fn flag_test(&self, id: i32) -> bool {
            self.0.contains(&id)
        }
        fn flag_set(&mut self, id: i32) {
            self.0.insert(id);
        }
        fn flag_clear(&mut self, id: i32) {
            self.0.remove(&id);
        }
    }

    /// The raw-to-packed conversion, on the two fixed points both layouts
    /// document independently.
    #[test]
    fn packed_pad_swaps_the_byte_halves() {
        assert_eq!(packed_pad(crate::input::PadButton::Cross.mask()), 0x0040);
        assert_eq!(packed_pad(crate::input::PadButton::Circle.mask()), 0x0020);
        assert_eq!(packed_pad(crate::input::PadButton::Up.mask()), 0x1000);
        assert_eq!(packed_pad(crate::input::PadButton::Down.mask()), 0x4000);
        // And the round trip is an involution, so a host can convert back.
        assert_eq!(packed_pad(packed_pad(0x1234)), 0x1234);
    }

    /// The confirm/cancel masks the panel kernels test are the packed ones;
    /// feeding the raw Cross bit must NOT read as a confirm.
    #[test]
    fn the_action_masks_are_packed_not_raw() {
        let raw_cross = u32::from(crate::input::PadButton::Cross.mask());
        assert_eq!(
            raw_cross & PANEL_ACTION_A,
            0,
            "raw Cross is not packed Cross"
        );
        assert_eq!(
            u32::from(packed_pad(crate::input::PadButton::Cross.mask())) & PANEL_ACTION_A,
            PANEL_ACTION_A
        );
    }

    fn confirm() -> u16 {
        packed_pad(crate::input::PadButton::Cross.mask())
    }
    fn cancel() -> u16 {
        packed_pad(crate::input::PadButton::Circle.mask())
    }

    #[test]
    fn the_sub_list_opens_a_window_and_sizes_the_party_panel() {
        let mut h = PanelActorHost::new();
        h.windows.party_members = 3;
        h.install(PanelActorKind::SubList, 0x11);
        let mut f = Flags::default();
        h.tick(0, 0, 1, &mut f);
        assert!(
            h.windows.is_open(SUBLIST_PANEL_INDEX),
            "the open script's op 1 spawned the window"
        );
        // `op 12` wrote the party panel descriptor: height = 3*56 - 7.
        let party = h
            .windows
            .descriptor(legaia_engine_vm::world_map_panel::PARTY_PANEL_INDEX);
        assert_eq!(party.height, 3 * 56 - 7);
        assert_eq!(party.y, 202 - party.height);
    }

    #[test]
    fn the_sub_list_cancel_closes_the_window_and_retires() {
        let mut h = PanelActorHost::new();
        h.install(PanelActorKind::SubList, 0x11);
        let mut f = Flags::default();
        h.tick(0, 0, 1, &mut f); // phase 0 -> 1, window open
        assert!(h.windows.is_open(SUBLIST_PANEL_INDEX));
        h.tick(0, cancel(), 1, &mut f); // phase 1 -> 2
        let frame = h.tick(0, 0, 1, &mut f); // phase 2: close + exit
        assert!(frame.retired);
        assert!(!h.is_active());
        assert!(!h.windows.is_open(SUBLIST_PANEL_INDEX));
    }

    #[test]
    fn the_sub_list_row_one_confirm_raises_the_hand_off() {
        let mut h = PanelActorHost::new();
        h.install(PanelActorKind::SubList, 0x11);
        let mut f = Flags::default();
        h.tick(0, 0, 1, &mut f);
        // Down then confirm: cursor 1 -> phase 3 (the hand-off).
        h.tick(
            packed_pad(crate::input::PadButton::Down.mask()),
            0,
            1,
            &mut f,
        );
        assert_eq!(h.cursor, 1);
        h.tick(0, confirm(), 1, &mut f);
        assert_eq!(h.phase, 3);
        let frame = h.tick(0, 0, 1, &mut f);
        assert!(frame.hand_off);
    }

    #[test]
    fn the_flag_window_sizes_its_panel_and_commits_a_flag() {
        let mut h = PanelActorHost::new();
        h.flag_desc = FlagWindowDescriptor {
            count: 8,
            first_visible: 0,
            rows: 4,
            base_flag: 0x100,
        };
        h.install(PanelActorKind::FlagWindow, 0x11);
        let mut f = Flags::default();
        f.flag_set(0x103);
        h.tick(0, 0, 1, &mut f);
        // Phase 0 sized descriptor 14 from the row count and opened it.
        let d = h
            .windows
            .descriptor(legaia_engine_vm::world_map_panel_actors::FLAG_WINDOW_PANEL_INDEX as i16);
        assert_eq!(d.height, 4 * 16);
        assert_eq!(d.y, (8 - 4) * 16 + 0x48);
        assert!(
            h.windows
                .is_open(legaia_engine_vm::world_map_panel_actors::FLAG_WINDOW_PANEL_INDEX as i16)
        );
        // The scan found flag 3 set, remembered it, and the range clear wiped
        // the whole span.
        assert_eq!(h.remembered_row, 3);
        assert!(!f.flag_test(0x103), "phase 0 clears the range it covers");
        // The list draws bottom-up, so the remembered selection 3 sits on
        // screen row 0 and only Down can leave it. Confirming on the
        // remembered row is retail's cancel, so the pick has to move first.
        h.tick(
            packed_pad(crate::input::PadButton::Down.mask()),
            0,
            1,
            &mut f,
        );
        assert_ne!(h.cursor, h.remembered_row);
        let frame = h.tick(0, confirm(), 1, &mut f);
        assert_eq!(frame.flags_set, vec![0x100 + h.cursor]);
        assert!(f.flag_test(frame.flags_set[0]));
    }

    /// `FUN_801EE90C` entered at phase 0 never reaches its prompt: it jumps to
    /// the fill-fade block and parks at phase 14, which has no exit arm. The
    /// host's entry phase has to be the prompt, or the actor wedges the screen
    /// - which is exactly what a live session showed before this was fixed.
    #[test]
    fn the_text_box_entry_phase_is_the_prompt_not_the_fade_block() {
        assert_eq!(PanelActorKind::TextBox.entry_phase(), 1);
        assert_eq!(PanelActorKind::SubList.entry_phase(), 0);

        let mut h = PanelActorHost::new();
        h.install(PanelActorKind::TextBox, 0x11);
        h.phase = 0; // retail's arrival path, seeded by hand
        let mut f = Flags::default();
        for _ in 0..256 {
            h.tick(0, 0, 4, &mut f);
        }
        assert_eq!(h.phase, 14, "the fade chain parks with no terminal arm");
        assert!(h.is_active(), "and nothing retires it");
        assert!(h.dismiss(), "so the host's escape hatch has to");
        assert!(!h.is_active());
    }

    #[test]
    fn the_text_box_confirm_asks_for_the_party_restore() {
        let mut h = PanelActorHost::new();
        h.install(PanelActorKind::TextBox, 0x11);
        assert_eq!(h.phase, 1, "the prompt");
        let mut f = Flags::default();
        let frame = h.tick(0, confirm(), 1, &mut f);
        assert!(frame.restore_party);
        assert_eq!(h.phase, 2);
        // Dismissing the confirmation retires the actor.
        let frame = h.tick(confirm(), confirm(), 1, &mut f);
        assert!(frame.retired);
    }

    #[test]
    fn the_fill_fade_closes_every_window_and_runs_to_its_exit() {
        let mut h = PanelActorHost::new();
        h.windows
            .run_script(legaia_engine_vm::world_map_panel_actors::SUBLIST_OPEN_SCRIPT);
        assert!(h.windows.is_open(SUBLIST_PANEL_INDEX));
        h.install(PanelActorKind::FillFade, 0x11);
        let mut f = Flags::default();
        // Phase 0 falls into 1 and runs the fill-fade script, which closes all.
        h.tick(0, 0, 1, &mut f);
        assert!(!h.windows.is_open(SUBLIST_PANEL_INDEX));
        assert_eq!(h.windows.open_count(), 0);
        let mut retired = false;
        let mut saw_scene_bit = false;
        for _ in 0..256 {
            let fr = h.tick(0, 0, 1, &mut f);
            saw_scene_bit |= h.scene_obj_flags & 0x0008_0000 != 0;
            if fr.retired {
                retired = true;
                break;
            }
        }
        assert!(saw_scene_bit, "phase 3 sets the scene object's flag bit");
        assert!(retired, "phase 4 exits");
    }

    #[test]
    fn the_fade_flash_parks_until_the_host_releases_it() {
        let mut h = PanelActorHost::new();
        h.install(PanelActorKind::FadeFlash, 0x11);
        let mut f = Flags::default();
        for _ in 0..200 {
            h.tick(0, 0, 4, &mut f);
            if h.phase == 3 {
                break;
            }
        }
        assert_eq!(h.phase, 3, "the ramp parks waiting on the flash counter");
        // Parked: 50 more frames change nothing.
        for _ in 0..50 {
            h.tick(0, 0, 4, &mut f);
        }
        assert_eq!(h.phase, 3);
        h.release_flash();
        let mut retired = false;
        for _ in 0..200 {
            if h.tick(0, 0, 8, &mut f).retired {
                retired = true;
                break;
            }
        }
        assert!(retired, "the released ramp-down reaches a terminal arm");
    }

    #[test]
    fn the_soft_reset_slides_then_reloads_on_a_face_press() {
        let mut h = PanelActorHost::new();
        h.install(PanelActorKind::SoftReset, 0x11);
        let mut f = Flags::default();
        let frame = h.tick(0, 0, 1, &mut f);
        assert!(frame.records_y.is_none(), "phase 0 only arms");
        assert_eq!(h.slide, SOFT_RESET_SLIDE_START);
        for _ in 0..(SOFT_RESET_SLIDE_START - SOFT_RESET_SLIDE_REST) {
            h.tick(0, 0, 1, &mut f);
        }
        assert_eq!(h.slide, SOFT_RESET_SLIDE_REST);
        // The pad is only sampled at rest.
        let frame = h.tick(0, SOFT_RESET_PAD_MASK as u16, 1, &mut f);
        assert!(frame.white_fade.is_some());
        let mut reloaded = false;
        for _ in 0..200 {
            if h.tick(0, 0, 1, &mut f).reload_executable {
                reloaded = true;
                break;
            }
        }
        assert!(reloaded);
    }

    #[test]
    fn the_travel_art_returns_the_party_to_the_stored_tile() {
        let mut h = PanelActorHost::new();
        h.note_visit(7, 96, 25);
        h.install(PanelActorKind::TravelArt(TravelArt::Riremito), 0x11);
        let mut f = Flags::default();
        let mut warp = None;
        for _ in 0..400 {
            let frame = h.tick(0, 0, 4, &mut f);
            if let Some(d) = frame.warp {
                warp = Some(d);
                break;
            }
        }
        let d = warp.expect("the scan hits the recorded map");
        assert_eq!((d.x, d.y, d.z), ((96 << 7) + 0x40, 0, (25 << 7) + 0x40));
        assert!(!h.is_active(), "the resolve retires the actor");
    }

    #[test]
    fn a_travel_art_with_no_recorded_map_parks_unfound() {
        let mut h = PanelActorHost::new();
        h.install(PanelActorKind::TravelArt(TravelArt::Rula), 0x11);
        let mut f = Flags::default();
        let mut unfound = false;
        for _ in 0..400 {
            if h.tick(0, 0, 4, &mut f).travel_unfound {
                unfound = true;
                break;
            }
        }
        assert!(unfound);
    }

    #[test]
    fn the_party_hud_suppresses_on_the_packed_dpad_and_counts_down_when_idle() {
        let mut h = PanelActorHost::new();
        // First call rearms (no cached position yet).
        let d = h.tick_party_hud(false, 0, 0, Some((10, 20)), 1, None);
        assert!(matches!(d, HudDecision::Rearmed { .. }));
        // A held packed d-pad direction suppresses outright.
        let d = h.tick_party_hud(
            false,
            0,
            HUD_SUPPRESS_PAD_MASK as u16,
            Some((10, 20)),
            1,
            None,
        );
        assert_eq!(d, HudDecision::Suppressed);
        // Stationary + no pad: the countdown runs and then draws.
        let mut drew = false;
        for _ in 0..0x100 {
            if let HudDecision::Draw { y } = h.tick_party_hud(false, 0, 0, Some((10, 20)), 1, None)
            {
                assert_eq!(y, HUD_Y_BOTTOM, "no projection forces the low panel");
                drew = true;
                break;
            }
        }
        assert!(drew);
    }

    #[test]
    fn an_idle_host_does_nothing() {
        let mut h = PanelActorHost::new();
        let mut f = Flags::default();
        assert_eq!(
            h.tick(confirm(), confirm(), 4, &mut f),
            PanelFrame::default()
        );
    }

    // ---------------------------------------------------------------------
    // The host chain: World::tick -> tick_world_map -> tick_world_map_panels
    // ---------------------------------------------------------------------

    fn world_on_the_overworld() -> crate::world::World {
        let mut w = crate::world::World::default();
        w.enter_world_map();
        w.world_map_ctrl.as_mut().unwrap().debug_enabled = true;
        w
    }

    /// The screen has to be reachable from the world tick, not just from a
    /// direct `PanelActorHost::install`. A raw Square edge must install the
    /// sub-list actor and open its window.
    #[test]
    fn the_world_tick_installs_the_sub_list_from_a_square_press() {
        let mut w = world_on_the_overworld();
        w.set_pad(0);
        let _ = w.tick();
        assert!(!w.world_map_ctrl.as_ref().unwrap().panels.is_active());
        w.set_pad(crate::input::PadButton::Square.mask());
        let _ = w.tick();
        let panels = &w.world_map_ctrl.as_ref().unwrap().panels;
        assert!(panels.is_active(), "Square installs the sub-list");
        assert!(
            panels.windows.is_open(SUBLIST_PANEL_INDEX),
            "and its open script spawned a window"
        );
    }

    /// The screen is a debug tool: without `debug_enabled` no chord installs
    /// anything, so a default overworld is unchanged.
    #[test]
    fn the_screen_stays_shut_without_the_debug_gate() {
        let mut w = crate::world::World::default();
        w.enter_world_map();
        w.set_pad(crate::input::PadButton::Square.mask());
        let _ = w.tick();
        assert!(!w.world_map_ctrl.as_ref().unwrap().panels.is_active());
    }

    /// The text box's confirm arm has to reach the *records*, not just the
    /// frame flag: a party on 1 HP must come back on full HP and MP.
    #[test]
    fn the_text_box_confirm_restores_the_partys_hp_and_mp() {
        let mut w = world_on_the_overworld();
        w.roster = legaia_save::Party::zeroed(3);
        for m in w.roster.members.iter_mut() {
            m.raw[0x104..0x106].copy_from_slice(&300u16.to_le_bytes()); // hp max
            m.raw[0x106..0x108].copy_from_slice(&1u16.to_le_bytes()); // hp cur
            m.raw[0x108..0x10A].copy_from_slice(&80u16.to_le_bytes()); // mp max
            m.raw[0x10A..0x10C].copy_from_slice(&0u16.to_le_bytes()); // mp cur
        }
        // R2 installs the text box; phase 0 hands to the fade block, so seat
        // the prompt phase directly and confirm.
        w.set_pad(crate::input::PadButton::R2.mask());
        let _ = w.tick();
        {
            let p = &mut w.world_map_ctrl.as_mut().unwrap().panels;
            assert_eq!(p.kind, Some(PanelActorKind::TextBox));
            assert_eq!(p.phase, 1, "installed straight at the prompt");
        }
        w.set_pad(crate::input::PadButton::Cross.mask());
        let _ = w.tick();
        for m in w.roster.members.iter() {
            assert_eq!(u16::from_le_bytes([m.raw[0x106], m.raw[0x107]]), 300);
            assert_eq!(u16::from_le_bytes([m.raw[0x10A], m.raw[0x10B]]), 80);
        }
    }

    /// The travel art has to move the *player actor*, and it has to move it
    /// back to where the screen was opened - not to where the player is when
    /// the dwell ends.
    #[test]
    fn the_travel_art_warps_the_player_actor_back_to_the_frozen_tile() {
        let mut w = world_on_the_overworld();
        w.spawn_actor(0).active = true;
        w.player_actor_slot = Some(0);
        w.seat_player_at_tile(20, 30);
        w.set_pad(0);
        let _ = w.tick();
        // Freeze the return point, then teleport the player somewhere else and
        // let the art run.
        {
            let p = &mut w.world_map_ctrl.as_mut().unwrap().panels;
            assert_eq!(p.visited.len(), 1, "the idle tick recorded the tile");
            p.install(PanelActorKind::TravelArt(TravelArt::Riremito), 0x1A);
        }
        w.seat_player_at_tile(200, 5);
        for _ in 0..400 {
            w.set_pad(0);
            let _ = w.tick();
            if !w.world_map_ctrl.as_ref().unwrap().panels.is_active() {
                break;
            }
        }
        let slot = w.player_actor_slot.expect("player installed") as usize;
        let a = &w.actors[slot];
        assert_eq!(a.move_state.world_x, ((20 << 7) + 0x40) as i16);
        assert_eq!(a.move_state.world_z, ((30 << 7) + 0x40) as i16);
    }

    /// The flag picker has to commit into the world's own system flag bank,
    /// which is what the field VM reads - not into a private copy.
    #[test]
    fn the_flag_window_commits_into_the_worlds_system_flag_bank() {
        let mut w = world_on_the_overworld();
        w.system_flag_set(0x0003);
        {
            let p = &mut w.world_map_ctrl.as_mut().unwrap().panels;
            p.flag_desc = FlagWindowDescriptor {
                count: 8,
                first_visible: 0,
                rows: 4,
                base_flag: 0,
            };
        }
        w.set_pad(crate::input::PadButton::R1.mask());
        let _ = w.tick();
        assert!(
            !w.system_flag_test(0x0003),
            "phase 0's range clear reached the world bank"
        );
        w.set_pad(crate::input::PadButton::Down.mask());
        let _ = w.tick();
        w.set_pad(crate::input::PadButton::Cross.mask());
        let _ = w.tick();
        let picked = w.world_map_ctrl.as_ref().unwrap().panels.cursor;
        assert!(
            w.system_flag_test(picked as u16),
            "the confirm set flag {picked} in the world bank"
        );
    }

    #[test]
    fn note_visit_updates_the_stored_tile_in_place() {
        let mut h = PanelActorHost::new();
        h.note_visit(3, 1, 1);
        h.note_visit(4, 2, 2);
        h.note_visit(3, 9, 9);
        assert_eq!(h.visited.len(), 2);
        assert_eq!(h.visited[0].tile_x, 9);
    }
}
