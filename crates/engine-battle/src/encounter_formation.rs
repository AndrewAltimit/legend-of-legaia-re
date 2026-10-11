//! Formation expansion of an on-disc encounter record: the step from the
//! wire layout (`legaia_scene_formats::encounter_record`) to the monster
//! catalog's [`FormationDef`].

use crate::monster_catalog::{FormationDef, FormationSlot};
use legaia_scene_formats::encounter_record::EncounterRecord;

/// Convert an encounter record into a [`FormationDef`] suitable for the runtime
/// battle session.
///
/// The retail engine identifies a battle by the bytes of the formation
/// cell - there is no separate "formation id" on disc. We synthesize one
/// from the cell bytes (`monster_ids` packed little-endian into the low
/// 32 bits, then folded into the u16 id space) so engines can register
/// and look up the formation in [`crate::monster_catalog::FormationTable`].
///
/// Engines that need a stable / human-meaningful id should prefer
/// constructing the [`FormationDef`] manually; this function is for the
/// "decode an on-disc record and play it" path.
pub fn formation_def_from_record(
    record: &EncounterRecord,
    label: impl Into<String>,
) -> FormationDef {
    let slots: Vec<FormationSlot> = record
        .active_ids()
        .map(|id| FormationSlot::new(id as u16))
        .collect();
    let formation_id = record.synthetic_formation_id();
    FormationDef::new(formation_id, slots).with_label(label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_formation_def_yields_one_slot_per_active_id() {
        let r = EncounterRecord {
            count: 2,
            monster_ids: [0x04, 0x07, 0, 0],
        };
        let f = formation_def_from_record(&r, "test_scene");
        assert_eq!(f.slots.len(), 2);
        assert_eq!(f.slots[0].monster_id, 4);
        assert_eq!(f.slots[1].monster_id, 7);
        assert_eq!(f.label, "test_scene");
    }

    #[test]
    fn to_formation_def_empty_record_has_no_slots() {
        let f = formation_def_from_record(&EncounterRecord::EMPTY, "empty");
        assert!(f.slots.is_empty());
    }

    #[test]
    fn rim_elm_training_resolves_to_a_single_slot_formation() {
        let r = EncounterRecord::rim_elm_training();
        // Resolves to a single-slot formation carrying the training id.
        let f = formation_def_from_record(&r, "town01");
        assert_eq!(f.slots.len(), 1);
        assert_eq!(f.slots[0].monster_id, 0x4F);
    }
}
