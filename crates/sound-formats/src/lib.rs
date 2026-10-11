//! The sound containers a PROT entry can be: the `bse.dat` battle SFX
//! descriptor bank, the `monster.snd` multi-bank VAB archive, the
//! `sound_data2` VAB + SEQ pack and the VAB-prefixed scene stream.
//!
//! These find and bound the banks; decoding a VAB or a SEQ is `legaia-vab` /
//! `legaia-seq`. Sits on `legaia-container-formats` for the DATA_FIELD chunk
//! walk and names no scene, UI, minigame or battle-model format.

#![forbid(unsafe_code)]

pub mod bse_bank;
pub mod scene_vab_stream;
pub mod sound_pack;
pub mod vab_multi_bank;
