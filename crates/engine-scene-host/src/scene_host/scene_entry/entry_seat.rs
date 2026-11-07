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
    /// A row of [`PICKER_SEAT_OVERRIDES`] seated it: a scene whose ranked
    /// arrival or cold seat lands somewhere a player cannot play from.
    Override {
        /// The row's rule.
        rule: PickerSeatRule,
        /// The world `(x, z)` seat it resolved to.
        seat: (i16, i16),
    },
}

/// How a [`PICKER_SEAT_OVERRIDES`] row places the party, in the tile form the
/// field VM's `0x3F` and `MOVE_TO` carry (`(tile << 7) + 0x40`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerSeatRule {
    /// The disc's inbound `0x3F` arrival from `from` at `tile` (an
    /// [`ArrivalCatalog::arrivals`] row), stepped `step` whole tiles. The
    /// party faces the arrival's sector and the scene's home track is
    /// inherited, as for a ranked arrival. Unlike the ranked path the seat is
    /// taken as is: the row is the review that the ranked path's enclosure
    /// and trigger tests stand in for.
    Arrival {
        /// The scene whose `0x3F` lands here.
        from: &'static str,
        /// The `0x3F`'s entry tile (low 7 bits of its entry bytes).
        tile: (u8, u8),
        /// Whole tiles added to `tile`.
        step: (i8, i8),
    },
    /// A tile the scene's own MAN seats the player on (a `MOVE_TO` of the
    /// player, `A3 F8 xb zb`), for a scene no usable door leads into.
    ManMoveTo {
        /// The op's tile operand.
        tile: (u8, u8),
    },
    /// A tile read off the scene's walkability grid, for a scene whose only
    /// arrival is the off-map hide box (`0x7F`, `0x7F`) or the map-centre
    /// placeholder a later script moves the party from.
    Tile {
        /// The tile.
        tile: (u8, u8),
    },
}

/// Scene-picker seats reviewed by hand: scenes where neither the ranked door
/// arrival nor the cold resolver lands the party somewhere playable. Every
/// other scene keeps [`SceneHost::arm_picker_entry_seat`]'s ranked rule.
/// Each row says why the default is wrong and where its seat comes from; the
/// disc-gated `picker_door_arrival_disc` sweep checks each seat is standable
/// and stays in the scene.
pub const PICKER_SEAT_OVERRIDES: &[(&str, PickerSeatRule)] = &[
    // Conkram (Past): its one arrival, the conc2 door, lands in the pocket
    // behind the grille door at tile (0x1E, 0x40); the grille's body walls
    // the party in with only the off-map opening behind it. Three tiles
    // towards the room puts the party in front of the door.
    (
        "conc",
        PickerSeatRule::Arrival {
            from: "conc2",
            tile: (0x1E, 0x40),
            step: (0, -3),
        },
    ),
    // Jeremi: the map02 entrance tile (0x1B, 0x08) carries the town's
    // gate-1 intro trigger, so the ranked rescue stepped off it - across
    // the warp portal's trigger row (z = 0x05) to the far side. One tile
    // past the arrival, towards the town, is in front of the portal.
    (
        "geremi",
        PickerSeatRule::Arrival {
            from: "map02",
            tile: (0x1B, 0x08),
            step: (0, 1),
        },
    ),
    // Gala's dream (Biron): the uru door's entry tile is the map-centre
    // placeholder (0x40, 0x40), whose rescue lands in a corridor behind a
    // locked door. The dream's own MAN seats the player at (0x32, 0x60)
    // (`A3 F8 32 60`, twice), in a furnished hall.
    ("urudre3", PickerSeatRule::ManMoveTo { tile: (0x32, 0x60) }),
    // Noa's dream (Conkram): the same map-centre placeholder, rescued into
    // the off-map strip by the long east corridor. The dream's MAN seats the
    // player in the plaza at (0x1E, 0x1F) (`A3 F8 1E 1F`, twice).
    ("urudre2", PickerSeatRule::ManMoveTo { tile: (0x1E, 0x1F) }),
    // Ra-Seru scene (2): the noaru warp lands at the deep end of the ramp
    // (0x1B, 0x71), which the scene's wide camera shows as the party
    // standing off the end of the walkway. One tile up the ramp stands on
    // it, short of Terra's trigger rows (z = 0x6E, 0x6F).
    (
        "juui2",
        PickerSeatRule::Arrival {
            from: "noaru",
            tile: (0x1B, 0x71),
            step: (0, -1),
        },
    ),
    // Rogue's Tower: the juui1 door's tile (0x22, 0x58) carries the gate-1
    // trigger of the party's entrance beat; the ranked rule steps off every
    // gate-1 tile, so the beat never played. Seating on the door itself
    // fires it on the first tick, as the door does.
    (
        "rugi",
        PickerSeatRule::Arrival {
            from: "juui1",
            tile: (0x22, 0x58),
            step: (0, 0),
        },
    ),
    // Sol Tower (rooms F): both kor3 doors land in the bar room, which the
    // enclosure test rejects (its doorway opens onto the unwalled map
    // edge), so the cold resolver seated the party in the arena wing. The
    // rec-3 door at (0x1B, 0x0B) is the one the retail route walk entered
    // by.
    (
        "koin1b",
        PickerSeatRule::Arrival {
            from: "kor3",
            tile: (0x1B, 0x0B),
            step: (0, 0),
        },
    ),
    // Zeto's Dungeon: its one arrival is a map01 script's hide-box warp
    // (0x7F, 0x7F), and the cold seat lands on the grass beside the
    // entrance path, off any floor the walls bound. The path is the walled
    // strip x = 0x14..0x15, z = 0x05..0x07 below the gate's triggers
    // (z = 0x08), whose gap leads back out to map01; (0x14, 0x06) stands
    // on it a tile clear of them.
    ("garmel", PickerSeatRule::Tile { tile: (0x14, 0x06) }),
];

/// The [`PICKER_SEAT_OVERRIDES`] row for `scene`.
pub fn picker_seat_override(scene: &str) -> Option<PickerSeatRule> {
    PICKER_SEAT_OVERRIDES
        .iter()
        .find(|(name, _)| *name == scene)
        .map(|(_, rule)| *rule)
}

/// A resolved [`PickerSeatRule`], armed for the next field entry.
#[derive(Debug, Clone)]
pub(crate) struct PickerOverrideSeat {
    pub(crate) rule: PickerSeatRule,
    /// World `(x, z)`.
    pub(crate) seat: (i16, i16),
    /// The arrival it came from, for its facing and home track.
    pub(crate) arrival: Option<SceneArrival>,
}

/// Retail's tile-centre conversion, `(tile << 7) + 0x40`.
fn tile_centre(t: i32) -> i16 {
    ((t << 7) + 0x40) as i16
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
        self.pending_picker_override = None;
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
        if let Some(rule) = picker_seat_override(scene)
            && let Some(seat) = self.resolve_picker_override(scene, rule)
        {
            self.pending_picker_override = Some(seat);
            self.pending_picker_arrivals = Some(Vec::new());
            return true;
        }
        let ranked = if scene == legaia_game_tables::new_game::OPENING_SCENE {
            Vec::new()
        } else {
            self.arrival_catalog().ranked(scene)
        };
        self.pending_picker_arrivals = Some(ranked);
        true
    }

    /// Resolve an override row against the disc's arrivals; `None` when an
    /// [`PickerSeatRule::Arrival`] row names an arrival the disc does not
    /// carry (the ranked rule then seats the entry).
    fn resolve_picker_override(
        &mut self,
        scene: &str,
        rule: PickerSeatRule,
    ) -> Option<PickerOverrideSeat> {
        let at = |(x, z): (u8, u8), (dx, dz): (i8, i8)| {
            (
                tile_centre(i32::from(x) + i32::from(dx)),
                tile_centre(i32::from(z) + i32::from(dz)),
            )
        };
        match rule {
            PickerSeatRule::Arrival { from, tile, step } => {
                let arrival = self
                    .arrival_catalog()
                    .arrivals(scene)
                    .iter()
                    .find(|a| a.from_scene == from && (a.entry_x & 0x7F, a.entry_z & 0x7F) == tile)
                    .cloned()?;
                Some(PickerOverrideSeat {
                    rule,
                    seat: at(tile, step),
                    arrival: Some(arrival),
                })
            }
            PickerSeatRule::ManMoveTo { tile } | PickerSeatRule::Tile { tile } => {
                Some(PickerOverrideSeat {
                    rule,
                    seat: at(tile, (0, 0)),
                    arrival: None,
                })
            }
        }
    }

    /// Where the last scene-picker entry seated the party, `None` when the
    /// last entry was not a picker entry.
    pub fn picker_seat(&self) -> Option<&PickerSeat> {
        self.picker_seat.as_ref()
    }
}
