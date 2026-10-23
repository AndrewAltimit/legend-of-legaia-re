/* play-view-input.js - PlayView input and modal sessions.
 *
 * Part of the PlayView class (play-app.js): the keyboard / gamepad listeners
 * and the held-key pad word, menu sound cues, the fishing toggle, and the
 * menu blitter staging (the screens tick inside `play_frame_ticks`).
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
        if (!id) {
          /* A field-only key under a front-end screen: say why nothing
           * changed (`refused_notice`), as the native window does. */
          if (typeof this.rt.play_host_hotkey_refusal === 'function') {
            let why = '';
            try { why = this.rt.play_host_hotkey_refusal(e.code); } catch (_) {}
            if (why) { e.preventDefault(); this._hotkeyNotice(why); }
          }
          return;
        }
        e.preventDefault();
        this._afterHotkey(id);
      };
      /* The notice line after a press (`"Ground fog: off"`), worded by the
       * engine's `toggle_notice` - the line the native window shows, so a
       * press of a key that changes nothing else on screen (`F2`, `F7`)
       * does not read as dead. The page-held states (its HUD bar, the debug
       * vantage, the encounter box) come from `__playHotkeyPageState`. */
      this._afterHotkey = (id) => {
        this._runHotkey(id);
        if (typeof this.rt.play_host_hotkey_notice !== 'function') return;
        const own = id === 'debug_camera'
          ? () => (this.debugCamera ? 'on' : 'off')
          : (window.__playHotkeyPageState || {})[id];
        let text = '';
        try { text = this.rt.play_host_hotkey_notice(id, own ? own() : ''); } catch (_) {}
        if (text) this._hotkeyNotice(text);
      };
      this._hotkeyNotice = (text) => {
        if (typeof window.__playShowNotice === 'function') window.__playShowNotice(text);
      };
      this._runHotkey = (id) => {
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
       * on the lead - the same reset as the Reset camera button. Under the
       * `F3` debug orbit it is left alone, as natively. */
      this.canvas.addEventListener('dblclick', (e) => {
        e.preventDefault();
        if (!this.debugCamera) this.resetCamera();
      });
    }

    /* The Reset camera button (and the canvas double-click): back to the
     * retail framing on the current lead. Three pieces of state can hold the
     * view off it, and the button used to touch only the third:
     *  - the ENGINE follow camera's orbit / tilt / zoom knobs and a scripted
     *    shot left holding the focus - `play_camera_reset_framing`, the
     *    shared `Camera::recenter_follow` the native double-click calls;
     *  - the `F3` debug orbit vantage, which frames the scene from the
     *    page's own orbit instead of the engine camera - switched off;
     *  - that vantage's local yaw / pitch / half-window - back to defaults,
     *    so the next `F3` starts from the stock framing.
     * Writing only the local fields was a no-op on the default (engine)
     * camera, which is why the button never recentred anything. */
    resetCamera() {
      this.debugCamera = false;
      this.cam.yaw = 0;
      this.cam.pitch = 0.62;
      this.cam.halfWidth = this.cam.halfHeight = 520;
      if (this._camBeforeBattle) {
        this._camBeforeBattle.yaw = 0;
        this._camBeforeBattle.pitch = 0.62;
        this._camBeforeBattle.halfWidth = this._camBeforeBattle.halfHeight = 520;
      }
      if (this.rt && typeof this.rt.play_camera_reset_framing === 'function') {
        try { this.rt.play_camera_reset_framing(); } catch (_) { /* no scene yet */ }
      }
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

    /* The live session's readout (`{live, phase, cast_power, tension, ...}`), or
     * `{live:false}`. */
    fishingState() {
      if (typeof this.rt.play_fishing_state_json !== 'function') return { live: false };
      try { return JSON.parse(this.rt.play_fishing_state_json()); }
      catch (e) { return { live: false }; }
    }

    /* ---------- retail pause menu (engine-driven) ---------- */

    /* What follows a frame's menu steps: a committed key rebind and an
     * in-canvas card Load. */
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

    /* The naming prompt committed: hand the entered name to the page. */
    _nameCommitted() {
      if (typeof this.opts.onNamed !== 'function') return;
      let name = '';
      try { name = this.rt.party_display_name(0); } catch (e) {}
      this.opts.onNamed(name);
    }

    /* Stage the menu blitters for whichever screen the engine has up (the
     * pause menu, the naming prompt, a menu-overlay screen). The screens
     * themselves take their ticks inside `play_frame_ticks`. */
    _stageScreenBlitters() {
      const rt = this.rt;
      let up = false;
      try {
        up = rt.play_menu_is_open() || rt.name_entry_is_active() || rt.play_shop_is_open();
      } catch (e) { up = false; }
      if (up) this._ensureMenuBlitters();
    }
  });
})();
