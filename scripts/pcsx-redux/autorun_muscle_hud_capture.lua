-- autorun_muscle_hud_capture.lua
--
-- Muscle Dome HUD capture: drive the dome load-transition save forward into
-- the live match with a scripted pad, and at named moments dump the FULL GPU
-- VRAM (PCSX.getVRAM), the full 2 MiB main RAM and a screenshot - the raw
-- material for pinning the battle-chrome sprite geometry (element table
-- 0x80076C10, live text-actor nodes, GP0 packets) and the texture pages the
-- chrome samples.
--
-- Checkpoints fire on: (a) every game_mode transition, (b) the first sight
-- of each ctx+6 match phase listed in LEGAIA_DUMP_PHASES, (c) each vsync
-- listed in LEGAIA_DUMP_AT. Each checkpoint writes
--   cp_<vsync>_m<mode>_p<phase>.{screen,screen.meta,vram,ram}
-- Poll-only (no breakpoints), so --fast is fine.
--
-- Env:
--   LEGAIA_SSTATE       save state (the minigame_muscle_dome_pcsx library copy)
--   LEGAIA_OUT_DIR      output dir
--   LEGAIA_FRAMES       vsyncs to run (default 6000)
--   LEGAIA_PAD_SCRIPT   "vsync:BUTTON[:hold],..." (pad_walk grammar; optional)
--   LEGAIA_AUTOTAP      "1" = tap CROSS 4 vsyncs out of every 100 while
--                       game_mode != 0x15 (menu advance; default on)
--   LEGAIA_DUMP_PHASES  comma hex ctx+6 values to checkpoint at first sight
--                       (default "0x14,0x50,0x5a,0x64,0x6e")
--   LEGAIA_DUMP_AT      comma vsync list for unconditional checkpoints
--   LEGAIA_SHOT_EVERY   periodic screenshot-only interval (default 120, 0=off)

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad = require("probe.pad")

local SSTATE = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate4")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 6000)
local SCRIPT = probe.getenv("LEGAIA_PAD_SCRIPT", "")
local AUTOTAP = probe.getenv("LEGAIA_AUTOTAP", "1") == "1"
local DUMP_PHASES = probe.getenv("LEGAIA_DUMP_PHASES", "0x14,0x50,0x5a,0x64,0x6e")
local DUMP_AT = probe.getenv("LEGAIA_DUMP_AT", "")
local SHOT_EVERY = probe.getenv_num("LEGAIA_SHOT_EVERY", 120)

local GAME_MODE_VA = 0x8007b83c
local CTX_PTR_VA = 0x8007bd24

local log_lines = {}
local function logf(fmt, ...)
    local s = string.format(fmt, ...)
    log_lines[#log_lines + 1] = s
    PCSX.log("[muscle_hud] " .. s)
end

local function parse_script(text)
    local steps = {}
    for chunk in string.gmatch(text, "[^,]+") do
        local parts = {}
        for p in string.gmatch(chunk, "[^:]+") do parts[#parts + 1] = p end
        local at = tonumber(parts[1])
        local name = parts[2] and string.upper(parts[2])
        local hold = tonumber(parts[3] or "6")
        local btn = name and probe.BTN[name]
        if at == nil or btn == nil then
            logf("BAD STEP %q", chunk)
        else
            steps[#steps + 1] = { at = at, btn = btn, hold = hold, name = name }
        end
    end
    return steps
end

local function parse_list(text, base)
    local out = {}
    for tok in string.gmatch(text, "[^,]+") do
        local v = tonumber((tok:gsub("%s", "")), base)
        if v ~= nil then out[v] = true end
    end
    return out
end

local function ctx_ptr()
    -- NB no bit.band here: PCSX's Lua bit ops are 32-bit SIGNED, so
    -- band(p, 0xFFE00000) == 0x80000000 is never true for a KSEG0 pointer.
    local p = probe.read_u32(CTX_PTR_VA)
    if p == nil or p < 0x80010000 or p >= 0x80200000 then return nil end
    return p
end

local function match_phase()
    local p = ctx_ptr()
    if p == nil then return nil end
    return probe.read_u8(p + 6)
end

local function shot(name)
    local ok, ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
    if not ok or not ss then return end
    local h = io.open(probe.out_path(name .. ".screen"), "wb")
    if not h then return end
    h:write(tostring(ss.data)); h:close()
    local m = io.open(probe.out_path(name .. ".screen.meta"), "w")
    if m then
        m:write(string.format("width=%d\nheight=%d\nbpp=%d\n",
            tonumber(ss.width), tonumber(ss.height),
            (tonumber(ss.bpp) or 0) > 16 and 24 or 16))
        m:close()
    end
end

local function dump_vram(name)
    local ok, err = pcall(function()
        local v
        if PCSX.getVRAM ~= nil then v = PCSX.getVRAM()
        elseif PCSX.GPU and PCSX.GPU.getVRAM then v = PCSX.GPU.getVRAM() end
        if v == nil then error("no getVRAM API") end
        local h = io.open(probe.out_path(name .. ".vram"), "wb")
        if h == nil then error("open failed") end
        h:write(tostring(v)); h:close()
    end)
    if ok then return end
    -- Fallback for builds without a VRAM accessor: write a full savestate
    -- (its GPU section carries the 1 MiB VRAM; extracted offline).
    local ok2, err2 = pcall(function()
        local w = PCSX.createSaveState()
        local fh = Support.File.open(probe.out_path(name .. ".rawsstate"), "CREATE")
        fh:writeMoveSlice(w); fh:close()
    end)
    if not ok2 then
        logf("vram dump FAILED for %s: %s / rawsstate: %s", name,
            tostring(err), tostring(err2))
    end
end

local function dump_ram(name)
    local ok = pcall(function()
        local mf = PCSX.getMemoryAsFile()
        local h = io.open(probe.out_path(name .. ".ram"), "wb")
        if h == nil then error("open") end
        local off, SIZE = 0, 0x200000
        while off < SIZE do
            local n = math.min(0x40000, SIZE - off)
            local chunk = mf:readAt(n, off)
            if chunk == nil then break end
            h:write(tostring(chunk))
            off = off + n
        end
        h:close()
    end)
    if not ok then logf("ram dump FAILED for %s", name) end
end

local steps = parse_script(SCRIPT)
local dump_phases = parse_list(DUMP_PHASES, 16)
local dump_at = parse_list(DUMP_AT, 10)
local releases = {}
local seen_phase = {}
local last_mode, last_phase = nil, nil
local checkpoints = 0

local function checkpoint(vsync, mode, phase, why)
    checkpoints = checkpoints + 1
    local name = string.format("cp_%05d_m%02x_p%s", vsync, mode or 0xFF,
        phase and string.format("%02x", phase) or "xx")
    logf("checkpoint %s (%s)", name, why)
    shot(name)
    dump_vram(name)
    dump_ram(name)
end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES + 8,

    on_arm = function(_)
        logf("sstate=%s frames=%d steps=%d autotap=%s", SSTATE, FRAMES, #steps,
            tostring(AUTOTAP))
        return {}
    end,

    on_capture = function(ctx, vsync)
        local mode = probe.read_u8(GAME_MODE_VA)
        local phase = mode == 0x15 and match_phase() or nil

        if mode ~= last_mode then
            logf("vsync=%d game_mode 0x%02x -> 0x%02x ctx=0x%08x", vsync,
                last_mode or 0xFF, mode or 0xFF, ctx_ptr() or 0)
            checkpoint(vsync, mode, phase, "mode change")
            last_mode = mode
        end
        if phase ~= nil and phase ~= last_phase then
            logf("vsync=%d match phase 0x%02x -> 0x%02x", vsync,
                last_phase or 0xFF, phase)
            if dump_phases[phase] and not seen_phase[phase] then
                checkpoint(vsync, mode, phase, "phase first sight")
            end
            seen_phase[phase] = true
            last_phase = phase
        end
        if dump_at[vsync] then
            checkpoint(vsync, mode, phase, "scheduled")
        end
        if SHOT_EVERY > 0 and vsync > 0 and vsync % SHOT_EVERY == 0 then
            shot(string.format("shot_%05d_m%02x_p%s", vsync, mode or 0xFF,
                phase and string.format("%02x", phase) or "xx"))
        end

        -- Menu advance: tap Cross while not yet in the match.
        if AUTOTAP and mode ~= 0x15 then
            local ph = vsync % 100
            if ph == 40 then pad.force(pad.BTN.CROSS) end
            if ph == 44 then pad.release(pad.BTN.CROSS) end
        end

        for _, s in ipairs(steps) do
            if s.at == vsync then
                pad.force(s.btn)
                logf("vsync=%d press %s hold=%d", vsync, s.name, s.hold)
                local rel = vsync + s.hold
                releases[rel] = releases[rel] or {}
                table.insert(releases[rel], s)
            end
        end
        local rel = releases[vsync]
        if rel ~= nil then
            for _, s in ipairs(rel) do
                pad.release(s.btn)
                logf("vsync=%d release %s", vsync, s.name)
            end
            releases[vsync] = nil
        end

        if vsync >= FRAMES then ctx.request_quit = true end
    end,

    on_done = function(_, _)
        for _, s in ipairs(steps) do pad.release(s.btn) end
        logf("done: %d checkpoints", checkpoints)
        local fh = io.open(probe.out_path("muscle_hud_capture.log"), "w")
        if fh ~= nil then
            fh:write(table.concat(log_lines, "\n") .. "\n")
            fh:close()
        end
    end,
})
