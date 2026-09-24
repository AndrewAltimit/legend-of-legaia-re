//! World-map **travel arts** - the Riremito and Rula actor handlers.
//!
//! `FUN_801EE094` (Riremito, string `"ON RIREMITO"`) and `FUN_801EE328`
//! (Rula, `"ON RULA"`) are two actor handlers in the field overlay's
//! world-map band (PROT 0897, base `0x801CE818`). They are the same machine
//! with different dwell constants and a different pre-warp flourish: a phase
//! halfword `actor[+0x54]`, a dwell counter `actor[+0x9E]` accumulating
//! `DAT_1F800393` per frame, and a shared phase that resolves the party's
//! current world map into a stored **visited-map record** and warps to it.
//!
//! ## The shared resolve-and-warp kernel
//!
//! The visited-map table is `count` records of `0x10` bytes, based at the
//! global buffer `FUN_80019788()` returns. Each record carries its map name
//! at `+0xC`, and the scan compares `FUN_8003CE9C(record + 0xC)` against the
//! party's current map id at `0x80084628`. On a miss the actor parks in the
//! diagnostic phase `0x63`, which prints `"UNFIND MAP NUMBER %d"` and does
//! nothing else - the travel art simply never fires.
//!
//! On a hit the handler installs the destination:
//!
//! ```text
//!   FUN_8001FD44(record, current_map_id)   // stage the scene
//!   *0x80073EFC = 0
//!   *0x80073EF4 = (*0x80084624 << 7) + 0x40
//!   *0x80073EF8 = (*0x8008462C << 7) + 0x40
//! ```
//!
//! `<< 7` plus a half-tile `0x40` is the 128-unit field tile grid's
//! tile-index → world-centre conversion, the same law the walk collision
//! probe uses (see `docs/subsystems/field-locomotion.md`). The two source
//! words are the party's stored tile X and tile Z; the third store zeroes the
//! Y term.
//!
//! ## Where the two differ
//!
//! | | Riremito `FUN_801EE094` | Rula `FUN_801EE328` |
//! |---|---|---|
//! | phase 0 | `FUN_8003CE08(0x0B)`, `FUN_801D5A24(1)` | `FUN_8003CE08(0x0B)`, `FUN_801D5A24(0)` |
//! | phase 1 dwell | `0x50`, then spawns the flash quad | `0x28`, no quad |
//! | phase 2 | dwell `0x28`, no quad | the **lift**: the player rises until above `-0x618`, then the quad |
//! | phase 3 | resolve + warp, restoring player `+0x72` / `+0x10` | resolve + warp |
//!
//! Rula's phase 2 (`0x801EE400..0x801EE4C4`) is not a dwell. Every frame it
//! sets the player actor's (`*0x8007C364`) `+0x10` bit 0 and the scratchpad
//! word `0x1F800394`'s bit 24, adds the frame step to `+0x9E` - which here is
//! a **velocity**, zeroed by phase 1's exit - and subtracts that velocity
//! from the player's Y `+0x16`. When Y drops below `-0x618` it clears
//! `0x8007B6B4`, spawns the fade quad (`FUN_80024E80`) and advances; the
//! player flies up out of shot on an accelerating arc before the warp.
//!
//! Only Riremito's resolve phase touches the player
//! (`0x801EE268..0x801EE294`): on a hit it stores render scale `0x1000` to
//! `+0x72` and clears `+0x10` bit `0x200000` before the warp. Rula's resolve
//! (`0x801EE4C8..`) stores neither.
//!
//! Both phase-1 bodies are additionally gated on `FUN_8003CE64(0x0B)`
//! returning zero, so the dwell does not start until the effect the phase-0
//! call queued has finished.
//!
//! `see ghidra/scripts/funcs/801ee094.txt`,
//! `see ghidra/scripts/funcs/801ee328.txt`

/// Stride of one visited-map record.
pub const VISITED_RECORD_STRIDE: usize = 0x10;
/// Offset of the map-name field inside a visited-map record.
pub const VISITED_NAME_OFFSET: usize = 0xC;
/// The diagnostic phase both handlers park in when the scan misses.
pub const PHASE_UNFOUND: u16 = 0x63;

/// Rula's lift ends once the player's Y (`+0x16`) is below this
/// (`slti v0,v0,-0x618` at `0x801EE464`).
pub const RULA_LIFT_EXIT_Y: i16 = -0x618;

/// The bit Rula's lift sets in the player's `+0x10` each frame.
pub const RULA_LIFT_PLAYER_FLAG: u32 = 0x1;

/// The bit Rula's lift sets in the scratchpad word `0x1F800394`.
pub const RULA_LIFT_SCRATCH_FLAG: u32 = 0x0100_0000;

/// Render scale Riremito's resolve stores to the player's `+0x72`.
pub const RIREMITO_RESTORE_SCALE: i16 = 0x1000;

/// The bit Riremito's resolve clears in the player's `+0x10`.
pub const RIREMITO_RESTORE_CLEARED_FLAG: u32 = 0x0020_0000;

/// Which travel art a handler is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TravelArt {
    /// `FUN_801EE094` - flourish on phase 1, dwell `0x50` then `0x28`.
    Riremito,
    /// `FUN_801EE328` - flourish on phase 2, dwell `0x28` on phase 1.
    Rula,
}

impl TravelArt {
    /// The `FUN_801D5A24` argument phase 0 passes.
    pub fn phase0_arg(self) -> u32 {
        match self {
            TravelArt::Riremito => 1,
            TravelArt::Rula => 0,
        }
    }

    /// The phase-1 dwell threshold.
    pub fn phase1_dwell(self) -> i16 {
        match self {
            TravelArt::Riremito => 0x50,
            TravelArt::Rula => 0x28,
        }
    }

    /// The phase-2 dwell threshold - Riremito only. Rula's phase 2 is the
    /// lift, which ends on the player's height ([`RULA_LIFT_EXIT_Y`]).
    pub fn phase2_dwell(self) -> Option<i16> {
        match self {
            TravelArt::Riremito => Some(0x28),
            TravelArt::Rula => None,
        }
    }

    /// The phase whose completion spawns the screen-flash quad.
    pub fn flash_phase(self) -> u16 {
        match self {
            TravelArt::Riremito => 1,
            TravelArt::Rula => 2,
        }
    }
}

/// Tile index -> world coordinate: `(tile << 7) + 0x40`, the centre of a
/// 128-unit field tile.
///
/// PORT: FUN_801ee094 (`0x801EE2B4..0x801EE2DC` destination store)
///
/// Wired: through [`destination_for`], whose host is
/// `legaia_engine_core::world_map_panel_host`.
pub fn tile_centre(tile: i32) -> i32 {
    (tile << 7) + 0x40
}

/// The destination the resolve phase installs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TravelDestination {
    /// Index of the matched visited-map record.
    pub record_index: usize,
    /// World X, `(tile_x << 7) + 0x40`.
    pub x: i32,
    /// World Y - retail stores a literal zero here.
    pub y: i32,
    /// World Z, `(tile_z << 7) + 0x40`.
    pub z: i32,
}

/// Scan the visited-map table for the record whose name field resolves to
/// `current_map`. `map_id_of` is the caller's binding for `FUN_8003CE9C`
/// applied to `record + 0xC`.
///
/// Returns `None` for the retail miss, which parks the actor in
/// [`PHASE_UNFOUND`].
///
/// PORT: FUN_801ee094 (`0x801EE1F0..0x801EE264` scan)
/// REF: FUN_801ee328 (the byte-identical scan in the Rula handler)
///
/// Wired: `legaia_engine_core::world_map_panel_host::PanelActorHost` passes
/// this as the `resolve` closure's scan half, over the visited-map table it
/// records from the overworld tick.
pub fn find_visited_map(
    count: usize,
    current_map: u32,
    map_id_of: impl Fn(usize) -> u32,
) -> Option<usize> {
    (0..count).find(|&i| map_id_of(i) == current_map)
}

/// Build the destination for a matched record.
///
/// PORT: FUN_801ee094 (`0x801EE268..0x801EE2E4`)
///
/// Wired: the other half of the `resolve` closure
/// `legaia_engine_core::world_map_panel_host::PanelActorHost` supplies.
pub fn destination_for(record_index: usize, tile_x: i32, tile_z: i32) -> TravelDestination {
    TravelDestination {
        record_index,
        x: tile_centre(tile_x),
        y: 0,
        z: tile_centre(tile_z),
    }
}

/// One frame of a travel-art handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TravelArtFrame {
    /// Queue the phase-0 effect (`FUN_8003CE08(0x0B)` + `FUN_801D5A24(arg)`).
    pub queue_effect: Option<u32>,
    /// Spawn the full-screen flash quad this frame.
    pub spawn_flash: bool,
    /// The warp to apply, on the frame the resolve phase succeeds.
    pub destination: Option<TravelDestination>,
    /// The scan missed; the handler parked in the diagnostic phase.
    pub unfound: bool,
    /// Rula's lift ran this frame: the player's new Y (`+0x16`), after the
    /// handler set [`RULA_LIFT_PLAYER_FLAG`] on the player and
    /// [`RULA_LIFT_SCRATCH_FLAG`] in `0x1F800394`.
    pub lift_player_y: Option<i16>,
    /// Riremito's resolve hit: the player's `+0x72` is set to
    /// [`RIREMITO_RESTORE_SCALE`] and [`RIREMITO_RESTORE_CLEARED_FLAG`] is
    /// cleared from its `+0x10`.
    pub restore_player: bool,
}

/// Riremito / Rula actor state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TravelArtActor {
    /// Which of the two handlers this is.
    pub art: TravelArt,
    /// Phase halfword `actor[+0x54]`.
    pub phase: u16,
    /// Dwell counter `actor[+0x9E]` - in Rula's phase 2, the lift velocity.
    pub dwell: i16,
    /// The player actor's Y (`+0x16`) the lift moves. Seed it with
    /// [`Self::with_player_y`]; a host that never does lifts from `0`.
    pub player_y: i16,
}

impl TravelArtActor {
    /// Fresh actor at phase `0`.
    pub fn new(art: TravelArt) -> Self {
        Self {
            art,
            phase: 0,
            dwell: 0,
            player_y: 0,
        }
    }

    /// Seed the player's Y the Rula lift starts from.
    pub fn with_player_y(self, y: i16) -> Self {
        Self {
            player_y: y,
            ..self
        }
    }

    /// Advance one frame.
    ///
    /// `effect_busy` is `FUN_8003CE64(0x0B) != 0`, the gate phase 1 waits on;
    /// `frame_delta` is `DAT_1F800393`. `resolve` is the caller's binding of
    /// the scan-and-warp kernel: it returns the destination, or `None` for
    /// the miss that parks the actor in [`PHASE_UNFOUND`].
    ///
    /// PORT: FUN_801ee094
    /// PORT: FUN_801ee328
    ///
    /// Wired: `PanelActorKind::TravelArt` in
    /// `legaia_engine_core::world_map_panel_host`. Retail installs the actor
    /// from the Arts menu, which the engine does not model; the port binds it
    /// to the world-map sub-list picker's state-3 hand-off instead, and that
    /// binding is named on `World::tick_world_map_panels`.
    ///
    /// The `effect_busy` gate is passed as `false` there - the engine has no
    /// effect queue for `FUN_8003CE64(0x0B)` to report on, so the dwell starts
    /// on the frame after the install rather than after the flourish clears.
    /// Neither host applies [`TravelArtFrame::lift_player_y`] or
    /// [`TravelArtFrame::restore_player`] yet: the panel host carries no
    /// handle on the player actor, so the Rula lift's arc times the phase
    /// but moves nothing on screen.
    pub fn tick(
        &mut self,
        effect_busy: bool,
        frame_delta: i16,
        resolve: impl FnOnce() -> Option<TravelDestination>,
    ) -> TravelArtFrame {
        let mut out = TravelArtFrame::default();
        match self.phase {
            0 => {
                out.queue_effect = Some(self.art.phase0_arg());
                self.dwell = 0;
                self.phase = 1;
            }
            1 => {
                if effect_busy {
                    return out;
                }
                self.dwell = self.dwell.wrapping_add(frame_delta);
                if self.dwell < self.art.phase1_dwell() {
                    return out;
                }
                out.spawn_flash = self.art.flash_phase() == 1;
                self.dwell = 0;
                self.phase = 2;
            }
            2 => match self.art.phase2_dwell() {
                Some(threshold) => {
                    self.dwell = self.dwell.wrapping_add(frame_delta);
                    if self.dwell < threshold {
                        return out;
                    }
                    out.spawn_flash = self.art.flash_phase() == 2;
                    self.dwell = 0;
                    self.phase = 3;
                }
                None => {
                    // Rula's lift: `+0x9E` is a velocity, never reset here.
                    self.dwell = self.dwell.wrapping_add(frame_delta);
                    self.player_y = self.player_y.wrapping_sub(self.dwell);
                    out.lift_player_y = Some(self.player_y);
                    if self.player_y < RULA_LIFT_EXIT_Y {
                        out.spawn_flash = true;
                        self.phase = 3;
                    }
                }
            },
            3 => match resolve() {
                Some(dest) => {
                    out.destination = Some(dest);
                    out.restore_player = self.art == TravelArt::Riremito;
                    self.phase = 4;
                }
                None => {
                    out.unfound = true;
                    self.phase = PHASE_UNFOUND;
                }
            },
            _ => {}
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_centre_is_half_tile_offset() {
        assert_eq!(tile_centre(0), 0x40);
        assert_eq!(tile_centre(1), 0xC0);
        assert_eq!(tile_centre(3), 0x1C0);
    }

    #[test]
    fn scan_finds_the_current_map() {
        let ids = [7u32, 9, 12, 4];
        assert_eq!(find_visited_map(4, 12, |i| ids[i]), Some(2));
        assert_eq!(find_visited_map(4, 99, |i| ids[i]), None);
    }

    #[test]
    fn scan_takes_the_first_match() {
        let ids = [5u32, 5, 5];
        assert_eq!(find_visited_map(3, 5, |i| ids[i]), Some(0));
    }

    #[test]
    fn destination_zeroes_the_y_term() {
        let d = destination_for(1, 2, 3);
        assert_eq!((d.x, d.y, d.z), (0x140, 0, 0x1C0));
    }

    #[test]
    fn riremito_flashes_on_phase_one() {
        let mut a = TravelArtActor::new(TravelArt::Riremito);
        assert_eq!(a.tick(false, 1, || None).queue_effect, Some(1));
        // Gated while the queued effect is busy.
        for _ in 0..10 {
            assert!(!a.tick(true, 1, || None).spawn_flash);
        }
        assert_eq!(a.dwell, 0);
        for _ in 0..TravelArt::Riremito.phase1_dwell() - 1 {
            assert!(!a.tick(false, 1, || None).spawn_flash);
        }
        assert!(a.tick(false, 1, || None).spawn_flash);
        assert_eq!(a.phase, 2);
    }

    #[test]
    fn rula_lifts_the_player_on_an_accelerating_arc_then_flashes() {
        let mut a = TravelArtActor::new(TravelArt::Rula);
        assert_eq!(a.tick(false, 1, || None).queue_effect, Some(0));
        for _ in 0..TravelArt::Rula.phase1_dwell() {
            a.tick(false, 1, || None);
        }
        assert_eq!(a.phase, 2);
        assert_eq!(a.dwell, 0, "phase 1's exit zeroes the velocity");
        // Step 2: velocity 2, 4, 6, ...; Y = -n(n+1). n = 39 lands exactly
        // on -0x618 (-1560), which is not below it; n = 40 (-1640) is.
        let mut frames = 0;
        let mut last_y = 0;
        loop {
            let f = a.tick(false, 2, || None);
            frames += 1;
            let y = f.lift_player_y.expect("the lift writes Y every frame");
            assert!(y < last_y, "the player rises every frame");
            last_y = y;
            if f.spawn_flash {
                break;
            }
            assert!(frames < 100);
        }
        assert_eq!(last_y, -(frames * (frames + 1)) as i16);
        assert!(last_y < RULA_LIFT_EXIT_Y);
        assert!(-((frames - 1) * frames) >= i32::from(RULA_LIFT_EXIT_Y));
        assert_eq!(a.phase, 3);
        // Rula's resolve does not restore the player.
        let f = a.tick(false, 2, || Some(destination_for(0, 0, 0)));
        assert!(f.destination.is_some() && !f.restore_player);
    }

    #[test]
    fn riremito_resolve_restores_the_player() {
        let mut a = TravelArtActor::new(TravelArt::Riremito);
        a.phase = 3;
        let f = a.tick(false, 1, || Some(destination_for(0, 0, 0)));
        assert!(f.restore_player);
        assert_eq!(f.lift_player_y, None);
    }

    #[test]
    fn miss_parks_in_the_diagnostic_phase() {
        let mut a = TravelArtActor::new(TravelArt::Rula);
        a.phase = 3;
        let f = a.tick(false, 1, || None);
        assert!(f.unfound);
        assert_eq!(a.phase, PHASE_UNFOUND);
        // The diagnostic phase is terminal for the state machine.
        let f = a.tick(false, 1, || Some(destination_for(0, 0, 0)));
        assert_eq!(f, TravelArtFrame::default());
    }

    #[test]
    fn hit_emits_the_destination_once() {
        let mut a = TravelArtActor::new(TravelArt::Riremito);
        a.phase = 3;
        let f = a.tick(false, 1, || Some(destination_for(2, 5, 6)));
        assert_eq!(
            f.destination,
            Some(TravelDestination {
                record_index: 2,
                x: 0x2C0,
                y: 0,
                z: 0x340,
            })
        );
        assert_eq!(a.phase, 4);
        assert_eq!(a.tick(false, 1, || None).destination, None);
    }
}
