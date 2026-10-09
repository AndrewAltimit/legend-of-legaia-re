//! Menu **window-widget choreography**: the engine host for the
//! window-script VM (`legaia_engine_vm::run`, retail `FUN_801D6628`).
//!
//! REF: FUN_801D6628 -- interpreter; ported in `legaia_engine_vm`
//!
//! Retail's menu overlay (PROT 0899) choreographs its UI windows with small
//! bytecode programs resident in the overlay's own data segment
//! ([`legaia_asset::widget_script`]): the shop picker dispatcher
//! `FUN_801DAFD4` runs the open script `DAT_801E4E38` when the Buy/Sell
//! picker comes up and the slide-away script `DAT_801E4E54` on the Sell
//! transition (docs/subsystems/shop.md). This module is the receiving side:
//!
//! - [`MenuWidgetScripts`]: the per-boot program lookup - raw disc bytes
//!   resolved out of the menu-overlay image by
//!   [`World::install_menu_overlay_tables`], which both hosts already call
//!   with the real PROT 0899 bytes.
//! - [`MenuWidgetState`]: the window-list state the interpreter drives - an
//!   engine model of the live `0x5C`-stride window list the retail helpers
//!   walk (list head `gp+0x148`, descriptor id at `+0x8`;
//!   `see ghidra/scripts/funcs/80035334.txt`). It implements
//!   `legaia_engine_vm::Host`, mapping each callback to the retail helper
//!   the VM dispatched to.
//!
//! The trigger point is `legaia_engine_core::menu_runtime::MenuRuntime::tick`: entering
//! the shop picker state runs the open script, entering the Sell state runs
//! the slide-away script - the same two edges the retail dispatcher drives.
//!
//! [`World::install_menu_overlay_tables`]: legaia_engine_core::world::World::install_menu_overlay_tables

use legaia_asset::widget_script;
use legaia_engine_vm as vm;
use std::collections::BTreeMap;

/// Resolved window-widget programs - raw disc bytes (terminator included),
/// exactly what `legaia_engine_vm::run` consumes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuWidgetScripts {
    /// Shop picker open script (`DAT_801E4E38`).
    pub shop_open: Vec<u8>,
    /// Shop Sell-transition slide-away script (`DAT_801E4E54`).
    pub shop_sell_away: Vec<u8>,
    /// Every program the `jal`-site scan recovered from the overlay image,
    /// as `(script_va, bytes)` - the full per-overlay program table, kept
    /// for tooling and tests.
    pub programs: Vec<(u32, Vec<u8>)>,
}

impl MenuWidgetScripts {
    /// Resolve the widget programs out of the as-loaded menu-overlay image
    /// (PROT 0899 extended entry bytes). `None` when the image does not
    /// carry the pinned scripts (short or foreign buffer).
    pub fn resolve_from_overlay(overlay: &[u8]) -> Option<Self> {
        let shop_open =
            widget_script::script_bytes_at(overlay, widget_script::SHOP_OPEN_SCRIPT_VA).ok()?;
        let shop_sell_away =
            widget_script::script_bytes_at(overlay, widget_script::SHOP_SELL_AWAY_SCRIPT_VA)
                .ok()?;
        let programs = widget_script::scan(overlay)
            .into_iter()
            .map(|r| {
                let bytes = widget_script::script_bytes_at(overlay, r.script.va)
                    .expect("scanned script re-slices");
                (r.script.va, bytes)
            })
            .collect();
        Some(Self {
            shop_open,
            shop_sell_away,
            programs,
        })
    }
}

/// The motion word at live node `+0x20`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetMotion {
    /// `0` - at rest.
    Still,
    /// `1` - sliding from [`WidgetWindow::from`] toward
    /// [`WidgetWindow::target`] (`FUN_800357FC`, and the creator).
    Sliding,
    /// `-1` - closing (`FUN_80035978` / `FUN_80035A4C`); the node is torn
    /// down when its close completes.
    Closing,
}

/// One live UI window as the choreography sees it - the engine analogue of
/// the retail `0x5C`-stride window-list node and its motion sub-object
/// (node `+0x24`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WidgetWindow {
    /// Live position, node `+0xA/+0xC`.
    pub pos: vm::Position,
    /// Slide source, sub-object `+6/+8`.
    pub from: vm::Position,
    /// Slide target, sub-object `+0xA/+0xC`; the home position is the
    /// descriptor's `x`/`y`.
    pub target: vm::Position,
    /// Whether [`Self::pos`] is still the creator's **park edge** - the
    /// off-window start `FUN_800326AC` derives from the descriptor's
    /// direction byte (`+1`) and the screen bounds, which this model does
    /// not compute. A slide started from the park edge has not arrived,
    /// whatever the coordinates say.
    pub at_park: bool,
    /// Whether [`Self::from`] was taken at the park edge (see
    /// [`Self::at_park`]).
    pub from_park: bool,
    /// Node `+0x20`.
    pub motion: WidgetMotion,
    /// Live node `+0x1D` style byte (`SetField1d`).
    pub style: u8,
}

impl WidgetWindow {
    /// Node `+0x20 == 1`.
    pub fn sliding(&self) -> bool {
        self.motion == WidgetMotion::Sliding
    }

    /// Node `+0x20 == -1`.
    pub fn closing(&self) -> bool {
        self.motion == WidgetMotion::Closing
    }
}

/// The window list the widget scripts drive. Implements
/// [`vm::Host`]; window ids index the menu window descriptor table
/// ([`legaia_asset::menu_windows`]), whose `x`/`y` seed
/// [`vm::Host::default_position`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MenuWidgetState {
    windows: BTreeMap<u8, WidgetWindow>,
    /// Home positions from the window descriptor table (`+0x4`/`+0x6` of
    /// each 16-byte record - the pair `FUN_801D6628` reads at
    /// `0x801D6678/0x801D667C`). Empty until the table installs; a missing
    /// entry reads as `(0, 0)`.
    defaults: Vec<vm::Position>,
    /// `FUN_80035A4C` (close-all) invocations - the scripts open with one.
    pub global_updates: u32,
    /// `FUN_800319A8` (immediate destroy) invocations.
    pub effect_fires: u32,
}

impl MenuWidgetState {
    /// Install home positions from the disc-parsed window descriptor table.
    pub fn set_defaults_from_table(&mut self, table: &legaia_asset::menu_windows::MenuWindowTable) {
        self.defaults = table
            .windows
            .iter()
            .map(|w| vm::Position::new(w.x, w.y))
            .collect();
    }

    /// Ids of the windows on the list that are not closing, ascending.
    pub fn open_ids(&self) -> Vec<u8> {
        self.windows
            .iter()
            .filter(|(_, w)| !w.closing())
            .map(|(id, _)| *id)
            .collect()
    }

    /// Ids of the windows still on the list but closing, ascending.
    pub fn closing_ids(&self) -> Vec<u8> {
        self.windows
            .iter()
            .filter(|(_, w)| w.closing())
            .map(|(id, _)| *id)
            .collect()
    }

    /// The live window for `id`, closing or not.
    pub fn window(&self, id: u8) -> Option<&WidgetWindow> {
        self.windows.get(&id)
    }

    /// Whether any window is open (not closing).
    pub fn any_open(&self) -> bool {
        self.windows.values().any(|w| !w.closing())
    }

    /// Drop every window (menu close / overlay swap).
    pub fn reset(&mut self) {
        self.windows.clear();
    }
}

impl vm::Host for MenuWidgetState {
    /// `FUN_80035334` - window-list lookup by descriptor id. A closing node
    /// is still on the list.
    fn actor_exists(&self, id: u8) -> bool {
        self.windows.contains_key(&id)
    }

    /// Descriptor-table `x`/`y` for `id` (the `s1`/`s2` pair the VM loads
    /// per instruction).
    fn default_position(&self, id: u8) -> vm::Position {
        self.defaults.get(id as usize).copied().unwrap_or_default()
    }

    /// `FUN_800326AC` - create the window from its descriptor record: the
    /// node at the park edge, its sub-object sliding from there toward the
    /// descriptor's home (`0x800329DC..0x80032A0C`: source = park,
    /// target = descriptor `+4/+6`, node `+0x20 = 1`).
    fn spawn(&mut self, id: u8, default_position: vm::Position) {
        self.windows.insert(
            id,
            WidgetWindow {
                pos: default_position,
                from: default_position,
                target: default_position,
                at_park: true,
                from_park: true,
                motion: WidgetMotion::Sliding,
                style: 0,
            },
        );
    }

    /// `FUN_800357FC` - start a slide from the live position to `position`
    /// (tail `0x80035874`). Ops `0x01` / `0x02`.
    fn slide_to(&mut self, id: u8, position: vm::Position) {
        if let Some(w) = self.windows.get_mut(&id) {
            w.from = w.pos;
            w.from_park = w.at_park;
            w.target = position;
            w.motion = WidgetMotion::Sliding;
        }
    }

    /// `FUN_800358C0` - snap to `target`: live position, source and target
    /// all written, motion cleared (tail `0x80035938`). Ops `0x09` / `0x0A`.
    fn snap_to(&mut self, id: u8, target: vm::Position) {
        if let Some(w) = self.windows.get_mut(&id) {
            w.pos = target;
            w.from = target;
            w.target = target;
            w.at_park = false;
            w.from_park = false;
            w.motion = WidgetMotion::Still;
        }
    }

    /// `FUN_80035978` - begin the close; the node stays on the list.
    fn begin_close(&mut self, id: u8) {
        if let Some(w) = self.windows.get_mut(&id) {
            w.motion = WidgetMotion::Closing;
        }
    }

    /// `FUN_80035A4C` - begin the close on every window.
    fn close_all(&mut self) {
        self.global_updates += 1;
        for w in self.windows.values_mut() {
            w.motion = WidgetMotion::Closing;
        }
    }

    /// `FUN_800319A8` - free and unlink the window now.
    fn destroy(&mut self, id: u8) {
        self.effect_fires += 1;
        self.windows.remove(&id);
    }

    /// Live node `+0x1D` style byte.
    fn set_field_1d(&mut self, id: u8, value: u8) {
        if let Some(w) = self.windows.get_mut(&id) {
            w.style = value;
        }
    }

    /// Live node `+0x20` motion-word clear.
    fn clear_field_20(&mut self, id: u8) {
        if let Some(w) = self.windows.get_mut(&id) {
            w.motion = WidgetMotion::Still;
        }
    }

    /// Retail reads the sub-object at `+0x24` and compares the slide's
    /// source (`+6/+8`) against its target (`+0xA/+0xC`): a slide that
    /// starts where it ends has nothing to animate. A source taken at the
    /// park edge never compares equal here, since the park coordinates are
    /// not modelled.
    fn snap_clear_condition(&self, id: u8) -> bool {
        self.windows
            .get(&id)
            .is_some_and(|w| !w.from_park && w.from == w.target)
    }

    /// `EffectMotion`'s captured position - the node's live `+0xA/+0xC`.
    fn motion_target(&self, id: u8) -> Option<vm::Position> {
        self.windows.get(&id).map(|w| w.pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_vm as vm;

    /// Hand-authored program (not disc bytes): open windows 1 and 2, close
    /// window 1, end.
    const SYNTH: &[u8] = &[
        0x05, 0x00, 0x00, 0x00, // GlobalUpdate
        0x01, 0x01, 0x00, 0x00, // SpawnDefault window 1
        0x01, 0x02, 0x00, 0x00, // SpawnDefault window 2
        0x04, 0x01, 0x00, 0x00, // DeleteSprite window 1
        0x00, 0x00, 0x00, 0x00, // End
    ];

    #[test]
    fn synthetic_program_drives_window_list() {
        let mut st = MenuWidgetState {
            defaults: vec![vm::Position::default(); 8],
            ..Default::default()
        };
        st.defaults[2] = vm::Position::new(40, 60);
        vm::run(&mut st, SYNTH).unwrap();
        assert_eq!(st.open_ids(), vec![2]);
        assert_eq!(st.window(2).unwrap().target, vm::Position::new(40, 60));
        assert_eq!(st.global_updates, 1);
    }

    #[test]
    fn slide_ops_slide_and_snap_ops_snap() {
        // Retail: ops 01/02 -> FUN_800357FC (slide, +0x20 = 1); ops 09/0A ->
        // FUN_800358C0 (snap, +0x20 = 0); op 08 -> FUN_800319A8 (destroy).
        let mut st = MenuWidgetState {
            defaults: vec![vm::Position::new(10, 20); 8],
            ..Default::default()
        };
        // 02 w3 to packed (x = (w >> 7) & 0x1FE, y = w & 0xFF): w = 0x2040
        // -> (0x40, 0x40).
        vm::run(&mut st, &[0x02, 0x03, 0x40, 0x20, 0, 0, 0, 0]).unwrap();
        let w = *st.window(3).unwrap();
        assert!(w.sliding(), "op 02 slides");
        assert_eq!(w.target, vm::Position::new(0x40, 0x40));
        assert_eq!(
            w.pos,
            vm::Position::new(10, 20),
            "a slide leaves pos to the walker"
        );
        // 09 w3 to packed (0x40, 0x10): w = 0x2010.
        vm::run(&mut st, &[0x09, 0x03, 0x10, 0x20, 0, 0, 0, 0]).unwrap();
        let w = *st.window(3).unwrap();
        assert!(!w.sliding(), "op 09 snaps");
        assert_eq!(w.pos, vm::Position::new(0x40, 0x10));
        assert_eq!(w.from, w.target);
        // 01 w3 home from a snapped position: a real slide.
        vm::run(&mut st, &[0x01, 0x03, 0, 0, 0, 0, 0, 0]).unwrap();
        let w = *st.window(3).unwrap();
        assert!(w.sliding());
        assert_eq!(
            (w.from, w.target),
            (vm::Position::new(0x40, 0x10), vm::Position::new(10, 20))
        );
        // 0A w3: destroy + re-create + snap back to the live position.
        vm::run(&mut st, &[0x0A, 0x03, 0, 0, 0, 0, 0, 0]).unwrap();
        let w = *st.window(3).unwrap();
        assert!(!w.sliding());
        assert_eq!(w.pos, vm::Position::new(0x40, 0x10));
        // 04 begins a close; the node stays on the list.
        vm::run(&mut st, &[0x04, 0x03, 0, 0, 0, 0, 0, 0]).unwrap();
        assert!(st.open_ids().is_empty());
        assert_eq!(st.closing_ids(), vec![3]);
        // 08 destroys it outright.
        vm::run(&mut st, &[0x08, 0x03, 0, 0, 0, 0, 0, 0]).unwrap();
        assert!(st.window(3).is_none());
        assert_eq!(st.effect_fires, 2, "op 0A destroys too");
    }

    #[test]
    fn resolve_from_overlay_rejects_short_buffer() {
        assert!(MenuWidgetScripts::resolve_from_overlay(&[0u8; 64]).is_none());
    }
}
