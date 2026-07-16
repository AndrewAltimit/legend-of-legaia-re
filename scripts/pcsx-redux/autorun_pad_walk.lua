-- autorun_pad_walk.lua
--
-- Drive the game with a scripted pad sequence and trace where it lands.
-- The tool for reaching a screen that no save state is parked on (the save
-- screen, a submenu, a confirm prompt) so a capture probe has somewhere to
-- start - or so you can save a state there once and stop re-walking.
--
-- The walk is data, not code: pass it in LEGAIA_PAD_SCRIPT as a comma list
-- of `<vsync>:<BUTTON>[:<hold>]` steps, e.g.
--
--   LEGAIA_PAD_SCRIPT="30:START,90:DOWN,120:CROSS:10"
--
-- Buttons are the probe.BTN names (CROSS, CIRCLE, START, UP, DOWN, ...).
-- `hold` defaults to 6 vsyncs. Steps may share a vsync; they are applied in
-- listed order.
--
-- Every game_mode transition is logged with the vsync it happened on, so a
-- failed walk tells you where it stalled rather than just failing. Optional
-- LEGAIA_WATCH_FN (hex VA, or a comma list) arms an exec breakpoint per
-- address and reports hit counts - proof that a given screen's code ran.
-- NB overlay VAs alias: the same address in a different overlay is a
-- different function, so a hit only means "this address executed".
--
-- Needs the interpreter + debugger for LEGAIA_WATCH_FN (Lua BPs do not fire
-- under --fast). Without watches, --fast is fine.
--
-- Env vars:
--   LEGAIA_SSTATE      sstate path
--   LEGAIA_OUT_DIR     output dir
--   LEGAIA_FRAMES      vsyncs to run (default 600)
--   LEGAIA_PAD_SCRIPT  the walk (required; empty = observe only)
--   LEGAIA_WATCH_FN    optional hex VAs to breakpoint, comma separated
--   LEGAIA_SNAP_AT     optional vsync to write a state snapshot marker at

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate9")
local OUT_LOG = probe.out_path("pad_walk.log")
local FRAMES  = probe.getenv_num("LEGAIA_FRAMES", 600)
local SCRIPT  = probe.getenv("LEGAIA_PAD_SCRIPT", "")
local WATCH   = probe.getenv("LEGAIA_WATCH_FN", "")

local GAME_MODE_VA  = 0x8007b83c
local SCENE_NAME_VA = 0x8007050c
-- BIOS pad data word (read by FUN_8001822C) + the player actor pointer.
-- Both are traced so a walk that does nothing can be told apart from a walk
-- whose buttons never reached the game: if the pad word never changes while
-- a step is held, the injection is broken, not the navigation.
local PAD_DATA_VA   = 0x800840f8
local PLAYER_PTR_VA = 0x8007c364
local PLAYER_X_OFF  = 0x14
local PLAYER_Z_OFF  = 0x18

local log_lines = {}
local function logf(fmt, ...)
    local s = string.format(fmt, ...)
    log_lines[#log_lines + 1] = s
    PCSX.log("[pad_walk] " .. s)
end

-- Parse "30:START,90:DOWN:10" -> { {at=30, btn=.., hold=6}, ... }
local function parse_script(text)
    local steps = {}
    for chunk in string.gmatch(text, "[^,]+") do
        local parts = {}
        for p in string.gmatch(chunk, "[^:]+") do parts[#parts + 1] = p end
        local at   = tonumber(parts[1])
        local name = parts[2] and string.upper(parts[2])
        local hold = tonumber(parts[3] or "6")
        local btn  = name and probe.BTN[name]
        if at == nil or btn == nil then
            logf("BAD STEP %q (want <vsync>:<BUTTON>[:<hold>])", chunk)
        else
            steps[#steps + 1] = { at = at, btn = btn, hold = hold, name = name }
        end
    end
    table.sort(steps, function(a, b) return a.at < b.at end)
    return steps
end

local function scene_name()
    local out = {}
    for i = 0, 7 do
        local b = probe.read_u8(SCENE_NAME_VA + i)
        if b == nil or b < 0x20 or b >= 0x7f then break end
        out[#out + 1] = string.char(b)
    end
    return table.concat(out)
end

local function player_pos()
    local p = probe.read_u32(PLAYER_PTR_VA)
    if p == nil or bit.band(p, 0xFFE00000) ~= 0x80000000 then return nil end
    local x = probe.read_u16(p + PLAYER_X_OFF)
    local z = probe.read_u16(p + PLAYER_Z_OFF)
    if x == nil or z == nil then return nil end
    if x >= 0x8000 then x = x - 0x10000 end
    if z >= 0x8000 then z = z - 0x10000 end
    return x, z
end

local steps = parse_script(SCRIPT)
local releases = {}   -- vsync -> list of buttons to release
local hits = {}
local last_mode = nil
local pad_seen = {}   -- distinct pad words observed, as evidence of injection

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,

    on_arm = function(_)
        logf("sstate=%s frames=%d steps=%d", SSTATE_PATH, FRAMES, #steps)
        for _, s in ipairs(steps) do
            logf("  step: vsync=%d %s hold=%d", s.at, s.name, s.hold)
        end
        for hexva in string.gmatch(WATCH, "[^,]+") do
            local addr = tonumber((hexva:gsub("%s", "")), 16)
            if addr ~= nil then
                local label = string.format("watch_%08x", addr)
                probe.arm_breakpoint(addr, "Exec", 4, label, function()
                    hits[addr] = (hits[addr] or 0) + 1
                end)
                logf("  watch: 0x%08x", addr)
            end
        end
        return {}
    end,

    on_capture = function(_, vsync)
        local mode = probe.read_u8(GAME_MODE_VA)
        if mode ~= last_mode then
            local x, z = player_pos()
            logf("vsync=%d game_mode=0x%02x scene=%q pos=%s", vsync, mode or 0xFF,
                scene_name(),
                x and string.format("(%d,%d)", x, z) or "n/a")
            last_mode = mode
        end
        local pw = probe.read_u32(PAD_DATA_VA)
        if pw ~= nil and not pad_seen[pw] then
            pad_seen[pw] = vsync
        end

        for _, s in ipairs(steps) do
            if s.at == vsync then
                probe.pad.force(s.btn)
                logf("vsync=%d press %s", vsync, s.name)
                local rel = vsync + s.hold
                releases[rel] = releases[rel] or {}
                table.insert(releases[rel], s)
            end
        end
        local rel = releases[vsync]
        if rel ~= nil then
            for _, s in ipairs(rel) do
                probe.pad.release(s.btn)
                logf("vsync=%d release %s", vsync, s.name)
            end
            releases[vsync] = nil
        end
    end,

    on_done = function(_, _)
        for _, s in ipairs(steps) do probe.pad.release(s.btn) end
        local any = false
        for addr, c in pairs(hits) do
            any = true
            logf("HIT 0x%08x x%d", addr, c)
        end
        if WATCH ~= "" and not any then logf("no watched address executed") end
        -- Injection evidence: >1 distinct pad word means presses reached the
        -- game. Exactly one means every step was a no-op at the pad layer,
        -- so nothing downstream can be concluded from the walk.
        local n, sample = 0, {}
        for w, at in pairs(pad_seen) do
            n = n + 1
            if #sample < 6 then sample[#sample + 1] = string.format("0x%08x@%d", w, at) end
        end
        logf("pad words seen: %d [%s]%s", n, table.concat(sample, " "),
            n <= 1 and "  <-- PAD INJECTION NOT REACHING THE GAME" or "")
        local x, z = player_pos()
        logf("final game_mode=0x%02x scene=%q pos=%s",
            probe.read_u8(GAME_MODE_VA) or 0xFF, scene_name(),
            x and string.format("(%d,%d)", x, z) or "n/a")
        local fh = io.open(OUT_LOG, "w")
        if fh ~= nil then
            fh:write(table.concat(log_lines, "\n") .. "\n")
            fh:close()
        end
    end,
})
