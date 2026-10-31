//! Content-keyed memos of the per-mesh derivations a scene build repeats.
//!
//! A field scene's static draws are built into several products besides the
//! drawn meshes, each derived mesh by mesh from the scene's resources: the
//! occlusion fade's occluder triangles ([`crate::field_occlusion`]), the fog
//! bank's surfaces ([`crate::fog_ground`]) and the coplanar pass's plane
//! summaries and lifts ([`crate::coplanar_draws`]). Each is a pure function
//! of the mesh's bytes and - for the texel-gated ones - of the VRAM it
//! samples. A scene built again from mostly the same meshes (a pack map
//! rebuilt after an edit in the expansion workbench's 3D preview, a map
//! re-entered) would derive them all again; a [`MeshMemo`] keeps the
//! products of the last two builds by content instead, so an unchanged mesh
//! is derived once. A hit hands back the value a fresh derivation makes.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

/// A content key: two independent 64-bit hashes of the bytes and their
/// length. Not a cryptographic digest - the inputs are the engine's own
/// meshes, not an adversary's - but two lanes over 128 bits leave a chance
/// collision out of reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ContentKey(u64, u64, usize);

const K1: u64 = 0x51_7c_c1_b7_27_22_0a_95;
const K2: u64 = 0x9e_37_79_b9_7f_4a_7c_15;

impl ContentKey {
    /// The key of `parts` read as one byte string.
    pub fn of(parts: &[&[u8]]) -> Self {
        let (mut a, mut b, mut n) = (0u64, 0x2545_f491_4f6c_dd1du64, 0usize);
        for p in parts {
            let (words, tail) = p.as_chunks::<8>();
            for w in words {
                let w = u64::from_le_bytes(*w);
                a = (a.rotate_left(5) ^ w).wrapping_mul(K1);
                b = (b ^ w).wrapping_mul(K2).rotate_left(29);
            }
            for &t in tail {
                a = (a.rotate_left(5) ^ u64::from(t)).wrapping_mul(K1);
                b = (b ^ u64::from(t)).wrapping_mul(K2).rotate_left(29);
            }
            // The part boundary, so `["ab", "c"]` and `["a", "bc"]` differ.
            a = (a.rotate_left(5) ^ p.len() as u64).wrapping_mul(K1);
            n += p.len();
        }
        Self(a, b, n)
    }

    /// The key of a VRAM image: its pixels and its written-cell map.
    pub fn of_vram(v: &legaia_tim::Vram) -> Self {
        Self::of(&[v.as_bytes(), v.written_bytes()])
    }

    /// The key of a TMD's bytes.
    pub fn of_bytes(raw: &[u8]) -> Self {
        Self::of(&[raw])
    }

    /// The key as 48 hex digits (a JS map key).
    pub fn to_hex(&self) -> String {
        format!("{:016x}{:016x}{:016x}", self.0, self.1, self.2 as u64)
    }

    /// The key of two keys together (a mesh and the VRAM it samples).
    pub fn pair(a: Self, b: Self) -> Self {
        let mut w = [0u8; 48];
        for (i, v) in [a.0, a.1, a.2 as u64, b.0, b.1, b.2 as u64]
            .into_iter()
            .enumerate()
        {
            w[i * 8..i * 8 + 8].copy_from_slice(&v.to_le_bytes());
        }
        Self::of(&[&w])
    }
}

struct Entry<V> {
    value: Arc<V>,
    /// The build that last used it.
    build: u64,
    bytes: usize,
}

struct Inner<V> {
    map: HashMap<ContentKey, Entry<V>>,
    build: u64,
    bytes: usize,
}

/// Products keyed by [`ContentKey`], kept for the build that made or used
/// them and the one after. Past `budget` bytes the entries of the older
/// build go first, then nothing new is kept until the next build.
pub struct MeshMemo<V> {
    inner: Mutex<Option<Inner<V>>>,
    budget: usize,
}

impl<V> MeshMemo<V> {
    /// An empty memo holding at most about `budget` bytes.
    pub const fn new(budget: usize) -> Self {
        Self {
            inner: Mutex::new(None),
            budget,
        }
    }

    fn with<R>(&self, f: impl FnOnce(&mut Inner<V>) -> R) -> R {
        let mut g = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let inner = g.get_or_insert_with(|| Inner {
            map: HashMap::new(),
            build: 0,
            bytes: 0,
        });
        f(inner)
    }

    /// Start a build: what neither this build nor the last one used goes.
    pub fn begin_build(&self) {
        self.with(|m| {
            m.build += 1;
            let keep = m.build - 1;
            m.map.retain(|_, e| e.build >= keep);
            m.bytes = m.map.values().map(|e| e.bytes).sum();
        });
    }

    /// The product for `key`, if kept (marked as used by this build).
    pub fn get(&self, key: &ContentKey) -> Option<Arc<V>> {
        self.with(|m| {
            let build = m.build;
            m.map.get_mut(key).map(|e| {
                e.build = build;
                e.value.clone()
            })
        })
    }

    /// Keep `value` (about `bytes` bytes) for `key`.
    pub fn put(&self, key: ContentKey, value: Arc<V>, bytes: usize) {
        self.with(|m| {
            if m.bytes + bytes > self.budget {
                let build = m.build;
                m.map.retain(|_, e| e.build == build);
                m.bytes = m.map.values().map(|e| e.bytes).sum();
            }
            if m.bytes + bytes > self.budget {
                return;
            }
            let build = m.build;
            if let Some(old) = m.map.insert(
                key,
                Entry {
                    value,
                    build,
                    bytes,
                },
            ) {
                m.bytes -= old.bytes;
            }
            m.bytes += bytes;
        });
    }

    /// The product for `key`: kept, or made by `make` (and kept).
    pub fn get_or(&self, key: ContentKey, make: impl FnOnce() -> (V, usize)) -> Arc<V> {
        if let Some(v) = self.get(&key) {
            return v;
        }
        let (v, bytes) = make();
        let v = Arc::new(v);
        self.put(key, v.clone(), bytes);
        v
    }

    /// How many products are kept.
    pub fn len(&self) -> usize {
        self.with(|m| m.map.len())
    }

    /// Whether nothing is kept.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_tell_contents_apart() {
        let a = ContentKey::of_bytes(b"0123456789abcdef");
        assert_eq!(a, ContentKey::of_bytes(b"0123456789abcdef"));
        assert_ne!(a, ContentKey::of_bytes(b"0123456789abcdeg"));
        assert_ne!(a, ContentKey::of_bytes(b"0123456789abcde"));
        assert_ne!(
            ContentKey::of(&[b"ab", b"c"]),
            ContentKey::of(&[b"a", b"bc"])
        );
        let v = legaia_tim::Vram::new();
        let mut w = legaia_tim::Vram::new();
        assert_eq!(ContentKey::of_vram(&v), ContentKey::of_vram(&w));
        // A written zero is content: the written map changes, the pixels not.
        w.write_block(0, 0, 1, 1, &[0, 0]);
        assert_ne!(ContentKey::of_vram(&v), ContentKey::of_vram(&w));
    }

    #[test]
    fn a_memo_keeps_two_builds_within_its_budget() {
        let m: MeshMemo<u32> = MeshMemo::new(100);
        let k = |n: u8| ContentKey::of_bytes(&[n]);
        m.begin_build();
        m.put(k(1), Arc::new(1), 10);
        m.begin_build();
        assert_eq!(m.get(&k(1)).as_deref(), Some(&1));
        m.put(k(2), Arc::new(2), 10);
        m.begin_build();
        m.begin_build();
        // Build 3 used neither; build 4 keeps what build 3 used.
        assert!(m.get(&k(1)).is_none());
        assert!(m.get(&k(2)).is_none());
        // Over budget: the older build's products go, then nothing is kept.
        m.put(k(3), Arc::new(3), 60);
        m.begin_build();
        m.put(k(4), Arc::new(4), 60);
        assert!(m.get(&k(3)).is_none());
        assert_eq!(m.get(&k(4)).as_deref(), Some(&4));
        m.put(k(5), Arc::new(5), 60);
        assert!(m.get(&k(5)).is_none());
        let made = m.get_or(k(6), || (6, 1));
        assert_eq!(*made, 6);
    }
}
