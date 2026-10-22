# legaia-engine-session

The game session both play hosts run, free of wgpu and winit so it builds
for native and `wasm32` alike. Audio reaches it through `engine-audio`, which
links cpal on native targets only (`BootSession::open` / `open_disc` are built
over its `AudioOut`); a `wasm32` build never links cpal.

- [`BootSession`](src/session.rs) / `BootConfig` - the scene host plus the
  per-frame order around its tick: the naming prompt, the mode seat, the
  pause menu, the camera's halves before and after the world tick, BGM and
  SFX routing, the world frame tail. `BootSession::open` / `open_disc` build
  one over the native cpal output from an extracted tree or a disc image;
  `BootSession::from_host` builds one over any `SceneHost` and the disc's
  `SCUS_942.54` bytes, for a host that assembled its scene host in memory.
- [`front_end`](src/front_end.rs) - the boot front end both play hosts and
  the soak harness step (`FrontEnd`): the publisher logos and their skip,
  the mode seat's `boot_handoff` to the title with the Continue scan at that
  moment, the title tick and its row counter, and the attract hand-off with
  its skip edge. A host supplies its movie lane (`AttractLane`) and acts on
  the `FrontEndStep` it gets back.
- [`title_continue`](src/title_continue.rs) - the title's Continue screen
  both play hosts and the soak harness step (`ContinueScreen`): the
  save-select, its `SaveScreenFlow` and the frame order over two
  `MountedCard` ports, plus the rack, port read and Continue scan
  (`card_port_rack`, `card_port_blocks`, `rack_has_save`). A host supplies
  the ports and lands the `ContinueStep::Loaded` save.
- [`AudioDirector`](src/audio_director.rs) - the `legaia_engine_scene_host::scene_host::BgmDirector`
  that turns field-VM BGM events into sequencers and fires SFX cues, VAB
  slot residency and the battle duck, generic over the audio output
  (`legaia_engine_audio::AudioSink`). It keeps a record of every command a
  host's routing hands it (`AudioCommandLog`), and `queue_ui_cue` is the one
  routing for a screen's blip.
- [`scene_actors`](src/scene_actors.rs) - the scene-entry step both hosts run
  before building meshes: the lead's locomotion clips (`bind_lead_rig`, again
  on a rig change), the placed NPCs' clip players and world cursors, the floor
  snap. A host's mesh upload only reads what it returns.
- [`party_spec`](src/party_spec.rs) - the present-party composition a host
  installs on request (the native `--party` flag, the browser page's
  `set_active_party_spec`): the spec parser and the install with its battle
  stat fold and MP ceiling.
- [`battle_stage`](src/battle_stage.rs) - the battle edge's stage resolve
  and the world state it leaves (stage class, fog stage cap, monster texture
  slots), and the summon creature's seat; both hosts' battle builds read the
  result.
- [`battle_presentation`](src/battle_presentation.rs) - the per-tick battle
  presentation step (`BootSession::step_battle_presentation`: battle audio,
  strike popups, the per-hit drain, effect-spawn routing, the HUD row and
  status fold) and the `Field <-> Battle` edge latch with its two edge steps;
  each host keeps its render build and its log.
- [`battle_intro`](src/battle_intro.rs) - the field-to-battle intro emitter's
  host step: arm it off the formation's style inputs, step it to the
  transition entity's clock, drop it when the transition ends. Each host
  keeps only its frame capture.
- [`battle_fx`](src/battle_fx.rs) - the battle presentation decisions both
  hosts take in their per-tick effect step: `spawn_pending_move_fx` seats a
  pending move effect and resolves its sound cue through the retail dispatch
  decode.
- [`battle_vram`](src/battle_vram.rs) - the battle VRAM's per-tick
  re-stamps both hosts run (`stamp_battle_faces`, `step_battle_vram_edits`):
  the party's facial animation, the status CLUT recolour, the effect CLUT
  stages and the stage module's queued `MoveImage`s. Each host keeps only
  its upload.
- [`xa_banks`](src/xa_banks.rs) - the CD-XA voice banks' staging: which
  files the arts shouts and the battle one-shot clips come from, and how a
  file's demuxed channels become bank entries. The native boot reads the
  sectors off the disc image, the play page slices them out of its disc
  bytes; both stage through `install_shout_file` / `install_clip_file`.

Both types are generic over the audio output. The native instantiations
are named once, in [`native`](src/native.rs): `NativeSession` =
`BootSession<AudioOut>` and `NativeAudioDirector` = `AudioDirector<AudioOut>`;
`open_disc_detached_audio` opens one over a device-less `AudioOut::detached`,
for a headless driver that compares audio routing.

## Host-neutral session, native policy

Everything above is host-neutral. What only a native host can do - open a
session from a path on disk, read the CD-XA banks off a disc image, read one
cast-voice channel from the image the first time a cast names it
(`XaDiscSource`, the native `XaChannelSource`) - is the
[`native`](src/native.rs) module, compiled for native targets only. The
browser host builds its session with `from_host` and stages the same banks
through `xa_banks`.

[`mode_trace`](src/mode_trace.rs) holds `ModeTraceFrame`, one sample of a
session's dispatch state: what a replay file's expected frames and the
mode-trace oracle's retail samples are compared against.

[`host_trace`](src/host_trace.rs) holds `HostTraceFrame`, one host-agnostic
sample of a session's simulation (plus, through `sample_with_overlay`, the
menu-overlay screen a host holds beside it), and `first_divergences`, the
per-channel compare. The frame-body comparison of the two play hosts
(`engine-shell/tests/host_frame_parity.rs`) is built on it.

[`hotkeys`](src/hotkeys.rs) is the one table of host hotkeys (the option,
cheat and camera keys outside the pad vocabulary) and the options edit each
makes, which the native window's keyboard handler and the browser page's
`play_host_hotkey` both resolve through. `BootSession::apply_option_knobs`
pushes the persisted options onto the world and the camera; both play hosts
run it ahead of every session tick.

`BootSession::claim_tick` (`session/tick_owner.rs`) is the one order both play
hosts hand each sim tick through: a host-held screen, the naming prompt, the
prologue skip, the pause menu's spawn and press, a menu-overlay screen that
suspends the field, the field. It runs the parts the session can do itself
and returns a `TickOwner` the host matches on.

Both play hosts run it. The browser play page holds a `BootSession` over its
WebAudio output (`crates/web-play/src/host_slot.rs`), built with
`from_host` over the scene host it assembles in memory, and its `tick_frame`
calls `BootSession::tick`. A host declares what it does itself:
`set_host_drains_queues` (the per-tick presentation queues) and
`set_host_owns_pause_menu` (the host drives `field_menu`'s sub-screens;
it still opens and closes the session's own `field_menu` through
`press_field_menu` / `close_field_menu`, the rule `tick` uses); an empty
`BootConfig::scene` boots no scene, and `camera_azimuth_override` hands the
next tick a host yaw. The per-tick audio routing is the director's own
(`route_world_sfx`, `enqueue_battle_cues`, `tick_audio_frame`), reached
through the session's frame steps (`session/frame_steps.rs`:
`route_battle_audio`, `poll_field_overlay`, `step_dance_tail`,
`sound_muscle_hub`, `sound_baka_announcer`, `step_footstep_cadence`,
`step_audio_frame`), which both play hosts call.

CD-XA voice has one request path, `AudioDirector::play_xa_clip`, and two ways
to stage a clip the bank lacks: a synchronous `XaChannelSource` (the native
disc image) or, given the files' extents (`set_xa_file_extents`), a deferred
`XaStageRequest` the host serves with `install_xa_span` (the browser page,
whose disc bytes live on the JavaScript side).
