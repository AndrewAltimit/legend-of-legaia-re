//! Party + save-game state: roster, active party and leader, money, inventory, ability masks, tactical arts, level-up tracking, banners, per-character save extensions and name entry.
//!
//! Split out of the composite [`World`] so the state one subsystem owns
//! reads as one unit. Fields keep their retail provenance notes.

use super::*;

/// Party + save-game state: roster, active party and leader, money, inventory, ability masks, tactical arts, level-up tracking, banners, per-character save extensions and name entry.
pub struct PartyState {
    /// Party-member actor slots - `party_actor_slots[i] = Some(actor_slot)`
    /// resolves move-VM ext sub-op 0x3B (`ext_party_member_lookup`) to the
    /// world-coords of the actor at that slot. Default empty (the lookup
    /// returns `None`, which forces sub-op 0x3B's "skip" path).
    pub party_actor_slots: Vec<Option<u8>>,
    /// Battle-action helper tables.
    ///
    /// There is deliberately **no** spell-cost or capture-spell table here.
    /// `BattleHostImpl` answers both questions from the models already loaded
    /// at boot - [`crate::world::DiscTables::spell_catalog`] for the price, and the disc spell
    /// table's class byte (`World::spell_table_class`, off
    /// [`crate::world::MenuState::text`]) for the capture route - so the battle-action
    /// state machine and the live cast path cannot price or classify the same
    /// spell differently. A pair of `HashMap`s here that nothing filled is
    /// exactly how they used to.
    /// There is no melee-range table here either, for the same reason and by
    /// the same evidence: retail computes reach (`FUN_8004E2F0`) from the
    /// attacker's per-character base, both actors' size classes and their live
    /// positions. [`crate::world::World::battle_range_metric`] answers from those models.
    pub character_ability_bits: [u32; 8],
    /// Number of party slots (default 3).
    pub party_count: u8,
    /// Present-party composition: `active_party[i]` = the **roster slot**
    /// (index into [`crate::world::PartyState::roster`]) occupying battle ordinal `i`. The
    /// engine mirror of retail's present-party list at `0x8007BD10`
    /// (1-based char ids there; 0-based roster slots here). Battle actor
    /// slot `i`, HUD row `i`, and the runtime VRAM texture band `i`
    /// (`relocate_tsb_cba` row `481 + i`) all key on the ORDINAL; the
    /// character content (player battle file `863 + roster_slot`,
    /// equipment, spell list, XP recipient) keys on the roster slot -
    /// the live-verified retail banding rule (band = ordinal, file =
    /// 862 + char_id). Empty = identity mapping (slot `i` = roster `i`,
    /// the Vahn/Noa/Gala default). Resolve through
    /// [`crate::world::World::party_roster_slot`]; install via [`crate::world::World::set_active_party`].
    pub active_party: Vec<u8>,
    /// Persistent per-character roster - populated by [`crate::world::World::load_party`]
    /// and written back by [`crate::world::World::save_party`]. Each record is the
    /// 0x414-byte struct documented in `docs/subsystems/battle.md`. The
    /// in-battle `BattleActor` slots mirror HP / MP from this; everything
    /// else (spells, equipment, ability bits) flows through this canonical
    /// store.
    pub roster: legaia_save::Party,
    /// Active party slot for the leader (op 0x4C sub-0 writes here, plus
    /// `party_add` populates it on the first member).
    pub party_leader_slot: Option<u8>,
    /// Running money total (gold). Modified by op 0x3A `add_money`,
    /// clamped to `[0, 9_999_999]` per the original retail formula.
    pub money: i32,
    /// The party's bag: retail's 256-slot `[id][count]` array at `0x80085958`
    /// with the active window `gp[+0x2D2..+0x2D6]` installed over it
    /// ([`crate::world::ItemBag`], `docs/subsystems/inventory.md`).
    ///
    /// The map-shaped calls (`get` / `insert` / `entry` / ...) are an adapter
    /// over that array, so an id-addressed consumer reads as it always did;
    /// what the array adds is a slot coordinate, holes, and the window - the
    /// three things PROT 0941's Steal sampler draws over and a
    /// `HashMap<u8, u8>` cannot express.
    pub inventory: crate::world::ItemBag,
    /// Per-character Tactical Arts use-counter tracker. Engines call
    /// [`crate::world::World::notify_art_used`] from the battle side-effects handler when
    /// a Tactical Arts strike lands; the tracker emits
    /// [`BattleEvent::TacticalArtLearned`] and sets
    /// [`crate::world::PartyState::current_art_banner`] on first learn.
    pub tactical_arts: TacticalArtsTracker,
    /// Active "art learned" HUD banner. Set by [`crate::world::World::notify_art_used`]
    /// when a new art crosses the learn threshold; its `frames_remaining`
    /// counter is decremented by [`crate::world::World::tick`] until it reaches zero.
    /// `None` when no banner is active. Engines render this as a dialog-
    /// font overlay above the battle HUD.
    pub current_art_banner: Option<ArtLearnedBanner>,
    /// Per-party XP accumulator and level state. Engines call
    /// [`crate::world::World::apply_battle_xp`] after a `BattleEndCause::MonsterWipe` to
    /// distribute XP and check for level-ups.
    pub level_up_tracker: LevelUpTracker,
    /// Active level-up HUD banner. Set by [`crate::world::World::apply_battle_xp`];
    /// `frames_remaining` is decremented by [`crate::world::World::tick`] until it reaches
    /// zero, at which point the next entry of
    /// [`crate::world::PartyState::pending_level_up_banners`] takes the slot. `None` when no
    /// banner is active. Engines render this as a dialog-font overlay after
    /// battle.
    pub current_level_up_banner: Option<LevelUpBanner>,
    /// Level-up banners waiting for the slot above, in the order they were
    /// earned.
    ///
    /// One fight can level several party members, and the banner is a single
    /// slot. Writing it per member inside the distribution loop meant each
    /// leveller overwrote the previous one in the same frame and the player
    /// saw exactly one banner - the last - for a battle that levelled three.
    /// Queueing is what makes "three members levelled" legible as three
    /// banners.
    pub pending_level_up_banners: std::collections::VecDeque<LevelUpBanner>,
    /// Active post-battle Seru-capture banner. Set by `World::resolve_captures`
    /// when a capture is accepted; advanced one frame per [`crate::world::World::tick`] and
    /// cleared when its [`crate::seru_learning::SeruCaptureSession`] reaches
    /// `Done`. Engines render [`crate::seru_learning::SeruCaptureSession::current_banner`]
    /// as a dialog-font overlay after battle, the sibling of
    /// [`crate::world::PartyState::current_level_up_banner`].
    pub current_capture_banner: Option<crate::seru_learning::SeruCaptureSession>,
    /// Per-character v2 save extension data. Mirrors `SaveExtV2` shape;
    /// engines populate from in-memory state at save time and consume on
    /// load. Index 0..=2 = main characters; entries beyond are story
    /// guests. Each entry holds learned-arts mask, learned spells, seru
    /// captures, and per-character active chain quick-slots.
    pub per_char_ext: Vec<(u8, legaia_save::CharSaveExt)>,
    /// Cross-character saved-chain library. Engines populate from a
    /// [`crate::tactical_arts_editor::ChainLibrary`] at save time and
    /// hydrate one back into the editor on load.
    pub saved_chains: Vec<legaia_save::SavedChainRecord>,
    /// Per-scene **save permission** - retail's `_DAT_8007B6A8`.
    ///
    /// Seeded at scene load from the scene MAN header's `[0x01] & 1`
    /// ([`legaia_asset::man_section::ManHeader::low_flag`]) by
    /// [`crate::world::World::install_scene_save_permission`]; a scene with no MAN, and a
    /// world that has not loaded one, reads `false` - the same state retail's
    /// own init leaves the byte in. Read by the pause menu, where a cleared
    /// flag greys the Save row and buzzes its confirm
    /// ([`crate::pause_screens::root_menu_confirm_route`]).
    pub scene_save_allowed: bool,
    /// Party-global 4×u32 ability mask - the engine mirror of retail
    /// `DAT_80074358..0x80074368` (every member's `+0xF4` bitfield OR'd
    /// together each rebuild). Bit-tested via [`crate::world::World::party_has_ability`]
    /// (the `FUN_800431D0` port); rebuilt by
    /// [`crate::world::World::refresh_party_ability_bits`].
    pub party_ability_mask: [u32; crate::accessory_passives::ABILITY_WORDS],
    /// Per-party-slot display names. Seeded from the starting-party template
    /// at [`crate::world::World::seed_starting_party`] and overwritten by the name-entry
    /// overlay ([`crate::world::World::open_name_entry`]). Indexed by party slot; a slot with
    /// no entry falls back to the template name at the call site.
    pub party_names: Vec<String>,
    /// Active name-entry overlay session, or `None` when no name is being
    /// entered. Installed by [`crate::world::World::open_name_entry`] (the opening `town01`
    /// script's lead-character prompt) and driven by
    /// [`crate::world::World::step_name_entry`]; on commit the name lands in
    /// [`crate::world::PartyState::party_names`].
    pub name_entry: Option<crate::name_entry::NameEntry>,
}

impl PartyState {
    pub fn new() -> Self {
        Self {
            party_actor_slots: Vec::new(),
            character_ability_bits: [0; 8],
            party_count: 3,
            active_party: Vec::new(),
            roster: legaia_save::Party::zeroed(0),
            party_leader_slot: None,
            money: 0,
            inventory: crate::world::ItemBag::new(),
            tactical_arts: TacticalArtsTracker::new(),
            current_art_banner: None,
            level_up_tracker: LevelUpTracker::new(),
            current_level_up_banner: None,
            pending_level_up_banners: std::collections::VecDeque::new(),
            current_capture_banner: None,
            per_char_ext: Vec::new(),
            saved_chains: Vec::new(),
            scene_save_allowed: false,
            party_ability_mask: [0; crate::accessory_passives::ABILITY_WORDS],
            party_names: Vec::new(),
            name_entry: None,
        }
    }
}

impl Default for PartyState {
    fn default() -> Self {
        Self::new()
    }
}
