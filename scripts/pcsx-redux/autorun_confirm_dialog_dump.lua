-- autorun_confirm_dialog_dump.lua
--
-- Capture ground-truth pixels of the save screen's **confirm dialog**
-- ("Do you wish to load? / save? / overwrite?") so its panel rects can be
-- measured rather than inferred.
--
-- The dialog is slide animator mode 3 of FUN_801E1C1C(mode, t, sx, sy, tx, ty).
-- The capture trigger is a breakpoint on that function's entry testing
-- a0 == 3: that is direct proof the confirm dialog is the thing being drawn
-- this frame, and a1 is its live slide timer. Capture the framebuffer on the
-- first vsync after a mode-3 call with t == 0x1000 (fully parked at the
-- retail rest position).
--
-- Do NOT poll the timer global DAT_801ef1a4 as the trigger instead: it is
-- uninitialised until the confirm sub-screen first runs, so before then it
-- holds stale menu-overlay bytes that happily compare >= 0x1000 and yield a
-- confident capture of the wrong screen. The breakpoint has no such failure
-- mode, and the BP also hands back the real call arguments (start/target),
-- which independently re-pins the mode-3 slide endpoints.
--
-- Tapping must stop the moment the dialog appears - CROSS on the parked
-- prompt answers it (default cursor = Yes) and commits the save/load.
--
-- Getting there: no save state is parked on the save screen (the
-- `save_select_idle` scenario was never captured), so this walks to it from
-- a field state. SELECT opens the pause menu - NOT Start; the menu command
-- list (window 50, FUN_801CFD68) is Items / Magic / Equip / Status /
-- Options / Load / Save, so Save is six DOWNs then CROSS. Then CROSS taps
-- pick a block, which raises the prompt. Override the walk with
-- LEGAIA_PAD_SCRIPT (see autorun_pad_walk.lua for the format) if starting
-- somewhere else.
--
-- Needs the interpreter + debugger (Lua BPs do not fire under --fast), so run
-- this WITHOUT --fast.
--
-- Outputs:
--   <out>/confirm_dialog_fb.{raw,meta}   framebuffer, dialog parked
--   <out>/confirm_dialog.log             BP arg trace + capture decision
--
-- Decode with:
--   scripts/pcsx-redux/decode_load_screen.py <out> --stem confirm_dialog_fb
--
-- Env vars:
--   LEGAIA_SSTATE      sstate path (default sstate9: field, map01)
--   LEGAIA_OUT_DIR     output dir
--   LEGAIA_FRAMES      total post-load capture vsyncs (default 900)
--   LEGAIA_PAD_SCRIPT  entry walk override, `<vsync>:<BUTTON>[:<hold>]` list
--   LEGAIA_TAP_FROM    vsync to start CROSS-tapping for a block (default 300)

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate9")
local OUT_RAW  = probe.out_path("confirm_dialog_fb.raw")
local OUT_META = probe.out_path("confirm_dialog_fb.meta")
local OUT_LOG  = probe.out_path("confirm_dialog.log")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 900)
local TAP_FROM = probe.getenv_num("LEGAIA_TAP_FROM", 300)

-- Field -> pause menu (SELECT) -> six DOWN to the Save row -> CROSS.
local DEFAULT_WALK =
    "40:SELECT,110:DOWN,130:DOWN,150:DOWN,170:DOWN,190:DOWN,210:DOWN,260:CROSS"
local SCRIPT = probe.getenv("LEGAIA_PAD_SCRIPT", DEFAULT_WALK)

local FUN_801E1C1C = 0x801e1c1c
-- The messagebox panel drawer FUN_801E36C4(center_x, y, w, h). Its calls made
-- during a parked mode-3 draw ARE the confirm dialog's panel rects, in the
-- units the renderer actually uses - log them rather than deriving them from
-- the mode-3 slide target, so the measured pixels have something exact to be
-- reconciled against.
local FUN_801E36C4 = 0x801e36c4
local MODE_CONFIRM = 3
local T_PARKED     = 0x1000
-- Vsyncs to let the parked frame reach the display before grabbing it.
local PARKED_SETTLE_VSYNCS = 12

-- After the entry walk, tap CROSS every TAP_PERIOD vsyncs (holding TAP_HOLD)
-- to pick a block, until the prompt appears. The card read ("Now checking")
-- has to finish before the grid accepts input, so this walks the flow rather
-- than assuming its timing.
local TAP_PERIOD = 45
local TAP_HOLD   = 6

local log_lines = {}
local function logf(fmt, ...)
    local s = string.format(fmt, ...)
    log_lines[#log_lines + 1] = s
    PCSX.log("[confirm_dialog] " .. s)
end

local function take_fb(raw_path, meta_path, label)
    local ok, ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
    if not ok or ss == nil then
        logf("%s takeScreenShot unavailable", label)
        return false
    end
    local bpp_bits = (tonumber(ss.bpp) or 0) > 16 and 24 or 16
    local w, h = tonumber(ss.width), tonumber(ss.height)
    local fh = io.open(raw_path, "wb")
    if fh == nil then
        logf("%s cannot open %s", label, raw_path)
        return false
    end
    local s = tostring(ss.data)
    fh:write(s); fh:close()
    local mh = io.open(meta_path, "w")
    if mh ~= nil then
        mh:write(string.format("width=%d\nheight=%d\nbpp=%d\nbytes_per_pixel=%d\n",
            w, h, bpp_bits, bpp_bits / 8))
        mh:close()
    end
    logf("%s fb: %dx%d %dbpp (%d bytes) -> %s", label, w, h, bpp_bits, #s, raw_path)
    return true
end

-- Set by the BP callback: the slide timer seen on the most recent mode-3
-- draw, and whether we have ever seen one at all.
local last_mode3_t = nil
local seen_mode3 = false
local logged_args = false
local captured = false
-- Consecutive vsyncs the parked draw has been observed for (see the
-- double-buffering note at the capture site).
local parked_vsyncs = 0
local tap_until = -1
local next_tap = TAP_FROM
-- mode -> hit count. Logged at the end: if the dialog is never reached this
-- says whether the BP fired at all (wrong address / overlay not resident)
-- or fired only for other modes (the pad walk never got there).
local mode_hits = {}
-- True only between a mode-3 entry with t == 0x1000 and the next slide call.
local in_parked_mode3 = false
-- Slide mode of the most recent FUN_801E1C1C entry; attributes panel calls.
local current_mode = nil
-- Deduped set of FUN_801E36C4 arg tuples seen during parked mode-3 draws.
local panel_calls = {}

-- Entry walk: parse `<vsync>:<BUTTON>[:<hold>]` steps (same grammar as
-- autorun_pad_walk.lua).
local walk = {}
local walk_release = {}
for chunk in string.gmatch(SCRIPT, "[^,]+") do
    local parts = {}
    for p in string.gmatch(chunk, "[^:]+") do parts[#parts + 1] = p end
    local at   = tonumber(parts[1])
    local name = parts[2] and string.upper(parts[2])
    local btn  = name and probe.BTN[name]
    if at ~= nil and btn ~= nil then
        walk[#walk + 1] = { at = at, btn = btn, name = name,
                            hold = tonumber(parts[3] or "6") }
    end
end

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,

    on_arm = function(_)
        logf("sstate=%s frames=%d", SSTATE_PATH, FRAMES)
        probe.arm_breakpoint(FUN_801E1C1C, "Exec", 4, "slide_primitive", function()
            local r = PCSX.getRegisters()
            local n = r.GPR and r.GPR.n
            if n == nil then return end
            local mode = tonumber(n.a0)
            current_mode = mode
            mode_hits[mode] = (mode_hits[mode] or 0) + 1
            if mode ~= MODE_CONFIRM then
                in_parked_mode3 = false
                return
            end
            local t = tonumber(n.a1)
            in_parked_mode3 = (t == T_PARKED)
            last_mode3_t = t
            if not seen_mode3 then
                seen_mode3 = true
                logf("first mode-3 draw: t=0x%x start=(%d,%d) target=(%d,%d)",
                    t, tonumber(n.a2), tonumber(n.a3),
                    tonumber(probe.read_u32(tonumber(r.GPR.n.sp) + 0x10)) or -1,
                    tonumber(probe.read_u32(tonumber(r.GPR.n.sp) + 0x14)) or -1)
            end
            if t == T_PARKED and not logged_args then
                logged_args = true
                logf("mode-3 parked draw observed (t=0x%x)", t)
            end
        end)

        -- Panel-rect recorder: only the calls made from inside a parked
        -- mode-3 draw are the confirm dialog's own panels. in_parked_mode3 is
        -- armed by the mode-3 BP above and cleared by any other mode, so the
        -- Now-checking / tab panels drawn on the same frame don't pollute it.
        probe.arm_breakpoint(FUN_801E36C4, "Exec", 4, "panel_drawer", function()
            local r = PCSX.getRegisters()
            local n = r.GPR and r.GPR.n
            if n == nil then return end
            -- Attribute every panel to the slide mode that drew it, so each
            -- measured rect on the captured frame has its own args to be
            -- checked against - not just the confirm dialog's.
            local key = string.format("m%s:%d,%d,%d,%d", tostring(current_mode),
                tonumber(n.a0), tonumber(n.a1), tonumber(n.a2), tonumber(n.a3))
            if panel_calls[key] then return end
            panel_calls[key] = true
            logf("mode %s: FUN_801E36C4(center_x=%d, y=%d, w=%d, h=%d)%s",
                tostring(current_mode),
                tonumber(n.a0), tonumber(n.a1), tonumber(n.a2), tonumber(n.a3),
                in_parked_mode3 and "  [parked]" or "")
        end)
        return {}
    end,

    on_capture = function(ctx, vsync)
        -- Entry walk: field -> pause menu -> Save row -> save screen.
        for _, s in ipairs(walk) do
            if s.at == vsync then
                probe.pad.force(s.btn)
                local rel = vsync + s.hold
                walk_release[rel] = walk_release[rel] or {}
                table.insert(walk_release[rel], s)
            end
        end
        local rel = walk_release[vsync]
        if rel ~= nil then
            for _, s in ipairs(rel) do probe.pad.release(s.btn) end
            walk_release[vsync] = nil
        end
        if vsync < TAP_FROM then return end

        -- Once the dialog is drawing, stop tapping: CROSS on the parked
        -- prompt would answer it.
        if seen_mode3 then
            if tap_until >= 0 then
                probe.pad.release(probe.BTN.CROSS)
                tap_until = -1
            end
        else
            if vsync >= next_tap and tap_until < 0 then
                probe.pad.force(probe.BTN.CROSS)
                tap_until = vsync + TAP_HOLD
            elseif tap_until >= 0 and vsync >= tap_until then
                probe.pad.release(probe.BTN.CROSS)
                tap_until = -1
                next_tap = vsync + TAP_PERIOD
            end
        end

        -- takeScreenShot returns the *displayed* buffer, which lags the draw
        -- by a frame or more (a game tick spans several vsyncs at 30fps), so
        -- capturing on the first parked vsync yields a LAST-SLIDE-STEP frame,
        -- not the parked dialog - measurably, panels one 16px step low per
        -- frame of lag. Once parked the dialog is static, so simply waiting
        -- costs nothing and removes the whole class of error.
        if last_mode3_t == T_PARKED then
            parked_vsyncs = parked_vsyncs + 1
        else
            parked_vsyncs = 0
        end
        if not captured and parked_vsyncs >= PARKED_SETTLE_VSYNCS then
            captured = true
            logf("capturing at vsync=%d (parked for %d vsyncs)", vsync, parked_vsyncs)
            take_fb(OUT_RAW, OUT_META, "confirm_dialog")
            ctx.request_quit = true
        end
    end,

    on_done = function(_, _)
        probe.pad.release(probe.BTN.CROSS)
        local modes = {}
        for m, c in pairs(mode_hits) do
            modes[#modes + 1] = string.format("mode %s: %d", tostring(m), c)
        end
        table.sort(modes)
        logf("slide-primitive hits by mode: %s",
            #modes > 0 and table.concat(modes, ", ") or "NONE (bp never fired)")
        if not seen_mode3 then
            logf("NEVER SAW A MODE-3 DRAW - confirm dialog not reached; "
                .. "no capture written")
        elseif not captured then
            logf("saw mode-3 but never parked (last t=%s); no capture written",
                tostring(last_mode3_t))
        end
        local fh = io.open(OUT_LOG, "w")
        if fh ~= nil then
            fh:write(table.concat(log_lines, "\n") .. "\n")
            fh:close()
        end
    end,
})
