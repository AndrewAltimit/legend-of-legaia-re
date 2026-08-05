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
//! The trigger point is [`crate::menu_runtime::MenuRuntime::tick`]: entering
//! the shop picker state runs the open script, entering the Sell state runs
//! the slide-away script - the same two edges the retail dispatcher drives.
//!
//! [`World::install_menu_overlay_tables`]: crate::world::World::install_menu_overlay_tables

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

/// One live UI window as the choreography sees it - the engine analogue of
/// the retail `0x5C`-stride window-list node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WidgetWindow {
    /// Slide destination (live node `+0xA/+0xC`); the home position is the
    /// descriptor's `x`/`y`.
    pub target: vm::Position,
    /// Live node `+0x20` motion word: set while the window is approaching
    /// its target, cleared by the VM's arrival check / `ClearField20`.
    pub sliding: bool,
    /// Live node `+0x1D` style byte (`SetField1d`).
    pub style: u8,
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
    /// `GlobalUpdate` (`FUN_80035A4C`) invocations - the per-program tick
    /// the scripts open with.
    pub global_updates: u32,
    /// `Effect` (`FUN_800319A8`) invocations, per window id. The helper's
    /// window-state side is undecoded; the count keeps the dispatch
    /// observable without inventing semantics.
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

    /// Ids of the currently open windows, ascending.
    pub fn open_ids(&self) -> Vec<u8> {
        self.windows.keys().copied().collect()
    }

    /// The live window for `id`, if open.
    pub fn window(&self, id: u8) -> Option<&WidgetWindow> {
        self.windows.get(&id)
    }

    /// Whether any window is open.
    pub fn any_open(&self) -> bool {
        !self.windows.is_empty()
    }

    /// Drop every window (menu close / overlay swap).
    pub fn reset(&mut self) {
        self.windows.clear();
    }
}

impl vm::Host for MenuWidgetState {
    /// `FUN_80035334` - window-list lookup by descriptor id.
    fn actor_exists(&self, id: u8) -> bool {
        self.windows.contains_key(&id)
    }

    /// Descriptor-table `x`/`y` for `id` (the `s1`/`s2` pair the VM loads
    /// per instruction).
    fn default_position(&self, id: u8) -> vm::Position {
        self.defaults.get(id as usize).copied().unwrap_or_default()
    }

    /// `FUN_800326AC` - create the window from its descriptor record. The
    /// retail creator parks the new node against its descriptor's park
    /// edge; the slide toward home is what the follow-up position write
    /// starts, so a fresh window begins `sliding`.
    fn spawn(&mut self, id: u8, default_position: vm::Position) {
        self.windows.insert(
            id,
            WidgetWindow {
                target: default_position,
                sliding: true,
                style: 0,
            },
        );
    }

    /// `FUN_800357FC` - target-position write.
    fn set_position(&mut self, id: u8, position: vm::Position) {
        if let Some(w) = self.windows.get_mut(&id) {
            w.target = position;
        }
    }

    /// `FUN_800358C0` - slide toward `target`.
    fn start_motion(&mut self, id: u8, target: vm::Position) {
        if let Some(w) = self.windows.get_mut(&id) {
            w.target = target;
            w.sliding = true;
        }
    }

    /// `FUN_80035978` - close / remove the window.
    fn delete_sprite(&mut self, id: u8) {
        self.windows.remove(&id);
    }

    /// `FUN_80035A4C` - global per-program tick.
    fn global_update(&mut self) {
        self.global_updates += 1;
    }

    /// `FUN_800319A8` - counted, window state untouched (helper undecoded).
    fn actor_effect(&mut self, _id: u8) {
        self.effect_fires += 1;
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
            w.sliding = false;
        }
    }

    /// Retail reads the sub-window at `+0x24` and compares current
    /// (`+6/+8`) against target (`+0xA/+0xC`). The engine model tracks no
    /// interpolated current position, so "arrived" is `!sliding`.
    fn snap_clear_condition(&self, id: u8) -> bool {
        self.windows.get(&id).is_some_and(|w| !w.sliding)
    }

    /// `EffectMotion`'s captured target - the window's current `+0xA/+0xC`.
    fn motion_target(&self, id: u8) -> Option<vm::Position> {
        self.windows.get(&id).map(|w| w.target)
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
    fn resolve_from_overlay_rejects_short_buffer() {
        assert!(MenuWidgetScripts::resolve_from_overlay(&[0u8; 64]).is_none());
    }
}
