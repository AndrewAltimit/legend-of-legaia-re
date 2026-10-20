//! Shop **vendors**: which placed actor opens each shop in a scene.
//!
//! There is no shop table. A gold shop is a field-VM op `0x49` sub-op `0`
//! record written inline into the scene MAN's script bytes
//! ([`legaia_scene_formats::shop_stock`], `docs/subsystems/shop.md`), so the
//! vendor is whoever's script those bytes belong to. The MAN's three record
//! partitions tile its data region with contiguous script records; the record
//! whose span contains the shop op is the one that issues it. When that is a
//! partition-1 placement (index `>= 1`), its MAN record index is the NPC
//! catalog's `slot` ([`crate::field_npc`]), which is what the shops page joins
//! on to draw the shopkeeper's field model.
//!
//! A shop op that sits in a partition-0 / partition-2 record, or in the
//! scene-entry controller (partition 1, record 0), is reached through some
//! other script and has no actor of its own; it is reported with the owning
//! record so the page can say why it shows no model.

use super::*;
use legaia_engine_field::man_field_scripts as mfs;
use legaia_engine_scene::scene::{ProtIndex, Scene};
use legaia_scene_formats::man_section::{self, ActorPlacement, ManFile};
use legaia_scene_formats::shop_stock;

/// One shop record in a scene, with the record that issues it.
#[derive(Debug, Clone)]
pub struct ShopVendor {
    /// Vendor name the record carries (the plate the shop screen draws).
    pub name: String,
    /// Stock ids, in record order (the named prefix when a mask was given).
    pub stock: Vec<u8>,
    /// Absolute offset of the record's `count` byte in the decoded MAN.
    pub count_off: usize,
    /// `(partition, record index)` of the MAN record whose span holds the op.
    pub owner: Option<(usize, usize)>,
    /// World `(x, z)` the owning placement stands on while the shop is open
    /// ([`vendor_seat`]); `None` for a record that is not a placement or
    /// whose prologue never seats it.
    pub seat: Option<(i16, i16)>,
}

/// Every partition record's absolute start, sorted, as
/// `(start, partition, index)`.
fn record_starts(mf: &ManFile, man_len: usize) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    for (p, offs) in mf.partitions.iter().enumerate() {
        for (i, &o) in offs.iter().enumerate() {
            let abs = mf.data_region_offset + o as usize;
            if abs < man_len {
                out.push((abs, p, i));
            }
        }
    }
    out.sort_unstable();
    out
}

/// The record whose span contains `off`: the last record starting at or
/// before it. Spans end where the next record (or the MAN's first section)
/// begins, so a byte past every record start but inside the section chain
/// belongs to none.
fn owner_of(
    starts: &[(usize, usize, usize)],
    sections_start: usize,
    off: usize,
) -> Option<(usize, usize)> {
    if off >= sections_start {
        return None;
    }
    let k = starts.partition_point(|s| s.0 <= off);
    let (_, p, i) = *starts.get(k.checked_sub(1)?)?;
    Some((p, i))
}

/// Scan `name`'s MAN for shop records and attribute each to its record.
/// `named` restricts stock ids to named items (the `asset shop-stock` mask).
pub fn scene_shop_vendors(
    index: &ProtIndex,
    name: &str,
    named: Option<&[bool; 256]>,
) -> Result<Vec<ShopVendor>, String> {
    let scene = Scene::load(index, name).map_err(|e| format!("{e:#}"))?;
    let man = scene
        .field_man_payload(index)
        .map_err(|e| format!("MAN: {e:#}"))?
        .ok_or_else(|| format!("{name}: scene has no MAN"))?;
    let mf = man_section::parse(&man).map_err(|e| format!("MAN parse: {e:#}"))?;
    let starts = record_starts(&mf, man.len());
    let placements = mfs::classify_placements(&mf, &man);
    // The first section header bounds the record data region.
    let sections_start = mf
        .sections
        .iter()
        .map(|s| s.offset)
        .filter(|&o| o > mf.data_region_offset)
        .min()
        .unwrap_or(man.len());
    Ok(shop_stock::scan(&man, named)
        .into_iter()
        .map(|r| ShopVendor {
            stock: r
                .id_offsets
                .iter()
                .filter_map(|&o| man.get(o).copied())
                .collect(),
            owner: owner_of(&starts, sections_start, r.count_off),
            seat: None,
            count_off: r.count_off,
            name: r.name,
        })
        .map(|mut v| {
            v.seat = match v.owner {
                Some((1, slot)) if slot > 0 => placements
                    .iter()
                    .find(|(p, _)| p.index == slot)
                    .and_then(|(p, _)| vendor_seat(&mf, &man, p)),
                _ => None,
            };
            v
        })
        .collect())
}

/// Where a vendor's actor stands while its shop is open, in world units, or
/// `None` when no state of its spawn prologue seats it on the map.
///
/// A shopkeeper's placement is often a **flag-dispatched spawn**
/// ([`mfs::placement_spawn_relocation`]): its header tile is the off-map hide
/// box or a staging square, and the prologue's story-flag arms either park it
/// or run it to its counter. Which arm a visit takes depends on the save, so
/// this takes the first arm that seats the actor anywhere but the hide box -
/// every flag set (the later story state the town's shops are visited in),
/// then every flag clear (the fresh game) - and falls back to the header tile
/// when neither arm moves it and the header is not the hide box.
pub fn vendor_seat(mf: &ManFile, man: &[u8], p: &ActorPlacement) -> Option<(i16, i16)> {
    let seated = |enc: Option<(u8, u8)>| {
        enc.filter(|&(x, z)| (x & 0x7F, z & 0x7F) != mfs::PARKED_SENTINEL_TILE)
            .map(|(x, z)| (mfs::grid_byte_to_world(x), mfs::grid_byte_to_world(z)))
    };
    let hot = mfs::placement_spawn_relocation(mf, man, p, &|_| true);
    let cold = mfs::placement_spawn_relocation(mf, man, p, &|_| false);
    seated(hot).or_else(|| seated(cold)).or_else(|| {
        let hidden =
            p.world_x == mfs::FIELD_OFFMAP_HIDE_XZ || p.world_z == mfs::FIELD_OFFMAP_HIDE_XZ;
        // An arm that parks the actor in both states still leaves the header
        // as its only station.
        (!hidden).then_some((p.world_x, p.world_z))
    })
}

#[wasm_bindgen]
impl LegaiaViewer {
    /// The shops of the scene loaded by [`Self::set_scene_npcs`], each tied to
    /// the actor that opens it. JSON array:
    ///
    /// ```text
    /// [ { "name": "Market", "stock": [34, 52, ...],
    ///     "stock_names": ["Healing Leaf", ...], // item table names (or null)
    ///     "partition": 1, "record": 7,   // owning MAN record (null if none)
    ///     "npc": 3,                      // NPC catalog index, or null
    ///     "why": null,                   // reason when npc is null
    ///     "seat": [x, z] } ]             // where it stands (vendor_seat), or null
    /// ```
    ///
    /// `npc` indexes `field_npc_catalog_json().npcs` (`field_npc_mesh(npc)`
    /// builds the model). Returns `"[]"` when no catalog is loaded.
    pub fn scene_shop_vendors_json(&mut self) -> String {
        let Some(scene) = self.field_npcs.as_ref().map(|n| n.scene.clone()) else {
            return "[]".to_string();
        };
        let Ok(index) = self.ensure_prot_index() else {
            return "[]".to_string();
        };
        let named = self.item_names.as_ref().map(|t| {
            let mut m = [false; 256];
            for (id, slot) in m.iter_mut().enumerate() {
                *slot = id != 0 && t.name(id as u8).is_some();
            }
            m
        });
        let vendors = match scene_shop_vendors(&index, &scene, named.as_ref()) {
            Ok(v) => v,
            Err(e) => {
                console_log(&format!("shop vendors {scene}: {e}"));
                return "[]".to_string();
            }
        };
        let npcs = self.field_npcs.as_ref().expect("checked above");
        let rows: Vec<serde_json::Value> = vendors
            .iter()
            .map(|v| {
                let (npc, why) = match v.owner {
                    None => (None, Some("the shop op sits outside every script record")),
                    Some((1, 0)) => (None, Some("opened by the scene's own controller script, not a placed actor")),
                    Some((1, slot)) => match npcs.entries.iter().position(|e| e.placement.index == slot) {
                        Some(i) => (Some(i), None),
                        None => (None, Some("the vendor's actor draws from the party pool or ships no pose, so it is not in the NPC catalog")),
                    },
                    Some((p, _)) => (None, Some(if p == 0 {
                        "opened from a partition-0 scene script record, which no single placed actor owns"
                    } else {
                        "opened from an event script, not from a placed actor"
                    })),
                };
                let names: Vec<Option<&str>> = v
                    .stock
                    .iter()
                    .map(|&id| self.item_names.as_ref().and_then(|t| t.name(id)))
                    .collect();
                serde_json::json!({
                    "name": v.name,
                    "stock": v.stock,
                    "stock_names": names,
                    "partition": v.owner.map(|o| o.0),
                    "record": v.owner.map(|o| o.1),
                    "npc": npc,
                    "why": why,
                    "seat": v.seat.map(|(x, z)| [x, z]),
                })
            })
            .collect();
        serde_json::Value::Array(rows).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::owner_of;

    #[test]
    fn owner_is_last_record_starting_at_or_before() {
        let starts = vec![(100, 0, 0), (150, 1, 0), (200, 1, 1), (260, 2, 0)];
        assert_eq!(owner_of(&starts, 400, 99), None);
        assert_eq!(owner_of(&starts, 400, 100), Some((0, 0)));
        assert_eq!(owner_of(&starts, 400, 199), Some((1, 0)));
        assert_eq!(owner_of(&starts, 400, 230), Some((1, 1)));
        assert_eq!(owner_of(&starts, 400, 399), Some((2, 0)));
        assert_eq!(owner_of(&starts, 400, 400), None);
    }
}
