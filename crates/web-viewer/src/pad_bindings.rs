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
//! The layout served here *starts* at `Mapping::web_default`, not
//! `Mapping::default`. The browser page binds `WASD` to the d-pad and the
//! desktop layout spends those keys on Triangle / Circle / R1, so the two
//! cannot be one table - a `HashMap<key, button>` holds either `S -> Down` or
//! `S -> Circle`, never both. They are two *named layouts in the engine*
//! rather than one layout plus a page-side override, which keeps the
//! single-source-of-truth property that made the tables above deletable.
//!
//! "Starts at", because the table is now **editable**: the options screen's
//! Key Config row ([`legaia_engine_core::key_rebind::KeyRebindSession`],
//! reached through [`legaia_engine_core::options::OptionsSession`]) commits
//! binds into it, and [`store_mapping`] round-trips the result through
//! `localStorage` under [`BINDINGS_STORAGE_KEY`] - the browser twin of the
//! native window's `legaia-input.toml`. A page that folded
//! [`pad_bindings_json`] into a JS object at load notices a rebind through
//! [`pad_bindings_revision`].
//!
//! A free function rather than a [`crate::runtime::LegaiaRuntime`] method
//! because the title screen's key loop runs before a runtime exists, and it
//! needs the same table.

use crate::runtime::LegaiaRuntime;
use legaia_engine_core::input::Mapping;
use std::cell::RefCell;
use wasm_bindgen::prelude::*;

/// `localStorage` key the page's edited binding table round-trips through -
/// the browser twin of the native window's `legaia-input.toml`.
///
/// Same store, same shape and the same "absent or unparseable falls back to
/// the default layout" rule as `legaia.options` beside it, so the two
/// persisted settings behave identically.
pub const BINDINGS_STORAGE_KEY: &str = "legaia.bindings";

thread_local! {
    /// The page's **live** binding table, plus a revision counter.
    ///
    /// A module-level cell rather than a [`LegaiaRuntime`] field because the
    /// title screen's key loop runs before a runtime exists and reads the same
    /// table (which is why [`pad_bindings_json`] is a free function), and
    /// because the minigames page has no `LegaiaRuntime` at all. wasm32 is
    /// single-threaded, so there is exactly one of these per page.
    ///
    /// The revision is what lets a page notice a rebind: a page folds this
    /// table into a JS object once at load, and nothing about
    /// [`pad_bindings_json`] returning something new would reach it otherwise.
    static LIVE_BINDINGS: RefCell<(Option<Mapping>, u32)> = const { RefCell::new((None, 0)) };
}

/// The live table, loading the persisted one (or the default layout) on first
/// use.
pub fn live_mapping() -> Mapping {
    LIVE_BINDINGS.with(|c| {
        let mut c = c.borrow_mut();
        if c.0.is_none() {
            c.0 = Some(load_persisted_mapping());
        }
        c.0.clone().expect("just seeded")
    })
}

/// Adopt `mapping` as the live table and persist it - the browser twin of the
/// native window writing `legaia-input.toml` when the options screen's Key
/// Config row commits a bind.
pub fn store_mapping(mapping: &Mapping) {
    LIVE_BINDINGS.with(|c| {
        let mut c = c.borrow_mut();
        c.0 = Some(mapping.clone());
        c.1 = c.1.wrapping_add(1);
    });
    #[cfg(target_arch = "wasm32")]
    if let Some(store) = bindings_storage()
        && let Ok(json) = serde_json::to_string(mapping)
        && store.set_item(BINDINGS_STORAGE_KEY, &json).is_err()
    {
        crate::console_log("bindings: localStorage write failed");
    }
}

/// Read the persisted table, falling back to [`Mapping::web_default`] when
/// nothing is stored, the store is unreachable (private mode, non-browser
/// target) or the stored JSON no longer parses.
fn load_persisted_mapping() -> Mapping {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(store) = bindings_storage()
            && let Ok(Some(raw)) = store.get_item(BINDINGS_STORAGE_KEY)
            && let Ok(m) = serde_json::from_str::<Mapping>(&raw)
            && !m.bindings.is_empty()
        {
            return m;
        }
    }
    Mapping::web_default()
}

#[cfg(target_arch = "wasm32")]
fn bindings_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

/// How many times the live table has changed since the page loaded. A page
/// polls this and re-reads [`pad_bindings_json`] when it moves.
#[wasm_bindgen]
pub fn pad_bindings_revision() -> u32 {
    LIVE_BINDINGS.with(|c| c.borrow().1)
}

/// Forget the persisted table and go back to [`Mapping::web_default`] - the
/// escape hatch for a player who bound themselves out of the menu.
#[wasm_bindgen]
pub fn pad_bindings_reset() {
    let d = Mapping::web_default();
    store_mapping(&d);
    #[cfg(target_arch = "wasm32")]
    if let Some(store) = bindings_storage() {
        let _ = store.remove_item(BINDINGS_STORAGE_KEY);
    }
}

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
    let mapping = live_mapping();
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

    /// See [`pad_bindings_revision`].
    ///
    /// A forwarder for the same reason the two above are, and it earns its
    /// keep: the page's `legaiaSyncPadBindings(src)` is handed a
    /// `LegaiaRuntime`, and a free function on the module namespace is not
    /// reachable through it. Without this the sync call answered
    /// `typeof !== 'function'` and silently did nothing, so a rebind stayed
    /// invisible to the running page - which is exactly the shape a browser
    /// check catches and a green cargo test cannot.
    pub fn pad_bindings_revision(&self) -> u32 {
        pad_bindings_revision()
    }

    /// See [`pad_bindings_reset`].
    pub fn pad_bindings_reset(&self) {
        pad_bindings_reset();
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
