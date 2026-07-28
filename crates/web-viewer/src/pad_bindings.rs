//! The keyboard-to-pad binding table, read from the engine instead of typed
//! into the page.
//!
//! Every browser page that takes input needs a `KeyboardEvent.code -> PSX pad
//! bit` table. There used to be two of them, hand-written in JS, and neither
//! agreed with the engine's own default layout
//! (`legaia_engine_core::input::Mapping::default`) or with each other:
//!
//! | key | native window | play page | boot / title |
//! |---|---|---|---|
//! | `X` | Square | Circle | Circle |
//! | `S` | Circle | Down | Down |
//! | `A` | Triangle | Left | - |
//! | `W` | R1 | Up | Up |
//! | `RShift` | Select | - | - |
//! | `Q` / `1` / `2` | L1 / L2 / R2 | - | - |
//!
//! Nothing about that is visible in a diff, because no file holds two of the
//! columns. The fix is not to correct the tables - it is to delete them: the
//! pages call [`pad_bindings_json`] and use whatever the engine binds, so a
//! rebind lands on every host at once and a disagreement can no longer be
//! written down.
//!
//! The layout served here is `Mapping::web_default`, not `Mapping::default`.
//! The browser page binds `WASD` to the d-pad and the desktop layout spends
//! those keys on Triangle / Circle / R1, so the two cannot be one table - a
//! `HashMap<key, button>` holds either `S -> Down` or `S -> Circle`, never
//! both. They are two *named layouts in the engine* rather than one layout
//! plus a page-side override, which keeps the single-source-of-truth property
//! that made the tables above deletable.
//!
//! A free function rather than a [`crate::runtime::LegaiaRuntime`] method
//! because the title screen's key loop runs before a runtime exists, and it
//! needs the same table.

use crate::runtime::LegaiaRuntime;
use wasm_bindgen::prelude::*;

/// The engine's default keyboard layout as `{ "<KeyboardEvent.code>": <bit> }`.
///
/// Bits are the PSX digital-pad masks
/// (`legaia_engine_core::input::PadButton`) the runtime's `set_pad` /
/// `*_input` entry points already take, so a page folds held or pressed codes
/// straight into a pad word with no second table.
///
/// Keys the browser has no `code` for are absent rather than guessed at - see
/// `Mapping::dom_code_bindings`. Ordering is stable across calls.
#[wasm_bindgen]
pub fn pad_bindings_json() -> String {
    let mapping = legaia_engine_core::input::Mapping::web_default();
    let mut out = String::from("{");
    for (i, (code, bit)) in mapping.dom_code_bindings().into_iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!("\"{code}\":{bit}"));
    }
    out.push('}');
    out
}

/// Pad bits by button name, for the handful of places a page needs to name a
/// button rather than a key ("is confirm pressed?").
///
/// The title screen used to test `pulse.has('KeyZ') || pulse.has('Space')`,
/// which hardcodes both the layout and the assumption that confirm is a key
/// rather than a button. With this a page asks for `Cross` and stays correct
/// through a rebind.
#[wasm_bindgen]
pub fn pad_buttons_json() -> String {
    use legaia_engine_core::input::PadButton::*;
    let all = [
        Select, L3, R3, Start, Up, Right, Down, Left, L2, R2, L1, R1, Triangle, Circle, Cross,
        Square,
    ];
    let body = all
        .iter()
        .map(|b| format!("\"{}\":{}", b.name(), b.mask()))
        .collect::<Vec<_>>()
        .join(",");
    format!("{{{body}}}")
}

/// Runtime-method forwarders.
///
/// The free functions above are the real definitions; these exist because a
/// page that already holds a `LegaiaRuntime` should not have to reach for the
/// module namespace as well, and because `typeof rt.pad_bindings_json ===
/// 'function'` is the guard the pages already use to tell a fresh bundle from
/// a cached one.
#[wasm_bindgen]
impl LegaiaRuntime {
    /// See [`pad_bindings_json`].
    pub fn pad_bindings_json(&self) -> String {
        pad_bindings_json()
    }

    /// See [`pad_buttons_json`].
    pub fn pad_buttons_json(&self) -> String {
        pad_buttons_json()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exported table must be parseable JSON carrying the whole default
    /// layout - a page that gets `{}` falls back to no input at all, silently.
    #[test]
    fn the_exported_table_is_json_and_carries_every_default_binding() {
        let json = pad_bindings_json();
        let v: serde_json::Value = serde_json::from_str(&json).expect("valid JSON object");
        let obj = v.as_object().expect("object");
        let want = legaia_engine_core::input::Mapping::web_default().dom_code_bindings();
        assert_eq!(obj.len(), want.len());
        for (code, bit) in want {
            assert_eq!(obj.get(code).and_then(|b| b.as_u64()), Some(u64::from(bit)));
        }
    }

    #[test]
    fn the_button_table_names_all_sixteen_pad_bits() {
        let v: serde_json::Value = serde_json::from_str(&pad_buttons_json()).unwrap();
        let obj = v.as_object().unwrap();
        assert_eq!(obj.len(), 16);
        assert_eq!(obj.get("Cross").and_then(|b| b.as_u64()), Some(0x4000));
        assert_eq!(obj.get("Square").and_then(|b| b.as_u64()), Some(0x8000));
        assert_eq!(obj.get("Select").and_then(|b| b.as_u64()), Some(0x0001));
    }
}
