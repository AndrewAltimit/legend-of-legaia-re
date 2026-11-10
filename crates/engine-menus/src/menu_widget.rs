//! Menu **window-widget choreography**: the engine host for the
//! window-script VM (`legaia_engine_vm::run`, retail `FUN_801D6628`).
//!
//! REF: FUN_801D6628 -- interpreter; ported in `legaia_engine_vm`
//!
//! Retail's menu overlay (PROT 0899) choreographs its UI windows with small
//! bytecode programs resident in the overlay's own data segment
//! ([`legaia_game_tables::widget_script`]): the shop picker dispatcher
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
//! The trigger point is `legaia_engine_menu_runtime::menu_runtime::MenuRuntime::tick`: entering
//! the shop picker state runs the open script, entering the Sell state runs
//! the slide-away script - the same two edges the retail dispatcher drives.
//!
//! [`World::install_menu_overlay_tables`]: legaia_engine_core::world::World::install_menu_overlay_tables

use legaia_engine_vm as vm;
use legaia_game_tables::widget_script;
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
    /// Node `+0x20`.
    pub motion: WidgetMotion,
    /// Node `+0x1E`, the motion's frame count: the window walker
    /// ([`MenuWidgetState::step_motion`]) advances it, a close reverses it.
    pub progress: i16,
    /// Sub-object `+2/+4`: the park the creator computed
    /// ([`park_position`]), where a close carries the window.
    pub park: vm::Position,
    /// Live node `+0x1D` style byte: the descriptor's class word's low byte
    /// at creation, then whatever `SetField1d` writes.
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

/// The motion sub-object's duration `+0`: `FUN_800326AC` writes `0xC` on
/// every window it creates (`0x801D29FC`), so every slide and close runs
/// twelve frames.
pub const WIDGET_MOTION_FRAMES: i16 = 0xC;

/// What a window's creation reads off its descriptor record
/// ([`legaia_game_tables::menu_windows::MenuWindowDescriptor`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WidgetDescriptor {
    /// `+4` / `+6`, the home position.
    pub home: vm::Position,
    /// `+8` / `+0xA`, the content size the park edge clears.
    pub size: (i16, i16),
    /// `+1`, the park-edge class.
    pub park_edge: u8,
    /// `+2`, the class word; its low byte seeds node `+0x1D`.
    pub kind: u16,
}

/// The display rect the park edges are measured from: the scratchpad
/// halfwords `0x1F800388` (left), `0x1F80038A` (top), `0x1F80038C` (right)
/// and `0x1F80038E` (bottom).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenBox {
    pub left: u16,
    pub top: u16,
    pub right: u16,
    pub bottom: u16,
}

impl Default for ScreenBox {
    fn default() -> Self {
        Self {
            left: 0,
            top: 0,
            right: 0x140,
            bottom: 0xF0,
        }
    }
}

/// Where `FUN_800326AC` creates a window: off the screen edge its
/// descriptor's park class names (`0x800326E8..0x80032870`), so the first
/// slide home comes in from there. Classes `1`, `3`, `5` and `7` are the
/// diagonals, which move both axes by the smaller of the two clearances; a
/// class of `8` or more creates the window at home.
///
/// PORT: FUN_800326AC (park switch)
pub fn park_position(desc: &WidgetDescriptor, screen: ScreenBox) -> vm::Position {
    let (mut x, mut y) = (i32::from(desc.home.x), i32::from(desc.home.y));
    let (w, h) = (i32::from(desc.size.0), i32::from(desc.size.1));
    let (left, top, right, bottom) = (
        i32::from(screen.left),
        i32::from(screen.top),
        i32::from(screen.right),
        i32::from(screen.bottom),
    );
    let past_left = (x + 0x14) - (left - w);
    let past_top = (y + 0x14) - (top - h);
    let past_right = right - (x - 0xC);
    let past_bottom = bottom - (y - 0xC);
    match desc.park_edge {
        0 => y = bottom + 0xC,
        2 => x = left - w - 0x14,
        4 => y = top - h - 0x14,
        6 => x = right + 0xC,
        1 => {
            let d = past_left.min(past_bottom);
            x -= d;
            y += d;
        }
        3 => {
            let d = past_left.min(past_top);
            x -= d;
            y -= d;
        }
        5 => {
            let d = past_right.min(past_top);
            x += d;
            y -= d;
        }
        7 => {
            let d = past_right.min(past_bottom);
            x += d;
            y += d;
        }
        _ => {}
    }
    vm::Position::new(x as i16, y as i16)
}

/// The window list the widget scripts drive. Implements
/// [`vm::Host`]; window ids index the menu window descriptor table
/// ([`legaia_game_tables::menu_windows`]), whose records seed
/// [`vm::Host::default_position`] and the creation park.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MenuWidgetState {
    windows: BTreeMap<u8, WidgetWindow>,
    /// The descriptor records `FUN_801D6628` indexes (`0x801E4738`, 16-byte
    /// stride; home pair read at `0x801D6678/0x801D667C`). Empty until the
    /// table installs; a missing entry reads as zeros.
    descriptors: Vec<WidgetDescriptor>,
    /// The display rect a creation parks against.
    pub screen: ScreenBox,
    /// `FUN_80035A4C` (close-all) invocations - the scripts open with one.
    pub global_updates: u32,
    /// `FUN_800319A8` (immediate destroy) invocations.
    pub effect_fires: u32,
    /// `_DAT_8007BB80` (`gp+0x868`): how many windows the last walker pass
    /// moved ([`Self::step_motion`]). Every menu sub-screen reads it before
    /// its pad and takes no input while it is non-zero.
    latch: u32,
}

impl MenuWidgetState {
    /// Install the descriptor records from the disc-parsed window table.
    pub fn set_defaults_from_table(
        &mut self,
        table: &legaia_game_tables::menu_windows::MenuWindowTable,
    ) {
        self.descriptors = table
            .windows
            .iter()
            .map(|w| WidgetDescriptor {
                home: vm::Position::new(w.x, w.y),
                size: (w.w, w.h),
                park_edge: w.park_edge,
                kind: w.kind,
            })
            .collect();
    }

    /// Install descriptor records directly (tests, tools).
    pub fn set_descriptors(&mut self, descriptors: Vec<WidgetDescriptor>) {
        self.descriptors = descriptors;
    }

    fn descriptor(&self, id: u8) -> WidgetDescriptor {
        self.descriptors
            .get(id as usize)
            .copied()
            .unwrap_or_default()
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

    /// Every window on the list in list order (retail keeps the list sorted
    /// by id, `0x800328F8..0x80032918`).
    pub fn windows(&self) -> impl Iterator<Item = (u8, &WidgetWindow)> {
        self.windows.iter().map(|(id, w)| (*id, w))
    }

    /// The live window for `id`, closing or not.
    pub fn window(&self, id: u8) -> Option<&WidgetWindow> {
        self.windows.get(&id)
    }

    /// The live window for `id`, for the walker that moves it.
    pub fn window_mut(&mut self, id: u8) -> Option<&mut WidgetWindow> {
        self.windows.get_mut(&id)
    }

    /// Whether any window is open (not closing).
    pub fn any_open(&self) -> bool {
        self.windows.values().any(|w| !w.closing())
    }

    /// Drop every window (menu close / overlay swap).
    pub fn reset(&mut self) {
        self.windows.clear();
        self.latch = 0;
    }

    /// `_DAT_8007BB80` as the last [`Self::step_motion`] left it: non-zero
    /// while a window is sliding in or out. A sub-screen that reads it
    /// before its pad (every one in the menu overlay does) ignores the pad
    /// until it drops to zero.
    pub fn slide_latch(&self) -> u32 {
        self.latch
    }

    /// One pass of the window motion walker `FUN_80031AE4`, the first thing
    /// the per-frame window walker `FUN_80031D00` does: it zeroes the latch
    /// `_DAT_8007BB80`, then for each window on the list
    ///
    /// - sliding or closing (`+0x20 = 1 / -1`): adds `frame_delta`
    ///   (`DAT_1F800393`) to the progress `+0x1E`; once that reaches the
    ///   duration (`0xC`) the progress resets, the motion word steps down
    ///   (`1 -> 0`, `-1 -> -2`) and the window lands on its target;
    /// - then, on the motion word as it now stands: still copies the live
    ///   position into the slide source; sliding places the window at
    ///   `source + (target - source) * progress / duration` and closing at
    ///   `source + (park - source) * progress / duration` (signed, truncated),
    ///   each counting one onto the latch; `-2` puts it on its park.
    ///
    /// A window whose close completed (`-2`) is destroyed by the draw walker
    /// in the same frame (`0x80031E04..0x80031E20`), so it leaves the list
    /// here. Returns the latch.
    ///
    /// PORT: FUN_80031AE4
    pub fn step_motion(&mut self, frame_delta: u8) -> u32 {
        let dur = i32::from(WIDGET_MOTION_FRAMES);
        let mut latch = 0u32;
        let mut finished = Vec::new();
        for (&id, w) in self.windows.iter_mut() {
            let mut word: i16 = match w.motion {
                WidgetMotion::Still => 0,
                WidgetMotion::Sliding => 1,
                WidgetMotion::Closing => -1,
            };
            if word != 0 {
                let p = (w.progress as u16).wrapping_add(u16::from(frame_delta));
                w.progress = p as i16;
                if i32::from(p) >= dur {
                    w.progress = 0;
                    word -= 1;
                    w.pos = w.target;
                }
            }
            let lerp = |from: i16, to: i16, p: i16| -> i16 {
                let span = (i32::from(to) - i32::from(from)).wrapping_mul(i32::from(p as u16));
                (i32::from(from as u16) + span / dur) as i16
            };
            match word {
                0 => {
                    w.motion = WidgetMotion::Still;
                    w.from = w.pos;
                }
                1 => {
                    w.pos = vm::Position::new(
                        lerp(w.from.x, w.target.x, w.progress),
                        lerp(w.from.y, w.target.y, w.progress),
                    );
                    latch += 1;
                }
                -1 => {
                    w.pos = vm::Position::new(
                        lerp(w.from.x, w.park.x, w.progress),
                        lerp(w.from.y, w.park.y, w.progress),
                    );
                    latch += 1;
                }
                _ => {
                    w.pos = w.park;
                    finished.push(id);
                }
            }
        }
        for id in finished {
            self.windows.remove(&id);
        }
        self.latch = latch;
        latch
    }
}

/// The close body `FUN_80035978` runs on one window, and `FUN_80035A4C` on
/// each: a still window starts its close from frame `0`, a sliding one
/// reverses its slide's progress (`duration - progress`), and one already
/// closing is left alone (`0x800359F8..0x80035A40`).
fn begin_close_window(w: &mut WidgetWindow) {
    match w.motion {
        WidgetMotion::Still => {
            w.motion = WidgetMotion::Closing;
            w.progress = 0;
        }
        WidgetMotion::Sliding => {
            w.motion = WidgetMotion::Closing;
            w.progress = WIDGET_MOTION_FRAMES.wrapping_sub(w.progress);
        }
        WidgetMotion::Closing => {}
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
        self.descriptor(id).home
    }

    /// `FUN_800326AC` - create the window from its descriptor record: the
    /// node at the park edge ([`park_position`]), its sub-object sliding from
    /// there toward the descriptor's home (`0x800329DC..0x80032A0C`), node
    /// `+0x1D` from the class word, node `+0x20 = 1`. An id already on the
    /// list is re-initialised in place.
    fn spawn(&mut self, id: u8, _default_position: vm::Position) {
        let desc = self.descriptor(id);
        let park = park_position(&desc, self.screen);
        self.windows.insert(
            id,
            WidgetWindow {
                pos: park,
                from: park,
                target: desc.home,
                motion: WidgetMotion::Sliding,
                progress: 0,
                park,
                style: desc.kind as u8,
            },
        );
    }

    /// `FUN_800357FC` - start a slide from the live position to `position`
    /// (tail `0x80035874`). Ops `0x01` / `0x02`. Progress is untouched.
    fn slide_to(&mut self, id: u8, position: vm::Position) {
        if let Some(w) = self.windows.get_mut(&id) {
            w.from = w.pos;
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
            w.motion = WidgetMotion::Still;
        }
    }

    /// `FUN_80035978` - begin the close; the node stays on the list.
    fn begin_close(&mut self, id: u8) {
        if let Some(w) = self.windows.get_mut(&id) {
            begin_close_window(w);
        }
    }

    /// `FUN_80035A4C` - [`Self::begin_close`] on every window.
    fn close_all(&mut self) {
        self.global_updates += 1;
        for w in self.windows.values_mut() {
            begin_close_window(w);
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
    /// starts where it ends has nothing to animate.
    fn snap_clear_condition(&self, id: u8) -> bool {
        self.windows.get(&id).is_some_and(|w| w.from == w.target)
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
        let mut st = MenuWidgetState::default();
        let mut descs = vec![WidgetDescriptor::default(); 8];
        descs[2].home = vm::Position::new(40, 60);
        st.set_descriptors(descs);
        vm::run(&mut st, SYNTH).unwrap();
        assert_eq!(st.open_ids(), vec![2]);
        assert_eq!(st.window(2).unwrap().target, vm::Position::new(40, 60));
        assert_eq!(st.global_updates, 1);
    }

    #[test]
    fn slide_ops_slide_and_snap_ops_snap() {
        // Retail: ops 01/02 -> FUN_800357FC (slide, +0x20 = 1); ops 09/0A ->
        // FUN_800358C0 (snap, +0x20 = 0); op 08 -> FUN_800319A8 (destroy).
        // Park class 8 creates a window at home.
        let mut st = MenuWidgetState::default();
        st.set_descriptors(vec![
            WidgetDescriptor {
                home: vm::Position::new(10, 20),
                park_edge: 8,
                ..Default::default()
            };
            8
        ]);
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
