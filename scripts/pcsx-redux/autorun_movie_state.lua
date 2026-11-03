-- autorun_movie_state.lua
--
-- Save a raw state with an STR movie on screen. From a field state it makes the
-- two stores field-VM op `4C E2` makes (sh LEGAIA_FMV -> 0x8007BA78, sh 0x1A ->
-- 0x8007B83C; handler 0x801E30E4..0x801E3104 in PROT 0897) at LEGAIA_TRIGGER_V,
-- then writes <LEGAIA_PREFIX>_t<t>.rawsstate LEGAIA_SHOTS ("300,900") vsyncs
-- after the master mode first reads a movie mode. The scenario names the stores.
-- The frame on screen is the movie's own, so the state is a retail reference for
-- the decoder whatever story the source state stands in.
--
-- Env: LEGAIA_SSTATE, LEGAIA_FMV, LEGAIA_TRIGGER_V (60), LEGAIA_SHOTS,
-- LEGAIA_PREFIX ("movie"), LEGAIA_OUT_DIR, LEGAIA_FRAMES (3000). --fast is fine.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 3000)
local FMV = probe.getenv_num("LEGAIA_FMV", 1)
local TRIG = probe.getenv_num("LEGAIA_TRIGGER_V", 60)
local PREFIX = probe.getenv("LEGAIA_PREFIX", "movie")
local SHOTS = {}
for t in string.gmatch(probe.getenv("LEGAIA_SHOTS", "300,900"), "%d+") do
    SHOTS[#SHOTS + 1] = tonumber(t)
end

local GAME_MODE_VA = 0x8007b83c
local FMV_ID_VA = 0x8007ba78

local log_lines = {}
local function logf(fmt, ...)
    local s = string.format(fmt, ...)
    log_lines[#log_lines + 1] = s
    PCSX.log("[movie] " .. s)
end

local fired, since, shot, last_mode = false, nil, 1, nil

probe.run({
    sstate = SSTATE_PATH,
    capture_frames = FRAMES,

    on_arm = function(ctx)
        logf("sstate=%s fmv=%d", SSTATE_PATH, FMV)
        return {}
    end,

    on_capture = function(ctx, vsync)
        local mode = probe.read_u8(GAME_MODE_VA) or 0
        if mode ~= last_mode then
            logf("vsync=%d game_mode=0x%02x", vsync, mode)
            last_mode = mode
        end
        if not fired and vsync >= TRIG then
            fired = true
            probe.write_u16(FMV_ID_VA, FMV)
            probe.write_u16(GAME_MODE_VA, 0x1A)
            logf("stores made at vsync=%d", vsync)
        end
        if mode == 0x1A or mode == 0x1B then
            since = since or vsync
            local want = SHOTS[shot]
            if want and vsync - since >= want then
                local name = string.format("%s_t%d", PREFIX, want)
                local ok, err = pcall(function()
                    local w = PCSX.createSaveState()
                    local fh = Support.File.open(probe.out_path(name .. ".rawsstate"), "TRUNCATE")
                    fh:writeMoveSlice(w)
                    fh:close()
                end)
                logf("CKPT %s vsync=%d ok=%s %s", name, vsync, tostring(ok), tostring(err or ""))
                shot = shot + 1
                if SHOTS[shot] == nil then ctx.request_quit = true end
            end
        end
    end,

    on_done = function(_, _)
        local fh = io.open(probe.out_path(PREFIX .. ".log"), "w")
        if fh then fh:write(table.concat(log_lines, "\n"), "\n"); fh:close() end
    end,
})
