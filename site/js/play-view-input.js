/* play-view-input.js - PlayView input and modal sessions.
 *
 * Part of the PlayView class (play-app.js): the keyboard / gamepad listeners
 * and the held-key pad word, menu sound cues, the fishing toggle, and the
 * per-tick drivers of the pause menu, name entry and field shop.
 *
 * Classic script. Requires play-view-constants.js and play-app.js first. */
(function () {
  'use strict';

  const {
    padTable, padMaskOf,
  } = window.LegaiaPlayShared;

  window.LegaiaUtil.installMethods(window.PlayView, class {

    /* ---------- input ---------- */

    _attachInput() {
      const onKey = (e, down) => {
        if (!this.canvas.matches(':focus-within') && document.activeElement !== this.canvas) return;
        /* Key Config (Select on the pause menu's Options screen) needs the
         * PHYSICAL key, not the pad bit it currently carries: the whole point
         * is binding a key that is not in the table yet, and the guard below
         * drops exactly those. `repeat` is skipped so holding the confirm key
         * does not bind it to the highlighted row the instant the screen
         * opens - the native window collapses the same stream in
         * `handle_key`. */
        if (down && !e.repeat && typeof this.rt.play_menu_key === 'function'
            && this.rt.play_menu_key(e.code)) {
          e.preventDefault();
        }
        const table = padTable();
        if (!table || table[e.code] === undefined) return;
        if (window.legaiaPadSwallows(e.code)) e.preventDefault();
        /* An OS auto-repeat `keydown` is not a press: the key is already in
         * `held`. Letting it into `pulse` turned every held key into a stream
         * of fresh edges at the OS repeat rate for every consumer that reads
         * `pulse` as "just pressed" - the pause menu, shop, naming prompt,
         * game-over panel - so holding a direction scrolled a cursor here and
         * not in the native window, whose menu edges are `pad & !prev_pad`
         * and never repeat. */
        if (down) { this.held.add(e.code); if (!e.repeat) this.pulse.add(e.code); }
        else this.held.delete(e.code);
        this._repack();
      };
      this._onDown = (e) => onKey(e, true);
      this._onUp = (e) => onKey(e, false);
      window.addEventListener('keydown', this._onDown);
      window.addEventListener('keyup', this._onUp);
      /* The host hotkeys (`F1`..`F9`, `I`, `Y`, `T`, `R`): the native
       * window's option / cheat / camera keys, resolved by the engine's one
       * table (`legaia_engine_session::hotkeys`) through `play_host_hotkey`,
       * which also runs the options edit those keys make. What comes back is
       * the hotkey's id; the page's own half (a checkbox to mirror, the HUD
       * bar) is `window.__playHotkeyActions[id]`, registered by play.html.
       * None of these keys is a pad button, so none can contradict a
       * rebindable binding. */
      this._onHotkey = (e) => {
        if (e.repeat) return;
        if (!this.canvas.matches(':focus-within') && document.activeElement !== this.canvas) return;
        if (typeof this.rt.play_host_hotkey !== 'function') return;
        let id = '';
        try { id = this.rt.play_host_hotkey(e.code); } catch (_) { return; }
        if (!id) return;
        e.preventDefault();
        if (id === 'debug_camera') {
          /* `F3`: the debug orbit vantage (`PlayWindowApp::field_debug_camera`
           * natively). Off is the default on both hosts, which is the ENGINE
           * camera - the retail follow view and the scripted shots. No
           * hand-off on the way out: both hosts compose the debug vantage as
           * `fixed diagonal + Camera::manual_orbit`, and the drag below
           * steers that one field in either mode, so the two vantages never
           * drift apart. */
          this.debugCamera = !this.debugCamera;
          return;
        }
        const act = window.__playHotkeyActions && window.__playHotkeyActions[id];
        if (typeof act === 'function') act();
      };
      window.addEventListener('keydown', this._onHotkey);
      /* Blur drops every held key - otherwise tabbing away mid-walk leaves the
       * player marching into a wall forever. */
      this._onBlur = () => {
        this.held.clear(); this.pulse.clear(); this.pad = 0;
        if (window.legaiaResetGamepad) window.legaiaResetGamepad();
      };
      window.addEventListener('blur', this._onBlur);
      this.canvas.addEventListener('blur', this._onBlur);

      /* Camera: drag to swing around and tilt over the player, wheel to zoom.
       * All three steer the ENGINE's follow camera (`Camera::manual_orbit` /
       * `manual_tilt` / `manual_zoom`), which stays centred on the character
       * and composes the knobs onto the retail per-scene shot; the engine
       * is told the new azimuth each frame, so "up" always walks away from
       * the camera - turning the camera turns the controls with it. The
       * setters are cutscene-gated engine-side: while a scripted shot owns
       * the camera the gesture is dropped, so the view stays where the
       * script put it and never snaps when control returns. Under the `F3`
       * debug orbit the same gestures steer the page's own vantage instead. */
      let dragging = false, lastX = 0, lastY = 0;
      const engineKnobs = () => !this.debugCamera
        && typeof this.rt.play_camera_orbit_by === 'function';
      this.canvas.addEventListener('pointerdown', (e) => {
        dragging = true; lastX = e.clientX; lastY = e.clientY;
        this.canvas.focus();
        this.canvas.setPointerCapture(e.pointerId);
        /* Autoplay backstop: a click on the canvas is a user gesture, so init
         * the BGM output here too (the scene-pick click already tries) and
         * always resume - browsers open the AudioContext suspended. Scene BGM
         * then feeds itself through the engine's per-tick op-0x35 routing. */
        this._enableAudio();
      });
      this.canvas.addEventListener('pointerup', (e) => {
        dragging = false;
        try { this.canvas.releasePointerCapture(e.pointerId); } catch (_) {}
      });
      this.canvas.addEventListener('pointermove', (e) => {
        if (!dragging) return;
        /* Horizontal drag swings `Camera::manual_orbit` - the SAME engine
         * field the native window's left-mouse drag writes, so the retail
         * follow camera and the movement compass both track it - and
         * vertical drag tips `Camera::manual_tilt`, at the rates the native
         * window's `window::camera::debug_orbit` quotes. The local yaw/pitch
         * below steer the page's debug-orbit vantage (`F3`), the one camera
         * this page still owns, and are kept in step so toggling it does
         * not snap the view. */
        const dx = (e.clientX - lastX) * 0.006;
        const dy = (e.clientY - lastY) * 0.004;
        this.cam.yaw += dx;
        this.cam.pitch = Math.max(0.12, Math.min(1.35, this.cam.pitch + dy));
        if (engineKnobs()) {
          try { this.rt.play_camera_orbit_by(dx); this.rt.play_camera_tilt_by(dy); } catch (_) {}
        } else if (this.debugCamera
                   && typeof this.rt.play_camera_debug_orbit_by === 'function') {
          /* Under `F3` the drag steers the SAME `Camera::manual_orbit` the
           * follow camera reads, through the un-gated engine setter the
           * native window's own `F3` drag calls
           * (`Camera::debug_orbit_by`) - the page's `cam.yaw` above is just
           * its local copy of that angle. */
          try { this.rt.play_camera_debug_orbit_by(dx); } catch (_) {}
        } else if (!this.debugCamera && typeof this.rt.play_camera_set_orbit === 'function') {
          /* A cached WASM that predates the gated setters: orbit only. */
          try { this.rt.play_camera_set_orbit(this.rt.play_camera_orbit() + dx); } catch (_) {}
        }
        lastX = e.clientX; lastY = e.clientY;
      });
      this.canvas.addEventListener('wheel', (e) => {
        e.preventDefault();
        /* A notch out pulls the eye back, a notch in brings it closer, by
         * the same factors on both hosts. On the engine camera this is a
         * continuous multiplier on the retail eye-back depth
         * (`Camera::manual_zoom`); on the debug orbit it is the page's own
         * framing half-window. */
        const f = e.deltaY > 0 ? 1.12 : 0.89;
        if (engineKnobs() && typeof this.rt.play_camera_zoom_by === 'function') {
          try { this.rt.play_camera_zoom_by(f); } catch (_) {}
          return;
        }
        this.cam.halfWidth = Math.max(220, Math.min(6000, this.cam.halfWidth * f));
        this.cam.halfHeight = this.cam.halfWidth;
      }, { passive: false });
      /* Double-click puts the follow camera back at the retail framing
       * (orbit, tilt and zoom to their identity defaults). */
      this.canvas.addEventListener('dblclick', (e) => {
        e.preventDefault();
        if (!this.debugCamera && typeof this.rt.play_camera_reset_framing === 'function') {
          try { this.rt.play_camera_reset_framing(); } catch (_) {}
        }
      });
    }

    /* Held keys OR the not-yet-consumed press edges -> one pad word. */
    _repack() {
      this.pad = padMaskOf(this.held) | padMaskOf(this.pulse);
    }

    /* Held-key state, for the on-screen control legend. */
    heldKeys() { return Array.from(this.held); }

    /* ---------- sound effects ---------- */

    /* Fire the cue mapped to a named event. The event -> cue-id map and each
     * id's provenance (`disc` = traced to a retail ring write, `site` = the
     * port's pick) live in the engine (`play_sfx_events_json`), so the page
     * names behaviour and never a cue number. Silent no-op until the disc's
     * descriptor table and program bank have staged, and against a cached WASM
     * that predates the channel. */
    sfxEvent(name) {
      if (typeof this.rt.play_sfx_event !== 'function') return false;
      try { return !!this.rt.play_sfx_event(name); } catch (e) { return false; }
    }

    /* The pause menu's blip for this frame's pad edges. Which cue fires - if
     * any - is the engine's one rule (`play_menu_edge_blip`, engine-core
     * `menu_cues::menu_edge_blip`), the same function the native window
     * asks; the page only says whether this frame's Start closed the menu.
     * Silent against a cached WASM that predates the export. */
    menuBlip(edge, startCloses) {
      if (typeof this.rt.play_menu_edge_blip !== 'function') return false;
      try { return !!this.rt.play_menu_edge_blip(edge, !!startCloses); } catch (e) { return false; }
    }

    /* The SFX channel's state (`{descriptors, bank_prot, vab_staged, fired,
     * last_cue, idle_voices, pending}`), or `null`. */
    sfxState() {
      if (typeof this.rt.play_sfx_state_json !== 'function') return null;
      try { return JSON.parse(this.rt.play_sfx_state_json()); }
      catch (e) { return null; }
    }

    /* ---------- fishing minigame ---------- */

    /* Is a fishing session live on the engine's world this frame? */
    fishingActive() {
      if (typeof this.rt.play_fishing_active !== 'function') return false;
      try { return !!this.rt.play_fishing_active(); } catch (e) { return false; }
    }

    /* Start / leave the fishing minigame. The engine suspends the current
     * scene mode and restores it on exit, banking the session's points into the
     * world's persistent pool - so the field is exactly where it was left and
     * the point total survives. Returns `true` on a state change. */
    toggleFishing() {
      const rt = this.rt;
      if (typeof rt.play_fishing_start !== 'function') return false;
      try {
        if (this.fishingActive()) {
          rt.play_fishing_stop();
          /* Drop the HUD immediately rather than waiting for the next layer to
           * claim the overlay. */
          if (this._menuCtx && this.menuOverlay) {
            this._menuCtx.clearRect(0, 0, this.menuOverlay.width, this.menuOverlay.height);
            this._overlayActive = false;
          }
          return true;
        }
        return !!rt.play_fishing_start();
      } catch (e) { return false; }
    }

    /* The live session's readout (`{live, phase, cast_power, tension, ...}`), or
     * `{live:false}`. */
    fishingState() {
      if (typeof this.rt.play_fishing_state_json !== 'function') return { live: false };
      try { return JSON.parse(this.rt.play_fishing_state_json()); }
      catch (e) { return { live: false }; }
    }

    /* ---------- retail pause menu (engine-driven) ---------- */

    /* Drive the retail pause menu from this frame's just-pressed edges (the
     * `pulse` set, read before the engine tick consumes it). The state machine
     * + navigation live in the engine (`play_menu_input`); this only opens it on
     * Start, closes it on Start, and forwards the remaining edges. Returns `true`
     * while the menu is up, so `_frame` freezes the field - retail holds the
     * world in SceneMode::Menu. */
    _updateFieldMenu(simSteps) {
      const rt = this.rt;
      const p = this.pulse;
      /* The per-tick owner (`play_menu_tick`, the native window's `sim_tick`
       * arms): the press, the wipe and the menu steps all run inside the
       * frame's tick loop, one owner per sim tick. This frame-level step
       * only answers whether a menu is up - which keeps the naming prompt
       * and the shop from stepping under it - and does the bookkeeping that
       * follows a menu step, which `_afterMenuTicks` runs after the loop. */
      if (typeof rt.play_menu_tick === 'function') {
        try { return rt.play_menu_is_open(); } catch (e) { return false; }
      }
      /* Test the Start *bit*, not a key name: Start answers to Enter and
       * Space, and a rebinding moves it again. Keying off the literal made
       * this handler the one place the engine's table did not reach. */
      const startEdge = (padMaskOf(p) & window.legaiaPadButton('Start')) !== 0;
      let open;
      try { open = rt.play_menu_is_open(); } catch (e) { return false; }
      if (!open) {
        /* A script's op-0x49 save point / ready check presses the menu
         * button itself (`play_menu_scripted_open_pending`): no Start edge,
         * no page-side gate, and no confirm blip. */
        let scripted = false;
        try { scripted = rt.play_menu_scripted_open_pending(); } catch (e) {}
        if (scripted || startEdge) {
          try { rt.play_menu_open(); } catch (e) { return false; }
          /* The engine decides - `play_menu_open` asks
           * `World::field_menu_open_allowed`, the rule the native window asks,
           * and on a press the menu lock refuses it queues the deny buzz
           * itself. Take none of the follow-up on a refusal: no confirm blip,
           * no swallowed pad edge, no menu clock. */
          let opened = false;
          try { opened = rt.play_menu_is_open(); } catch (e) {}
          if (!opened) return false;
          if (!scripted) this.sfxEvent('menu_confirm');
          this._ensureMenuBlitters();
          p.clear();
          this._repack();
          return rt.play_menu_is_open();
        }
        return false;
      }
      /* Menu up: every edge, Start included, goes to the engine's menu step.
       * What Start does is the shared picker's rule
       * (`FieldMenuSession::tick`, via `tick_root_list`): on the root list it
       * closes the menu, on the save point notice it does nothing, and under
       * an op-0x49 kind-0x0D ready check it opens the Yes / No confirm
       * instead of closing. Inside a sub-screen it is the sub-screen's own
       * edge. The page used to close the menu itself on any root-level
       * Start, which skipped all three rules and released a parked ready
       * check as answered - the native window has always handed Start to
       * the kernel. Guarded so a cached WASM without the export keeps a
       * sensible blip rather than trapping. */
      let inSubScreen = false;
      try {
        inSubScreen = typeof rt.play_menu_sub_is_open === 'function'
          && rt.play_menu_sub_is_open();
      } catch (e) {}
      /* A frame that carries no sim tick steps nothing and keeps its edges
       * for the next one that does (the shop's rule, `_updateFieldShop`). */
      if (!(simSteps > 0)) return true;
      {
        let edge = 0;
        edge |= padMaskOf(p);
        /* Cue the engine's own blip off this frame's edges. Which edge fires
         * which cue (a direction a cursor move, Cross a confirm, Circle a
         * cancel; Start a cancel on the root list, nothing inside a
         * sub-screen) is the engine's rule, and so are the ids - the page
         * never spells either. `!inSubScreen` is the native window's
         * `start_closes_menu = sub.is_none()`. The engine counts every request
         * (`menu_cue_requests` in `play_sfx_state_json`), so the wiring stays
         * measurable. See `play_sfx::CUE_MENU_CURSOR` for the one inexactness
         * left, which is a bank choice rather than a pitch. */
        /* `play_menu_input` fires it itself now, after the save screen's
         * refusal box and block grid have filtered the edge (the native
         * window's order); a cached WASM without `play_menu_blips_inline`
         * still gets the page-side blip. */
        let inlineBlip = false;
        try { inlineBlip = typeof rt.play_menu_blips_inline === 'function' && rt.play_menu_blips_inline(); } catch (e) {}
        if (edge && !inlineBlip) this.menuBlip(edge, !inSubScreen);
        /* One menu step per sim tick, from the engine's frame stepper
         * (`play_drain_sim_steps`, the `SimStepper` the native window's
         * redraw drains): the save screen's "Now checking" beat and every
         * slide-in are counted in retail frames. The page used to keep a
         * menu clock of its own that ran at least one step every display
         * frame - twice retail speed at 120 Hz - and caught up to eight. The
         * edge is delivered on the first tick only, so one keypress can
         * never register twice. */
        for (let i = 0; i < simSteps; i++) {
          try { rt.play_menu_input(i === 0 ? edge : 0); } catch (e) {}
        }
        this._afterMenuTicks();
      }
      /* The menu owns every edge while it is up - clear them so none leak into
       * the frozen field on the next tick. */
      p.clear();
      this._repack();
      try { return rt.play_menu_is_open(); } catch (e) { return false; }
    }

    /* What follows a frame's menu steps, on either path: a committed key
     * rebind and an in-canvas card Load. */
    _afterMenuTicks() {
      const rt = this.rt;
    /* A rebind committed inside Options > Key Config. The page folded the
     * engine's table into one JS object at load, so without this the new
     * binding would be live in the engine and invisible here. One integer
     * compare per menu frame. */
    if (window.legaiaSyncPadBindings && window.legaiaSyncPadBindings(rt)) {
      this.held.clear();
      this._repack();
    }
    /* An in-canvas Load off a memory card parks the save in the engine;
     * the page lands it through the engine's resume entry
     * (`play_resume_save`, via `onCardLoad`) the frame it appears. A
     * save naming no scene counts too - it lands on the running scene. */
    let loaded = false;
    try { loaded = rt.play_menu_take_load(); } catch (e) {}
    if (loaded) {
      /* Hand the score from whatever was playing (the title theme, on a
       * load out of the boot chooser) to the save's own op-0x35 track -
       * the native window's `bgm.stop(); restore_field_bgm();` pair. The
       * field VM does not re-emit a start for music that was already
       * playing when the save was written, so without this the title
       * theme simply keeps going under the loaded scene. */
      try { rt.play_bgm_title_handoff(); } catch (e) { /* audio down */ }
    }
    if (loaded && typeof this.opts.onCardLoad === 'function') {
      this.opts.onCardLoad();
    }
    }

    /* Drive the opening name-entry overlay from this frame's just-pressed edges.
     *
     * Like the field shop this has no open/close key of its own: the `town01`
     * establishing timeline's pinned op-0x49 opens it and the SM's own Select ->
     * "Is this name okay?" -> Yes closes it, at which point the engine writes the
     * name into the party record and the suspended opening script resumes. So
     * there is nothing to toggle - just forward edges while it is up.
     *
     * One INPUT step per frame is right: the overlay steps exactly one cell /
     * glyph per press. The caret blink is not - it is frame-counted off
     * `World::frame`, which the native window advances once per SIM tick
     * while the field is frozen under the overlay. Driving it off the display
     * refresh instead made the blink period a property of the monitor (half
     * speed at 120 Hz, double on a throttled tab), so this host spends the
     * frame's `simSteps` on the counter the same way.
     *
     * Returns `true` while the overlay is up, so `_frame` freezes the field -
     * the naming prompt is modal, as it is natively. */
    /* The naming prompt committed: hand the entered name to the page. */
    _nameCommitted() {
      if (typeof this.opts.onNamed !== 'function') return;
      let name = '';
      try { name = this.rt.party_display_name(0); } catch (e) {}
      this.opts.onNamed(name);
    }

    _updateNameEntry(simSteps) {
      const rt = this.rt;
      if (typeof rt.name_entry_is_active !== 'function') return false;
      let open;
      try { open = rt.name_entry_is_active(); } catch (e) { return false; }
      if (!open) return false;
      this._ensureMenuBlitters();
      /* The per-tick owner (`play_name_entry_tick`, the native window's
       * per-tick arm) steps the prompt inside the frame's tick loop, edge
       * and caret clock together; the frame level only stages blitters. */
      if (typeof rt.play_name_entry_tick === 'function') return false;
      let edge = 0;
      edge |= padMaskOf(this.pulse);
      let committed = false;
      /* One edge per display frame - that is the rate the page collects
       * presses at - and the caret clock separately, on this frame's SIM
       * step count, so the blink period is the retail one at any refresh
       * rate. A cached bundle without the split keeps the old coupled call,
       * which advanced both per frame. */
      try { committed = rt.name_entry_input(edge); } catch (e) {}
      if (typeof rt.name_entry_advance_frames === 'function') {
        try { rt.name_entry_advance_frames(simSteps | 0); } catch (e) {}
      }
      if (committed) this._nameCommitted();
      /* The overlay owns every edge while it is up - clear them so none leak
       * into the frozen field on the next tick. */
      this.pulse.clear();
      this._repack();
      try { return rt.name_entry_is_active(); } catch (e) { return false; }
    }

    /* Drive an open field shop from this frame's just-pressed edges.
     *
     * Unlike the pause menu this has no open/close key: the field VM opens it
     * (a merchant's op-0x49 sub-0 record) and the player's **Exit** row closes
     * it, at which point the engine resumes the suspended script. So there is
     * nothing to toggle here - just forward edges while it is up.
     *
     * The shop steps on the SIM clock, one `play_shop_input` per drained sim
     * step - the native window's cadence. Its open fade and window slides are
     * tick-counted (`MenuRuntime::shop_fade_level`), so one call per display
     * frame ran them at the monitor's rate: twice as fast on a 120 Hz panel.
     * The frame's edges ride the first step; a frame that drains no step
     * keeps them for the next one rather than dropping the press.
     *
     * Returns `true` while the shop is up, so `_frame` freezes the field. */
    /* Steps the open menu-runtime screen and answers whether it FREEZES the
     * field this frame. A shop or the prize exchange does
     * (`play_shop_suspends_field`, the engine's `MenuRuntime::suspends_field`);
     * the inn prompt does not - retail runs it as a field-VM dialogue - so
     * under it the field ticks on a neutral pad (`_shopHoldsPad`), as the
     * native window feeds it one while `MenuRuntime::is_open` holds. */
    _updateFieldShop(simSteps) {
      const rt = this.rt;
      this._shopHoldsPad = false;
      if (typeof rt.play_shop_is_open !== 'function') return false;
      let open;
      try { open = rt.play_shop_is_open(); } catch (e) { return false; }
      if (!open) return false;
      this._ensureMenuBlitters();
      /* The per-tick owner (`play_shop_tick` / `play_shop_after_tick`, the
       * native window's `sim_tick` arms) steps the screen inside the frame's
       * tick loop, so the frame-level step only stages its blitters. */
      if (typeof rt.play_shop_tick === 'function') return false;
      const freezes = () => {
        let open2 = false;
        try { open2 = rt.play_shop_is_open(); } catch (e) { return false; }
        if (!open2) return false;
        this._shopHoldsPad = true;
        if (typeof rt.play_shop_suspends_field !== 'function') return true;
        try { return rt.play_shop_suspends_field(); } catch (e) { return true; }
      };
      if (!(simSteps > 0)) return freezes();
      let edge = 0;
      edge |= padMaskOf(this.pulse);
      for (let s = 0; s < simSteps; s++) {
        try { rt.play_shop_input(s === 0 ? edge : 0); } catch (e) { break; }
      }
      /* The shop owns every edge while it is up - clear them so none leak into
       * the frozen field on the next tick. */
      this.pulse.clear();
      this._repack();
      return freezes();
    }

    /* Did the tick just run open a screen that suspends the field? The
     * shop / prize counter (`play_shop_is_open`, the engine's
     * `MenuRuntime::is_open`) or a pending scripted menu press
     * (`play_menu_scripted_open_pending`). Both are engine answers. A
     * screen already up when the frame began (the inn prompt, which the
     * field ticks under) is not a new one. */
    _modalOpenedThisTick() {
      const rt = this.rt;
      /* A screen the tick opened takes the frame's next tick through
       * `play_shop_tick`, inside the loop, as the native window's next
       * `sim_tick` does; only a bundle without that export ends the frame. */
      try {
        if (!this._shopHoldsPad && typeof rt.play_shop_tick !== 'function'
            && typeof rt.play_shop_is_open === 'function'
            && rt.play_shop_is_open()) return true;
      } catch (e) { /* fall through */ }
      /* A scripted menu press is picked up by the next tick's
       * `play_menu_tick`, inside this frame's loop, as the native window's
       * next `sim_tick` picks it up; only a bundle without that export ends
       * the frame here for the frame-level open. */
      try {
        if (typeof rt.play_menu_tick !== 'function'
            && typeof rt.play_menu_scripted_open_pending === 'function'
            && rt.play_menu_scripted_open_pending()) return true;
      } catch (e) { /* fall through */ }
      return false;
    }

    /* Whether Start opens the menu right now.
     *
     * The engine owns the rule and answers it through `play_menu_can_open()`
     * (`World::field_menu_open_allowed`): the scene mode plus retail's
     * engaged-bit refusal. This used to be a page-side `scene_mode() !==
     * 'Field'` test whose comment asserted Start was inert "on the world
     * map" - false, since the overworld runs the same locomotion controller
     * the field does, and a page-side copy of an engine rule is drift no gate
     * can see because no Rust symbol is missing.
     *
     * Nothing page-side is added to it. The page used to refuse on two
     * copies of its own - a dialog box in the last HUD state, and the
     * opening chain / narration beat - and the native window asked neither.
     * The first was the engine's `dialogue_owns_input` read back a frame
     * later; the second is now the engine's (`opening_chain_active`, with
     * the crawl and title card already in the rule), so both hosts answer
     * the same press the same way. */
    _canOpenFieldMenu() {
      try { return !!this.rt.play_menu_can_open(); } catch (e) { return false; }
    }
  });
})();
