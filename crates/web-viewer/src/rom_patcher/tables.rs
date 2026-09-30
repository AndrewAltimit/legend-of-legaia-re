//! Read-only table exports for the page's editors (equipment, manual edits).
//! Split out of `rom_patcher.rs`.

use super::*;

/// Read every equippable item off the user's disc for the ROM-patcher page's
/// equipment editor: `{ items: [{ id, name, slot, row, shares_row_with: [id],
/// mask, atk, costs: [cost|null x3] }] }`. `costs` is the Arts-gauge swing
/// cost each character's player file carries for a weapon (`null` = that file
/// has no section for it); `mask` is the equip-owner bits (1 Vahn, 2 Noa,
/// 4 Gala). Decoded in this tab from the supplied image; nothing is uploaded.
#[wasm_bindgen]
pub fn read_equipment_table(image: Vec<u8>) -> Result<JsValue, JsValue> {
    let patcher = DiscPatcher::open(image).map_err(|e| err(format!("open disc image: {e}")))?;
    let table = apply::read_equipment_table(&patcher)
        .map_err(|e| err(format!("equipment table: {e}")))?
        .ok_or_else(|| err("equipment table not found in SCUS_942.54"))?;
    drop(patcher);
    let num = JsValue::from_f64;
    let out = Object::new();
    let opt = |c: Option<u8>| c.map(|v| num(v as f64)).unwrap_or(JsValue::NULL);
    let defaults = js_sys::Array::new();
    for d in table.defaults {
        let o = Object::new();
        Reflect::set(&o, &"weapon".into(), &opt(d.weapon))?;
        Reflect::set(&o, &"raseru".into(), &opt(d.ra_seru))?;
        Reflect::set(&o, &"down".into(), &opt(d.down))?;
        Reflect::set(&o, &"up".into(), &opt(d.up))?;
        defaults.push(&o.into());
    }
    Reflect::set(&out, &"defaults".into(), &defaults)?;
    let hands = js_sys::Array::new();
    for h in table.weapon_hand {
        hands.push(&h.map(JsValue::from).unwrap_or(JsValue::NULL));
    }
    Reflect::set(&out, &"weapon_hand".into(), &hands)?;
    let items = js_sys::Array::new();
    for r in &table.rows {
        let o = Object::new();
        Reflect::set(&o, &"id".into(), &num(r.id as f64))?;
        Reflect::set(&o, &"name".into(), &r.name.as_str().into())?;
        Reflect::set(&o, &"slot".into(), &r.slot.into())?;
        Reflect::set(&o, &"row".into(), &num(r.row as f64))?;
        let sib = js_sys::Array::new();
        for &s in &r.shares_row_with {
            sib.push(&num(s as f64));
        }
        Reflect::set(&o, &"shares_row_with".into(), &sib)?;
        Reflect::set(&o, &"mask".into(), &num(r.mask as f64))?;
        Reflect::set(&o, &"atk".into(), &num(r.atk as f64))?;
        Reflect::set(&o, &"ra_seru_arm".into(), &r.ra_seru_arm.into())?;
        let costs = js_sys::Array::new();
        for c in r.costs {
            costs.push(&opt(c));
        }
        Reflect::set(&o, &"costs".into(), &costs)?;
        let ups = js_sys::Array::new();
        for c in r.up_costs {
            ups.push(&opt(c));
        }
        Reflect::set(&o, &"up_costs".into(), &ups)?;
        let cmds = js_sys::Array::new();
        for c in r.cmds {
            cmds.push(&c.map(JsValue::from).unwrap_or(JsValue::NULL));
        }
        Reflect::set(&o, &"cmds".into(), &cmds)?;
        items.push(&o.into());
    }
    Reflect::set(&out, &"items".into(), &items)?;
    Ok(out.into())
}

/// Read the disc-resident tables behind the ROM-patcher page's structured
/// value editors, so those controls can show the disc's own current values
/// instead of asking the user to type raw ids: the 16 world-map location-name
/// slots (the SCUS table [`legaia_patcher::location_name`] renames in place)
/// and the fishing point-exchange prize rows (PROT 972, the table
/// [`legaia_patcher::fishing_price`] reprices), each prize's item id resolved
/// to its display name through the SCUS item-name table.
///
/// Returns `{ max_name_len, locations: [name; 16], fishing: [{ page, row,
/// item, name, price, one_time }] }`. Everything is decoded from the image the
/// user supplied, in this call, in this tab - the site ships no game text and
/// nothing is uploaded. The patcher itself can only *reprice* a fishing prize
/// (the 12 rows and their item ids are fixed on the disc) and only *rename* a
/// location slot, which is exactly the shape this listing exposes.
#[wasm_bindgen]
pub fn read_manual_edit_tables(image: Vec<u8>) -> Result<JsValue, JsValue> {
    let scus = legaia_iso::iso9660::read_file_in_image(&image, "SCUS_942.54")
        .ok_or_else(|| err("SCUS_942.54 not found in disc image"))?;
    let locations = legaia_patcher::location_name::list_names(&scus)
        .map_err(|e| err(format!("location-name table: {e}")))?;
    let item_names = legaia_asset::item_names::ItemNameTable::from_scus(&scus);
    drop(scus);
    let patcher = DiscPatcher::open(image).map_err(|e| err(format!("open disc image: {e}")))?;
    let overlay = patcher
        .read_entry(legaia_patcher::fishing_price::OVERLAY_PROT_INDEX)
        .map_err(|e| err(format!("read fishing overlay: {e}")))?;
    let prizes = legaia_patcher::fishing_price::list_prizes(&overlay)
        .map_err(|e| err(format!("fishing prize table: {e}")))?;
    // The world-map label table names 14 places the 16 quick-travel cells have
    // no room for; those are renamed by name, not by cell index.
    let world_map_only: Vec<String> = legaia_patcher::apply::list_world_map_labels(&patcher)
        .into_iter()
        .map(|(_, _, _, name)| name)
        .filter(|name| !locations.iter().any(|(_, cell)| cell == name))
        .collect();
    drop(patcher);

    let num = JsValue::from_f64;
    let out = Object::new();
    Reflect::set(
        &out,
        &"max_name_len".into(),
        &num(legaia_patcher::location_name::MAX_NAME_LEN as f64),
    )?;
    let locs = js_sys::Array::new();
    for (_idx, name) in &locations {
        locs.push(&JsValue::from_str(name));
    }
    Reflect::set(&out, &"locations".into(), &locs)?;
    // The 14 places that have a world-map label + an entry banner but no
    // quick-travel cell ("Hunter's Spring", "Sol Tower", ...). They are keyed
    // by their current name, so the editor sends `Old=New` rather than
    // `index=New`.
    let extra = js_sys::Array::new();
    for name in world_map_only {
        extra.push(&JsValue::from_str(&name));
    }
    Reflect::set(&out, &"world_map_only".into(), &extra)?;
    let fish = js_sys::Array::new();
    for p in &prizes {
        // All-zero rows are structural padding in the 6-row page, not prizes.
        if p.item_id == 0 && p.price == 0 {
            continue;
        }
        let o = Object::new();
        Reflect::set(&o, &"page".into(), &num(p.page as f64))?;
        Reflect::set(&o, &"row".into(), &num(p.row as f64))?;
        Reflect::set(&o, &"item".into(), &num(p.item_id as f64))?;
        let name = u8::try_from(p.item_id)
            .ok()
            .and_then(|id| item_names.as_ref().and_then(|t| t.name(id)))
            .unwrap_or("");
        Reflect::set(&o, &"name".into(), &name.into())?;
        Reflect::set(&o, &"price".into(), &num(p.price as f64))?;
        Reflect::set(&o, &"one_time".into(), &JsValue::from_bool(p.one_time))?;
        fish.push(&o.into());
    }
    Reflect::set(&out, &"fishing".into(), &fish)?;
    Ok(out.into())
}
