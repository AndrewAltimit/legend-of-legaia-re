-- probe/sm.lua  -- WAIT_BOOT -> ARMED_LOADED -> DONE state-machine driver.
--
-- Every probe under scripts/pcsx-redux/autorun_*.lua runs the same
-- three-state machine around the same handful of helpers (memory,
-- save-state, pad, breakpoints, snapshot). This module is the
-- driver; it composes the sister submodules.
--
-- Options table for `run`:
--   sstate          (string)  path to save-state file (gzipped)
--   capture_frames  (int)     vsyncs to capture after load (default 600)
--   boot_delay      (int)     vsyncs to wait before load (default 60)
--   snapshot_every  (int)     vsyncs between live snapshots (default 60)
--   snapshot_path   (string)  optional snapshot file (auto-rotated each tick)
--   quit_delay      (int)     vsyncs after disarm before quit (default 30)
--
--   hold_button     (int|nil) pad button to hold while capturing
--   hold_frames     (int)     vsyncs to keep the button held
--
--   on_arm(ctx) -> descs      arm breakpoints; return descriptor list
--   on_capture(ctx, vsync_in_capture)  optional per-vsync hook
--   on_done(ctx, descs)       optional final-output writer (before quit)
--   on_summary(ctx, descs)    optional human-readable summary writer
--                             (defaults to PCSX.log(...) per descriptor)
--
-- The probe may set `ctx.request_quit = true` from any hook (on_arm,
-- on_capture, or a breakpoint callback) to end the capture loop early
-- without waiting for capture_frames.

local sstate   = require("probe.sstate")
local pad      = require("probe.pad")
local bp       = require("probe.bp")
local snapshot = require("probe.snapshot")

local M = {}

function M.run(opts)
    local sstate_path    = assert(opts.sstate, "probe.run: opts.sstate required")
    local capture_frames = opts.capture_frames or 600
    local boot_delay     = opts.boot_delay or 60
    local snapshot_every = opts.snapshot_every or 60
    local quit_delay     = opts.quit_delay or 30
    local on_arm         = assert(opts.on_arm, "probe.run: opts.on_arm required")
    local on_capture     = opts.on_capture
    local on_done        = opts.on_done
    local on_summary     = opts.on_summary

    local ctx = {
        sstate         = sstate_path,
        capture_frames = capture_frames,
        snapshot_path  = opts.snapshot_path,
        descs          = nil,
    }

    PCSX.log(string.format(
        "[probe] sstate=%s frames=%d snapshot=%s",
        sstate_path, capture_frames, tostring(opts.snapshot_path)))

    local STATE_WAIT_BOOT    = 1
    local STATE_ARMED_LOADED = 2
    local STATE_DONE         = 3

    local state         = STATE_WAIT_BOOT
    local vsync_count   = 0
    local capture_start = nil
    local pad_held      = false

    local function on_vsync()
        vsync_count = vsync_count + 1

        if state == STATE_WAIT_BOOT then
            if vsync_count >= boot_delay then
                ctx.descs = on_arm(ctx) or {}
                if not sstate.load(sstate_path) then
                    PCSX.quit(2)
                    return
                end
                PCSX.log(string.format(
                    "[probe] %d probes armed; capture started", #ctx.descs))
                capture_start = vsync_count
                state         = STATE_ARMED_LOADED

                if opts.hold_button and (opts.hold_frames or 0) > 0 then
                    pad.force(opts.hold_button)
                    pad_held = true
                    PCSX.log(string.format(
                        "[probe] holding pad button %d for %d vsyncs",
                        opts.hold_button, opts.hold_frames))
                end
            end
        elseif state == STATE_ARMED_LOADED then
            local elapsed = vsync_count - capture_start

            if pad_held and elapsed >= (opts.hold_frames or 0) then
                pad.release(opts.hold_button)
                pad_held = false
                PCSX.log(string.format("[probe] released pad button %d at vsync %d",
                    opts.hold_button, elapsed))
            end

            if on_capture then on_capture(ctx, elapsed) end

            if ctx.snapshot_path and (vsync_count % snapshot_every) == 0 then
                snapshot.write(ctx.snapshot_path, "live", ctx.descs,
                    { string.format("vsync=%d capture_start=%d",
                        vsync_count, capture_start) })
            end

            -- Early-quit signal. Probes set `ctx.request_quit = true`
            -- when their stop condition is met (e.g. every probe has
            -- hit at least once); the driver exits the capture loop on
            -- the next vsync rather than waiting for capture_frames.
            if ctx.request_quit then
                PCSX.log("[probe] ctx.request_quit set; ending capture")
                elapsed = capture_frames
            end

            if elapsed >= capture_frames then
                if pad_held then
                    pad.release(opts.hold_button)
                    pad_held = false
                end
                bp.disarm()
                if ctx.snapshot_path then
                    snapshot.write(ctx.snapshot_path, "final", ctx.descs,
                        { string.format("vsync=%d capture_frames=%d",
                            vsync_count, capture_frames) })
                end
                if on_summary then
                    on_summary(ctx, ctx.descs)
                else
                    PCSX.log("=== probe hits ===")
                    for _, d in ipairs(ctx.descs or {}) do
                        local hits = (d.hits_ref and d.hits_ref.n) or d.hits or 0
                        PCSX.log(string.format("  0x%08X  %10d  %s",
                            d.addr, hits, d.name or ""))
                    end
                    PCSX.log("=== end ===")
                end
                if on_done then on_done(ctx, ctx.descs) end
                state = STATE_DONE
                PCSX.log(string.format(
                    "[probe] capture done; quitting in %d vsyncs", quit_delay))
            end
        elseif state == STATE_DONE then
            if vsync_count - capture_start >= capture_frames + quit_delay then
                PCSX.quit(0)
            end
        end
    end

    PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
    PCSX.log("[probe] vsync listener installed; waiting for boot")
end

return M
