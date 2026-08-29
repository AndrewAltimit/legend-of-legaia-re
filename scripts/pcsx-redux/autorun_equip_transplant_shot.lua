-- From a field save with Noa in the party, set Noa's equipped weapon
-- to 0xBA (Astral Sword) in her character record, force an encounter the way
-- autorun_delilas_battle_load.lua does, mash an attack, and screenshot the
-- battle every SHOT_EVERY frames.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad = require("probe.pad")

local GAME_MODE      = 0x8007B83C
local FORMATION_CELL = 0x8007BD0C
local BATTLE_MAIN    = 0x15
local CHAR_REC       = 0x80084708
local REC_STRIDE     = 0x414
local NOA            = 1

local SSTATE   = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 1400)
local FORCE_AT = probe.getenv_num("LEGAIA_FORCE_AT", 60)
local SHOT_EVERY = probe.getenv_num("LEGAIA_SHOT_EVERY", 60)
local WEAPON   = probe.getenv_num("LEGAIA_WEAPON", 0xBA)
local IDS_RAW  = probe.getenv("LEGAIA_IDS", "4")
local TAG      = probe.getenv("LEGAIA_TAG", "run")
local ids = {}
for tok in string.gmatch(IDS_RAW, "[^,%s]+") do ids[#ids + 1] = tonumber(tok) end

local function shot(name)
    local ok, ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
    if ok and ss then
        local bpp = (tonumber(ss.bpp) or 0) > 16 and 24 or 16
        local h = io.open(probe.out_path(name .. ".raw"), "wb"); h:write(tostring(ss.data)); h:close()
        local m = io.open(probe.out_path(name .. ".meta"), "w")
        m:write(string.format("width=%d\nheight=%d\nbpp=%d\n", tonumber(ss.width), tonumber(ss.height), bpp)); m:close()
    end
end

local installed, settle, last_mode = false, 0, -1
probe.run({
    sstate = SSTATE, capture_frames = FRAMES,
    on_arm = function() return {} end,
    on_capture = function(ctx, elapsed)
        local mode = probe.read_u16(GAME_MODE) or -1
        if elapsed == FORCE_AT then
            local rec = CHAR_REC + NOA * REC_STRIDE
            local name = ""
            for i = 0, 7 do local c = probe.read_u8(rec + 0x2A7 + i) or 0; if c == 0 then break end; name = name .. string.char(c) end
            local eq = {}
            for i = 0, 4 do eq[#eq+1] = string.format("%02X", probe.read_u8(rec + 0x196 + i) or 0) end
            PCSX.log(string.format("[lane-c] mode=0x%X party=%d ids=%02X %02X %02X noa-rec name='%s' equip=%s",
                mode, probe.read_u8(0x80084594) or 0, probe.read_u8(0x80084598) or 0, probe.read_u8(0x80084599) or 0, probe.read_u8(0x8008459A) or 0, name, table.concat(eq, " ")))
            probe.write_u8(rec + 0x199, WEAPON)
            PCSX.log(string.format("[lane-c] wrote weapon byte +0x199 = 0x%02X (now %02X)", WEAPON, probe.read_u8(rec + 0x199) or 0))
            for i = 0, 3 do probe.write_u8(FORMATION_CELL + i, ids[i + 1] or 0) end
            probe.write_u16(GAME_MODE, 8)
            installed = true
            return
        end
        if mode ~= last_mode then PCSX.log(string.format("[lane-c] t%d mode 0x%X", elapsed, mode)); last_mode = mode end
        if installed and mode == BATTLE_MAIN then
            settle = settle + 1
            if settle % SHOT_EVERY == 1 then shot(string.format("%s_s%04d", TAG, settle)) end
            local phase = settle % 40
            if phase == 0 then pad.force(pad.BTN.CROSS)
            elseif phase == 6 then pad.release(pad.BTN.CROSS)
            elseif phase == 20 then pad.force(pad.BTN.UP)
            elseif phase == 26 then pad.release(pad.BTN.UP) end
            if settle >= FRAMES - FORCE_AT - 300 then ctx.request_quit = true end
        end
    end,
    on_done = function() PCSX.log(string.format("[lane-c] done settle=%d last_mode=0x%X", settle, last_mode)) end,
})
