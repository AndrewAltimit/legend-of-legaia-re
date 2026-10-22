//! The title's CONTINUE screen both play hosts run: the retail save-select
//! over the console's two memory-card ports, opened by the title rather than
//! by the pause menu.
//!
//! Retail runs Continue inside the title overlay's own card manager
//! (`FUN_801DDCCC`; [`SaveSelectSession::title_continue`] carries the
//! sub-mode chain). No field runs under the title, so no pause-menu gate
//! stands in front of it, and a backed-out Continue returns to the title
//! card. [`ContinueScreen`] is that screen as one object: the save-select
//! session, the shared save-screen driver ([`SaveScreenFlow`]: the card read,
//! the 5x3 block-grid cursor, the empty-block Load gate, the refusal notice),
//! and the frame order that runs them.
//!
//! The native window and the browser play page used to host it separately -
//! the window as a standalone boot state over its own `save_flow`, the page
//! as a title-opened pause-menu sub-screen - and their frame orders differed
//! (where the refusal notice took the pad, whether the screen's cursor blips
//! fired). One owner leaves each host only its glue: the pad word in, the
//! [`ContinueStep`] out, the draws through
//! `legaia_engine_screens::pause_screens::save_select_overlay_draws` over
//! [`ContinueScreen::flow`] / [`ContinueScreen::session`], and the landing of
//! a loaded save through the host's own scene entry
//! ([`crate::BootSession::resume_save`] natively, `play_resume_save` on the
//! page).
//!
//! The card ports are [`CardPorts`]: both hosts mount every port as a
//! [`MountedCard`], so the rack, the block read and the Continue-enable scan
//! are decided here once ([`card_port_rack`], [`card_port_blocks`],
//! [`rack_has_save`]).

use legaia_engine_menus::menu_cues::{MenuBlip, menu_edge_blip};
use legaia_engine_menus::save_screen::{
    SLOT_GRID_CELLS, SaveCommitKind, SaveRefusal, SaveScreenFlow, card_port_snapshot,
};
use legaia_engine_menus::save_select::{
    SaveRack, SaveSelectSession, SelectInput, SelectOutcome, SlotSnapshot, card_block_snapshots,
};
use legaia_save::emu::MountedCard;

/// Memory-card ports the console has: retail's save screen is `SLOT 1` /
/// `SLOT 2` and nothing else.
pub const CARD_PORTS: usize = 2;

/// A host's two card ports, each holding a mounted card image or nothing.
pub type CardPorts<'a> = [Option<&'a MountedCard>; CARD_PORTS];

/// The save rack over `ports`: retail's two-stage card flow, each pill built
/// through [`card_port_snapshot`] (the card's label when one is mounted).
pub fn card_port_rack(ports: CardPorts<'_>) -> SaveRack {
    SaveRack::CardPorts(
        (0..CARD_PORTS as u8)
            .map(|port| card_port_snapshot(port, ports[port as usize].map(|c| c.label.as_str())))
            .collect(),
    )
}

/// The fifteen blocks behind `port` - the answer to
/// [`SaveScreenFlow::pending_read`]. A port with nothing in it reads fifteen
/// free cells, never the other port's: the grid previews the port picked.
pub fn card_port_blocks(ports: CardPorts<'_>, port: u8) -> Vec<SlotSnapshot> {
    match ports.get(port as usize).copied().flatten() {
        Some(card) => card_block_snapshots(card),
        None => (0..SLOT_GRID_CELLS).map(SlotSnapshot::empty).collect(),
    }
}

/// Whether any port holds a readable save: the title's Continue row is live
/// exactly then (a port guard with no retail counterpart; see
/// `docs/subsystems/save-screen.md`).
pub fn rack_has_save(ports: CardPorts<'_>) -> bool {
    ports
        .iter()
        .flatten()
        .any(|card| card_block_snapshots(card).iter().any(|b| b.present))
}

/// A save the Continue screen loaded, for the host to land.
#[derive(Debug, Clone)]
pub struct ContinueLoad {
    /// The port the save was read from (0-based).
    pub port: u8,
    /// The card block it occupies (1..=15; block 0 is the directory).
    pub block: u8,
    /// The save itself.
    pub save: legaia_save::SaveFile,
    /// The scene the save resumes in (empty when it names none).
    pub scene: String,
}

/// What one [`ContinueScreen::step`] did.
#[derive(Debug, Clone)]
pub enum ContinueStep {
    /// The screen is still up. `blip` is the menu cue this frame's edge
    /// earns, off the edge the refusal notice and the block grid let
    /// through (a blocked Load on an empty block is silent).
    Running { blip: Option<MenuBlip> },
    /// The player backed out: the host puts the title card back up.
    BackedOut { blip: Option<MenuBlip> },
    /// A block was confirmed and read: the host lands the save and drops
    /// the screen.
    Loaded {
        load: Box<ContinueLoad>,
        blip: Option<MenuBlip>,
    },
}

impl ContinueStep {
    /// The frame's menu cue, whichever way the step went.
    pub fn blip(&self) -> Option<MenuBlip> {
        match self {
            Self::Running { blip } | Self::BackedOut { blip } | Self::Loaded { blip, .. } => *blip,
        }
    }
}

/// The title's Continue screen ([module docs](self)).
#[derive(Debug, Clone)]
pub struct ContinueScreen {
    select: SaveSelectSession,
    flow: SaveScreenFlow,
}

impl ContinueScreen {
    /// Open Continue over `ports`: the Load screen
    /// ([`SaveSelectSession::title_continue`]) with a fresh driver.
    pub fn open(ports: CardPorts<'_>) -> Self {
        Self {
            select: SaveSelectSession::title_continue(&card_port_rack(ports)),
            flow: SaveScreenFlow::new(),
        }
    }

    /// The save-select session (what the screen draws).
    pub fn session(&self) -> &SaveSelectSession {
        &self.select
    }

    /// The session, mutably (a test shortening the "Now checking" beat).
    pub fn session_mut(&mut self) -> &mut SaveSelectSession {
        &mut self.select
    }

    /// The save-screen driver (the block grid, the refusal notice).
    pub fn flow(&self) -> &SaveScreenFlow {
        &self.flow
    }

    /// Whether the title art stays up behind the screen this frame: retail
    /// keeps the dimmed title strips behind the slot pills
    /// (`FUN_801DD35C` calls `FUN_801E0418` while `_DAT_8007BB00` is set)
    /// and pivots to black once a slot is confirmed.
    pub fn shows_title_backdrop(&self) -> bool {
        self.select.phase().shows_title_backdrop()
    }

    /// Step the screen one frame on `edge`, the frame's just-pressed pad
    /// bits, over the host's `ports`.
    ///
    /// The order: a refusal notice owns the pad while it is up (the edge
    /// that dismisses it drives nothing behind it); then the card read the
    /// driver is waiting on is answered off `ports`; then the block-grid
    /// cursor and the empty-block Load gate filter the edge
    /// ([`SaveScreenFlow::before_tick`]) before the session ticks on it, so
    /// a confirm commits the cell on screen. A confirmed block is read off
    /// its card; a block that will not read raises the `CardReadFailed`
    /// notice over a fresh screen instead of closing on nothing.
    // REF: FUN_801DDCCC
    pub fn step(&mut self, edge: u16, ports: CardPorts<'_>) -> ContinueStep {
        if self.flow.tick_refusal(edge) {
            return ContinueStep::Running { blip: None };
        }
        if let Some(port) = self.flow.pending_read(&self.select) {
            self.flow
                .install_blocks(port, card_port_blocks(ports, port));
        }
        let edge = self.flow.before_tick(&mut self.select, edge);
        // Inside the save screen Start is an ordinary edge, not a close.
        let blip = menu_edge_blip(edge, false);
        let _ = self.select.tick(SelectInput::from_pad_edge(edge));
        let Some(outcome) = self.select.outcome() else {
            return ContinueStep::Running { blip };
        };
        if !matches!(outcome, SelectOutcome::Loaded(_)) {
            // Cancelled is the only other outcome a Load screen reaches.
            return ContinueStep::BackedOut { blip };
        }
        let read = self
            .flow
            .commit(&self.select)
            .filter(|c| c.kind == SaveCommitKind::Load)
            .and_then(|c| {
                let card = ports.get(c.port as usize).copied().flatten()?;
                let (save, resume) = card.save_at(c.cell)?;
                Some(ContinueLoad {
                    port: c.port,
                    block: c.cell + 1,
                    save,
                    scene: resume.scene,
                })
            });
        match read {
            Some(load) => {
                log::info!(
                    "continue: loaded port {} block {} (scene '{}')",
                    load.port + 1,
                    load.block,
                    load.scene
                );
                ContinueStep::Loaded {
                    load: Box::new(load),
                    blip,
                }
            }
            None => {
                log::warn!("continue: the confirmed block holds no readable save");
                self.select = SaveSelectSession::title_continue(&card_port_rack(ports));
                self.flow.reset();
                self.flow.refuse(SaveRefusal::CardReadFailed);
                ContinueStep::Running { blip }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_menus::save_select::SelectPhase;

    const CROSS: u16 = 0x4000;
    const CIRCLE: u16 = 0x2000;

    fn blank_card() -> MountedCard {
        MountedCard::from_bytes(legaia_save::card::formatted_card_image(), "blank")
            .expect("a formatted card mounts")
    }

    #[test]
    fn rack_pills_follow_the_mounted_ports() {
        let card = blank_card();
        let rack = card_port_rack([None, Some(&card)]);
        assert!(rack.is_card_ports());
        let pills = rack.slots();
        assert_eq!(pills.len(), CARD_PORTS);
        assert!(!pills[0].present);
        assert!(pills[1].present);
        assert_eq!(pills[1].label, "blank");
        assert!(
            !rack_has_save([None, Some(&card)]),
            "a blank card has no save"
        );
        assert_eq!(
            card_port_blocks([None, None], 0).len(),
            SLOT_GRID_CELLS as usize
        );
    }

    #[test]
    fn circle_on_the_pills_backs_out() {
        let card = blank_card();
        let ports = [Some(&card), None];
        let mut screen = ContinueScreen::open(ports);
        assert!(screen.shows_title_backdrop());
        match screen.step(CIRCLE, ports) {
            ContinueStep::BackedOut { blip } => assert_eq!(blip, Some(MenuBlip::Cancel)),
            other => panic!("expected a back-out, got {other:?}"),
        }
    }

    /// A confirm on a blank card's grid never loads: the empty-block gate
    /// swallows it, and the screen stays up.
    #[test]
    fn a_blank_card_never_loads() {
        let card = blank_card();
        let ports = [Some(&card), None];
        let mut screen = ContinueScreen::open(ports);
        screen.session_mut().set_now_checking_frames(1);
        for f in 0..400u32 {
            let edge = if f % 4 == 0 { CROSS } else { 0 };
            match screen.step(edge, ports) {
                ContinueStep::Running { .. } => {}
                other => panic!("frame {f}: {other:?}"),
            }
        }
        assert!(matches!(
            screen.session().phase(),
            SelectPhase::SlotPreview { .. } | SelectPhase::NowChecking { .. }
        ));
    }
}
