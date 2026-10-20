//! The entry seat: where the player is placed when the next scene is entered.

use super::*;

use legaia_engine_scene::scene_arrivals::{ArrivalCatalog, SceneArrival};

/// Where a scene-picker entry seated the party
/// ([`SceneHost::arm_picker_entry_seat`], [`SceneHost::picker_seat`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerSeat {
    /// A door / warp arrival another scene's `0x3F` lands the party on.
    Arrival {
        /// The ranked arrival that seated it.
        arrival: SceneArrival,
        /// The world `(x, z)` seat (the arrival, or its bounded wall rescue).
        seat: (i16, i16),
        /// How many ranked arrivals were rejected before it.
        rejected: usize,
    },
    /// No arrival qualified (or the scene has none): the cold-entry
    /// resolver's seat ([`legaia_engine_core::world::World::resolve_cold_field_spawn`]).
    ColdFallback {
        /// Ranked arrivals the scene has (all rejected), `0` for a scene no
        /// door leads into.
        arrivals: usize,
        /// The world `(x, z)` seat the resolver chose.
        seat: (i16, i16),
    },
}

impl SceneHost {
    /// Load `name`, switch the world to [`legaia_engine_mode::mode::SceneMode::Field`],
    /// and load the requested event-script record (default 0) into the
    /// field-VM bytecode buffer. Returns `Err` if the scene has no event
    /// scripts or the record index is out of range.
    /// Arm the **destination-entry operand** the next [`Self::enter_field_scene`]
    /// seats the player from, in world coordinates.
    ///
    /// This is the port's `_DAT_80073EF4` / `_DAT_80073EF8` write: every retail
    /// path into a field scene performs it before the initialiser runs (the
    /// door's `0x3F` arm, the world-map arrival kernel `FUN_801EE328`, the New
    /// Game seed). A host that enters a scene without calling this gets the
    /// engine's own picker seat instead - see [`Self::pending_entry_seat`].
    pub fn set_entry_seat(&mut self, x: i16, z: i16) {
        self.pending_entry_seat = Some((x, z));
    }

    /// `true` while an entry operand is armed and no scene entry has
    /// consumed it yet.
    pub fn entry_seat_armed(&self) -> bool {
        self.pending_entry_seat.is_some()
    }

    /// Drop an armed entry operand no entry consumed (a failed entry must not
    /// hand its seat to whatever scene is entered next).
    pub fn disarm_entry_seat(&mut self) {
        self.pending_entry_seat = None;
    }

    /// Arm the seat a **card load** resumes at, before entering `scene`:
    /// the save's field position ([`legaia_save::SaveExtV2::field_position`]),
    /// and only when `scene` is the one the save was written in
    /// ([`crate::resume::saved_entry_seat`]). An operand the caller already
    /// armed wins - a driver that knows a better seat (a live capture's own
    /// position) keeps it. Returns `true` when this call armed one, so the
    /// caller can [`Self::disarm_entry_seat`] if the entry then fails.
    ///
    /// This is retail's card-load arm of the MAN loader: `FUN_801DD35C`
    /// raises `_DAT_8007B8C0` after the load copy, and `FUN_8003AEB0` on that
    /// flag copies the save's `0x80084568` / `0x8008456C` into the entry
    /// operand `_DAT_80073EF4` / `_DAT_80073EF8` (`0x8003B764..0x8003B798`).
    ///
    /// The same arm zeroes the arrival facing `_DAT_80073EFC`
    /// (`0x8003B778`) - a save stores no heading - so the entry script's
    /// `4C 3A` stands the player facing retail's `0`, towards the camera.
    ///
    /// REF: FUN_8003AEB0 (`0x8003B764..0x8003B798`, the card-load seat)
    pub fn arm_resume_seat(
        &mut self,
        save: &legaia_save::SaveFile,
        save_scene: &str,
        scene: &str,
    ) -> bool {
        self.world.locomotion.arrival_facing = 0;
        if self.pending_entry_seat.is_some() {
            return false;
        }
        match crate::resume::saved_entry_seat(save, save_scene, scene) {
            Some((x, z)) => {
                self.set_entry_seat(x, z);
                true
            }
            None => false,
        }
    }

    /// Arm the entry operand from a **tile** pair, the form the field VM's
    /// `0x3F` and the world-map arrival kernel carry: `(tile << 7) + 0x40`,
    /// retail's tile-centre conversion.
    pub fn set_entry_seat_tile(&mut self, tile_x: u8, tile_z: u8) {
        let centre = |t: u8| ((i32::from(t) << 7) + 0x40) as i16;
        self.set_entry_seat(centre(tile_x), centre(tile_z));
    }

    /// The disc's inbound-arrival catalog, built on first use.
    pub fn arrival_catalog(&mut self) -> Arc<ArrivalCatalog> {
        self.arrival_catalog
            .get_or_insert_with(|| Arc::new(ArrivalCatalog::build(&self.index)))
            .clone()
    }

    /// Install an already-built arrival catalog (one built by another host
    /// over the same disc), so this host does not walk every scene again.
    pub fn set_arrival_catalog(&mut self, catalog: Arc<ArrivalCatalog>) {
        self.arrival_catalog = Some(catalog);
    }

    /// Arm a **scene-picker** entry into `scene`: the next
    /// [`Self::enter_field_scene`] (an overworld's too) seats the party where
    /// a door or warp would land them, rather than at the engine's cold-entry
    /// synthesis.
    ///
    /// A picked scene is one nobody walked into, so it has no
    /// destination-entry operand. The disc still says where its doors land:
    /// every other scene's `0x3F` scene changes into it, ranked by
    /// [`ArrivalCatalog::ranked`] (overworld entrance, then walk-on door, then
    /// scripted warp; the tile most sites share; disc order). Once the
    /// destination's walkability grid is loaded the entry takes the first
    /// arrival [`legaia_engine_core::world::World::picker_arrival_seat`]
    /// accepts, faces the party along its sector, and otherwise falls back to
    /// [`legaia_engine_core::world::World::resolve_cold_field_spawn`] - the
    /// scenes no door leads into (the ending cutscenes) keep that seat, and
    /// so does Rim Elm ([`legaia_game_tables::new_game::OPENING_SCENE`]),
    /// whose cold seat is retail's own New Game spawn.
    ///
    /// A no-op returning `false` when an entry operand is already armed (a
    /// door, a resume, a host's explicit seat wins) or while the opening chain
    /// / a cutscene timeline owns the entry - the same entries
    /// [`legaia_engine_core::world::World::picker_entry_stages_free_roam`]
    /// refuses to stage. Otherwise returns `true` (even when the scene has no
    /// arrival: the entry then records a [`PickerSeat::ColdFallback`]).
    pub fn arm_picker_entry_seat(&mut self, scene: &str) -> bool {
        self.picker_seat = None;
        self.pending_picker_arrivals = None;
        if self.pending_entry_seat.is_some()
            || self.world.cutscene.opening_chain_active
            || self.world.cutscene.entering_town01_opening
            || self.world.cutscene_timeline_active()
        {
            return false;
        }
        // Rim Elm is the one scene retail itself enters without a door: the
        // New Game seat in Vahn's yard (`FIELD_COLD_SPAWN_XZ`), which the cold
        // resolver keeps. A picked Rim Elm starts there, not at its gate.
        let ranked = if scene == legaia_game_tables::new_game::OPENING_SCENE {
            Vec::new()
        } else {
            self.arrival_catalog().ranked(scene)
        };
        self.pending_picker_arrivals = Some(ranked);
        true
    }

    /// Where the last scene-picker entry seated the party, `None` when the
    /// last entry was not a picker entry.
    pub fn picker_seat(&self) -> Option<&PickerSeat> {
        self.picker_seat.as_ref()
    }
}
