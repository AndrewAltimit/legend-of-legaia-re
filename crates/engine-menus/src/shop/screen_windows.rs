//! Which windows each shop phase shows, their slide-in state and the party compare strip.

/// Which retail shop screen is up - the sub-screen family the menu
/// overlay's shop dispatchers run, collapsed to what decides the window set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShopScreenPhase {
    /// The Buy / Sell / Quit picker (`FUN_801DAFD4`, sub-screen `0x1A`).
    Root,
    /// The buy list (`FUN_801DB21C`, sub-screen `0x1B`).
    BuyList,
    /// The buy-quantity stepper (`FUN_801DB7F4`, sub-screen `0x1D`).
    BuyQuantity,
    /// The equipment-buy recipient picker (`FUN_801DB380`, sub-screen `0x1C`).
    BuyRecipient,
    /// The sell list (`FUN_801DBC5C`, sub-screen `0x1E`).
    SellList,
    /// The sell-quantity stepper (`FUN_801DBD94`, sub-screen `0x1F`).
    SellQuantity,
}

/// The menu-overlay windows a shop screen shows, in retail draw order (a
/// later window's frame covers an earlier one's).
///
/// Each set is what the shop's widget scripts leave on screen
/// (`docs/formats/window-script.md`):
///
/// * root - `0x801E4E38` opens `0x21` vendor plate, `0x2A` picker, `0x20`
///   purse, `0x28` buy list (parked) and `0x22` item info;
/// * buy list - `0x801E4E64` re-creates `0x28` and adds `0x29` (the party
///   compare), whose frame covers the picker it does not close;
/// * buy quantity / recipient - `0x801E4EB0` / `0x801E4E84` move `0x28`
///   off screen (`02 28 A52E`: x `0x14A`) and open `0x23` / `0x24`;
/// * sell list - `0x801E4E54` closes `0x28` / `0x2A` / `0x22`, then
///   `0x801E4EE4` opens `0x26` (the price-gated bag list) and `0x27`;
/// * sell quantity - `0x801E4F08` adds `0x25`.
///
/// The covered picker is left out of the buy sets rather than drawn under
/// window 41: a host draws every frame before any text, so a covered
/// window's rows would print through the frame that hides them in retail.
/// The Point Card toast (`0x1F`, `0x801E4EA8` / `0x801E4EDC`) is the last
/// window when `toast` is set. Every set is capture-confirmed on Retock's
/// Items Shop.
pub fn shop_screen_windows(phase: ShopScreenPhase, toast: bool) -> Vec<usize> {
    let mut v: Vec<usize> = match phase {
        ShopScreenPhase::Root => vec![33, 42, 32, 40, 34],
        ShopScreenPhase::BuyList => vec![33, 32, 40, 34, 41],
        ShopScreenPhase::BuyQuantity => vec![33, 32, 34, 41, 35],
        ShopScreenPhase::BuyRecipient => vec![33, 32, 34, 41, 36],
        ShopScreenPhase::SellList => vec![33, 32, 38, 39],
        ShopScreenPhase::SellQuantity => vec![33, 32, 38, 39, 37],
    };
    if toast {
        // The quantity commit closes the stepper before it raises the toast:
        // `0x801E4ED4` is `04 23` (close window 35), and the toast script
        // `0x801E4EDC` only opens `0x1F`; the buy list comes back with
        // `0x801E4E64` after the press. The Retock capture shows the toast
        // over windows 33 / 32 / 34 / 41 alone.
        if phase == ShopScreenPhase::BuyQuantity {
            v.retain(|&w| w != 35);
        }
        v.push(31);
    }
    v
}

/// Frames a shop window takes to slide between its park position and its
/// home rect. The Retock capture, stepped one vsync at a time, has every
/// window of the open set (picker, list, purse, plate) leave its park edge
/// together and land eleven frames later at a constant per-window speed.
pub const SHOP_SLIDE_FRAMES: u8 = 11;

/// The shop's window slides: which windows are on screen and how far along
/// their slide each one is.
///
/// A window that joins the screen's set starts at its park edge (the
/// descriptor's `+0x1` class, off screen) and travels home linearly over
/// [`SHOP_SLIDE_FRAMES`]; one that leaves the set travels back out and is
/// dropped when it arrives. That is the motion the widget scripts' open (op
/// `1`) and close (op `4`) commands produce on the live window list. The
/// geometry (home rect, park edge) is the descriptor table's, which the
/// drawing side owns; this keeps only the timing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShopSlides {
    /// `(window id, progress 0..=SHOP_SLIDE_FRAMES, leaving)`, leaving
    /// windows first, then the current set in its draw order.
    entries: Vec<(usize, u8, bool)>,
}

impl ShopSlides {
    /// Advance one frame toward `target` (the current screen's window set).
    pub fn tick(&mut self, target: &[usize]) {
        for e in &mut self.entries {
            if e.2 {
                e.1 = e.1.saturating_sub(1);
            } else if e.1 < SHOP_SLIDE_FRAMES {
                e.1 += 1;
            }
        }
        self.entries.retain(|e| !(e.2 && e.1 == 0));
        let mut leaving: Vec<(usize, u8, bool)> = self
            .entries
            .iter()
            .filter(|e| !target.contains(&e.0))
            // A window that starts leaving this frame already takes its
            // first step out.
            .map(|e| (e.0, if e.2 { e.1 } else { e.1.saturating_sub(1) }, true))
            .filter(|e| e.1 > 0)
            .collect();
        let staying: Vec<(usize, u8, bool)> = target
            .iter()
            .map(|&id| match self.entries.iter().find(|e| e.0 == id) {
                Some(e) => (id, e.1, false),
                // A new window is drawn on its first frame already one step
                // in from the park edge.
                None => (id, 1, false),
            })
            .collect();
        leaving.extend(staying);
        self.entries = leaving;
    }

    /// Drop every window at once (the shop closed).
    pub fn reset(&mut self) {
        self.entries.clear();
    }

    /// `(window id, progress)` in draw order. Progress
    /// [`SHOP_SLIDE_FRAMES`] is home; `0` is fully parked.
    pub fn draw_list(&self) -> Vec<(usize, u8)> {
        self.entries.iter().map(|e| (e.0, e.1)).collect()
    }

    /// Whether every window is home (nothing in motion).
    pub fn settled(&self) -> bool {
        self.entries
            .iter()
            .all(|e| !e.2 && e.1 == SHOP_SLIDE_FRAMES)
    }
}

/// One party member's block in window 41 (`FUN_801D4C28`), as a host hands
/// it to `engine-ui`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartyCompareMember {
    /// Character id (`world.party.roster.members[member]`).
    pub member: usize,
    /// Display name (character record `+0x2A7`).
    pub name: String,
    /// The staged id already sits in one of the member's eight equip bytes
    /// (`+0x196..+0x19D`) - retail prints the "Equipped" note.
    pub already_equipped: bool,
    /// The equip record's `+6` character mask accepts the member. Always
    /// `true` for a non-equipment id, which skips the mask test.
    pub equippable: bool,
    /// The live eight-word menu block (`0x801EF080`): HP max, MP max, AGL,
    /// ATK, UDF, LDF, SPD, INT, equipment bonuses summed in.
    pub current: [i32; 8],
    /// The trial-equip block (`0x801EF0A0`), or `None` for a staged id that
    /// is not equipment (retail draws the current values with no arrows).
    pub candidate: Option<[i32; 8]>,
}

/// Window 41's model for staged item `item_id`: one block per **present
/// party** member, in party-list order.
///
/// Retail walks `DAT_80084594` rows and resolves each through the party list
/// at `0x80084598` (`lbu s1,0x458(v0)` at `0x801D4D14`), drawing member `i`
/// `0x37` lines below member `i - 1`; a character who has not joined has no
/// block. `present` is that list (`World::present_party_list`), and each id
/// indexes `members`, the four character records.
///
/// Both blocks are the menu aggregator's ([`crate::pause_screens::menu_stat_block`],
/// `FUN_801CF5D0` + `FUN_801CF650`): the live one off the record's equip
/// bytes, the trial one with the staged piece written into the **record
/// byte** its equip class names - the inline trial equip `FUN_801D4C28`
/// runs at `0x801D4DE4..0x801D4E20`: class `(+7 & 0x60) >> 5` maps
/// weapon to the per-character halfword `DAT_8007B42C` (`2, 3, 2`) and the
/// other three through `DAT_801E43E8` (head `1`, body `0`, legs `4`). The
/// record order matters: writing the weapon into byte `0` would displace
/// the body armour instead of the weapon.
///
/// The mask test (`+6`, bit `1 << char`) runs for characters `0..=2` only
/// (`0x801D4E24..0x801D4E9C`); any other id falls through as equippable.
/// Shared by the buy list, the quantity stepper and the recipient picker on
/// both hosts, so the three screens cannot disagree about one member.
///
/// REF: FUN_801D4C28
pub fn party_compare_members_of(
    members: &[legaia_save::CharacterRecord],
    present: &[u8],
    table: &legaia_engine_battle::battle_stats::EquipmentTable,
    info: Option<&crate::equipment::DiscEquipInfo>,
    item_id: u8,
) -> Vec<PartyCompareMember> {
    use legaia_game_tables::equip_stats::EquipSlot as Disc;
    let entry = info.and_then(|i| i.entry(item_id));
    present
        .iter()
        .filter_map(|&ch| {
            members
                .get(usize::from(ch))
                .map(|rec| (usize::from(ch), rec))
        })
        .map(|(ch, rec)| {
            let hp_mp = rec.hp_mp_sp();
            let stat = crate::pause_screens::stat_record_from_character(rec);
            let current =
                crate::pause_screens::menu_stat_block(&stat, table, hp_mp.hp_max, hp_mp.mp_max);
            let candidate = entry.map(|e| {
                let byte = match e.category {
                    Disc::Weapon => crate::equip_session::RETAIL_WEAPON_EQUIP_BYTE
                        .get(ch)
                        .map_or(2, |&b| b as usize),
                    Disc::Head => 1,
                    Disc::Body => 0,
                    Disc::Footwear => 4,
                };
                let mut trial = stat;
                if let Some(slot) = trial.equip.get_mut(byte) {
                    *slot = item_id;
                }
                crate::pause_screens::menu_stat_block(&trial, table, hp_mp.hp_max, hp_mp.mp_max)
            });
            PartyCompareMember {
                member: ch,
                name: rec.name(),
                already_equipped: item_id != 0 && rec.equipment().slots.contains(&item_id),
                equippable: match entry {
                    None => true,
                    Some(e) => ch > 2 || e.mask & (1 << ch) != 0,
                },
                current,
                candidate,
            }
        })
        .collect()
}
