//! Seru-trade vendor offers: a deterministic, time-bucketed trade table shared
//! by the randomizer (which embeds the toggle + master seed on the disc and
//! previews offers) and the clean-room engine (which renders the trade UI and
//! performs the swap at runtime).
//!
//! ## What a "trade" is
//!
//! A trading vendor offers to take **one of the seru a character already owns**
//! and hand back a *different* seru. The seru id space is the player Seru-magic
//! block - spell ids [`SERU_POOL_START`]`..=`[`SERU_POOL_END`] (base + evolved),
//! the same id space [`crate::spell_names`] resolves to display names like
//! `Gimard` / `Orb`. Each owned seru also carries the roster slot of the
//! character who holds it (so the UI can show "Gimard (Vahn)").
//!
//! ## Why a deterministic generator instead of a static table
//!
//! The vendor's preferences **reseed every two in-game hours**
//! ([`SECONDS_PER_RESEED`]). Rather than store one frozen table, both tracks
//! compute the offers from `(master_seed, vendor_id, time_bucket, owned_set)`
//! with the *same* pure function ([`vendor_offers`]). The randomizer's preview
//! and the engine's live UI therefore always agree for the same inputs, and the
//! only thing the randomizer has to write to the disc is a tiny config blob
//! (enabled flag + master seed) - see [`SeruTradeConfig`].
//!
//! The generator is intentionally free of game-data lookups (names,
//! equippability, who-can-learn) so it stays a stable, testable kernel; the
//! caller layers those on top.

use crate::item_names;

/// 4-byte magic prefixing the on-disc config blob ("Seru TRaDe").
pub const CONFIG_MAGIC: [u8; 4] = *b"STRD";

/// Config blob version. Bump if the on-disc layout ever changes.
pub const CONFIG_VERSION: u8 = 1;

/// Virtual address the config blob is written to inside the preserved 1028-byte
/// rodata zero gap at `0x8007AB38` (see [`item_names`] /
/// [`crate::move_power`]). Placed near the top of the gap, clear of the two
/// MIPS-injection routines that live lower in it (the bonus-equipment routine at
/// `0x8007AB80` and the flee-EXP routine at `0x8007AD00`), so the seru-trade
/// config and those features coexist without overlap.
pub const CONFIG_VA: u32 = 0x8007_AF00;

/// Byte length of the on-disc config blob (magic + fields + reserved padding).
pub const CONFIG_LEN: usize = 0x18;

/// In-game seconds between offer reseeds (two hours). Matches the retail
/// play-time counter at `0x80084570` (mirrored by the engine as
/// `World::play_time_seconds`); the time bucket is `play_time / this`.
pub const SECONDS_PER_RESEED: u32 = 2 * 60 * 60;

/// First seru id in the player Seru-magic block (base seru, e.g. `Gimard`).
pub const SERU_POOL_START: u8 = 0x81;
/// Last seru id in the player Seru-magic block (covers base + evolved seru).
pub const SERU_POOL_END: u8 = 0x95;

/// Default cap on how many trades a single vendor offers at once.
pub const DEFAULT_MAX_OFFERS: u8 = 4;

/// The default tradeable-seru pool: the whole player Seru-magic block.
pub fn default_pool() -> Vec<u8> {
    (SERU_POOL_START..=SERU_POOL_END).collect()
}

/// The time bucket `play_time_seconds` falls in (offers reseed each bucket).
pub fn time_bucket(play_time_seconds: u32) -> u32 {
    play_time_seconds / SECONDS_PER_RESEED
}

/// Derive a stable vendor id from a shop's identity - its display name plus the
/// item ids it stocks. Each distinct vendor then reseeds its trade offers
/// independently, and the same vendor is stable across visits (the offer
/// generator keys on this id). FNV-1a over the name bytes then the stock ids,
/// folded to `u16`.
pub fn vendor_id_from_shop(name: &str, item_ids: &[u8]) -> u16 {
    let mut h: u32 = 0x811C_9DC5;
    let mut mix = |b: u8| {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    };
    for &b in name.as_bytes() {
        mix(b);
    }
    for &id in item_ids {
        mix(id);
    }
    (h ^ (h >> 16)) as u16
}

/// The randomizer's seru-trade settings, as carried on the patched disc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeruTradeConfig {
    /// Whether seru trading is active.
    pub enabled: bool,
    /// Master seed feeding the per-vendor / per-bucket offer generator.
    pub seed: u64,
    /// Maximum simultaneous trades a vendor offers.
    pub max_offers: u8,
}

impl Default for SeruTradeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            seed: 0,
            max_offers: DEFAULT_MAX_OFFERS,
        }
    }
}

impl SeruTradeConfig {
    /// Serialize to the fixed-size on-disc blob:
    /// `[magic:4][version:1][enabled:1][max_offers:1][reserved:1][seed:8 LE]`
    /// then zero-padded to [`CONFIG_LEN`].
    pub fn to_blob(&self) -> [u8; CONFIG_LEN] {
        let mut out = [0u8; CONFIG_LEN];
        out[0..4].copy_from_slice(&CONFIG_MAGIC);
        out[4] = CONFIG_VERSION;
        out[5] = self.enabled as u8;
        out[6] = self.max_offers;
        // out[7] reserved (0)
        out[8..16].copy_from_slice(&self.seed.to_le_bytes());
        out
    }

    /// Parse a blob produced by [`Self::to_blob`]. `None` when the magic or
    /// version doesn't match (so an absent / foreign blob reads as "no config").
    pub fn from_blob(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 16 || bytes[0..4] != CONFIG_MAGIC || bytes[4] != CONFIG_VERSION {
            return None;
        }
        let seed = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
        let max_offers = if bytes[6] == 0 {
            DEFAULT_MAX_OFFERS
        } else {
            bytes[6]
        };
        Some(Self {
            enabled: bytes[5] != 0,
            seed,
            max_offers,
        })
    }

    /// Read the config from a `SCUS_942.54` image (resolving [`CONFIG_VA`] to a
    /// file offset). `None` when the image isn't a parseable PSX-EXE, the VA is
    /// out of range, or no seru-trade blob has been written.
    pub fn from_scus(scus: &[u8]) -> Option<Self> {
        let off = item_names::file_offset_for_va(scus, CONFIG_VA)?;
        Self::from_blob(scus.get(off..off + CONFIG_LEN)?)
    }
}

/// A seru a character currently owns, the unit a trade gives away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnedSeru {
    /// Seru / spell id (player block [`SERU_POOL_START`]`..=`[`SERU_POOL_END`]).
    pub seru_id: u8,
    /// Roster slot of the owning character (0 = lead, etc.).
    pub owner_slot: u8,
    /// The seru's current level in that character's spell list (parallel
    /// `levels[]` array at record `+0x161`), shown in the trade UI as `LVL n`.
    pub level: u8,
}

/// One trade a vendor offers this bucket: give [`give`](Self::give), receive a
/// different seru. (Legacy per-instance model - see [`BucketOffer`] /
/// [`expand_offers`] for the want-a-type / offer-a-partner model the trade UI
/// renders.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TradeOffer {
    /// The owned seru the vendor wants (and which character it comes from).
    pub give: OwnedSeru,
    /// The seru id the vendor hands back.
    pub receive_seru_id: u8,
}

/// A vendor's standing preference for one time bucket, independent of who owns
/// what. The vendor wants every instance of [`want_id`](Self::want_id) the party
/// holds and hands back [`give_id`](Self::give_id) for each. Ownership-independent
/// by design, so the whole bucket schedule can be precomputed at patch time (the
/// randomizer writes [`BUCKET_*`](bucket_offers) bytes to the disc) and the retail
/// handler only has to index it by `play_time / `[`SECONDS_PER_RESEED`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BucketOffer {
    /// Seru id the vendor wants (the party gives one up). `0` = no offer this
    /// bucket (e.g. degenerate pool).
    pub want_id: u8,
    /// Seru id the vendor hands back (always `!= want_id` when `want_id != 0`).
    pub give_id: u8,
    /// Level the handed-back seru comes at - part of the trade's value, shown to the
    /// player before trading and applied to the received seru on confirm. Fixed per
    /// bucket (rolled into [`GIVE_LEVEL_MIN`]`..=`[`GIVE_LEVEL_MAX`] from the same RNG
    /// stream as the ids), so it's stable while the vendor shows this offer. `0` when
    /// there's no offer (`want_id == 0`).
    pub give_level: u8,
}

/// Inclusive level range the vendor's handed-back seru rolls within.
pub const GIVE_LEVEL_MIN: u8 = 4;
pub const GIVE_LEVEL_MAX: u8 = 9;

/// One concrete, selectable trade line in the UI: a specific owner's instance of
/// the bucket's wanted seru. Expanded from a [`BucketOffer`] against the live
/// party by [`expand_offers`]; the UI renders it as
/// `give_name <-> want_name (owner, LVL level)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnerTrade {
    /// Roster slot whose spell list changes on confirm.
    pub owner_slot: u8,
    /// Seru id removed from that character (the vendor's want).
    pub given_id: u8,
    /// Seru id added to that character (the vendor's give-back).
    pub received_id: u8,
    /// The given seru's current level (for the `LVL n` display).
    pub given_level: u8,
    /// The level the received seru comes at (the bucket's fixed give level).
    pub received_level: u8,
}

/// SplitMix64, duplicated here (instead of depending on the randomizer's copy)
/// so this kernel stays a leaf both `legaia-rando` and `engine-core` can share.
/// Same constants as `legaia_rando::rng::SplitMix64`, so the streams match.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        debug_assert!(n > 0);
        (self.next() % n as u64) as usize
    }

    fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i + 1);
            items.swap(i, j);
        }
    }
}

/// Mix `(seed, vendor_id, bucket)` into a single generator seed. SplitMix64
/// avalanches well, so a plain XOR of spread-out inputs is enough to keep
/// adjacent vendors / buckets independent.
fn mix(seed: u64, vendor_id: u16, bucket: u32) -> u64 {
    seed ^ ((vendor_id as u64).wrapping_mul(0xA24B_AED4_963E_E407))
        ^ ((bucket as u64).wrapping_mul(0x9FB2_1C65_1E98_DF25))
}

/// Compute the trades a vendor offers for a given `(vendor_id, time_bucket)`.
///
/// Pure and deterministic: the same `(seed, vendor_id, time_bucket, owned set)`
/// always yields the same offers, regardless of the order `owned` is passed
/// (the function canonicalizes it). At most `max_offers` trades are returned -
/// fewer when the character party owns fewer seru. Each receive id is drawn from
/// `pool` and is guaranteed to differ from the seru being given. An empty
/// `owned` or `pool` yields no offers.
pub fn vendor_offers(
    seed: u64,
    vendor_id: u16,
    time_bucket: u32,
    owned: &[OwnedSeru],
    pool: &[u8],
    max_offers: usize,
) -> Vec<TradeOffer> {
    if owned.is_empty() || pool.is_empty() || max_offers == 0 {
        return Vec::new();
    }

    // Canonicalize so caller-ordering can't change the result.
    let mut candidates: Vec<OwnedSeru> = owned.to_vec();
    candidates.sort_by_key(|o| (o.owner_slot, o.seru_id));
    candidates.dedup();

    let mut rng = Rng(mix(seed, vendor_id, time_bucket));
    rng.shuffle(&mut candidates);
    candidates.truncate(max_offers);

    candidates
        .into_iter()
        .filter_map(|give| {
            // Draw a receive id distinct from what's being given.
            let viable: Vec<u8> = pool
                .iter()
                .copied()
                .filter(|&id| id != give.seru_id)
                .collect();
            if viable.is_empty() {
                return None;
            }
            let receive_seru_id = viable[rng.below(viable.len())];
            Some(TradeOffer {
                give,
                receive_seru_id,
            })
        })
        .collect()
}

/// Convenience wrapper: offers for `vendor_id` at the current
/// `play_time_seconds`, using a config's seed + offer cap and the default pool.
pub fn offers_at(
    config: &SeruTradeConfig,
    vendor_id: u16,
    play_time_seconds: u32,
    owned: &[OwnedSeru],
) -> Vec<TradeOffer> {
    vendor_offers(
        config.seed,
        vendor_id,
        time_bucket(play_time_seconds),
        owned,
        &default_pool(),
        config.max_offers as usize,
    )
}

/// Number of time buckets in the precomputed schedule the randomizer writes to
/// the disc. The retail handler indexes it with `(play_time / `[`SECONDS_PER_RESEED`]`)
/// % BUCKET_COUNT`, so the vendor's preferences cycle every `BUCKET_COUNT * 2`
/// in-game hours. A power of two so the runtime modulo is a single `andi`.
pub const BUCKET_COUNT: usize = 64;

/// Byte length of the serialized bucket schedule: `BUCKET_COUNT` entries of
/// `[want_id, give_id, give_level]`.
pub const BUCKET_TABLE_LEN: usize = BUCKET_COUNT * 3;
/// On-disc bytes per bucket entry (`[want_id, give_id, give_level]`).
pub const BUCKET_ENTRY_LEN: usize = 3;

/// Precompute the whole vendor schedule: for each bucket `0..count`, deterministically
/// pick a `(want_id, give_id)` pair of distinct ids from `pool`. Ownership-independent
/// - the live party is only consulted at render time (see [`expand_offers`]). The same
/// `(seed, count, pool)` always yields the same schedule, so the randomizer's on-disc
/// table and any engine preview agree. An empty / single-element `pool` yields all
/// `(0, 0)` (no offer) entries.
pub fn bucket_offers(seed: u64, count: usize, pool: &[u8]) -> Vec<BucketOffer> {
    (0..count)
        .map(|bucket| {
            if pool.len() < 2 {
                return BucketOffer {
                    want_id: 0,
                    give_id: 0,
                    give_level: 0,
                };
            }
            // One RNG stream per bucket, mixed off the master seed (vendor id folded
            // in as 0 - a single global trader; distinct vendors can reseed later).
            let mut rng = Rng(mix(seed, 0, bucket as u32));
            let want_id = pool[rng.below(pool.len())];
            // give id distinct from want.
            let viable: Vec<u8> = pool.iter().copied().filter(|&id| id != want_id).collect();
            let give_id = viable[rng.below(viable.len())];
            // give level, fixed per bucket: GIVE_LEVEL_MIN..=GIVE_LEVEL_MAX.
            let span = (GIVE_LEVEL_MAX - GIVE_LEVEL_MIN + 1) as usize;
            let give_level = GIVE_LEVEL_MIN + rng.below(span) as u8;
            BucketOffer {
                want_id,
                give_id,
                give_level,
            }
        })
        .collect()
}

/// Serialize a bucket schedule to the on-disc byte layout
/// (`[want, give, give_level]` per entry). Truncated / zero-padded to
/// [`BUCKET_TABLE_LEN`].
pub fn bucket_table_to_bytes(buckets: &[BucketOffer]) -> [u8; BUCKET_TABLE_LEN] {
    let mut out = [0u8; BUCKET_TABLE_LEN];
    for (i, b) in buckets.iter().take(BUCKET_COUNT).enumerate() {
        out[i * BUCKET_ENTRY_LEN] = b.want_id;
        out[i * BUCKET_ENTRY_LEN + 1] = b.give_id;
        out[i * BUCKET_ENTRY_LEN + 2] = b.give_level;
    }
    out
}

/// Parse a bucket schedule from the on-disc bytes written by
/// [`bucket_table_to_bytes`].
pub fn bucket_table_from_bytes(bytes: &[u8]) -> Vec<BucketOffer> {
    bytes
        .chunks_exact(BUCKET_ENTRY_LEN)
        .take(BUCKET_COUNT)
        .map(|c| BucketOffer {
            want_id: c[0],
            give_id: c[1],
            give_level: c[2],
        })
        .collect()
}

/// The bucket index a play-time falls in, wrapped to the precomputed schedule:
/// `(play_time / `[`SECONDS_PER_RESEED`]`) % `[`BUCKET_COUNT`]. Mirrors the
/// retail handler's `divu` + `andi`.
pub fn bucket_index(play_time_seconds: u32) -> usize {
    (time_bucket(play_time_seconds) as usize) % BUCKET_COUNT
}

/// Expand one bucket's `(want, give)` preference against the live party: one
/// [`OwnerTrade`] per party member who currently owns the wanted seru, in party
/// order. `owned` is the cross-party enumeration (e.g.
/// `engine_core::seru_trade::party_owned_seru`). Empty when nobody owns the want
/// or the bucket has no offer (`want_id == 0`).
///
/// Owners who **already own the give-back seru** are filtered out - trading the
/// wanted seru for one you already hold is pointless (and would duplicate it), so
/// they aren't offered the trade.
pub fn expand_offers(offer: BucketOffer, owned: &[OwnedSeru]) -> Vec<OwnerTrade> {
    if offer.want_id == 0 {
        return Vec::new();
    }
    owned
        .iter()
        .filter(|o| o.seru_id == offer.want_id)
        .filter(|o| {
            !owned
                .iter()
                .any(|x| x.owner_slot == o.owner_slot && x.seru_id == offer.give_id)
        })
        .map(|o| OwnerTrade {
            owner_slot: o.owner_slot,
            given_id: offer.want_id,
            received_id: offer.give_id,
            given_level: o.level,
            received_level: offer.give_level,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owned(set: &[(u8, u8)]) -> Vec<OwnedSeru> {
        set.iter()
            .map(|&(seru_id, owner_slot)| OwnedSeru {
                seru_id,
                owner_slot,
                level: 0,
            })
            .collect()
    }

    #[test]
    fn config_blob_round_trips() {
        let c = SeruTradeConfig {
            enabled: true,
            seed: 0x0123_4567_89AB_CDEF,
            max_offers: 5,
        };
        let blob = c.to_blob();
        assert_eq!(blob.len(), CONFIG_LEN);
        assert_eq!(SeruTradeConfig::from_blob(&blob), Some(c));
    }

    #[test]
    fn from_blob_rejects_absent_or_foreign() {
        assert_eq!(SeruTradeConfig::from_blob(&[0u8; CONFIG_LEN]), None);
        assert_eq!(SeruTradeConfig::from_blob(b"nope"), None);
        let mut blob = SeruTradeConfig::default().to_blob();
        blob[4] = 0xFF; // wrong version
        assert_eq!(SeruTradeConfig::from_blob(&blob), None);
    }

    #[test]
    fn offers_are_deterministic_and_order_independent() {
        let pool = default_pool();
        let a = owned(&[(0x81, 0), (0x85, 1), (0x88, 2), (0x90, 0)]);
        let mut b = a.clone();
        b.reverse();

        let oa = vendor_offers(0xC0FFEE, 3, 1, &a, &pool, 4);
        let ob = vendor_offers(0xC0FFEE, 3, 1, &b, &pool, 4);
        assert_eq!(oa, ob, "result must not depend on input ordering");

        let again = vendor_offers(0xC0FFEE, 3, 1, &a, &pool, 4);
        assert_eq!(oa, again, "same inputs => same offers");
    }

    #[test]
    fn receive_always_differs_from_give() {
        let pool = default_pool();
        let owned_set = owned(&[(0x81, 0), (0x82, 0), (0x83, 1), (0x84, 1), (0x85, 2)]);
        for vendor in 0..20u16 {
            for bucket in 0..20u32 {
                for o in vendor_offers(42, vendor, bucket, &owned_set, &pool, 4) {
                    assert_ne!(o.receive_seru_id, o.give.seru_id);
                    assert!(pool.contains(&o.receive_seru_id));
                }
            }
        }
    }

    #[test]
    fn offer_count_capped_by_max_and_owned() {
        let pool = default_pool();
        let owned_set = owned(&[(0x81, 0), (0x82, 0), (0x83, 1)]);
        // max larger than owned => limited by owned
        assert_eq!(vendor_offers(1, 0, 0, &owned_set, &pool, 8).len(), 3);
        // max smaller => limited by max
        assert_eq!(vendor_offers(1, 0, 0, &owned_set, &pool, 2).len(), 2);
        // empties
        assert!(vendor_offers(1, 0, 0, &[], &pool, 4).is_empty());
        assert!(vendor_offers(1, 0, 0, &owned_set, &[], 4).is_empty());
    }

    #[test]
    fn different_buckets_reseed_offers() {
        let pool = default_pool();
        let owned_set = owned(&[(0x81, 0), (0x85, 1), (0x88, 2), (0x8C, 0), (0x90, 1)]);
        let b0 = vendor_offers(7, 1, 0, &owned_set, &pool, 4);
        // Over several buckets, at least one differs from bucket 0 (reseed works).
        let changed =
            (1..12u32).any(|bucket| vendor_offers(7, 1, bucket, &owned_set, &pool, 4) != b0);
        assert!(changed, "offers should reseed across buckets");
    }

    #[test]
    fn vendor_id_is_stable_and_distinguishes_shops() {
        let a = vendor_id_from_shop("Variety Store", &[0x22, 0x34]);
        assert_eq!(a, vendor_id_from_shop("Variety Store", &[0x22, 0x34]));
        // A different name or different stock yields a different vendor.
        assert_ne!(a, vendor_id_from_shop("Weapon Shop", &[0x22, 0x34]));
        assert_ne!(a, vendor_id_from_shop("Variety Store", &[0x22, 0x35]));
    }

    #[test]
    fn time_bucket_boundary() {
        assert_eq!(time_bucket(0), 0);
        assert_eq!(time_bucket(SECONDS_PER_RESEED - 1), 0);
        assert_eq!(time_bucket(SECONDS_PER_RESEED), 1);
        assert_eq!(time_bucket(SECONDS_PER_RESEED * 3 + 5), 3);
    }

    // --- want-a-type / offer-a-partner (precomputed bucket) model ---

    #[test]
    fn bucket_schedule_is_deterministic_and_distinct_pairs() {
        let pool = default_pool();
        let a = bucket_offers(0xC0FFEE, BUCKET_COUNT, &pool);
        let b = bucket_offers(0xC0FFEE, BUCKET_COUNT, &pool);
        assert_eq!(a, b, "same seed => same schedule");
        assert_eq!(a.len(), BUCKET_COUNT);
        for o in &a {
            assert_ne!(o.want_id, 0);
            assert_ne!(o.want_id, o.give_id, "give must differ from want");
            assert!(pool.contains(&o.want_id) && pool.contains(&o.give_id));
            assert!(
                (GIVE_LEVEL_MIN..=GIVE_LEVEL_MAX).contains(&o.give_level),
                "give level in {GIVE_LEVEL_MIN}..={GIVE_LEVEL_MAX}"
            );
        }
        // Different seed shifts at least one bucket.
        let c = bucket_offers(0xBADF00D, BUCKET_COUNT, &pool);
        assert!(a != c, "a different seed should change the schedule");
    }

    #[test]
    fn bucket_table_round_trips() {
        let pool = default_pool();
        let sched = bucket_offers(0x1234_5678, BUCKET_COUNT, &pool);
        let bytes = bucket_table_to_bytes(&sched);
        assert_eq!(bytes.len(), BUCKET_TABLE_LEN);
        assert_eq!(bucket_table_from_bytes(&bytes), sched);
    }

    #[test]
    fn degenerate_pool_yields_no_offer() {
        assert!(bucket_offers(1, 4, &[]).iter().all(|o| o.want_id == 0));
        assert!(bucket_offers(1, 4, &[0x81]).iter().all(|o| o.want_id == 0));
    }

    #[test]
    fn expand_lists_one_line_per_owner_of_want() {
        // Vahn (slot 0) and Noa (slot 1) both own 0x82; Gala (slot 2) owns 0x85.
        let owned_set = vec![
            OwnedSeru {
                seru_id: 0x82,
                owner_slot: 0,
                level: 1,
            },
            OwnedSeru {
                seru_id: 0x85,
                owner_slot: 2,
                level: 4,
            },
            OwnedSeru {
                seru_id: 0x82,
                owner_slot: 1,
                level: 3,
            },
        ];
        let offer = BucketOffer {
            want_id: 0x82,
            give_id: 0x81,
            give_level: 7,
        };
        let lines = expand_offers(offer, &owned_set);
        assert_eq!(lines.len(), 2, "both owners of 0x82 listed");
        assert!(
            lines
                .iter()
                .all(|t| t.given_id == 0x82 && t.received_id == 0x81 && t.received_level == 7)
        );
        // Owner + level carried through for the display.
        assert!(
            lines
                .iter()
                .any(|t| t.owner_slot == 0 && t.given_level == 1)
        );
        assert!(
            lines
                .iter()
                .any(|t| t.owner_slot == 1 && t.given_level == 3)
        );
        // Nobody owns the want -> no lines.
        assert!(
            expand_offers(
                BucketOffer {
                    want_id: 0x90,
                    give_id: 0x81,
                    give_level: 5,
                },
                &owned_set
            )
            .is_empty()
        );
        // No-offer bucket -> no lines.
        assert!(
            expand_offers(
                BucketOffer {
                    want_id: 0,
                    give_id: 0,
                    give_level: 0,
                },
                &owned_set
            )
            .is_empty()
        );
        // An owner who already owns the give-back is filtered out (pointless trade):
        // Noa (slot 1) owns both 0x82 (want) and 0x81 (give) -> only Vahn (slot 0) is
        // offered the 0x82 -> 0x81 trade.
        let mut owns_give = owned_set.clone();
        owns_give.push(OwnedSeru {
            seru_id: 0x81,
            owner_slot: 1,
            level: 2,
        });
        let filtered = expand_offers(
            BucketOffer {
                want_id: 0x82,
                give_id: 0x81,
                give_level: 7,
            },
            &owns_give,
        );
        assert_eq!(
            filtered.len(),
            1,
            "owner already holding the give-back is skipped"
        );
        assert_eq!(filtered[0].owner_slot, 0);
    }

    #[test]
    fn bucket_index_wraps_schedule() {
        assert_eq!(bucket_index(0), 0);
        assert_eq!(bucket_index(SECONDS_PER_RESEED), 1);
        assert_eq!(
            bucket_index(SECONDS_PER_RESEED * (BUCKET_COUNT as u32)),
            0,
            "wraps after BUCKET_COUNT buckets"
        );
    }
}
