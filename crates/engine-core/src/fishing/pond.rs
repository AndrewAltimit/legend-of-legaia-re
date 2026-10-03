//! The pond session (`PondSession`): the venue-faithful fishing loop, its tables, persistence and catch HUD.
//! Split out of `fishing.rs`.

use super::*;

impl PondSession {
    /// Open a session at `venue` over the disc tables, with the persistent
    /// save-block state (`lure` / `rod` / `casts` / `record` /
    /// `purchased_mask`) supplied by the host.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        species: Vec<FishingSpecies>,
        spawn: Vec<[u32; 8]>,
        cadence_templates: Vec<CadenceTemplate>,
        venue: usize,
        lure: u32,
        rod: i32,
        casts: i32,
        record: FishingRecord,
        purchased_mask: u32,
        seed: u32,
    ) -> Self {
        Self {
            species,
            spawn,
            venue,
            lure: lure.min(2),
            rod: rod.clamp(0, 2),
            casts,
            record,
            purchased_mask,
            phase: PondPhase::Idle,
            cast: CastPower::new(),
            cadence: ReelCadence::new(cadence_templates),
            band: BandCheck::default(),
            rng: BiosRand::new(seed),
            timer: 0,
            line_record: 0,
            depth: 0,
            lateral: 0,
            fight_species: None,
            fish: FishAi::default(),
            gauge: TensionGauge::new(0),
            strength: 0,
            last_award: 0,
            events: Vec::new(),
            venue_map: None,
            lure_actor: None,
            lure_probe: Default::default(),
            hub: None,
            rod_actor: None,
            line: None,
            line_latched: false,
            line_fish_prev: None,
        }
    }

    /// Attach the venue the lure is cast into.
    ///
    /// Without it the session still runs - every cast simply lands nowhere in
    /// particular and the water-class credit stays at zero, which is what the
    /// far-band ladder already does for a short cast.
    pub fn attach_venue(&mut self, venue: PondVenue) {
        self.venue_map = Some(venue);
    }

    /// The venue the lure casts into (the pond's `.MAP` floor buffer), when
    /// one is attached.
    pub fn venue_map(&self) -> Option<&PondVenue> {
        self.venue_map.as_ref()
    }

    /// The live cast lure, from the cast lock until the line is reeled in.
    pub fn lure_actor(&self) -> Option<crate::fishing_actors::LureActor> {
        self.lure_actor
    }

    /// Last frame's lure probe: the water flag, the strike-credit addend and
    /// the class's fish weight (retail's `s4`, `10` off water).
    pub fn lure_probe(&self) -> crate::fishing_actors::LureProbe {
        self.lure_probe
    }

    /// The live phase.
    pub fn phase(&self) -> PondPhase {
        self.phase
    }

    /// The live cast-power meter value.
    pub fn cast_power(&self) -> i32 {
        self.cast.value()
    }

    /// Line record (`DAT_801d927c`); `0` before a cast.
    pub fn line_record(&self) -> i32 {
        self.line_record
    }

    /// The HUD length readout term (`DAT_801d9280`).
    pub fn readout(&self) -> i32 {
        (self.line_record - RECORD_STRIKE_BASE).max(0)
    }

    /// Line depth (`DAT_801d9298`).
    pub fn depth(&self) -> i32 {
        self.depth
    }

    /// Fish lateral offset (dart accumulator), for the presentation layer.
    pub fn lateral(&self) -> i32 {
        self.lateral
    }

    /// Live tension, `0..=0x1000`.
    pub fn tension(&self) -> i32 {
        self.gauge.tension()
    }

    /// Accumulated fight strength (`DAT_801d91b8`).
    pub fn strength(&self) -> i32 {
        self.strength
    }

    /// The hooked species record, while fighting (and through the resolved
    /// phases, for the result banner).
    pub fn hooked(&self) -> Option<&FishingSpecies> {
        self.fight_species.and_then(|i| self.species.get(i))
    }

    /// The fish's current behaviour state, while hooked.
    pub fn fish_move(&self) -> Option<FishMove> {
        (self.phase == PondPhase::Hooked).then_some(self.fish.state)
    }

    /// Points awarded by the last landed catch.
    pub fn last_award(&self) -> i32 {
        self.last_award
    }

    /// The current band (hidden state; surfaced for tests + debug overlays).
    pub fn band(&self) -> u32 {
        self.band.band
    }

    /// Drain the events raised since the last call.
    pub fn take_events(&mut self) -> Vec<PondEvent> {
        std::mem::take(&mut self.events)
    }

    /// The rod actor, from the cast lock until its recover swing retires it.
    pub fn rod_actor(&self) -> Option<&crate::fishing_actors::RodActor> {
        self.rod_actor.as_ref()
    }

    /// This frame's rod model as retail links it: the rod actor's last pose
    /// through [`crate::fishing_actors::rod_faces`], in packet order, in
    /// retail 320x240 screen space. Every host draws these ahead of the line
    /// (the rod actor runs before the lure tick, so in a shared bucket its
    /// packets are the earlier `AddPrim`s). Empty with no rod out or no rod
    /// geometry.
    pub fn rod_faces(&self) -> Vec<crate::fishing_actors::RodFace> {
        let Some(pose) = self.rod_actor.as_ref().and_then(|r| r.pose) else {
            return Vec::new();
        };
        let Some(mesh) = self.venue_map.as_ref().and_then(|v| v.rod_mesh.as_ref()) else {
            return Vec::new();
        };
        crate::fishing_actors::rod_faces(mesh, self.rod.clamp(0, 2) as usize, pose)
    }

    /// The lure tick's rod writes for one in-water frame, off the held pad.
    pub(super) fn drive_rod(&mut self, input: PondInput, hooked: bool, fs: i32) {
        if let Some(rod) = self.rod_actor.as_mut() {
            rod.drive(
                crate::fishing_actors::RodDrive {
                    hooked,
                    held: input.reel_mask,
                },
                fs,
            );
        }
    }

    /// This frame's fishing line - the tail of retail's lure tick
    /// `FUN_801D26CC`, which every host calls once per frame after
    /// [`Self::tick`].
    ///
    /// The line runs from the fish to the rod tip. The fish end is the lure's
    /// world point with its height zeroed (`sh zero,0x3a(sp)` at
    /// `0x801D3A90`) pushed through the **scene** camera - which is the
    /// host's, so the host supplies `project` (world `[x, y, z]` to a retail
    /// 320x240 screen point, `None` when it does not project). The rod end is
    /// the rod actor's own projected tip. The first call after a tick
    /// measures the rod's yaw toward the fish off the previous frame's fish
    /// point (`0x801D2A90..0x801D2AA8`, `3 * (tip.x - fish.x)`), latches this
    /// frame's, and builds the clipped packet
    /// ([`crate::fishing_actors::fishing_line`]); later calls in the same
    /// frame return the same line.
    ///
    /// `None` outside the in-water phases, with no lure or no rod tip.
    /// Retail also draws the line while the lure flies out; the port's lure
    /// exists only from the landing, so the line does too.
    pub fn line_frame(
        &mut self,
        project: impl FnOnce([i32; 3]) -> Option<(i16, i16)>,
    ) -> Option<crate::fishing_actors::FishingLine> {
        if self.line_latched {
            return self.line;
        }
        self.line_latched = true;
        if !matches!(self.phase, PondPhase::Waiting | PondPhase::Hooked) {
            return None;
        }
        let lure = self.lure_actor?;
        let rod = self.rod_actor.as_mut()?;
        let tip = rod.tip?;
        let fish = project([lure.x() as i32, 0, lure.z as i32])?;
        let prev = self.line_fish_prev.unwrap_or(fish);
        rod.yaw = (tip.sxy.0 as i32 - prev.0 as i32) * 3;
        self.line_fish_prev = Some(fish);
        self.line = Some(crate::fishing_actors::fishing_line(fish, tip));
        self.line
    }

    /// Line-record seed for a locked cast power: the deep-cast readout is
    /// ~1000 (`denom` context in the doc), so full power maps to
    /// `300 + 1000` and the floor stays above the `500` band-check gate.
    /// (Approximation - the retail line-projection vector math is unpinned.)
    pub(super) fn record_for_power(power: i32) -> i32 {
        RECORD_STRIKE_BASE + 260 + power * 1000 / CAST_POWER_MAX
    }

    /// Advance one frame. `frame_step` is the retail `DAT_1f800393` (1 at
    /// 60 fps); `cast_step` is the casting-power meter step per frame (the
    /// native driver uses `0x80`).
    pub fn tick(&mut self, input: PondInput, frame_step: i32, cast_step: i32) {
        // The hub menu owns the frame while it is up: retail's states
        // `0x64..=0x78` run instead of the pond's (`crate::fishing_hub`).
        if self.hub.is_some() {
            return;
        }
        let fs = frame_step.max(1);
        // The rod actor runs ahead of the lure tick in the actor pool, so it
        // poses off the rod globals the lure tick wrote last frame.
        self.line = None;
        self.line_latched = false;
        if let Some(rod) = self.rod_actor.as_mut() {
            let mesh = self.venue_map.as_ref().and_then(|v| v.rod_mesh.as_ref());
            rod.tick(mesh, self.rod.clamp(0, 2) as usize, fs);
            if rod.retired() {
                self.rod_actor = None;
            }
        }
        match self.phase {
            PondPhase::Idle => {
                if input.cast_edge {
                    self.phase = PondPhase::WindUp;
                    self.timer = 0;
                }
            }
            PondPhase::WindUp => {
                self.timer += fs;
                if self.timer >= WINDUP_FRAMES {
                    self.cast = CastPower::new();
                    self.phase = PondPhase::Power;
                }
            }
            PondPhase::Power => {
                self.cast.advance(cast_step * fs);
                if input.cast_edge {
                    let power = self.cast.lock();
                    self.line_record = Self::record_for_power(power);
                    // The cast lock spawns the rod actor (`jal FUN_80020DE0`
                    // at `0x801CFC48`) and seeds its bend.
                    self.rod_actor = Some(crate::fishing_actors::RodActor::cast());
                    self.line_fish_prev = None;
                    self.depth = 0;
                    self.timer = 0;
                    self.phase = PondPhase::Flight;
                }
            }
            PondPhase::Flight => {
                self.timer += fs;
                if self.timer >= FLIGHT_FRAMES {
                    // The lure lands: the persistent cast counter increments
                    // here (the same event that advances the retail SM to
                    // state 0x19).
                    self.casts += 1;
                    self.band = BandCheck::default();
                    self.cadence.reset();
                    self.lure_actor = self.venue_map.as_ref().and_then(|v| {
                        crate::fishing_actors::LureActor::cast(v.anchor_x, v.anchor_z, v.facing, fs)
                    });
                    self.lure_probe = Default::default();
                    self.phase = PondPhase::Waiting;
                }
            }
            PondPhase::Waiting => {
                self.drive_rod(input, false, fs);
                let button = match ReelInput::from_pad_mask(input.reel_mask) {
                    ReelInput::ReelA => 1,
                    ReelInput::ReelB => 2,
                    ReelInput::Idle => 0,
                };
                let matched = self.cadence.feed(button, fs);
                if matched.is_some() {
                    self.events.push(PondEvent::Splash);
                }
                let reel_held = input.reel_mask & 0xc0 != 0;
                let readout = self.readout();
                // The lure's own frame: the walk-grid drift, then the water
                // class of the tile it now sits over. Both feed the same
                // strike roll retail runs them into.
                self.lure_probe = match (self.lure_actor.as_mut(), self.venue_map.as_ref()) {
                    (Some(lure), Some(venue)) => {
                        let region = venue
                            .region_block
                            .as_deref()
                            .and_then(crate::field_regions::RegionTable::parse);
                        lure.probe(&venue.map, region.as_ref(), self.casts, fs)
                    }
                    _ => Default::default(),
                };
                let struck = self.band.tick(
                    &mut self.rng,
                    matched,
                    self.line_record,
                    readout,
                    input.edge_bonus,
                    self.lure_probe.countdown_bonus,
                    reel_held,
                    fs,
                );
                if struck {
                    let mut band = self.band.band;
                    if band4_gate(
                        self.venue,
                        self.lure,
                        self.rod,
                        band,
                        self.casts,
                        &mut self.rng,
                    ) {
                        band = 4;
                    }
                    if let Some(id) = spawn_species(&self.spawn, self.lure, band) {
                        self.fight_species = Some(id);
                        self.fish = FishAi::default();
                        self.gauge = TensionGauge::new(self.rod);
                        self.strength = 0;
                        self.lateral = 0;
                        self.events.push(PondEvent::Hooked(id));
                        self.phase = PondPhase::Hooked;
                    }
                }
                // Reeling the empty line back in shortens it; fully reeled in
                // returns to the idle shore (an engine convenience - retail
                // parks in the cast loop until the leave confirm).
                if reel_held {
                    self.line_record -= 4 * fs;
                    if self.line_record <= RECORD_STRIKE_BASE {
                        self.line_record = 0;
                        self.lure_actor = None;
                        self.rod_actor = None;
                        self.events.push(PondEvent::Recast);
                        self.phase = PondPhase::Idle;
                    }
                }
            }
            PondPhase::Hooked => {
                let Some(sp) = self
                    .fight_species
                    .and_then(|i| self.species.get(i))
                    .copied()
                else {
                    self.phase = PondPhase::Idle;
                    return;
                };
                self.drive_rod(input, true, fs);
                let reel = ReelInput::from_pad_mask(input.reel_mask);
                let frame = self.fish.tick(&sp, self.depth, &mut self.rng, fs);
                // The per-frame pull accumulates into the fight strength
                // (`DAT_801d91b8`, "the accumulated pull / strength for the
                // fight") - the value the landed score is computed over.
                self.strength = self.strength.saturating_add(frame.pull);
                self.lateral = (self.lateral + frame.lateral).clamp(-0x400, 0x400);
                // Tension: the confirmed tug-of-war.
                self.gauge.apply_reel(reel, frame.pull, fs);
                // Line record: reeling brings the fish in, the fish's run
                // pays line back out (rates are engine-side glue - the doc's
                // Open list).
                match reel {
                    ReelInput::ReelA => {
                        self.line_record -= 3 * fs;
                        self.depth -= 2 * fs;
                    }
                    ReelInput::ReelB => {
                        self.line_record -= 2 * fs;
                        self.depth -= fs;
                    }
                    ReelInput::Idle => self.line_record += frame.pull >> 6,
                }
                self.depth = (self.depth + frame.sink).clamp(0, 0x1000);
                if self.gauge.at_max() {
                    // Reconstruction: tension pinned at the ceiling snaps the
                    // line (doc Open list).
                    self.events.push(PondEvent::Snapped);
                    self.phase = PondPhase::Snapped;
                    // The snap arm starts the rod's recover swing
                    // (`DAT_801d91ac = 10` at `0x801D3C44`).
                    if let Some(rod) = self.rod_actor.as_mut() {
                        rod.recover();
                    }
                } else if self.line_record < LAND_RECORD {
                    // Reel-in complete (`record < 0x136`): score the catch.
                    let award = sp.score_for(self.strength);
                    self.record.credit(sp.index, award);
                    self.last_award = award;
                    self.events.push(PondEvent::Landed(award));
                    self.phase = PondPhase::Landed;
                    // So does the reel-in arm (`0x801D3CB4`).
                    if let Some(rod) = self.rod_actor.as_mut() {
                        rod.recover();
                    }
                }
            }
            PondPhase::Landed | PondPhase::Snapped => {
                if input.cast_edge {
                    self.fight_species = None;
                    self.line_record = 0;
                    self.depth = 0;
                    self.lure_actor = None;
                    self.rod_actor = None;
                    self.events.push(PondEvent::Recast);
                    self.phase = PondPhase::Idle;
                }
            }
        }
    }
}

/// The departure-scene id that selects the **Vidna** venue (`DAT_801d90d0 =
/// 1`): the raw CDNAME `#define` of the Sebucus overworld, `map02`.
pub const VENUE_SCENE_VIDNA: u32 = 0xF4;
/// The departure-scene id that selects the **Buma** venue (`DAT_801d90d0 =
/// 0`): the raw CDNAME `#define` of the Karisto overworld, `map03`.
pub const VENUE_SCENE_BUMA: u32 = 0x187;

/// The venue variant (`DAT_801d90d0`: `0` Buma, `1` Vidna) the fishing
/// driver's setup state picks from the scene the door warp left.
///
/// The mode-24 entry backs the departure scene's id word `_DAT_80084540` up
/// into `0x8007BAC4` (`FUN_80025980`), and state `1` of the driver compares
/// that backup against two immediates:
///
/// ```text
/// 801cf5a4  lw   a0,-0x453c(v0)     ; 0x8007BAC4, the departure scene id
/// 801cf5a8  li   v0,0xf4
/// 801cf5ac  bne  a0,v0,0x801cf5c0
/// 801cf5bc  sw   v0,-0x6f30(v1)     ; == 0xF4  -> DAT_801d90d0 = 1
/// 801cf5c0  li   v0,0x187
/// 801cf5c4  bne  a0,v0,0x801cf5d8
/// 801cf5d0  sw   zero,-0x6f30(v0)   ; == 0x187 -> DAT_801d90d0 = 0
/// ```
///
/// The two immediates are the raw CDNAME `#define`s of `map02` and `map03`,
/// the only two scenes whose scripts carry a fishing door. Any other id
/// leaves the variant as it was, which is `current` here.
///
/// PORT: FUN_801cf3bc (state `1` venue select, `0x801cf5a4..0x801cf5d0`)
pub fn venue_for_departure_scene(scene_id: u32, current: usize) -> usize {
    match scene_id {
        VENUE_SCENE_VIDNA => 1,
        VENUE_SCENE_BUMA => 0,
        _ => current,
    }
}

/// The three disc tables a [`PondSession`] runs over, all rodata of the
/// fishing overlay (PROT 0972): the ten-record species table, the two venue
/// spawn pages and the reel-cadence gesture templates.
///
/// One decode serves every host - the mode-24 door warp, both play hosts'
/// debug launchers and the minigames page all build a session from this.
#[derive(Debug, Clone)]
pub struct FishingTables {
    /// Per-species parameter records ([`legaia_asset::fishing_species::parse`]).
    pub species: Vec<FishingSpecies>,
    /// The two venue spawn pages, Buma then Vidna
    /// ([`legaia_asset::fishing_species::parse_spawn_tables`]).
    pub spawn: [Vec<[u32; 8]>; 2],
    /// Reel-cadence gesture templates
    /// ([`legaia_asset::fishing_species::parse_cadence_templates`]).
    pub cadence: Vec<CadenceTemplate>,
}

impl FishingTables {
    /// Decode all three tables from the loaded overlay image. `None` when any
    /// of them fails to decode - a session without a spawn page can never
    /// hook a fish, so a partial decode is not a playable session.
    pub fn from_overlay(loaded: &[u8]) -> Option<Self> {
        use legaia_asset::fishing_species as fs;
        Some(Self {
            species: fs::parse(loaded)?,
            spawn: fs::parse_spawn_tables(loaded)?,
            cadence: fs::parse_cadence_templates(loaded)?,
        })
    }
}

/// The persistent save-block words a [`PondSession`] opens from and banks
/// back into: retail `_DAT_8008444C..0x8008446C`, which the port keeps on
/// `World::minigames` between sessions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FishingPersist {
    /// Equipped lure row (`_DAT_80084450`, `0..=2`).
    pub lure: u32,
    /// Rod stat (`_DAT_80084454`, `0..=2`).
    pub rod: i32,
    /// Lifetime cast counter (`_DAT_80084460`).
    pub casts: i32,
    /// Point record (`_DAT_8008444C` / `58` / `5C`).
    pub record: FishingRecord,
    /// One-time prize bitmask (`_DAT_8008446C`).
    pub purchased_mask: u32,
}

impl PondSession {
    /// Open a session at `venue` over the decoded `tables` with the
    /// persistent words in `persist`.
    pub fn from_tables(
        tables: &FishingTables,
        venue: usize,
        persist: FishingPersist,
        seed: u32,
    ) -> Self {
        let venue = venue.min(1);
        Self::new(
            tables.species.clone(),
            tables.spawn[venue].clone(),
            tables.cadence.clone(),
            venue,
            persist.lure,
            persist.rod,
            persist.casts,
            persist.record,
            persist.purchased_mask,
            seed,
        )
    }

    /// The persistent words as they stand now - what leaving the session
    /// banks back into the save block.
    pub fn persist(&self) -> FishingPersist {
        FishingPersist {
            lure: self.lure,
            rod: self.rod,
            casts: self.casts,
            record: self.record,
            purchased_mask: self.purchased_mask,
        }
    }

    /// The catch HUD's inputs this frame (`FUN_801d1580`): drawn once a cast
    /// is out, its gauge block only while a fish is on (`DAT_801d91b4`).
    ///
    /// One derivation for all three hosts. The play hosts used to feed the
    /// HUD a fight's reel *progress* as its line record and the minigames
    /// page the line record itself; the value retail draws is the line record
    /// `DAT_801d927c`, which is what this returns.
    pub fn catch_hud(&self) -> PondCatchHud {
        PondCatchHud {
            visible: self.phase != PondPhase::Idle,
            record: self.line_record,
            cast_power: self.cast.value(),
            depth: self.depth,
            tension: self.gauge.tension(),
            gauges_visible: self.phase == PondPhase::Hooked,
        }
    }

    /// The host status rows - a phase line and a key hint - both play hosts
    /// print above the retail HUD while the fishing sprite page is undecoded.
    /// Engine affordance text, not retail; one copy so the hosts agree.
    /// `cast` / `reel_a` / `reel_b` are the host's own key names for Circle,
    /// Cross and Square.
    pub fn status_rows(&self, cast: &str, reel_a: &str, reel_b: &str) -> (String, String) {
        let line = match self.phase {
            PondPhase::Idle => format!("FISHING  ({cast} = cast)"),
            PondPhase::WindUp => "FISHING  winding up".to_string(),
            PondPhase::Power => {
                format!("FISHING  cast power {}  ({cast} = lock)", self.cast.value())
            }
            PondPhase::Flight => "FISHING  the lure is flying".to_string(),
            PondPhase::Waiting => format!("FISHING  line {}  waiting for a bite", self.readout()),
            PondPhase::Hooked => format!(
                "FISHING  tension {}/{TENSION_MAX}  strength {}",
                self.gauge.tension(),
                self.strength
            ),
            PondPhase::Landed => format!(
                "FISHING  landed! +{} points  ({cast} = again)",
                self.last_award
            ),
            PondPhase::Snapped => format!("FISHING  the line snapped!  ({cast} = again)"),
        };
        let hint = match self.phase {
            PondPhase::Waiting | PondPhase::Hooked => {
                format!("hold {reel_a} / {reel_b} to reel")
            }
            _ => format!("{cast} casts, {reel_a} / {reel_b} reel"),
        };
        (line, hint)
    }
}

/// The catch HUD's inputs, host-neutral (the `engine-ui` `CatchHudState`
/// minus its `line_extent`, which has no engine analogue yet).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PondCatchHud {
    /// Whether the catch HUD draws at all (a cast is out).
    pub visible: bool,
    /// Line record `DAT_801d927c`.
    pub record: i32,
    /// Live cast-power meter.
    pub cast_power: i32,
    /// Line depth `DAT_801d9298`.
    pub depth: i32,
    /// Live tension.
    pub tension: i32,
    /// The gauge block's gate (`DAT_801d91b4`): a fish is on.
    pub gauges_visible: bool,
}
