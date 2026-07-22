//! Seru trading: let vendors offer to swap one of a character's seru for a
//! different seru, with the vendor's preferences reseeding every two in-game
//! hours.
//!
//! ## What the randomizer actually writes
//!
//! Unlike a drop / shop edit, the trade offers aren't a fixed table - they're a
//! deterministic function of `(master_seed, vendor_id, in-game-time bucket,
//! the character party's currently-owned seru)`, evaluated identically by the
//! randomizer's preview and the clean-room engine's live UI (the shared kernel
//! [`legaia_asset::seru_trade`]). So all the randomizer embeds on the disc is a
//! tiny config blob - an *enabled* flag plus the run's master seed - and the
//! engine recomputes the per-vendor offers at runtime, reseeding as the retail
//! play-time counter crosses each two-hour boundary.
//!
//! ## Where the blob lives
//!
//! The blob ([`legaia_asset::seru_trade::SeruTradeConfig::to_blob`], 24 bytes)
//! is written into the preserved 1028-byte rodata zero gap at `0x8007AB38` in
//! `SCUS_942.54` - the same loaded-and-preserved padding the
//! [`crate::item_name`] string and the [`crate::bonus_drop`] / [`crate::flee_exp`]
//! code hooks use, but at a higher, non-overlapping offset
//! ([`legaia_asset::seru_trade::CONFIG_VA`] = `0x8007AF00`). It is plain data,
//! not code: nothing in the retail executable reads it (retail has no trade UI),
//! so on a real console the patch is inert; the clean-room engine is what gives
//! it meaning.
//!
//! The write is a single same-size, in-place `SCUS_942.54` edit. The planner
//! refuses to write unless the target region is all-zero dead space (or already
//! holds a prior seru-trade blob, so re-running with a new seed is idempotent),
//! exactly like the [`crate::item_name`] injection - a differently-laid-out
//! image is left untouched rather than corrupted. No Sony bytes are embedded.

use anyhow::{Result, bail};

use legaia_asset::item_names;
use legaia_asset::seru_trade::{CONFIG_LEN, CONFIG_MAGIC, CONFIG_VA, SeruTradeConfig};

/// A planned seru-trade config write: the resolved `SCUS_942.54` file offset and
/// the serialized blob to drop there.
#[derive(Debug, Clone)]
pub struct SeruTradePlan {
    /// File offset of [`CONFIG_VA`] within `SCUS_942.54`; receives [`Self::blob`].
    pub config_off: usize,
    /// The serialized config blob.
    pub blob: Vec<u8>,
    /// The config the blob encodes (echoed for reporting).
    pub config: SeruTradeConfig,
}

impl SeruTradePlan {
    /// Plan the config write for `config` against a `SCUS_942.54` image.
    ///
    /// Errors (without touching the disc) if the image isn't a parseable
    /// PSX-EXE, if [`CONFIG_VA`] is out of range, or if the target region is
    /// neither all-zero dead space nor an existing seru-trade blob.
    pub fn plan(scus: &[u8], config: SeruTradeConfig) -> Result<Self> {
        let config_off = item_names::file_offset_for_va(scus, CONFIG_VA)
            .ok_or_else(|| anyhow::anyhow!("can't resolve config VA {CONFIG_VA:#x} in SCUS"))?;
        let region = scus
            .get(config_off..config_off + CONFIG_LEN)
            .ok_or_else(|| anyhow::anyhow!("config region past end of SCUS"))?;

        // Accept all-zero dead space, or a region already holding our magic (a
        // prior run we're free to overwrite). Anything else means the rodata gap
        // isn't where we expect on this build - refuse rather than clobber it.
        let all_zero = region.iter().all(|&b| b == 0);
        let ours = region.len() >= 4 && region[0..4] == CONFIG_MAGIC;
        if !all_zero && !ours {
            bail!(
                "config region {CONFIG_VA:#x}..+{CONFIG_LEN} is not all-zero dead space \
                 (or a prior seru-trade blob); refusing to write on this build"
            );
        }

        Ok(Self {
            config_off,
            blob: config.to_blob().to_vec(),
            config,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_asset::seru_trade::DEFAULT_MAX_OFFERS;

    /// Minimal PSX-EXE whose `t_addr` segment spans [`CONFIG_VA`] so the planner
    /// can be exercised without any Sony bytes.
    fn synth_scus() -> Vec<u8> {
        const T_ADDR: u32 = 0x8007_0000;
        let off = (CONFIG_VA - T_ADDR) as usize + 0x800;
        let total = off + CONFIG_LEN + 0x40;
        let mut buf = vec![0u8; total];
        buf[0..8].copy_from_slice(b"PS-X EXE");
        buf[0x18..0x1C].copy_from_slice(&T_ADDR.to_le_bytes());
        buf[0x1C..0x20].copy_from_slice(&((total - 0x800) as u32).to_le_bytes());
        buf
    }

    fn cfg() -> SeruTradeConfig {
        SeruTradeConfig {
            enabled: true,
            seed: 0xDEAD_BEEF_CAFE_1234,
            max_offers: DEFAULT_MAX_OFFERS,
        }
    }

    #[test]
    fn plan_targets_dead_space_and_blob_round_trips() {
        let mut scus = synth_scus();
        let plan = SeruTradePlan::plan(&scus, cfg()).unwrap();
        // Apply the write and read it back through the shared reader.
        scus[plan.config_off..plan.config_off + plan.blob.len()].copy_from_slice(&plan.blob);
        assert_eq!(SeruTradeConfig::from_scus(&scus), Some(cfg()));
    }

    #[test]
    fn re_planning_over_a_prior_blob_is_allowed() {
        let mut scus = synth_scus();
        let p1 = SeruTradePlan::plan(&scus, cfg()).unwrap();
        scus[p1.config_off..p1.config_off + p1.blob.len()].copy_from_slice(&p1.blob);
        // A second run with a different seed must be accepted (idempotent slot).
        let new = SeruTradeConfig {
            seed: 0x1111_2222_3333_4444,
            ..cfg()
        };
        let p2 = SeruTradePlan::plan(&scus, new).unwrap();
        scus[p2.config_off..p2.config_off + p2.blob.len()].copy_from_slice(&p2.blob);
        assert_eq!(SeruTradeConfig::from_scus(&scus), Some(new));
    }

    #[test]
    fn plan_refuses_nonzero_foreign_region() {
        let mut scus = synth_scus();
        let off = item_names::file_offset_for_va(&scus, CONFIG_VA).unwrap();
        scus[off + 2] = 0xAB; // poison the region with non-magic bytes
        assert!(SeruTradePlan::plan(&scus, cfg()).is_err());
    }

    #[test]
    fn plan_rejects_non_psx_exe() {
        assert!(SeruTradePlan::plan(b"nope", cfg()).is_err());
    }
}
