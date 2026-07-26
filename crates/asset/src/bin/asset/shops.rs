//! `asset shop-stock` - dump every town gold shop's inventory off a disc.
//!
//! Joins two files that live nowhere near each other:
//!
//! * the **stock** is inline in each scene's MAN field-VM script (op `0x49`
//!   sub-op `0`), inside a `PROT.DAT` scene bundle - there is no shop table;
//! * the **names and prices** come from the static `SCUS_942.54` item record
//!   table (`DAT_80074368`, `0xC` stride; price is the `u16` at `+2`).
//!
//! See [`docs/subsystems/shop.md`](../../../../../docs/subsystems/shop.md).

use std::path::Path;

use anyhow::{Context, Result};
use legaia_asset::{item_names, shop_stock};
use legaia_prot::{archive::Archive, cdname};

/// A CD sector's 12-byte sync pattern, for the "you fed me the disc image"
/// guard - the same mix-up `field-disasm scan-prot` catches.
const CD_SECTOR_SYNC: [u8; 12] = [
    0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00,
];

/// One decoded shop, ready to print.
struct Shop {
    entry_index: u32,
    scene: String,
    name: String,
    /// Offset of the record's `count` byte in the decompressed MAN. A scene can
    /// carry the same shop more than once (one record per script path that
    /// opens it), so this is what tells two identical listings apart.
    man_off: usize,
    /// `(id, name, price)` in record order - the whole `count`, tail included.
    items: Vec<(u8, String, u16)>,
    /// How many leading ids are actually purchasable.
    sellable: usize,
}

pub(crate) fn shop_stock_cmd(
    prot: &Path,
    scus: &Path,
    cdname_path: Option<&Path>,
    scene_filter: Option<&str>,
    entry_filter: Option<u32>,
    json: bool,
) -> Result<()> {
    // Friendly redirect for the most common mix-up: handing over the whole
    // disc image where PROT.DAT is expected.
    {
        use std::io::Read as _;
        let mut head = [0u8; 12];
        let mut f = std::fs::File::open(prot)
            .with_context(|| format!("opening PROT.DAT file {}", prot.display()))?;
        let n = f.read(&mut head)?;
        if n == head.len() && head == CD_SECTOR_SYNC {
            anyhow::bail!(
                "{} is a raw disc image - extract PROT.DAT first with \
                 `disc-extract extract <bin> extracted/` (or run `legaia-extract`)",
                prot.display()
            );
        }
    }

    let scus_bytes = crate::common::read_input(scus)?;
    let names = item_names::ItemNameTable::from_scus(&scus_bytes).ok_or_else(|| {
        anyhow::anyhow!(
            "no item-name table at VA {:#x} in {} - is this SCUS_942.54?",
            item_names::TABLE_VA,
            scus.display()
        )
    })?;
    // Two different masks, deliberately. The scan is gated on ids that *name*
    // a real item, because the unsellable template tail is part of the record
    // and a price-gated mask would reject or truncate it. Sellability is then
    // computed separately, so the tail can be shown rather than hidden.
    let named_mask: [bool; 256] = std::array::from_fn(|id| names.name(id as u8).is_some());
    let price_of = |id: u8| item_names::item_price(&scus_bytes, id).unwrap_or(0);

    let map = match cdname_path {
        Some(p) => cdname::parse(p)?,
        None => cdname::IndexMap::new(),
    };

    let mut archive = Archive::open(prot)?;
    let entries = archive.entries.clone();
    let mut buf = Vec::new();
    let mut shops: Vec<Shop> = Vec::new();

    for entry in &entries {
        if entry_filter.is_some_and(|want| want != entry.index) {
            continue;
        }
        // Retail-semantic block name: CDNAME `#define` numbers are raw in-RAM
        // TOC indices, so the block covering extraction entry `p` is looked up
        // at `p + 2`. Never reimplement that arithmetic here.
        let scene = cdname::block_for_extraction_index(&map, entry.index).unwrap_or("");
        if let Some(want) = scene_filter
            && !scene.eq_ignore_ascii_case(want)
            && !scene.contains(want)
        {
            continue;
        }
        if archive.read_entry(entry, &mut buf).is_err() {
            continue;
        }
        let Some(located) = shop_stock::locate(&buf, Some(&named_mask)) else {
            continue;
        };
        for rec in &located.records {
            let items: Vec<(u8, String, u16)> = rec
                .id_offsets
                .iter()
                .filter_map(|&o| located.decoded.get(o).copied())
                .map(|id| {
                    (
                        id,
                        names.name(id).unwrap_or("<unnamed>").to_string(),
                        price_of(id),
                    )
                })
                .collect();
            let sellable = rec.sellable_count(&located.decoded, |id| price_of(id) > 0);
            shops.push(Shop {
                entry_index: entry.index,
                scene: scene.to_string(),
                name: rec.name.clone(),
                man_off: rec.count_off,
                items,
                sellable,
            });
        }
    }

    if json {
        let rows: Vec<_> = shops
            .iter()
            .map(|s| {
                serde_json::json!({
                    "entry_index": s.entry_index,
                    "scene": s.scene,
                    "shop": s.name,
                    "man_offset": s.man_off,
                    "decoded_ids": s.items.len(),
                    "sellable": s.sellable,
                    "stock": s.items.iter().take(s.sellable).map(|(id, n, p)| {
                        serde_json::json!({ "id": id, "name": n, "price": p })
                    }).collect::<Vec<_>>(),
                    "unsellable_tail": s.items.iter().skip(s.sellable).map(|(id, n, p)| {
                        serde_json::json!({ "id": id, "name": n, "price": p })
                    }).collect::<Vec<_>>(),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }

    if shops.is_empty() {
        println!("no shops found (filters: scene={scene_filter:?} entry={entry_filter:?})");
        return Ok(());
    }
    println!(
        "{} shop(s) across {} PROT entries. Stock is inline in each scene's MAN \
         (field-VM op 0x49 sub-op 0); names + prices from the SCUS item table.",
        shops.len(),
        entries.len()
    );
    println!("Entry indices are EXTRACTION indices; scene labels are the retail");
    println!("CDNAME block (define number - 2).\n");
    for s in &shops {
        let scene = if s.scene.is_empty() { "?" } else { &s.scene };
        println!(
            "== {} (entry {:04}, MAN +{:#x}) - \"{}\"  decodes {} ids, sells {}",
            scene,
            s.entry_index,
            s.man_off,
            s.name,
            s.items.len(),
            s.sellable
        );
        for (i, (id, name, price)) in s.items.iter().enumerate() {
            let tail = if i >= s.sellable {
                "   <- unsellable tail (record padding, not stock)"
            } else {
                ""
            };
            println!("   {i:>2}. {id:#04x}  {name:<22} {price:>6}{tail}");
        }
        println!();
    }
    Ok(())
}
