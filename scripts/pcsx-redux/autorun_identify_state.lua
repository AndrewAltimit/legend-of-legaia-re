-- autorun_identify_state.lua
--
-- Say what a save state actually IS: game mode, CDNAME scene label, and
-- which overlay-resident UI is currently ticking. Diagnostic first aid for
-- "my probe's breakpoint never fired" - usually the state is not on the
-- screen you thought, and this tells you in one run instead of by guessing
-- at pad taps.
--
-- Reads the same anchors the legaia-pcsxr crate uses (GAME_MODE_VA /
-- SCENE_NAME_VA), which is the host-side reader for .sstate files; use this
-- when that reader cannot parse the state (e.g. a newer PCSX-Redux payload
-- layout) and you need the answer from inside the emulator.
--
-- Env vars:
--   LEGAIA_SSTATE   sstate path
--   LEGAIA_OUT_DIR  output dir
--   LEGAIA_FRAMES   vsyncs to observe (default 120)

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate9")
local OUT_LOG = probe.out_path("identify_state.log")
local FRAMES  = probe.getenv_num("LEGAIA_FRAMES", 120)

local GAME_MODE_VA  = 0x8007b83c
local SCENE_NAME_VA = 0x8007050c

-- Candidate overlay entry points, each tagged with the screen it implies.
-- A hit proves that code is executing; no hit proves nothing on its own.
local WATCH = {
    { addr = 0x801dc6b4, name = "FUN_801DC6B4 save-screen dispatcher" },
    { addr = 0x801dd35c, name = "FUN_801DD35C save/load main frame" },
    { addr = 0x801e1c1c, name = "FUN_801E1C1C slide primitive" },
    { addr = 0x801e08d8, name = "FUN_801E08D8 info panel renderer" },
    { addr = 0x801de840, name = "FUN_801DE840 field/event VM" },
}

local log_lines = {}
local function logf(fmt, ...)
    local s = string.format(fmt, ...)
    log_lines[#log_lines + 1] = s
    PCSX.log("[identify] " .. s)
end

local hits = {}

local function scene_name()
    local out = {}
    for i = 0, 7 do
        local b = probe.read_u8(SCENE_NAME_VA + i)
        if b == nil or b < 0x20 or b >= 0x7f then break end
        out[#out + 1] = string.char(b)
    end
    return table.concat(out)
end

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,

    on_arm = function(_)
        logf("sstate=%s", SSTATE_PATH)
        for _, w in ipairs(WATCH) do
            local name = w.name
            probe.arm_breakpoint(w.addr, "Exec", 4, name, function()
                hits[name] = (hits[name] or 0) + 1
            end)
        end
        return {}
    end,

    on_capture = function(_, vsync)
        if vsync == 1 or vsync == FRAMES - 1 then
            logf("vsync=%d game_mode=0x%02x scene=%q", vsync,
                probe.read_u8(GAME_MODE_VA) or 0xFF, scene_name())
        end
    end,

    on_done = function(_, _)
        local any = false
        for _, w in ipairs(WATCH) do
            local c = hits[w.name]
            if c ~= nil then
                any = true
                logf("HIT %-42s x%d", w.name, c)
            end
        end
        if not any then
            logf("no watched entry point executed")
        end
        local fh = io.open(OUT_LOG, "w")
        if fh ~= nil then
            fh:write(table.concat(log_lines, "\n") .. "\n")
            fh:close()
        end
    end,
})
