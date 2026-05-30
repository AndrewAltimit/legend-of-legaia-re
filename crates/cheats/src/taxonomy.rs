//! Write-surface taxonomy: bucket a set of changed RAM addresses by the
//! [`Category`](crate::classify::Category) region they fall in.
//!
//! The companion to [`classify_address`](crate::classify::classify_address):
//! given the addresses that changed between two points in time (e.g. the
//! per-byte deltas from a pair of save states via
//! `legaia_mednafen::diff_ram`), this rolls them up into per-region counts +
//! a few sample classifications each. It is the classification half of a
//! gameplay-driven write tracer — feed it a capture diff and it answers
//! "*what* changed, bucketed by subsystem", surfacing writes that land in
//! unexpected regions (the `Unknown` bucket, or the `0x8007Bxxx` script-VM /
//! build-flag scratch) as candidates worth a closer look.
//!
//! Pure and capture-free: the input is just a list of `u32` addresses, so the
//! roll-up is unit-tested with synthetic deltas and needs no save state.

use crate::classify::{Category, ClassifiedAddress, classify_address};

/// Default number of sample [`ClassifiedAddress`]es retained per bucket.
pub const DEFAULT_SAMPLES: usize = 8;

/// One region bucket of a [`WriteTaxonomy`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaxonomyBucket {
    /// The region these addresses classified into.
    pub category: Category,
    /// How many distinct addresses fell in this bucket.
    pub count: usize,
    /// Up to [`DEFAULT_SAMPLES`] representative classifications (lowest
    /// addresses first), for a human-readable summary.
    pub samples: Vec<ClassifiedAddress>,
}

/// Per-region roll-up of a set of changed addresses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteTaxonomy {
    /// Total distinct addresses classified.
    pub total: usize,
    /// Buckets in a stable order (by [`Category`] discriminant), most-changed
    /// regions are found via [`Self::dominant`].
    pub buckets: Vec<TaxonomyBucket>,
}

impl WriteTaxonomy {
    /// The bucket for `category`, if any addresses fell in it.
    pub fn bucket(&self, category: Category) -> Option<&TaxonomyBucket> {
        self.buckets.iter().find(|b| b.category == category)
    }

    /// Count for `category` (`0` if absent).
    pub fn count(&self, category: Category) -> usize {
        self.bucket(category).map_or(0, |b| b.count)
    }

    /// Buckets that are *interesting* for an attack-surface scan: writes that
    /// landed outside every known data region ([`Category::Unknown`]) or in
    /// the script-VM / build-flag scratch band ([`Category::ScriptVmGlobal`],
    /// which contains the `0x8007B8C2` build-mode selector and the debug-menu
    /// enable). A change showing up here is the signal a write tracer is for.
    pub fn interesting(&self) -> impl Iterator<Item = &TaxonomyBucket> {
        self.buckets
            .iter()
            .filter(|b| matches!(b.category, Category::Unknown | Category::ScriptVmGlobal))
    }

    /// The bucket with the most changed addresses, if any.
    pub fn dominant(&self) -> Option<&TaxonomyBucket> {
        self.buckets.iter().max_by_key(|b| b.count)
    }
}

/// Bucket `addrs` by region. Duplicate addresses are de-duplicated (a write
/// surface is about *which* bytes are reachable, not how often). Sample lists
/// keep the lowest addresses for stable, readable output.
pub fn classify_writes<I: IntoIterator<Item = u32>>(addrs: I) -> WriteTaxonomy {
    classify_writes_with_samples(addrs, DEFAULT_SAMPLES)
}

/// [`classify_writes`] with an explicit per-bucket sample cap.
pub fn classify_writes_with_samples<I: IntoIterator<Item = u32>>(
    addrs: I,
    max_samples: usize,
) -> WriteTaxonomy {
    // De-dup + sort so output is deterministic and samples are the lowest
    // addresses in each region.
    let mut unique: Vec<u32> = addrs.into_iter().collect();
    unique.sort_unstable();
    unique.dedup();

    // Preserve a stable category order: first-seen by ascending address.
    let mut buckets: Vec<TaxonomyBucket> = Vec::new();
    for addr in &unique {
        let c = classify_address(*addr);
        if let Some(b) = buckets.iter_mut().find(|b| b.category == c.category) {
            b.count += 1;
            if b.samples.len() < max_samples {
                b.samples.push(c);
            }
        } else {
            buckets.push(TaxonomyBucket {
                category: c.category,
                count: 1,
                samples: vec![c],
            });
        }
    }
    buckets.sort_by_key(|b| b.category);

    WriteTaxonomy {
        total: unique.len(),
        buckets,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify::{BATTLE_ACTOR_BASE, INVENTORY_BASE};

    #[test]
    fn buckets_by_region_and_dedups() {
        // Two inventory bytes (one repeated), one battle-actor byte, one
        // wholly-unknown address.
        let tax = classify_writes([
            INVENTORY_BASE,
            INVENTORY_BASE,
            INVENTORY_BASE + 2,
            BATTLE_ACTOR_BASE + 0x14C,
            0x8019_0000,
        ]);
        assert_eq!(tax.total, 4, "the repeated inventory byte is de-duped");
        assert_eq!(tax.count(Category::Inventory), 2);
        assert_eq!(tax.count(Category::BattleActor), 1);
        assert_eq!(tax.count(Category::Unknown), 1);
    }

    #[test]
    fn unknown_writes_are_flagged_interesting() {
        let tax = classify_writes([0x8019_0000, INVENTORY_BASE]);
        let interesting: Vec<_> = tax.interesting().map(|b| b.category).collect();
        assert_eq!(interesting, vec![Category::Unknown]);
        assert!(
            tax.bucket(Category::Inventory).is_some(),
            "the inventory write is recorded but not flagged interesting"
        );
    }

    #[test]
    fn samples_are_capped_and_lowest_first() {
        let addrs: Vec<u32> = (0..20).map(|i| INVENTORY_BASE + i * 2).collect();
        let tax = classify_writes_with_samples(addrs, 3);
        let b = tax.bucket(Category::Inventory).unwrap();
        assert_eq!(b.count, 20);
        assert_eq!(b.samples.len(), 3);
        assert_eq!(b.samples[0].addr, INVENTORY_BASE);
        assert_eq!(b.samples[1].addr, INVENTORY_BASE + 2);
    }

    #[test]
    fn empty_input_is_empty() {
        let tax = classify_writes(std::iter::empty());
        assert_eq!(tax.total, 0);
        assert!(tax.buckets.is_empty());
        assert!(tax.dominant().is_none());
    }
}
