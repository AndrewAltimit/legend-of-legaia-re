//! Which bank occupies the SPU region VAB slots `2` and `6` share, per game
//! mode - retail's per-mode SFX bank residency.
//!
//! `FUN_800265E8` gives slots `2` and `6` one SPU base (`0x33010`), so the
//! class-2 bank and the field bank are never resident together; each mode's
//! initialiser refills the region with its own bank. Read off the loader call
//! sites (every `FUN_8001FC00` / `FUN_8001E54C` pair on the disc) and the three
//! writers of the field-bank latch `0x8007BAFC`:
//!
//! | Retail step | Where | Effect on slots 2 / 6 |
//! |---|---|---|
//! | field init | `FUN_801D6704` `0x801D684C..0x801D68B8`, `0x801D6FF4..0x801D7048` (PROT 0897) | closes slots `2`, `7`, `8`, `11`; when the latch is clear, loads raw `0x36E` (PROT 0876) into slot `6` and sets the latch |
//! | battle mode init | `FUN_8001DCF8` `0x8001DF74..0x8001DFC0` (next mode `0x14`) | closes slots `6` and `3`, clears the latch |
//! | battle scene loader | `FUN_800520F0` `0x80052378..0x800523AC` | loads raw `0x367` (PROT 0869; `0x36D` = 0875 when `DAT_8007BD11 == 4`) into slot `2` |
//! | minigame warp | `FUN_80025980` `0x800259A4` | clears the latch (closes nothing) |
//! | minigame overlay init | fishing `0x801CF29C`, slot machine `0x801CF064`, dance `0x801CF428`, Baka `0x801CF248` | loads the minigame's own bank into slot `2`; `FUN_8001DCF8` runs under mode word `0x18`, so its close arm (keyed on `0x14`, `0x8001DF74..0x8001DF80`) does not, and slot `6` stays open |
//! | side-band teardown | `FUN_801D8450` (field-VM op `0x36` sub `3`) | closes slot `6`, clears the latch |
//! | side-band request `>= 3000` / `1000..=1999` | `FUN_800243F0` `0x800248B4..0x8002494C` | streams a `vab_01` bank into slot `6`, over the field bank |
//!
//! The latch is what makes the field bank survive a field-to-field scene
//! change without a reload, and what makes it come back after a battle, a
//! minigame or a side-band teardown: each of those clears it, and the next
//! field init sees it clear.
//!
//! The two slots are opened and closed separately, so both can be open over
//! the one region: on the Baka Fighter's path the warp leaves slot `6` open
//! and the overlay loads PROT 0869 into slot `2`, and from then on slot `6`'s
//! header (PROT 0876's) sits over slot `2`'s samples (retail capture,
//! `docs/subsystems/audio.md`). [`SfxBankResidency::slot_open`] tracks each
//! slot; [`SfxBankResidency::shared`] names the bank whose samples the region
//! holds. The hosts stage that one bank under its own slot and leave the
//! other slot unstaged, so a cue routed to a slot left open over a foreign
//! bank's samples is silent in the port where retail plays the stale header
//! over the wrong samples.
//!
//! A closed slot is silent, not rerouted: the cue drainer `FUN_80016B6C` skips
//! a cue whose mixer record's `+0xB` enable byte is zero (`lb v0,0xb(v1)` /
//! `beq v0,zero` at `0x80016CE4..0x80016CEC`), and `FUN_8001FF58` zeroes that
//! byte when it closes the slot.
//!
//! REF: FUN_801D6704, FUN_8001DCF8, FUN_800520F0, FUN_80025980, FUN_8001FF58,
//! FUN_800243F0, FUN_80016B6C, FUN_801D8450

use legaia_game_tables::sfx_table::{SLOT2_CLASS2_BANK_PROT_INDEX, SLOT6_FIELD_BANK_PROT_INDEX};

use crate::mode::SceneMode;

/// The SPU region slots `2` and `6` share (`FUN_800265E8`: both `0x33010`).
pub const SHARED_REGION_SLOTS: (u8, u8) = (2, 6);

/// Fishing's slot-2 bank: raw `0x4AF` at `0x801CF29C` in PROT 0972.
pub const FISHING_SLOT2_PROT_INDEX: u32 = 0x4AF - 2;
/// The slot machine's slot-2 bank: raw `0x4B0` at `0x801CF064` in PROT 0975.
pub const SLOT_MACHINE_SLOT2_PROT_INDEX: u32 = 0x4B0 - 2;
/// The dance minigame's slot-2 bank: raw `0x4D1` at `0x801CF428` in PROT 0980.
pub const DANCE_SLOT2_PROT_INDEX: u32 = 0x4D1 - 2;

/// The bank occupying the shared slot-2 / slot-6 region: which of the two
/// slots is open over it, and the extraction-frame PROT entry it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SharedRegionBank {
    /// `2` or `6`.
    pub slot: u8,
    /// Extraction-frame PROT index.
    pub prot_entry: u32,
}

impl SharedRegionBank {
    /// The field bank, PROT 0876 in slot 6.
    pub const FIELD: Self = Self {
        slot: 6,
        prot_entry: SLOT6_FIELD_BANK_PROT_INDEX,
    };
    /// The class-2 battle bank, PROT 0869 in slot 2.
    pub const CLASS2: Self = Self {
        slot: 2,
        prot_entry: SLOT2_CLASS2_BANK_PROT_INDEX,
    };
}

/// Retail's slot-2 / slot-6 residency: the field-bank latch `0x8007BAFC` and
/// the bank the shared region currently holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SfxBankResidency {
    /// `0x8007BAFC` - set by the field init once it has loaded PROT 0876,
    /// cleared by the battle mode init, the minigame warp and the side-band
    /// teardown.
    pub field_bank_latch: bool,
    /// The bank the shared region holds, `None` while neither slot is open.
    pub shared: Option<SharedRegionBank>,
    /// Slot `2`'s mixer-record enable (`0x80091508 + 2 * 12 + 0xB`).
    slot2_open: bool,
    /// Slot `6`'s mixer-record enable (`0x80091508 + 6 * 12 + 0xB`).
    slot6_open: bool,
    /// The mode the last [`Self::sync`] saw.
    last_mode: Option<SceneMode>,
    /// The scene the last field init ran for.
    last_field_scene: String,
    /// The side-band request last applied to slot 6, so one request streams
    /// once (retail's driver acts on the request edge, not on its level).
    last_slot6_request: Option<i32>,
}

impl SfxBankResidency {
    /// Advance the residency to `mode` and the field scene `scene_label`,
    /// and return the bank the shared region should hold. `slot6_side_band`
    /// is the live side-band request when it resolves to slot `6`, as
    /// `(request, prot_entry)`.
    ///
    /// Mode edges stand in for retail's mode initialisers: entering the field
    /// or the world map (or changing field scene) runs the field init's arm,
    /// entering battle the battle init's, entering a warp minigame the warp's.
    /// The title, cutscene and pause-menu modes run none of them.
    pub fn sync(
        &mut self,
        mode: SceneMode,
        scene_label: &str,
        slot6_side_band: Option<(i32, u32)>,
    ) -> Option<SharedRegionBank> {
        let entered = self.last_mode != Some(mode);
        self.last_mode = Some(mode);
        match mode {
            SceneMode::Field | SceneMode::WorldMap => {
                if entered || self.last_field_scene != scene_label {
                    self.last_field_scene = scene_label.to_string();
                    self.field_init();
                }
                match slot6_side_band {
                    Some((request, prot_entry)) => self.side_band_slot6(request, prot_entry),
                    None => self.last_slot6_request = None,
                }
            }
            SceneMode::Battle | SceneMode::MuscleDome if entered => self.battle_init(),
            m if entered => {
                if let Some(bank) = minigame_slot2_bank(m) {
                    self.minigame_warp(bank);
                }
            }
            _ => {}
        }
        self.shared
    }

    /// Whether VAB slot `slot` (`2` or `6`) is open. `false` for any other
    /// slot.
    pub fn slot_open(&self, slot: u8) -> bool {
        match slot {
            2 => self.slot2_open,
            6 => self.slot6_open,
            _ => false,
        }
    }

    /// The open slot whose header is **not** the region's current bank -
    /// retail's stale-header state (slot `6` over a minigame's slot-2
    /// samples). `None` when at most one slot is open.
    pub fn stale_open_slot(&self) -> Option<u8> {
        let owner = self.shared.map(|b| b.slot)?;
        [2u8, 6]
            .into_iter()
            .find(|&s| s != owner && self.slot_open(s))
    }

    /// `FUN_8001FF58` on slot 2 or 6: the enable drops, and the region's bank
    /// goes with it when that slot owned it.
    fn close(&mut self, slot: u8) {
        match slot {
            2 => self.slot2_open = false,
            6 => self.slot6_open = false,
            _ => return,
        }
        if self.shared.is_some_and(|b| b.slot == slot) {
            self.shared = None;
        }
    }

    /// `FUN_8001FC00` + `FUN_8001E54C` into slot 2 or 6: the region takes the
    /// bank and the slot opens. The other slot's enable is untouched.
    fn load(&mut self, bank: SharedRegionBank) {
        match bank.slot {
            2 => self.slot2_open = true,
            6 => self.slot6_open = true,
            _ => return,
        }
        self.shared = Some(bank);
    }

    /// `FUN_801D6704`'s bank arm: slot 2 closes; the field bank reloads when
    /// the latch is clear.
    pub fn field_init(&mut self) {
        self.close(2);
        if !self.field_bank_latch {
            self.load(SharedRegionBank::FIELD);
            self.field_bank_latch = true;
        }
    }

    /// `FUN_8001DCF8`'s mode-word-`0x14` arm (close 6 and 3, clear the latch,
    /// `0x8001DF74..0x8001DFC0`) followed by `FUN_800520F0` loading the
    /// class-2 bank into slot 2.
    pub fn battle_init(&mut self) {
        self.close(6);
        self.field_bank_latch = false;
        self.load(SharedRegionBank::CLASS2);
    }

    /// `FUN_80025980` (clear the latch, close nothing) followed by the
    /// minigame overlay's own slot-2 load. Slot 6 keeps whatever enable it
    /// had, because the overlay's `FUN_8001DCF8` call runs under mode word
    /// `0x18` and skips the close arm. `bank` is `None` for a minigame that
    /// loads nothing into slot 2.
    pub fn minigame_warp(&mut self, bank: Option<u32>) {
        self.field_bank_latch = false;
        if let Some(prot_entry) = bank {
            self.load(SharedRegionBank {
                slot: 2,
                prot_entry,
            });
        }
    }

    /// `FUN_801D8450` - field-VM op `0x36` sub `3`. Stops voices `0x17` /
    /// `0x16` (host side), closes slot 6, clears `_DAT_8007BA88` and the
    /// latch. The field bank stays gone until the next field init.
    pub fn side_band_teardown(&mut self) {
        self.close(6);
        self.field_bank_latch = false;
    }

    /// A side-band request resolved to slot 6 streams its bank over the
    /// field bank. The latch is untouched: retail's streaming slot does not
    /// write it.
    pub fn side_band_slot6(&mut self, request: i32, prot_entry: u32) {
        if self.last_slot6_request == Some(request) {
            return;
        }
        self.last_slot6_request = Some(request);
        self.load(SharedRegionBank {
            slot: 6,
            prot_entry,
        });
    }
}

/// The slot-2 bank a minigame mode's overlay init loads, `None` for a mode
/// that is not a warp minigame.
///
/// [`SceneMode::MuscleDome`] is not one: the port's dome mode is a **leg** (a
/// round), and retail runs a round as an ordinary battle - the arena stores
/// mode word `0x14` (`0x801D15B8`, PROT 0977), so `FUN_8001DCF8`'s close arm
/// runs and the battle scene loader stages the class-2 bank, the
/// [`SfxBankResidency::battle_init`] path. A retail capture of a round
/// (`scripts/pcsx-redux/run_w3a_captures.sh dome`) sees exactly that: the
/// close arm shuts slots 6 and 3, and the battle scene loader stages PROT
/// 0869 into slot 2. The arena's hub (retail mode `0x19`) holds slot 2 closed
/// and slot 6 open over the field bank the warp left (retail capture); the
/// port has no hub mode - its hub is the field it returns to, whose init
/// reloads that bank.
pub fn minigame_slot2_bank(mode: SceneMode) -> Option<Option<u32>> {
    Some(match mode {
        SceneMode::Fishing => Some(FISHING_SLOT2_PROT_INDEX),
        SceneMode::SlotMachine => Some(SLOT_MACHINE_SLOT2_PROT_INDEX),
        SceneMode::Dance => Some(DANCE_SLOT2_PROT_INDEX),
        SceneMode::BakaFighter => Some(SLOT2_CLASS2_BANK_PROT_INDEX),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_latched_scene_change_keeps_a_slot_six_side_band_bank() {
        let mut r = SfxBankResidency::default();
        r.field_init();
        r.side_band_slot6(3001, 1071);
        r.field_init();
        assert_eq!(
            r.shared,
            Some(SharedRegionBank {
                slot: 6,
                prot_entry: 1071
            })
        );
        // The same request does not re-stream after the teardown.
        r.side_band_teardown();
        r.side_band_slot6(3001, 1071);
        assert_eq!(r.shared, None);
    }
}
