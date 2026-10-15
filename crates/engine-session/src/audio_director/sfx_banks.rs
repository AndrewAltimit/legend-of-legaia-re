//! The SFX region's banks: cue-to-slot routing, the per-slot VABs, the
//! transient reward bank, and the shared slot-2 / slot-6 region with its
//! eviction and reclaim.

use super::*;

impl<S: AudioSink> AudioDirector<S> {
    /// Cues waiting in the scheduler (queued, not yet matured).
    pub fn sfx_pending(&self) -> usize {
        self.sfx_sched.pending_count()
    }

    /// The VAB slot cue `id`'s `+4` category names, before the closed-slot
    /// rule ([`Self::sfx_slot_for_cue`]); `None` for an id the installed
    /// table does not carry.
    pub fn cue_slot_raw(&self, id: u8) -> Option<u8> {
        self.sfx_cue_slots.get(&id).copied()
    }

    /// The extraction-frame PROT entry the bank staged in `slot` came from:
    /// the slot-0 system bank, the shared slot-2 / slot-6 region's bank, the
    /// reward bank, a side-band bank, or `monster.snd` for the battle's
    /// slots `7` / `8`. `None` for an empty slot.
    pub fn prot_for_slot(&self, slot: u8) -> Option<u32> {
        use legaia_game_tables::sfx_table::{
            SLOT0_SYSTEM_BANK_PROT_INDEX, SLOT11_REWARD_BANK_PROT_INDEX,
        };
        if !self.sfx_vabs.contains_key(&slot) {
            return None;
        }
        if slot == 0 && self.resident_slot0.is_some() {
            return Some(SLOT0_SYSTEM_BANK_PROT_INDEX);
        }
        if let Some(b) = self.shared_region.filter(|b| b.slot == slot) {
            return Some(b.prot_entry);
        }
        if slot == TRANSIENT_REWARD_SLOT && self.tail.reward().is_some() {
            return Some(SLOT11_REWARD_BANK_PROT_INDEX);
        }
        if let Some((b, _)) = self.tail.side_band().filter(|(b, _)| b.slot == slot) {
            return Some(b.prot_entry);
        }
        self.tail
            .monsters()
            .is_some_and(|(_, borrows)| borrows.iter().any(|t| t.slot == slot))
            .then_some(legaia_container_formats::vab_multi_bank::MONSTER_SND_PROT_INDEX as u32)
    }

    /// Whether the battle-end reward bank is parked in its slot.
    pub fn has_reward_bank(&self) -> bool {
        self.tail.reward().is_some()
    }

    /// Stage the reward bank (PROT 0889) behind the BGM unless it is already
    /// parked - the results frame's `LEVEL_UP_CUE` request, which both hosts
    /// make. `read_entry` reads an extraction-frame PROT entry. Returns
    /// whether the bank is parked afterwards.
    pub fn ensure_reward_bank(&mut self, read_entry: impl FnOnce(u32) -> Option<Vec<u8>>) -> bool {
        self.observe_track();
        if self.has_reward_bank() {
            return true;
        }
        let Some(bytes) = read_entry(legaia_game_tables::sfx_table::SLOT11_REWARD_BANK_PROT_INDEX)
        else {
            return false;
        };
        self.stage_transient_sfx_vab(TRANSIENT_REWARD_SLOT, &bytes)
    }

    /// Whether a VAB is staged in `slot`.
    pub fn has_sfx_vab_slot(&self, slot: u8) -> bool {
        self.sfx_vabs.contains_key(&slot)
    }

    /// Stage a VAB into `slot` **behind the resident BGM bank**, in the free
    /// tail of the BGM region - the port's version of retail's results-time
    /// load of PROT 0889 into slot 11 (`FUN_8001FC00(0x37B, 0xB, ..)` +
    /// `FUN_8001E54C(0xB, ..)` in `FUN_8004E568` phases 2 / 4). The SFX
    /// region is full (its two pinned banks leave ~2.5 KB), so the reward
    /// bank borrows BGM room instead. It stays parked until a track overruns
    /// its base or the field init closes it - the residency rule of
    /// [`legaia_engine_audio::bgm_tail`], which the browser page drives too.
    /// Returns `false` when the entry has no VAB header or the tail is too
    /// small.
    // REF: FUN_8001E54C, FUN_8004E568
    pub fn stage_transient_sfx_vab(&mut self, slot: u8, entry_bytes: &[u8]) -> bool {
        let Some(borrow) = self.park_tail_bank(slot, entry_bytes) else {
            return false;
        };
        if slot == TRANSIENT_REWARD_SLOT {
            self.tail.commit_reward(borrow);
        }
        true
    }

    /// Upload `entry_bytes`' VAB into `slot` above the track and every other
    /// tail borrower ([`BgmTail::place`]). `None` when the entry has no VAB
    /// header or the tail is too small; the slot is then empty.
    pub(super) fn park_tail_bank(&mut self, slot: u8, entry_bytes: &[u8]) -> Option<TailBorrow> {
        let (report, vab_off) = [4usize, 0]
            .into_iter()
            .find_map(|o| legaia_vab::parse(entry_bytes, o).ok().map(|r| (r, o)))?;
        self.observe_track();
        self.sfx_vabs.remove(&slot);
        let base = self.tail.place_report(slot, &report)?;
        let body = &entry_bytes[vab_off..];
        let bank = self
            .audio
            .with_spu(|spu| legaia_engine_audio::bgm_tail::upload_at(spu, base, &report, body));
        let end = legaia_engine_audio::spu_layout::bank_used_end(&bank).unwrap_or(base);
        self.sfx_vabs.insert(slot, bank);
        Some(TailBorrow { slot, base, end })
    }

    /// Install the sound-effect descriptor bank (decoded from the user's
    /// `SCUS_942.54` `DAT_8006F198` table at boot). Replaces any prior bank.
    pub fn set_sfx_bank(&mut self, bank: SfxBank) {
        self.sfx_bank = bank;
    }

    /// Install the cue id -> VAB slot routing decoded from the same
    /// descriptor table as [`Self::set_sfx_bank`]
    /// (`legaia_game_tables::sfx_table::SfxTable::cue_slots`). Without it every cue
    /// falls back to the class-2 bank, which is what a single-bank host did.
    pub fn set_sfx_cue_slots<I: IntoIterator<Item = (u8, u8)>>(&mut self, slots: I) {
        self.sfx_cue_slots = slots.into_iter().collect();
    }

    /// Install one resident SFX program bank at its VAB `slot` (0 = the
    /// PROT 0868 system bank, 2 = the PROT 0869 class-2 bank), uploaded into
    /// the shared SPU RAM region at boot. Cues fire against the bank their own
    /// category names so their programs are always resident; see
    /// [`Self::sfx_vabs`].
    pub fn set_sfx_vab(&mut self, slot: u8, bank: VabBank) {
        self.sfx_vabs.insert(slot, bank);
    }

    /// Whether any resident SFX bank was staged.
    pub fn has_sfx_vab(&self) -> bool {
        !self.sfx_vabs.is_empty()
    }

    /// The VAB slots that have a resident bank, ascending.
    pub fn staged_sfx_slots(&self) -> Vec<u8> {
        self.sfx_vabs.keys().copied().collect()
    }

    /// Borrow the active SFX bank - useful for tests / inspection.
    pub fn sfx_bank(&self) -> &SfxBank {
        &self.sfx_bank
    }

    /// The VAB slot cue `id` resolves to on this director, `None` when its
    /// slot is closed. See [`resolve_sfx_slot`].
    pub fn sfx_slot_for_cue(&self, id: u8) -> Option<u8> {
        resolve_sfx_slot(&self.sfx_cue_slots, &self.sfx_vabs, id)
    }

    /// The bank cue `id` keys: its slot's resident bank, or - only while no
    /// SFX bank staged at all (a disc-free boot) - the scene BGM bank.
    pub(super) fn sfx_vab_for_cue(&self, id: u8) -> Option<&VabBank> {
        if self.sfx_vabs.is_empty() {
            // Not while a track holds the SFX region: that bank is the
            // ending theme's, and its programs are not the cue's.
            return (!self.sfx_evicted).then_some(self.bank.as_ref()).flatten();
        }
        self.sfx_vabs.get(&self.sfx_slot_for_cue(id)?)
    }

    /// Set where the shared slot-2 / slot-6 region starts - one past the
    /// slot-0 system bank's samples.
    pub fn set_shared_region_base(&mut self, base: u32) {
        self.shared_region_base = base.div_ceil(16) * 16;
    }

    /// The bank the shared slot-2 / slot-6 region holds, if any.
    pub fn shared_region(&self) -> Option<SharedRegionBank> {
        self.shared_region
    }

    /// Refill the region VAB slots `2` and `6` share with `want` - the bank
    /// the world's residency says the current mode holds there - the way each
    /// mode's initialiser refills it in retail (`FUN_801D6704` loads PROT 0876
    /// into slot 6, `FUN_800520F0` PROT 0869 into slot 2). Whatever the region
    /// held is dropped first, under both slot keys: the two are one region.
    /// `read_entry` reads an extraction-frame PROT entry. Returns whether the
    /// wanted bank is now resident. A bank that does not fit (the dance's
    /// PROT 1231 is 234 400 bytes of samples against the region's
    /// 190 720) leaves both slots closed, which is what its cues then are.
    // REF: FUN_801D6704, FUN_800520F0, FUN_8001E54C
    pub fn sync_shared_region(
        &mut self,
        want: Option<SharedRegionBank>,
        read_entry: impl FnOnce(u32) -> Option<Vec<u8>>,
    ) -> bool {
        if self.sfx_evicted {
            // The region holds a track's samples; the refill waits for the
            // re-stage (`reclaim_sfx_region`), which re-arms this sync.
            return false;
        }
        if self.shared_region == want {
            let resident = want.is_none_or(|b| self.sfx_vabs.contains_key(&b.slot));
            // A bank that did not fit is retried once the tail has moved -
            // the dance's track replacing the field's, or a spill the new
            // track overran.
            if resident || self.shared_retry_gen == Some(self.tail.generation()) {
                return resident;
            }
        }
        self.shared_region = want;
        self.shared_retry_gen = None;
        let (a, b) = legaia_engine_mode::sfx_residency::SHARED_REGION_SLOTS;
        self.sfx_vabs.remove(&a);
        self.sfx_vabs.remove(&b);
        self.tail.drop_shared_spill();
        let Some(want) = want else {
            return true;
        };
        let staged = self.stage_shared_region(want, read_entry);
        if !staged {
            self.shared_retry_gen = Some(self.tail.generation());
        }
        staged
    }

    /// Read and upload `want` into the shared region - spilling into the BGM
    /// tail when its bodies do not fit above slot 0
    /// ([`legaia_engine_audio::spu_layout::upload_shared_region_spilled`]).
    fn stage_shared_region(
        &mut self,
        want: SharedRegionBank,
        read_entry: impl FnOnce(u32) -> Option<Vec<u8>>,
    ) -> bool {
        let Some(bytes) = read_entry(want.prot_entry) else {
            log::debug!("shared-region bank PROT {} unreadable", want.prot_entry);
            return false;
        };
        let Some((report, vab_off)) = [4usize, 0]
            .into_iter()
            .find_map(|o| legaia_vab::parse(&bytes, o).ok().map(|r| (r, o)))
        else {
            return false;
        };
        let base = self.shared_region_base;
        let room = legaia_engine_audio::spu_layout::SPU_RAM_BYTES.saturating_sub(base);
        let body_total: u32 = report.vag_samples.iter().map(|v| v.size as u32).sum();
        if body_total > room {
            return self.stage_shared_spilled(want, &report, &bytes[vab_off..]);
        }
        let body = &bytes[vab_off..];
        let bank = match self.sfx_vabs.get(&0) {
            // Above the resident slot-0 bank, through the kernel the boot
            // staging and the page share.
            Some(slot0) => self.audio.with_spu(|spu| {
                legaia_engine_audio::spu_layout::upload_shared_region(spu, slot0, &report, body)
            }),
            None => Some(self.audio.with_spu(|spu| {
                let mut alloc = legaia_engine_audio::SpuAllocator::new(base, room);
                VabBank::upload(spu, &mut alloc, &report, body)
            })),
        };
        let Some(bank) = bank else {
            return false;
        };
        self.sfx_vabs.insert(want.slot, bank);
        true
    }

    /// Stage an oversize shared-region bank across the region and the BGM
    /// tail, parking the tail half as a [`BgmTail`] borrower so a track that
    /// overruns it drops the bank (and the next sync re-stages it).
    fn stage_shared_spilled(
        &mut self,
        want: SharedRegionBank,
        report: &legaia_vab::VabReport,
        body: &[u8],
    ) -> bool {
        self.observe_track();
        let Some(tail_base) = self.tail.spill_base() else {
            return false;
        };
        let Some(slot0) = self.sfx_vabs.get(&0) else {
            return false;
        };
        let up = self.audio.with_spu(|spu| {
            legaia_engine_audio::spu_layout::upload_shared_region_spilled(
                spu, slot0, tail_base, report, body,
            )
        });
        let Some(up) = up else {
            log::debug!(
                "shared-region bank PROT {} does not fit above slot 0 and behind the BGM",
                want.prot_entry
            );
            return false;
        };
        if let Some((base, end)) = up.spill {
            self.tail
                .commit_shared_spill(legaia_engine_audio::bgm_tail::TailBorrow {
                    slot: want.slot,
                    base,
                    end,
                });
        }
        self.sfx_vabs.insert(want.slot, up.bank);
        true
    }

    /// Stage the slot-0 system bank (PROT 0868's entry `bytes`) at the
    /// bottom of the SFX region through the shared layout kernel
    /// ([`legaia_engine_audio::spu_layout::upload_resident_sfx`]), and keep
    /// the bytes so [`Self::reclaim_sfx_region`] can re-stage it. The shared
    /// region is then filled above it by [`Self::sync_shared_region`].
    /// Returns `false` when the entry carries no VAB at `+4` or `+0`.
    // REF: FUN_8001E54C
    pub fn stage_resident_slot0(&mut self, bytes: Vec<u8>) -> bool {
        let Some((report, vab_off)) = [4usize, 0]
            .into_iter()
            .find_map(|o| legaia_vab::parse(&bytes, o).ok().map(|r| (r, o)))
        else {
            return false;
        };
        let staged = self.audio.with_spu(|spu| {
            legaia_engine_audio::spu_layout::upload_resident_sfx(
                spu,
                (&report, &bytes[vab_off..]),
                None,
            )
        });
        self.set_shared_region_base(legaia_engine_audio::spu_layout::shared_region_base(
            &staged.slot0,
        ));
        self.sfx_vabs.insert(0, staged.slot0);
        self.resident_slot0 = Some(bytes);
        true
    }

    /// Whether a track bank currently holds the SFX region (the resident
    /// SFX banks are dropped until it lets go).
    pub fn sfx_evicted(&self) -> bool {
        self.sfx_evicted
    }

    /// Keep the resident SFX banks consistent with the BGM bank just
    /// staged. A bank reaching into the SFX region (the ending theme's)
    /// overwrote them: drop every one, so no cue keys a stale address. A
    /// bank that fits the BGM region after such a track hands the region
    /// back: re-stage slot 0 from the kept bytes and re-arm the shared
    /// region, which the next [`Self::sync_shared_region`] refills for the
    /// current mode.
    pub(super) fn reclaim_sfx_region(&mut self) {
        use legaia_engine_audio::spu_layout::sfx_region_free;
        if !sfx_region_free(self.bank.as_ref()) {
            self.sfx_evicted = true;
            self.sfx_vabs.clear();
            self.tail.clear();
            self.shared_region = None;
            return;
        }
        if !self.sfx_evicted {
            return;
        }
        self.sfx_evicted = false;
        self.shared_region = None;
        if let Some(bytes) = self.resident_slot0.take()
            && !self.stage_resident_slot0(bytes)
        {
            log::warn!("slot-0 system bank did not re-stage after the SFX region was freed");
        }
    }
}

/// Which VAB slot a cue resolves to, given the installed cue -> slot routing
/// and the set of slots that actually staged. `None` means the cue is silent.
///
/// A routed cue keys the slot its `+4` category names **or nothing**: retail's
/// drainer `FUN_80016B6C` skips a cue whose mixer record's `+0xB` enable byte
/// is zero (`0x80016CE4..0x80016CEC`), and a closed slot has it zero - so a
/// category-6 cue in battle (slot 6 closed by `FUN_8001DCF8`) or a category-2
/// cue in the field (slot 2 closed by `FUN_801D6704`) is silent there, not
/// rerouted to whichever bank is open. The hosts restage the slot-2 / slot-6
/// region per mode ([`AudioDirector::sync_shared_region`]), so each
/// category's own bank is resident exactly where retail's is.
///
/// Only an id the routing does not carry (no descriptor table installed, or an
/// id past it) still falls back to [`FALLBACK_VAB_SLOT`] when that is staged.
///
/// Free function rather than a method so it is testable without a cpal device
/// (an [`AudioDirector`] needs a live audio output).
pub(crate) fn resolve_sfx_slot<T>(
    cue_slots: &BTreeMap<u8, u8>,
    staged: &BTreeMap<u8, T>,
    id: u8,
) -> Option<u8> {
    let slot = cue_slots.get(&id).copied().unwrap_or(FALLBACK_VAB_SLOT);
    staged.contains_key(&slot).then_some(slot)
}
