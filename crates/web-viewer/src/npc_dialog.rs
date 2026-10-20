//! The NPC page's **conversation tree**: an NPC's
//! [`DialogTree`](legaia_engine_field::man_field_scripts::DialogTree) as JSON,
//! its lines decoded to display text.
//!
//! The walk itself is the field kernel's
//! (`legaia_engine_field::man_field_scripts::placement_dialog_tree`); this
//! module only renders it. Text is decoded here, in the browser, from the
//! user's own disc - nothing of it is shipped.
//!
//! Glyph bytes `0x20..=0x7E` are ASCII. The two-byte escapes become what the
//! game would print where the page can know it: `0xC1 n` the party member's
//! default name (`99` = the party leader), `0xC2` / `0xC4` the item name and
//! `0xC3` the spell name from the disc's own `SCUS_942.54` tables; the rest
//! (sprite / number escapes, colour changes, wide glyphs) become a bracketed
//! placeholder or nothing.

use legaia_engine_field::man_field_scripts::{
    Condition, DialogAction, DialogNode, DialogTree, EndReason, PageStyle,
};
use legaia_game_tables::item_names::ItemNameTable;
use legaia_game_tables::spell_names::SpellNameTable;
use serde_json::{Value, json};

/// The disc tables a line's escapes resolve against.
#[derive(Default, Clone, Copy)]
pub struct NameTables<'a> {
    pub items: Option<&'a ItemNameTable>,
    pub spells: Option<&'a SpellNameTable>,
}

/// Default party names for `0xC1 n` (the new-game names; a save can rename
/// them, and the page has no save).
const PARTY: [&str; 3] = ["Vahn", "Noa", "Gala"];

impl NameTables<'_> {
    fn item(&self, id: u8) -> String {
        self.items
            .and_then(|t| t.name(id))
            .map(str::to_string)
            .unwrap_or_else(|| format!("item 0x{id:02X}"))
    }

    fn spell(&self, id: u8) -> String {
        self.spells
            .and_then(|t| t.name(id))
            .map(str::to_string)
            .unwrap_or_else(|| format!("spell 0x{id:02X}"))
    }
}

fn party_name(n: u8) -> String {
    match n {
        0..=2 => PARTY[n as usize].to_string(),
        99 => "Vahn".to_string(),
        _ => format!("[member {n}]"),
    }
}

/// Decode one line's glyph bytes to display text.
pub fn line_text(bytes: &[u8], names: NameTables<'_>) -> String {
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let arg = bytes.get(i + 1).copied();
        if (b & 0xF0 == 0xC0 || b == 0x5E || b == 0xFF) && arg.is_some() {
            let a = arg.unwrap_or(0);
            match b {
                0xC1 => out.push_str(&party_name(a)),
                0xC2 | 0xC4 => out.push_str(&names.item(a)),
                0xC3 => out.push_str(&names.spell(a)),
                0xC5 => out.push_str("[spell]"),
                0xC7 => out.push_str("[place]"),
                0xCE => out.push_str("[icon]"),
                0xCF => {} // colour change
                0x5E => out.push(' '),
                _ => out.push('\u{00B7}'), // a wide glyph the page has no font for
            }
            i += 2;
            continue;
        }
        match b {
            0x7C => out.push(' '), // the in-line newline glyph
            0x20..=0x7E => out.push(b as char),
            _ => out.push('\u{00B7}'),
        }
        i += 1;
    }
    out
}

fn cond_text(c: &Condition, names: NameTables<'_>) -> String {
    match c {
        Condition::HoldsItem { item } => format!("the party holds {}", names.item(*item)),
        Condition::WearsItem { item } => format!("someone wears {}", names.item(*item)),
        Condition::KnowsSpell { spell } => {
            format!("a party member knows {}", names.spell(*spell))
        }
        Condition::LearnedArt { ch, art } => {
            format!("{} has learned art {art}", party_name(*ch))
        }
        other => other.describe(),
    }
}

fn action_json(a: &DialogAction, names: NameTables<'_>) -> (&'static str, String) {
    match a {
        DialogAction::GiveItem { item } => ("item", format!("Receive {}", names.item(*item))),
        DialogAction::TakeItem { item } => ("item", format!("Hand over {}", names.item(*item))),
        DialogAction::Money { delta } => (
            "money",
            if *delta >= 0 {
                format!("Receive {delta} gold")
            } else {
                format!("Pay {} gold", -delta)
            },
        ),
        DialogAction::PartyJoin { ch } => ("party", format!("{} joins the party", party_name(*ch))),
        DialogAction::PartyLeave { ch } => {
            ("party", format!("{} leaves the party", party_name(*ch)))
        }
        DialogAction::Equip { selector, item } => (
            "item",
            format!(
                "Equip {} on {} (slot {})",
                names.item(*item),
                party_name(selector >> 4),
                selector & 0xF
            ),
        ),
        DialogAction::SetFlag { flag } => ("flag", format!("Sets story flag 0x{flag:03X}")),
        DialogAction::ClearFlag { flag } => ("flag", format!("Clears story flag 0x{flag:03X}")),
        DialogAction::SceneChange { name } => (
            "scene",
            match name {
                Some(n) => format!("Moves to scene {n}"),
                None => "Moves to another scene".to_string(),
            },
        ),
        DialogAction::Minigame { sub_id } => ("scene", format!("Starts minigame {sub_id}")),
        DialogAction::Battle { op0, row } => (
            "battle",
            format!("Starts a battle (formation row {row}, op 0x{op0:02X})"),
        ),
        DialogAction::Menu { sub_op } => (
            "menu",
            match sub_op {
                0 => "Opens the shop".to_string(),
                1 => "Opens the save menu".to_string(),
                7 => "Opens the prize exchange".to_string(),
                n => format!("Opens menu screen {n}"),
            },
        ),
        DialogAction::Movie { fmv_id } => ("movie", format!("Plays movie {fmv_id}")),
        DialogAction::Spawn { record } => ("event", format!("Starts event record {record}")),
    }
}

fn end_json(r: &EndReason) -> (&'static str, String) {
    match r {
        EndReason::Close => ("close", "End of conversation".to_string()),
        EndReason::Parked => ("parked", "The event waits here".to_string()),
        EndReason::RecordEnd => ("record_end", "End of script".to_string()),
        EndReason::LeavesRecord { .. } => {
            ("leaves", "Jumps outside this actor's script".to_string())
        }
        EndReason::Undecoded { opcode, sub_op } => (
            "undecoded",
            match sub_op {
                Some(s) => format!("Unresolved op 0x{opcode:02X} sub 0x{s:02X}"),
                None => format!("Unresolved op 0x{opcode:02X}"),
            },
        ),
        EndReason::BadPicker => ("undecoded", "A choice that does not decode".to_string()),
        EndReason::Budget => ("budget", "Tree truncated".to_string()),
    }
}

fn nodes_json(nodes: &[DialogNode], names: NameTables<'_>) -> Vec<Value> {
    nodes.iter().map(|n| node_json(n, names)).collect()
}

fn node_json(n: &DialogNode, names: NameTables<'_>) -> Value {
    match n {
        DialogNode::Page {
            id, style, lines, ..
        } => json!({
            "id": id,
            "t": "page",
            "style": match style { PageStyle::Box => "box", PageStyle::Balloon => "balloon" },
            "lines": lines.iter().map(|l| line_text(&l.bytes, names)).collect::<Vec<_>>(),
        }),
        DialogNode::Choice { id, options, .. } => json!({
            "id": id,
            "t": "choice",
            "options": options.iter().map(|o| json!({
                "label": line_text(&o.label.bytes, names),
                "then": nodes_json(&o.then, names),
            })).collect::<Vec<_>>(),
        }),
        DialogNode::Branch {
            id,
            cond,
            actor,
            taken,
            otherwise,
            ..
        } => json!({
            "id": id,
            "t": "branch",
            "cond": cond_text(cond, names),
            "actor": actor,
            "taken": nodes_json(taken, names),
            "else": nodes_json(otherwise, names),
        }),
        DialogNode::Action { id, action, .. } => {
            let (kind, text) = action_json(action, names);
            json!({ "id": id, "t": "action", "kind": kind, "text": text })
        }
        DialogNode::Event {
            id, record, body, ..
        } => json!({
            "id": id,
            "t": "event",
            "record": record,
            "body": nodes_json(body, names),
        }),
        DialogNode::NextTalk { id, then, .. } => json!({
            "id": id,
            "t": "next",
            "then": nodes_json(then, names),
        }),
        DialogNode::Goto { id, to, .. } => json!({ "id": id, "t": "goto", "to": to }),
        DialogNode::End { id, reason, .. } => {
            let (why, text) = end_json(reason);
            json!({ "id": id, "t": "end", "why": why, "text": text })
        }
    }
}

/// The whole tree as the page's JSON. Shape:
/// ```text
/// { "record": 16,          // partition-1 record the tree was walked from
///   "via_proxy": false,    // true: this actor hands its talk to `record`
///   "node_count": 18, "page_count": 9,
///   "nodes": [ node, ... ] }
/// node = { "id", "t": "page",   "style": "box"|"balloon", "lines": [str] }
///      | { "id", "t": "choice", "options": [{ "label": str, "then": [node] }] }
///      | { "id", "t": "branch", "cond": str, "actor": n|null,
///          "taken": [node], "else": [node] }
///      | { "id", "t": "action", "kind": str, "text": str }
///      | { "id", "t": "event",  "record": n, "body": [node] }
///      | { "id", "t": "next",   "then": [node] }   // the next talk
///      | { "id", "t": "goto",   "to": id|null }    // joins node `to`
///      | { "id", "t": "end",    "why": str, "text": str }
/// ```
pub fn tree_json(tree: &DialogTree, names: NameTables<'_>) -> Value {
    json!({
        "record": tree.record_placement,
        "via_proxy": tree.via_proxy,
        "node_count": tree.node_count,
        "page_count": tree.page_count(),
        "nodes": nodes_json(&tree.nodes, names),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_render_as_names_or_placeholders() {
        let t = NameTables::default();
        assert_eq!(line_text(&[0xC1, 0x00, b'!'], t), "Vahn!");
        assert_eq!(line_text(&[0xC1, 0x02], t), "Gala");
        assert_eq!(line_text(&[0xC2, 0x10], t), "item 0x10");
        assert_eq!(line_text(&[b'a', 0xCF, 0x03, b'b'], t), "ab");
        assert_eq!(line_text(&[b'a', 0x7C, b'b'], t), "a b");
        assert_eq!(line_text(&[0xCE, 0x05], t), "[icon]");
    }
}
