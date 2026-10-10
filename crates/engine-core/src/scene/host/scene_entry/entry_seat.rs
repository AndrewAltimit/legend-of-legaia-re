//! The entry seat: where the player is placed when the next scene is entered.

use super::*;

impl SceneHost {
    /// Load `name`, switch the world to [`crate::world::SceneMode::Field`],
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
}
