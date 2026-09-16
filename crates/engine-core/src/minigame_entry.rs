//! The **mode-24 minigame door-warp** id space: field-VM op `0x3E`'s
//! `op0 >= 100` arm and the seven overlay slots its `sub_id` selects.
//!
//! ## Why this module exists
//!
//! Op `0x3E`'s warp arm is the *only* way a retail player reaches a minigame
//! from the field, and it is not a scene change. The whole chain is
//! SCUS-resident; the arm itself is pinned in the field overlay's VM
//! (`see ghidra/scripts/funcs/overlay_0897_801de840.txt`, `case 0x3e` at
//! `0x801E078C`):
//!
//! ```text
//! 801e078c  lui   v0,0x8008
//! 801e0790  lui   a1,0x8008
//! 801e0794  sw    zero,-0x4540(v0)   ; _DAT_8007BAC0 = 0
//! 801e0798  lbu   v1,0x0(s6)         ; v1 = op0 (operand byte 0)
//! 801e079c  li    v0,0x18
//! 801e07a0  sh    v0,-0x47c4(a1)     ; _DAT_8007B83C = 0x18   -> game mode 24
//! 801e07a4  lui   v0,0x8008
//! 801e07a8  sw    zero,0x4440(v0)    ; _DAT_80084440 = 0      -> winnings acc
//! 801e07ac  lui   v0,0x8008
//! 801e07b0  addiu v1,v1,-0x64        ; sub_id = op0 - 100
//! 801e07b4  jal   0x8003ce08
//! 801e07b8  _sh   v1,-0x45cc(v0)     ; _DAT_8007BA34 = sub_id
//! 801e07bc  lui   a0,0xfff7
//! 801e07c4  ori   a0,a0,0xffff       ; a0 = 0xFFF7FFFF
//! 801e07cc  addiu s8,s8,0x6          ; PC += 6
//! 801e07d0  and   v0,v0,a0           ; player[+0x10] &= ~0x80000
//! ```
//!
//! The arm calls **no scene-change packet** (`func_0x8001FD44`, which op `0x3F`
//! does call) and the op carries no destination name. `sub_id` selects a *code
//! overlay*, loaded by the mode-24 init `FUN_80025980` through
//! `FUN_8003EBE4(sub_id + 0x4D)`; that init also backs the *current* scene name
//! up (`memcpy(0x8007BAE8, 0x80084548, 8)`) so the minigame can warp back
//! through `FUN_80026018`. Those two halves are [`crate::world::World::arm_minigame_warp`]
//! and [`crate::world::World::minigame_return_warp`].
//!
//! ## The trap this replaces
//!
//! `sub_id` reads exactly like a map id - a small dense integer arriving on a
//! warp opcode - and the engine used to route it as one, through
//! [`crate::scene::DefaultMapIdResolver`] into a CDNAME-ordinal scene name.
//! That resolver's own note conceded the id "maps to a code overlay at PROT
//! `0x4d + map_id`" while still resolving it to a scene, calling the ordering
//! "an approximation" for a retail table "in an uncaptured overlay". There is
//! no such table: the destination is the overlay, and the only scene name in
//! the chain is the *departure* scene being saved for the return trip. A
//! player who walked into a venue therefore warped to an unrelated scene
//! instead of entering the minigame.
//!
//! ## PROT index arithmetic
//!
//! `prot_index = (sub_id + if sub_id >= 6 { 2 } else { 0 }) + 0x4D + 0x37F`
//! in extraction index space (the `+ 0x37F` re-keys the loader's in-RAM TOC
//! index, see `docs/subsystems/boot.md`). The `+ 2` step at `sub_id >= 6` is
//! retail's own, not a fudge - it skips the two entries between `0977` and
//! `0980`.
// REF: FUN_801DE840 case 0x3e (the arm), FUN_80025980 (mode-24 init +
//      overlay load), FUN_80026018 (return warp)

use crate::world::SceneMode;

/// Re-key from the loader's in-RAM TOC index space to the extraction PROT
/// index space (see `docs/subsystems/boot.md` - overlay loaders).
///
/// The `+ 0x4D` bias and the `+ 2` step are **not** repeated here: they belong
/// to the mode-24 init and are already ported as
/// [`crate::mode::other_warp_init_stage`], which this module calls. Copying
/// that arithmetic would give the disc two sources of truth for the same seven
/// numbers.
const PROT_INDEX_REKEY: u32 = 0x37F;

/// The seven mode-24 door-warp slots, indexed by the `sub_id` field-VM op
/// `0x3E` computes as `op0 - 100`.
///
/// Two of the seven (`Other2` / `Other3`) are dev modules with no shipped
/// gameplay; [`Self::is_playable`] separates them from the five minigames the
/// engine implements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MinigameSubId {
    /// `sub_id 0` - fishing (dev `other1`), PROT 0972.
    Fishing,
    /// `sub_id 1` - dev module `OTHER2`, PROT 0973. One sector; identity open.
    Other2,
    /// `sub_id 2` - dev module `OTHER3`, PROT 0974. Identity open.
    Other3,
    /// `sub_id 3` - casino slot machine (dev `other4`), PROT 0975.
    SlotMachine,
    /// `sub_id 4` - Baka Fighter (dev `other5`), PROT 0976.
    BakaFighter,
    /// `sub_id 5` - Muscle Dome arena roster / contest hub (dev `other6`),
    /// PROT 0977. The match state machine itself runs in the *battle* overlay;
    /// this slot is the door/init that sets the contest up.
    MuscleDome,
    /// `sub_id 6` - Noa dance rhythm minigame (Disco King), PROT 0980.
    Dance,
}

/// Global `music_01` track the Baka Fighter duel overlay's init loads:
/// sound-test **55** (`M112` "Sol disco fever"), i.e. extraction PROT entry
/// `1043`, the entry the init's own `FUN_8001FC00(0x415, ...)` names (see
/// [`minigame-baka-fighter.md`](../../docs/subsystems/minigame-baka-fighter.md)).
///
/// The bank map is **piecewise** - `988 + index` below sound-test 68 and
/// `990 + index` at or above it - so an extraction entry converts to a global
/// id through [`crate::music_labels::sound_test_index_for_prot_entry`] and not
/// by subtracting a single base. Reading `1043` as `990 + 53` is what put
/// `2053` here, two slots off, and the same slip cost both dance tracks below;
/// the round-trip is pinned by this module's tests.
pub const BAKA_FIGHTER_BGM_ID: u16 = 2055;

/// Global `music_01` track the dance overlay loads for the **short** song:
/// sound-test **60** (`M116` "Sol disco final 1"), extraction PROT `1048` -
/// the entry the overlay's `0x41A` loader index names.
pub const DANCE_SHORT_SONG_BGM_ID: u16 = 2060;

/// Global `music_01` track the dance overlay loads for the **long** song:
/// sound-test **66** (`M120` "Sol disco final 2"), extraction PROT `1054`
/// (loader index `0x420`). Which of the two a heat plays is the overlay's own
/// mode arm (`DAT_801D514C`) and is unpinned; the engine picks by song length.
pub const DANCE_LONG_SONG_BGM_ID: u16 = 2066;

impl MinigameSubId {
    /// All seven slots in `sub_id` order.
    pub const ALL: [MinigameSubId; 7] = [
        MinigameSubId::Fishing,
        MinigameSubId::Other2,
        MinigameSubId::Other3,
        MinigameSubId::SlotMachine,
        MinigameSubId::BakaFighter,
        MinigameSubId::MuscleDome,
        MinigameSubId::Dance,
    ];

    /// Decode a raw `sub_id` (`op0 - 100`). `None` outside `0..=6`, which is
    /// the whole warp id space - an out-of-range value is a desynced walk, not
    /// an eighth minigame.
    pub fn from_sub_id(sub_id: u8) -> Option<Self> {
        Self::ALL.get(sub_id as usize).copied()
    }

    /// Decode straight from the op `0x3E` operand byte (`op0 >= 100`).
    pub fn from_op0(op0: u8) -> Option<Self> {
        Self::from_sub_id(op0.checked_sub(100)?)
    }

    /// The raw `sub_id` retail stores at `_DAT_8007BA34`.
    pub fn sub_id(self) -> u8 {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(0) as u8
    }

    /// The operand byte a scene MAN carries for this slot (`sub_id + 100`).
    pub fn op0(self) -> u8 {
        self.sub_id() + 100
    }

    /// PROT extraction index of this slot's overlay.
    ///
    /// The bias and the `sub_id >= 6` step come from the ported mode-24 init
    /// staging plan ([`crate::mode::other_warp_init_stage`]) rather than being
    /// re-derived here; this only re-keys its loader param into extraction
    /// index space. Every slot is in range, so the `None` arm is unreachable -
    /// it falls back to the raw bias so a future id-space change cannot make
    /// this silently return a neighbouring entry.
    pub fn prot_index(self) -> u32 {
        let sub_id = self.sub_id();
        let param = crate::mode::other_warp_init_stage(i16::from(sub_id))
            .map(|stage| stage.overlay_a_param)
            .unwrap_or(i32::from(sub_id) + 0x4D);
        param as u32 + PROT_INDEX_REKEY
    }

    /// The overlay init VA retail `jalr`s for this slot, from the mode-24
    /// jump table at `0x80010AE4`.
    pub fn overlay_init_va(self) -> Option<u32> {
        crate::mode::other_warp_init_stage(i16::from(self.sub_id()))
            .map(|stage| stage.overlay_entry)
    }

    /// The scene mode the engine suspends into for this slot, or `None` for
    /// the two dev modules the engine does not implement.
    pub fn scene_mode(self) -> Option<SceneMode> {
        match self {
            MinigameSubId::Fishing => Some(SceneMode::Fishing),
            MinigameSubId::SlotMachine => Some(SceneMode::SlotMachine),
            MinigameSubId::BakaFighter => Some(SceneMode::BakaFighter),
            MinigameSubId::MuscleDome => Some(SceneMode::MuscleDome),
            MinigameSubId::Dance => Some(SceneMode::Dance),
            MinigameSubId::Other2 | MinigameSubId::Other3 => None,
        }
    }

    /// The **global-pool `music_01` track** this slot's overlay init loads,
    /// or `None` for the two slots that inherit the host scene's BGM.
    ///
    /// Three of the five playable slots bring their own music and two do not,
    /// and the split is retail's: the duel overlay's init (`FUN_801CF00C`)
    /// loads a Sol-disco track, the dance overlay loads one of two chart
    /// loops, and the Muscle Dome arena reuses the **battle** engine wholesale
    /// and therefore plays a battle theme
    /// ([`crate::music_labels::BATTLE_THEME_1_BGM_ID`]). The slot machine and
    /// the fishing venue load no track at all - the casino / lake scene's own
    /// music keeps playing under them.
    ///
    /// The dance is `None` here even though it has two tracks of its own
    /// ([`DANCE_SHORT_SONG_BGM_ID`] / [`DANCE_LONG_SONG_BGM_ID`]): its song
    /// starts when the pre-song count-in clears, not at entry, so
    /// [`crate::world::World::enter_dance`] holds the id and the world's
    /// dance tick starts it. Which of the two plays is the overlay's own
    /// mode arm, which is unpinned - the engine picks by song length.
    pub fn bgm_id(self) -> Option<u16> {
        match self {
            MinigameSubId::BakaFighter => Some(BAKA_FIGHTER_BGM_ID),
            MinigameSubId::MuscleDome => Some(crate::music_labels::BATTLE_THEME_1_BGM_ID),
            MinigameSubId::Dance
            | MinigameSubId::Fishing
            | MinigameSubId::SlotMachine
            | MinigameSubId::Other2
            | MinigameSubId::Other3 => None,
        }
    }

    /// Does the engine implement this slot as a playable minigame?
    ///
    /// The two dev modules do not resolve to a [`SceneMode`]; a warp naming
    /// one is a real retail site that reaches an unshipped module, so the host
    /// completes the round trip rather than parking.
    pub fn is_playable(self) -> bool {
        self.scene_mode().is_some()
    }

    /// Short stable label for logs / trace channels.
    pub fn label(self) -> &'static str {
        match self {
            MinigameSubId::Fishing => "fishing",
            MinigameSubId::Other2 => "other2",
            MinigameSubId::Other3 => "other3",
            MinigameSubId::SlotMachine => "slot_machine",
            MinigameSubId::BakaFighter => "baka_fighter",
            MinigameSubId::MuscleDome => "muscle_dome",
            MinigameSubId::Dance => "dance",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The loader arithmetic reproduces the pinned PROT indices, including the
    /// `+2` step retail applies at `sub_id >= 6`.
    #[test]
    fn prot_indices_match_the_dispatch_table() {
        assert_eq!(MinigameSubId::Fishing.prot_index(), 972);
        assert_eq!(MinigameSubId::Other2.prot_index(), 973);
        assert_eq!(MinigameSubId::Other3.prot_index(), 974);
        assert_eq!(MinigameSubId::SlotMachine.prot_index(), 975);
        assert_eq!(MinigameSubId::BakaFighter.prot_index(), 976);
        assert_eq!(MinigameSubId::MuscleDome.prot_index(), 977);
        // The `+2` step: 6 + 2 + 0x4D + 0x37F = 980, not 978.
        assert_eq!(MinigameSubId::Dance.prot_index(), 980);
    }

    /// `op0` round-trips through the `- 100` the VM applies.
    #[test]
    fn op0_round_trips_through_sub_id() {
        for slot in MinigameSubId::ALL {
            assert_eq!(MinigameSubId::from_op0(slot.op0()), Some(slot));
            assert_eq!(MinigameSubId::from_sub_id(slot.sub_id()), Some(slot));
        }
        // The whole id space is 7 wide; 107 is not an eighth minigame.
        assert_eq!(MinigameSubId::from_op0(107), None);
        assert_eq!(MinigameSubId::from_sub_id(7), None);
        // Below the warp threshold is the INTERACT arm, not a warp.
        assert_eq!(MinigameSubId::from_op0(99), None);
    }

    /// Each minigame BGM constant resolves to the **extraction PROT entry**
    /// its overlay's own loader call names.
    ///
    /// The bank map is piecewise, so this is the one arithmetic that has to
    /// hold: entry `1043` is sound-test `55`, not `53`. Three constants here
    /// were written by subtracting the high base from a documented extraction
    /// entry, which pointed every one of them two slots low.
    #[test]
    fn the_minigame_tracks_resolve_to_their_documented_prot_entries() {
        use crate::music_labels::prot_entry_for_bgm_id;
        assert_eq!(prot_entry_for_bgm_id(BAKA_FIGHTER_BGM_ID), Some(1043));
        assert_eq!(prot_entry_for_bgm_id(DANCE_SHORT_SONG_BGM_ID), Some(1048));
        assert_eq!(prot_entry_for_bgm_id(DANCE_LONG_SONG_BGM_ID), Some(1054));
        // The dome reuses the battle engine, so it plays a battle theme.
        assert_eq!(
            MinigameSubId::MuscleDome.bgm_id(),
            Some(crate::music_labels::BATTLE_THEME_1_BGM_ID)
        );
    }

    /// Two slots hand the door-warp drain a track to start; the dance's own
    /// pair is deferred to the end of its count-in, and the remaining two
    /// inherit the host scene's music.
    #[test]
    fn only_the_entry_started_tracks_are_named_here() {
        let carried: Vec<_> = MinigameSubId::ALL
            .into_iter()
            .filter(|s| s.bgm_id().is_some())
            .collect();
        assert_eq!(
            carried,
            vec![MinigameSubId::BakaFighter, MinigameSubId::MuscleDome]
        );
        // The slot machine and fishing inherit the host scene's BGM; the
        // dance holds its own until the count-in clears.
        for slot in [
            MinigameSubId::Fishing,
            MinigameSubId::SlotMachine,
            MinigameSubId::Dance,
        ] {
            assert_eq!(slot.bgm_id(), None, "{slot:?}");
        }
    }

    /// Exactly five of the seven slots are playable minigames; the two dev
    /// modules resolve to no scene mode.
    #[test]
    fn five_of_seven_slots_are_playable() {
        let playable: Vec<_> = MinigameSubId::ALL
            .into_iter()
            .filter(|s| s.is_playable())
            .collect();
        assert_eq!(playable.len(), 5, "playable slots: {playable:?}");
        assert!(!MinigameSubId::Other2.is_playable());
        assert!(!MinigameSubId::Other3.is_playable());
    }
}
