//! STR movie playback state: the FMV mode's dispatch and frame pacing
//! ([`cutscene`]), the MDEC DMA sync kernel, the movie's XA audio gate, and
//! the catalogue of retail code paths that trigger a movie.

pub mod cutscene;
pub mod cutscene_trigger;
pub mod mdec_dma_sync;
pub mod movie_audio;
