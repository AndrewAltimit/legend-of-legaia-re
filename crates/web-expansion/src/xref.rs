//! The workbench's reference tools over the page's pack JSON: Find's id
//! index, rename with references, and the flow graph. Each is the format
//! crate's model ([`legaia_expansion_format::xref`],
//! [`legaia_expansion_format::flow`]), so what the page shows is what the
//! validator checks.

use anyhow::{Context, Result};
use legaia_expansion_format::ExpansionPack;
use legaia_expansion_format::flow;
use legaia_expansion_format::xref::{self, Kind, RenameError};
use serde_json::{Value, json};

/// Every site of every pack id: `{ kinds, sites: [{ kind, id, role, file,
/// field, key, owner }] }`, over the page's own JSON so the paths are the
/// page's.
pub fn sites(pack_json: &str) -> Result<Value> {
    let v: Value = serde_json::from_str(pack_json).context("pack does not parse")?;
    let ix = xref::index_value(&v);
    let kinds: Vec<&str> = Kind::ALL.iter().map(|k| k.as_str()).collect();
    Ok(json!({ "kinds": kinds, "sites": ix.sites }))
}

fn kind(name: &str) -> Result<Kind> {
    Kind::parse(name).with_context(|| format!("no id kind {name:?}"))
}

fn refused(e: RenameError) -> Value {
    let kind = match e {
        RenameError::BadId(_) => "bad_id",
        RenameError::Unknown(_) => "unknown",
        RenameError::Taken(_) => "taken",
    };
    json!({ "ok": false, "error": { "kind": kind, "message": e.to_string() } })
}

/// What renaming `kind` `old` to `new` would change: `{ ok, changes: [{
/// file, field, role, key, before, after }] }` or `{ ok: false, error }`.
pub fn rename_plan(pack_json: &str, kind_name: &str, old: &str, new: &str) -> Result<Value> {
    let v: Value = serde_json::from_str(pack_json).context("pack does not parse")?;
    Ok(match xref::rename_plan(&v, kind(kind_name)?, old, new) {
        Ok(changes) => json!({ "ok": true, "changes": changes }),
        Err(e) => refused(e),
    })
}

/// Rename `kind` `old` to `new` everywhere: `{ ok, pack, changes }`, the
/// pack being the page's JSON with every site rewritten (and still a pack:
/// a result that no longer parses is refused).
pub fn rename(pack_json: &str, kind_name: &str, old: &str, new: &str) -> Result<Value> {
    let mut v: Value = serde_json::from_str(pack_json).context("pack does not parse")?;
    let changes = match xref::rename(&mut v, kind(kind_name)?, old, new) {
        Ok(c) => c,
        Err(e) => return Ok(refused(e)),
    };
    serde_json::from_value::<ExpansionPack>(v.clone())
        .context("the renamed pack does not parse")?;
    Ok(json!({ "ok": true, "pack": v, "changes": changes }))
}

/// The flow graph with its lint findings ([`flow::graph`]).
pub fn graph(pack_json: &str) -> Result<Value> {
    let pack: ExpansionPack = serde_json::from_str(pack_json).context("pack does not parse")?;
    Ok(serde_json::to_value(flow::graph(&pack))?)
}
