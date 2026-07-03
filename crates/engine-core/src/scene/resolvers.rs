use super::*;

/// Resolver from a field-VM `scene_transition(map_id)` byte to a CDNAME
/// scene name. The retail engine reads this from a table in the field
/// overlay we haven't fully captured; engines wire their own table.
///
/// Implementors return `None` when the map id has no mapped scene
/// (the host then leaves the world in its current scene; the engine
/// can log the unknown id).
pub trait MapIdResolver {
    fn resolve(&self, map_id: u8) -> Option<String>;
}

/// Empty resolver - every `scene_transition` is a no-op. Useful for tests
/// + engines that haven't wired a real table yet.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullMapIdResolver;

impl MapIdResolver for NullMapIdResolver {
    fn resolve(&self, _: u8) -> Option<String> {
        None
    }
}

/// Plain `Vec<String>`-backed resolver - index into a list of scene names
/// by map id. Useful for hardcoded test fixtures.
#[derive(Debug, Clone, Default)]
pub struct VecMapIdResolver {
    pub names: Vec<String>,
}

impl VecMapIdResolver {
    pub fn new(names: Vec<String>) -> Self {
        Self { names }
    }
}

impl MapIdResolver for VecMapIdResolver {
    fn resolve(&self, map_id: u8) -> Option<String> {
        self.names.get(map_id as usize).cloned()
    }
}

/// CDNAME-derived map-id resolver. Builds the map-id → scene-name table
/// from the PROT archive's CDNAME index at startup, using ascending
/// PROT-entry-index order as the sequential map-id.
///
/// Map-id 0 maps to the first CDNAME block name (lowest PROT index),
/// map-id 1 to the second, and so on.
///
/// **Ordering note (from `FUN_8001f7c0` trace):** The field-VM WARP opcode
/// (`0x3E`, `op0 >= 100`) only supports map_ids 0–6. Each maps to a code
/// overlay at PROT `0x4d + map_id` (+ 2 for map_id >= 6); the scene name is
/// pre-set in `DAT_80084548` by a pre-WARP handler not yet fully traced.
/// The sequential CDNAME ordering here is an approximation; the exact
/// retail map_id → scene-name table lives in an uncaptured overlay.
/// See `docs/subsystems/asset-loader.md` → "WARP opcode → scene transition flow".
///
/// Suitable for use in [`BootSession::open`] as the default resolver.
#[derive(Debug, Clone, Default)]
pub struct DefaultMapIdResolver {
    inner: VecMapIdResolver,
}

impl DefaultMapIdResolver {
    /// Build from a `ProtIndex` - calls [`ProtIndex::cdname_scene_names`]
    /// and wraps the resulting ordered list.
    pub fn from_index(index: &ProtIndex) -> Self {
        Self {
            inner: VecMapIdResolver::new(index.cdname_scene_names()),
        }
    }

    /// Construct directly from a name list. Useful for tests that can't
    /// open a real ProtIndex.
    pub fn new(names: Vec<String>) -> Self {
        Self {
            inner: VecMapIdResolver::new(names),
        }
    }
}

impl MapIdResolver for DefaultMapIdResolver {
    fn resolve(&self, map_id: u8) -> Option<String> {
        self.inner.resolve(map_id)
    }
}

/// A resolver backed by a scene's **disc-sourced** scene-destination table
/// ([`crate::man_field_scripts::scene_destinations`]).
///
/// This resolves the **named scene-change** (`0x3F`) id space: each `0x3F` op
/// carries an `i16` index alongside the inline destination name, and a scene's
/// controller script lists every reachable destination as one such op.
/// [`SceneHost`] rebuilds one per scene from the entered scene's MAN, so the
/// engine has a live, byte-accurate index → scene-name map (no uncaptured
/// overlay needed).
///
/// **Not a [`MapIdResolver`].** That trait keys on a `u8` map id (the `0x3E`
/// door-warp's 7 scene-*type* selectors, `0..=6`). The `0x3F` index is a
/// distinct, wider id space - `i16`, observed past `u8` range (e.g. `630`) - so
/// a `u8`-keyed resolver can't represent it without lossy truncation. Hence the
/// dedicated [`Self::resolve`]/[`Self::destination`] by `i16`.
#[derive(Debug, Clone, Default)]
pub struct SceneDestinationResolver {
    by_index: std::collections::HashMap<i16, crate::man_field_scripts::SceneDestination>,
}

impl SceneDestinationResolver {
    /// Build from a decoded destination list (first entry per index wins, which
    /// matches [`scene_destinations`](crate::man_field_scripts::scene_destinations)'s
    /// first-seen dedup).
    pub fn new(destinations: Vec<crate::man_field_scripts::SceneDestination>) -> Self {
        let mut by_index = std::collections::HashMap::new();
        for d in destinations {
            by_index.entry(d.index).or_insert(d);
        }
        Self { by_index }
    }

    /// Resolve an `i16` scene-change index to its destination scene name.
    pub fn resolve(&self, index: i16) -> Option<&str> {
        self.by_index.get(&index).map(|d| d.scene_name.as_str())
    }

    /// The full destination record for an `i16` scene-change index (name +
    /// entry tile).
    pub fn destination(&self, index: i16) -> Option<&crate::man_field_scripts::SceneDestination> {
        self.by_index.get(&index)
    }

    /// Number of distinct destinations (indices) in the table.
    pub fn len(&self) -> usize {
        self.by_index.len()
    }

    /// `true` when the table carries no destinations.
    pub fn is_empty(&self) -> bool {
        self.by_index.is_empty()
    }
}
