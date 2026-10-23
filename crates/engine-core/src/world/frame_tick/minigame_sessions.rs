use super::*;

mod baka_fighter;
mod dance;
mod fishing;
mod muscle_dome;
mod round_trip;
mod slot_machine;

impl World {
    /// Whether the CD input (the XA stream) feeds the SPU's reverb send this
    /// frame - `SPUCNT` bit 2, which only two minigame overlays touch, each
    /// through `SpuSetCommonAttr` under mask `0x300` (CD reverb + CD mix):
    ///
    /// - **Baka Fighter**: the overlay init `FUN_801CF00C` sets it
    ///   (`0x801CF364`, `cd.reverb = 1`) and the shared exit into
    ///   `FUN_80026018` clears it (`0x801D1F10`, `cd.reverb = 0`), so it is
    ///   up for the whole duel mode.
    /// - **Muscle Dome**: the contest init / re-entry `FUN_801CEA6C` sets it
    ///   (`0x801CF054`), the opponent installer `FUN_801D1510` clears it as
    ///   the hub hands a leg over (`0x801D153C`), and the settlement
    ///   `FUN_801D0F60` (`0x801D0F9C`) and the shared exit clear it too. So
    ///   it is up while the hub owns the frame - the first visit, the ROUND
    ///   card, the between-legs INTERVAL - and down while a leg is fought.
    ///
    /// Every other path leaves the bit alone, and boot clears it. Captured:
    /// `SPUCNT = 0xC085` in the duel and the dome hub library states,
    /// `0xC081` in every other one.
    pub fn cd_input_reverb(&self) -> bool {
        use legaia_engine_mode::mode::SceneMode;
        match self.mode {
            SceneMode::BakaFighter => true,
            SceneMode::MuscleDome => {
                self.minigames.muscle_dome.is_none() || self.minigames.muscle_hub.covers_leg()
            }
            _ => false,
        }
    }
}
