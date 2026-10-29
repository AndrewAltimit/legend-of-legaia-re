//! Battle waves: a pack formation's later waves, seated into the freed
//! monster seats of the battle in progress.
//!
//! The battle's monster block is [`MAX_FORMATION_SLOTS`] seats wide (the
//! actor table's eight seats less the party's three and the summon seat).
//! A formation fills it with its first wave as any formation does; the
//! waves after it ([`legaia_expansion_format::battle::FormationDef::waves`])
//! wait in a [`WaveRun`], which a `World` asks for arrivals at two safe
//! points:
//!
//! - [`Moment::Wipe`]: the action that downed the last standing monster has
//!   resolved (the battle action SM's end-of-action wipe scan). A run with a
//!   wave still to come keeps the battle going and seats it now, so no frame
//!   ever sees a fight with nobody on the monster side.
//! - [`Moment::RoundStart`]: a round is about to open. Only a `refill` wave
//!   arrives here, one monster per freed seat.
//!
//! Every monster the run seats is recorded ([`WaveRun::seated`]); the
//! `World` adds them to the fight's formation, so the one victory at the end
//! pays the spoils of every wave together.

use legaia_expansion_format::battle::{MAX_FORMATION_SLOTS, WaveArrival};

/// One compiled wave after a formation's first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledWave {
    /// Base monster ids, in arrival order.
    pub monsters: Vec<u16>,
    pub arrival: WaveArrival,
    /// Arrivals in all (a `wipe` wave: its monster count).
    pub count: u16,
    /// A `refill` wave sends nobody while this many monsters stand.
    pub max_standing: u8,
}

impl CompiledWave {
    /// The monster id of arrival `n` (a refill wave cycles its list).
    pub fn arrival_id(&self, n: u16) -> Option<u16> {
        if self.monsters.is_empty() {
            return None;
        }
        self.monsters
            .get(usize::from(n) % self.monsters.len())
            .copied()
    }
}

/// When the battle asks a [`WaveRun`] for arrivals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Moment {
    /// Every monster standing is down.
    Wipe,
    /// A round is about to open.
    RoundStart,
}

/// What one ask of a [`WaveRun`] seats.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Arrivals {
    /// `(monster seat, base monster id)`, a seat being `0..MAX_FORMATION_SLOTS`
    /// within the monster block.
    pub seats: Vec<(usize, u16)>,
    /// The monster block's new seat layout (the count the stage seat table
    /// is read with), when it changes: every seat below it is placed anew.
    pub layout: Option<u8>,
    /// The 1-based number of each wave whose first monster arrived, for
    /// the "Wave N" banner.
    pub waves_begun: Vec<usize>,
}

impl Arrivals {
    pub fn is_empty(&self) -> bool {
        self.seats.is_empty()
    }
}

/// The waves still to come in one battle.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WaveRun {
    waves: Vec<CompiledWave>,
    /// Index into `waves` of the wave arriving next.
    current: usize,
    /// Arrivals the current wave has already sent.
    sent: u16,
    /// The monster block's seat layout count.
    layout: u8,
    /// Every monster seated after the first wave, in order.
    pub seated: Vec<u16>,
}

impl WaveRun {
    /// A run over `waves`, the battle having opened on `first_wave`
    /// monsters.
    pub fn new(waves: Vec<CompiledWave>, first_wave: usize) -> Self {
        Self {
            waves,
            layout: first_wave.clamp(1, MAX_FORMATION_SLOTS) as u8,
            ..Default::default()
        }
    }

    /// Whether a wave is still to come.
    pub fn pending(&self) -> bool {
        self.current < self.waves.len()
    }

    /// The 1-based number of the wave arriving next (the first wave is 1).
    pub fn next_wave_number(&self) -> usize {
        self.current + 2
    }

    /// Waves in the battle, the first included.
    pub fn total_waves(&self) -> usize {
        self.waves.len() + 1
    }

    /// The monster block's seat layout count.
    pub fn layout(&self) -> u8 {
        self.layout
    }

    /// Seat what `moment` brings. `standing[k]` says whether monster seat
    /// `k` holds a monster still standing; a seat that does not is free.
    pub fn arrivals(&mut self, moment: Moment, standing: &[bool]) -> Arrivals {
        let mut out = Arrivals::default();
        let mut taken: Vec<bool> = (0..MAX_FORMATION_SLOTS)
            .map(|k| standing.get(k).copied().unwrap_or(false))
            .collect();
        while let Some(wave) = self.waves.get(self.current).cloned() {
            let up = taken.iter().filter(|&&t| t).count();
            match wave.arrival {
                WaveArrival::Wipe => {
                    // A wipe wave waits for the field to clear, and takes
                    // the whole moment: the block is laid out for it alone.
                    if up != 0 || moment != Moment::Wipe || !out.is_empty() {
                        break;
                    }
                    let n = wave.monsters.len().min(MAX_FORMATION_SLOTS);
                    out.seats
                        .extend(wave.monsters.iter().take(n).copied().enumerate());
                    out.waves_begun.push(self.next_wave_number());
                    self.set_layout(n as u8, &mut out);
                    self.seated.extend(wave.monsters.iter().take(n));
                    self.current += 1;
                    self.sent = 0;
                    break;
                }
                WaveArrival::Refill => {
                    let cap = usize::from(wave.max_standing).clamp(1, MAX_FORMATION_SLOTS);
                    // The block widens to the wave's standing cap once, as
                    // the wave starts (the standing monsters move to the
                    // wider layout's seats).
                    if self.sent == 0 && usize::from(self.layout) < cap {
                        self.set_layout(cap as u8, &mut out);
                    }
                    let mut up = up;
                    while self.sent < wave.count && up < cap {
                        let Some(seat) =
                            (0..usize::from(self.layout).min(cap)).find(|&k| !taken[k])
                        else {
                            break;
                        };
                        let Some(id) = wave.arrival_id(self.sent) else {
                            break;
                        };
                        if self.sent == 0 {
                            out.waves_begun.push(self.next_wave_number());
                        }
                        taken[seat] = true;
                        up += 1;
                        out.seats.push((seat, id));
                        self.seated.push(id);
                        self.sent += 1;
                    }
                    if self.sent >= wave.count {
                        self.current += 1;
                        self.sent = 0;
                        continue;
                    }
                    break;
                }
            }
        }
        out
    }

    fn set_layout(&mut self, layout: u8, out: &mut Arrivals) {
        if layout != self.layout {
            self.layout = layout;
            out.layout = Some(layout);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wipe(ids: &[u16]) -> CompiledWave {
        CompiledWave {
            monsters: ids.to_vec(),
            arrival: WaveArrival::Wipe,
            count: ids.len() as u16,
            max_standing: MAX_FORMATION_SLOTS as u8,
        }
    }

    fn refill(ids: &[u16], count: u16, max_standing: u8) -> CompiledWave {
        CompiledWave {
            monsters: ids.to_vec(),
            arrival: WaveArrival::Refill,
            count,
            max_standing,
        }
    }

    #[test]
    fn a_wipe_wave_waits_for_the_field_to_clear() {
        let mut run = WaveRun::new(vec![wipe(&[5, 6, 7])], 2);
        assert!(run.pending());
        // One monster still standing: nothing, at either moment.
        assert!(run.arrivals(Moment::RoundStart, &[true, false]).is_empty());
        assert!(run.arrivals(Moment::Wipe, &[false, true]).is_empty());
        // A round start with the field clear is not the wipe moment.
        assert!(run.arrivals(Moment::RoundStart, &[false, false]).is_empty());
        let a = run.arrivals(Moment::Wipe, &[false, false]);
        assert_eq!(a.seats, [(0, 5), (1, 6), (2, 7)]);
        assert_eq!(a.layout, Some(3));
        assert_eq!(a.waves_begun, [2]);
        assert!(!run.pending());
        assert_eq!(run.seated, [5, 6, 7]);
        // Nothing is left to come.
        assert!(run.arrivals(Moment::Wipe, &[false; 4]).is_empty());
    }

    #[test]
    fn wipe_waves_arrive_one_per_wipe() {
        let mut run = WaveRun::new(vec![wipe(&[5]), wipe(&[6, 6])], 4);
        let a = run.arrivals(Moment::Wipe, &[false; 4]);
        assert_eq!((a.seats.as_slice(), a.layout), (&[(0, 5)][..], Some(1)));
        assert_eq!(run.next_wave_number(), 3);
        let a = run.arrivals(Moment::Wipe, &[false; 4]);
        assert_eq!(a.seats, [(0, 6), (1, 6)]);
        assert_eq!(a.waves_begun, [3]);
        assert_eq!(run.total_waves(), 3);
    }

    #[test]
    fn a_refill_wave_tops_up_freed_seats_to_its_cap() {
        let mut run = WaveRun::new(vec![refill(&[8, 9], 5, 3)], 3);
        // One of three down: a round start refills that seat.
        let a = run.arrivals(Moment::RoundStart, &[true, false, true]);
        assert_eq!(a.seats, [(1, 8)]);
        assert_eq!(a.waves_begun, [2]);
        assert_eq!(a.layout, None);
        // Full again: nothing.
        assert!(run.arrivals(Moment::RoundStart, &[true; 3]).is_empty());
        // A wipe refills every seat, cycling the list.
        let a = run.arrivals(Moment::Wipe, &[false; 3]);
        assert_eq!(a.seats, [(0, 9), (1, 8), (2, 9)]);
        assert!(a.waves_begun.is_empty());
        // One arrival left of five.
        let a = run.arrivals(Moment::RoundStart, &[false, false, true]);
        assert_eq!(a.seats, [(0, 8)]);
        assert!(!run.pending());
        assert_eq!(run.seated, [8, 9, 8, 9, 8]);
    }

    #[test]
    fn a_refill_wave_widens_the_block_as_it_starts() {
        // Opened on two; the refill wave stands four at once.
        let mut run = WaveRun::new(vec![refill(&[8], 6, 4)], 2);
        // The first freed seat starts the wave: the block widens to four
        // and every free seat of it fills.
        let a = run.arrivals(Moment::RoundStart, &[true, false]);
        assert_eq!(a.seats, [(1, 8), (2, 8), (3, 8)]);
        assert_eq!(a.layout, Some(4));
        let a = run.arrivals(Moment::RoundStart, &[true, true, true, false]);
        assert_eq!((a.seats.as_slice(), a.layout), (&[(3, 8)][..], None));
    }

    #[test]
    fn a_refill_then_a_wipe_wave_chain() {
        let mut run = WaveRun::new(vec![refill(&[8], 1, 4), wipe(&[30])], 1);
        // The wipe sends the refill's one arrival; the wipe wave waits for
        // that monster to fall.
        let a = run.arrivals(Moment::Wipe, &[false]);
        assert_eq!(a.seats, [(0, 8)]);
        assert_eq!(a.waves_begun, [2]);
        assert!(run.pending());
        assert!(run.arrivals(Moment::RoundStart, &[true]).is_empty());
        let a = run.arrivals(Moment::Wipe, &[false; 4]);
        assert_eq!(a.seats, [(0, 30)]);
        assert_eq!(a.waves_begun, [3]);
        assert_eq!(a.layout, Some(1));
    }

    #[test]
    fn an_empty_run_never_seats() {
        let mut run = WaveRun::default();
        assert!(!run.pending());
        assert!(run.arrivals(Moment::Wipe, &[]).is_empty());
    }
}
