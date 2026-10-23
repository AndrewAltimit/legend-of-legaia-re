//! `PanelWindowHost`: the panel's windows and the window scripts that drive them.

use super::*;

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
    /// `obj[+0x20] == 1`: set by the `op 1` / `op 2` slide arms
    /// (`FUN_800357FC`), cleared by the `op 9` / `op 10` snap arms
    /// (`FUN_800358C0`); a renderer eases toward `(x, y)` while it holds.
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
    /// [`crate::world_map_panel::party_panel_geometry`]), the close
    /// script closes it, the fill-fade script clears the screen, and the
    /// flag-window script opens descriptor `14` at the geometry the picker
    /// just wrote into it.
    pub fn stand_in() -> Self {
        use crate::world_map_panel_actors as actors;
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
        // `0x801F2A88` is also the draw tick's install script
        // (`crate::submode_hub_actors::PANEL_DRAW_TICK`, `FUN_801F20B0`):
        // one `CloseAll` record, the shape the field overlay's own copy has.
        s.set(
            actors::TEXT_BOX_DECLINE_SCRIPT,
            vec![cmd(5, 0, 0), cmd(0, 0, 0)],
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

    pub(super) fn apply(&mut self, effect: PanelEffect) {
        match effect {
            PanelEffect::OpenAtDescriptor { panel, x, y } | PanelEffect::OpenAt { panel, x, y } => {
                if let Some(i) = Self::slot(panel) {
                    let w = self.windows[i].get_or_insert_with(PanelWindow::default);
                    w.x = x;
                    w.y = y;
                    w.sliding = true;
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
            PanelEffect::SnapTo { panel, x, y } => {
                if let Some(i) = Self::slot(panel) {
                    let w = self.windows[i].get_or_insert_with(PanelWindow::default);
                    w.x = x;
                    w.y = y;
                    w.sliding = false;
                }
            }
            PanelEffect::Respawn { panel } => {
                // Retail retires and respawns, snapping the fresh object back
                // to where the live one already was - so the observable result
                // is the object at rest at its own position.
                if let Some(w) = Self::slot(panel).and_then(|i| self.windows[i].as_mut()) {
                    w.sliding = false;
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
