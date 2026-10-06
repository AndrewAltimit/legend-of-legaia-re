# legaia-engine-session

The game session both play hosts run, free of wgpu, winit and cpal so it
builds for native and `wasm32` alike.

- [`BootSession`](src/boot.rs) / `BootConfig` - the scene host plus the
  per-frame order around its tick: the naming prompt, the mode seat, the
  pause menu, the camera's halves before and after the world tick, BGM and
  SFX routing, the world frame tail. `BootSession::open` / `open_disc` build
  one over the native cpal output from an extracted tree or a disc image;
  `BootSession::from_host` builds one over any `SceneHost` and the disc's
  `SCUS_942.54` bytes, for a host that assembled its scene host in memory.
- [`AudioBgmDirector`](src/bgm.rs) - the `legaia_engine_core::scene::BgmDirector`
  that turns field-VM BGM events into sequencers and fires SFX cues, VAB
  slot residency and the battle duck, generic over the audio output
  (`legaia_engine_audio::AudioSink`).

Both types are generic over the audio output. `engine-shell` names the
native instantiations (`legaia_engine_shell::BootSession` =
`BootSession<AudioOut>`).

The browser play page holds an `AudioBgmDirector` over its WebAudio output
(`crates/web-viewer/src/play_sfx.rs`), and the per-tick audio routing both
hosts run is the director's own (`route_world_sfx`, `enqueue_battle_cues`,
`tick_audio_frame`). It does not hold a `BootSession` yet: it still runs its
own frame order (`crates/web-viewer/src/runtime.rs`). Moving it onto this
crate is what retires the frame-order tiers of
[`docs/tooling/host-drift.md`](../../docs/tooling/host-drift.md).
