//! Window 41, the shop's party-wide stat compare `FUN_801D4C28` (PROT 0899),
//! against `engine-menus`' `party_compare_members_of` and `engine-ui`'s
//! `party_compare_panel_fields`, on `menu_painters`' rig.
//!
//! The routine is a composite: per present member it tests the eight equip
//! bytes for the staged id, the equip record's character mask, then trial
//! equips the piece in the record byte its class names, runs the menu block
//! summer `FUN_801CF650` and the stat aggregator `FUN_80042558` on both sides
//! of the swap, restores the record and draws. Both callees run for real, so
//! the compared numbers are the ones a player reads.

use crate::common::{Rng, disc, scus};
use crate::menu_painters::rig;
use crate::menu_screens::field_of;
use legaia_engine_menus::equipment::{DiscEquipInfo, equip_modifier_table_from_disc};
use legaia_engine_menus::shop::party_compare_members_of;
use legaia_engine_ui::menu::equip_stat_compare::party_compare_panel_fields;
use legaia_engine_ui::shop_screen::{
    ShopCompareLabels, ShopCompareMember, shop_party_compare_views,
};
use legaia_game_tables::equip_stats::EquipStatTable;
use legaia_save::CharacterRecord;
use psx_retail_exec::Stop;

const PARTY_COMPARE: u32 = 0x801D_4C28;
const AGGREGATOR: u32 = 0x8004_2558;
const PARTY_COUNT: u32 = 0x8008_4594;
const ROSTER: u32 = 0x8008_4598;
const RECORDS: u32 = 0x8008_4708;
const STRIDE: u32 = 0x414;
const STAGED: u32 = 0x801E_46B0;
const ITEMS: u32 = 0x8007_4368;
const EQUIPMENT: u32 = 0x8007_4F68;
/// The overlay's "Equipped" / "Cannot Equip" notes and the ATK / UDF / LDF
/// labels, as the routine addresses them.
const EQUIPPED: u32 = 0x801C_EBAC;
const CANNOT: u32 = 0x801C_EBB8;
const LABELS: [u32; 3] = [0x801C_E9A0, 0x801C_E9A4, 0x801C_E9A8];

/// The equipment ids a record wears, sorted the way a game holds them.
pub(crate) struct Gear {
    /// Every sellable piece.
    pub gear: Vec<u8>,
    /// The Ra-Seru forms: the item table's three runs (`0x01..=0x1A`) and
    /// any row flagged Ra-Seru (`+7` bit 0). They sit in their own byte and
    /// are never sold.
    pub raseru: Vec<u8>,
    /// [`Self::gear`] by `+7` class: body, head, weapon, footwear.
    pub by_class: [Vec<u8>; 4],
}

pub(crate) fn gear(m: &mut psx_retail_exec::Memory, info: &DiscEquipInfo) -> Gear {
    let all: Vec<u8> = (1..=255u8).filter(|id| info.entry(*id).is_some()).collect();
    let mut is_raseru = |id: u8| {
        let row = m.rd8(ITEMS + u32::from(id) * 12 + 1);
        id <= 0x1A || m.rd8(EQUIPMENT + u32::from(row) * 8 + 7) & 1 != 0
    };
    let (raseru, gear): (Vec<u8>, Vec<u8>) = all.into_iter().partition(|&id| is_raseru(id));
    assert!(!raseru.is_empty());
    let mut by_class: [Vec<u8>; 4] = Default::default();
    for &id in &gear {
        use legaia_game_tables::equip_stats::EquipSlot as S;
        let c = match info.entry(id).unwrap().category {
            S::Body => 0,
            S::Head => 1,
            S::Weapon => 2,
            S::Footwear => 3,
        };
        by_class[c].push(id);
    }
    Gear {
        gear,
        raseru,
        by_class,
    }
}

/// Worn bytes in retail's order: body, head, the weapon byte (`2` for Vahn
/// and Gala, `3` for Noa), the Ra-Seru byte, footwear, three Goods. Each
/// armament byte holds a piece of its own class, as the equip screen leaves it.
pub(crate) fn seed_record(
    rng: &mut Rng,
    r: &mut [u8],
    ch: usize,
    by_class: &[Vec<u8>; 4],
    raseru: &[u8],
) {
    for b in r.iter_mut() {
        *b = rng.below(256) as u8;
    }
    // No spells, so the aggregator's grant scan has nothing to shuffle.
    r[0x13C] = 0;
    let (weapon, seru) = if ch == 1 { (3, 2) } else { (2, 3) };
    let pick = |rng: &mut Rng, class: usize| {
        let v = &by_class[class];
        if rng.below(5) == 0 {
            0
        } else {
            v[rng.below(v.len() as u32) as usize]
        }
    };
    let worn: [u8; 8] = std::array::from_fn(|slot| match slot {
        0 => pick(rng, 0),
        1 => pick(rng, 1),
        4 => pick(rng, 3),
        s if s == weapon => pick(rng, 2),
        s if s == seru && rng.below(2) == 0 => raseru[rng.below(raseru.len() as u32) as usize],
        s if s < 4 => 0,
        _ => match rng.below(3) {
            0 => 0,
            _ => rng.below(256) as u8,
        },
    });
    r[0x196..0x19E].copy_from_slice(&worn);
    for (off, top) in [
        (0x11Cusize, 9999u32),
        (0x11E, 999),
        (0x120, 999),
        (0x122, 999),
        (0x124, 999),
        (0x126, 999),
        (0x128, 999),
        (0x12A, 999),
        (0x12C, 999),
    ] {
        let v = rng.below(top) as u16;
        r[off..off + 2].copy_from_slice(&v.to_le_bytes());
    }
    let name = format!("Hero{}", rng.below(100));
    r[0x2A7..0x2A7 + name.len()].copy_from_slice(name.as_bytes());
    r[0x2A7 + name.len()] = 0;
}

#[test]
fn the_party_compare_column_matches() {
    differential!(prot(899, PARTY_COMPARE), scus(AGGREGATOR));
    let Some(mut rig) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus = scus().unwrap();
    let _ = disc();
    let stats = EquipStatTable::from_scus(&scus).expect("equipment");
    let table = equip_modifier_table_from_disc(&stats);
    let info = DiscEquipInfo::from_disc(&stats);
    let Gear {
        gear,
        raseru,
        by_class,
    } = gear(&mut rig.cpu.mem, &info);
    let others: Vec<u8> = (1..=255u8)
        .filter(|&id| rig.cpu.mem.rd8(ITEMS + u32::from(id) * 12) != 1)
        .collect();
    let equipped = rig.string(EQUIPPED);
    let cannot = rig.string(CANNOT);
    let labels: Vec<String> = LABELS.iter().map(|&a| rig.string(a)).collect();
    assert!(equipped.ends_with("Equipped") && cannot.ends_with("Cannot Equip"));
    assert_eq!(labels.len(), 3);

    let mut rng = Rng(0x5041_5254);
    let mut seen = [0u32; 4];
    for run in 0..4_000 {
        let r = crate::menu_painters::rect(&mut rng);
        for ch in 0..3u32 {
            let mut raw = vec![0u8; STRIDE as usize];
            seed_record(&mut rng, &mut raw, ch as usize, &by_class, &raseru);
            rig.cpu.mem.load(RECORDS + ch * STRIDE, &raw);
        }
        let n = 1 + rng.below(3);
        let mut ids = vec![0u8, 1, 2];
        let present: Vec<u8> = (0..n)
            .map(|_| ids.remove(rng.below(ids.len() as u32) as usize))
            .collect();
        rig.cpu.mem.wr8(PARTY_COUNT, n as u8);
        for (i, &id) in present.iter().enumerate() {
            rig.cpu.mem.wr8(ROSTER + i as u32, id);
        }
        // The menu leaves the records aggregated.
        assert_eq!(rig.cpu.call(AGGREGATOR, &[]), Stop::Returned);
        let staged: u8 = match rng.below(8) {
            0 => others[rng.below(others.len() as u32) as usize],
            // An id one member already wears (a shop never stages a Ra-Seru).
            1 => {
                let id = rig
                    .cpu
                    .mem
                    .rd8(RECORDS + u32::from(present[0]) * STRIDE + 0x196 + rng.below(8));
                if raseru.contains(&id) { 0 } else { id }
            }
            _ => gear[rng.below(gear.len() as u32) as usize],
        };
        if staged == 0 {
            continue;
        }
        rig.cpu.mem.wr32(STAGED, u32::from(staged));
        let records: Vec<CharacterRecord> = (0..3)
            .map(|ch| {
                CharacterRecord::parse(&rig.cpu.mem.read(RECORDS + ch * STRIDE, STRIDE as usize))
                    .unwrap()
            })
            .collect();
        let before: Vec<Vec<u8>> = (0..3)
            .map(|ch| rig.cpu.mem.read(RECORDS + ch * STRIDE, STRIDE as usize))
            .collect();

        let log = rig.run(PARTY_COMPARE, r);
        // The trial equip is undone.
        for ch in 0..3u32 {
            let after = rig.cpu.mem.read(RECORDS + ch * STRIDE, STRIDE as usize);
            assert!(
                after == before[ch as usize],
                "run {run}: record {ch} restored: {:?} staged {staged:#x} worn {:x?} present {present:?}",
                crate::common::diff_offsets(&before[ch as usize], &after),
                &before[ch as usize][0x196..0x19E]
            );
        }

        let members = party_compare_members_of(&records, &present, &table, Some(&info), staged);
        let shop: Vec<ShopCompareMember<'_>> = members
            .iter()
            .map(|m| ShopCompareMember {
                name: m.name.as_str(),
                already_equipped: m.already_equipped,
                equippable: m.equippable,
                current: m.current,
                candidate: m.candidate,
            })
            .collect();
        let views = shop_party_compare_views(
            &shop,
            ShopCompareLabels {
                equipped: &equipped,
                cannot_equip: &cannot,
                stats: [&labels[0], &labels[1], &labels[2]],
            },
        );
        let port = party_compare_panel_fields(&views, (r.x, r.y));
        let got: Vec<_> = log.iter().filter_map(field_of).collect();
        assert_eq!(
            got, port,
            "run {run}: staged {staged:#x} present {present:?} members {members:#?}"
        );
        for m in &members {
            seen[if m.already_equipped {
                0
            } else if !m.equippable {
                1
            } else if m.candidate.is_some() {
                2
            } else {
                3
            }] += 1;
        }
    }
    assert!(seen.iter().all(|&n| n > 100), "{seen:?}");
}
