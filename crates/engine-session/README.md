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

Both play hosts run it. The browser play page holds a `BootSession` over its
WebAudio output (`crates/web-viewer/src/host_slot.rs`), built with
`from_host` over the scene host it assembles in memory, and its `tick_frame`
calls `BootSession::tick`. A host declares what it does itself:
`set_host_drains_queues` (the per-tick presentation queues),
`set_host_owns_pause_menu` (the host drives `field_menu`'s sub-screens;
it still opens and closes the session's own `field_menu` through
`press_field_menu` / `close_field_menu`, the rule `tick` uses) and
`set_host_stages_field_xa` (the field CD-XA queues, for a host with an
asynchronous XA lane); an empty `BootConfig::scene` boots no scene, and
`camera_azimuth_override` hands the next tick a host yaw. The per-tick audio
routing is the director's own (`route_world_sfx`, `enqueue_battle_cues`,
`tick_audio_frame`).
