-- autorun_chitei2_corridor_pillars.lua - chitei2 corridor-pillar (4C D6) capture: door hop chitei2 -> deroa -> chitei2
-- by tile pokes, then poke the player across P2[1] (100,110) and P2[0]
-- (101,110) and stand inside at TILE_IN. Checkpoints: before (on the hop
-- arrival tile), after (inside, lights set). Only player +0x14/+0x18 pokes.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local mem   = require("probe.mem")
local GAME_MODE, SCENE_NAME, PLAYER_PTR = 0x8007B83C, 0x8007050C, 0x8007C364
local SSTATE  = probe.getenv("LEGAIA_SSTATE", "")
local OUT_DIR = probe.getenv("LEGAIA_OUT_DIR", "captures/chitei2_corridor_pillars")
local SEQ = probe.getenv("LEGAIA_SEQ", "100,110;101,110;102,110")
local HOLD = probe.getenv_num("LEGAIA_HOLD", 20)
local AFTER = probe.getenv_num("LEGAIA_AFTER", 120)
local LOGF = io.open(OUT_DIR .. "/lights.log", "w")
local function log(s) PCSX.log("[lights] " .. s); if LOGF then LOGF:write(s .. "\n"); LOGF:flush() end end
local function u8(a) return mem.read_u8(a) or 0 end
local function u32n(v) v = tonumber(v) or 0; if v < 0 then v = v + 4294967296 end; return v end
local function scene_name()
    local s = {}
    for i = 0, 7 do local b = u8(SCENE_NAME + i); if b < 0x20 or b >= 0x7F then break end; s[#s+1] = string.char(b) end
    return table.concat(s)
end
local function pp() local p = u32n(mem.read_u32(PLAYER_PTR) or 0); if p < 0x80000000 or p >= 0x80200000 then return nil end; return p end
local function poke(tx, tz) local p = pp(); if p then mem.write_u16(p + 0x14, tx * 128 + 0x40); mem.write_u16(p + 0x18, tz * 128 + 0x40) end end
local function ckpt(label)
    local ok, err = pcall(function()
        local w = PCSX.createSaveState()
        local fh = Support.File.open(OUT_DIR .. "/" .. label .. ".rawsstate", "TRUNCATE")
        fh:writeMoveSlice(w); fh:close()
    end)
    log("checkpoint " .. label .. " " .. tostring(ok) .. " " .. tostring(err))
end
local seq = {}
for a, b in string.gmatch(SEQ, "(%d+),(%d+)") do seq[#seq+1] = { tonumber(a), tonumber(b) } end
local legs = { { "chitei2", 50, 18 }, { "deroa", 43, 112 } }
local vsync, loaded, leg, ft, phase, step, stepT, doneT = 0, false, 1, 0, "hop", 1, 0, 0
local function lights()
    local out = {}
    for k = 0, 8 do
        local p = u32n(mem.read_u32(0x8007C34C + 4 * k) or 0); local n = 0
        while p >= 0x80000000 and p < 0x80200000 and n < 300 do
            local id = mem.read_u16(p + 0x50) or 0
            if id >= 5 and id <= 0x10 and u32n(mem.read_u32(p + 0x0C) or 0) == 0x8003BC08 then
                out[#out+1] = string.format("%02X:%08X", id, u32n(mem.read_u32(p + 0x74) or 0))
            end
            p = u32n(mem.read_u32(p) or 0); n = n + 1
        end
    end
    return table.concat(out, " ")
end
local function on_vsync()
    vsync = vsync + 1
    if not loaded then
        if vsync >= 60 then
            if not probe.load_save_state(SSTATE) then log("load failed"); PCSX.quit(1) end
            loaded = true; log("loaded " .. scene_name())
        end
        return
    end
    local sc, md = scene_name(), u8(GAME_MODE)
    if md == 0x03 then ft = ft + 1 else ft = 0 end
    if vsync % 30 == 0 then local p = pp(); log(string.format("v=%d %s m=%02X phase=%s p=(%d,%d) %s", vsync, sc, md, phase, p and mem.read_u16(p+0x14) or -1, p and mem.read_u16(p+0x18) or -1, lights())) end
    if phase == "hop" then
        local l = legs[leg]
        if l == nil then
            if sc == "chitei2" and md == 0x03 and ft >= 90 then ckpt("chitei2_corridor_pillars_before"); phase = "walk"; stepT = 0 end
        elseif sc == l[1] and md == 0x03 and ft >= 90 then poke(l[2], l[3])
        elseif sc ~= l[1] and sc ~= "" and ft >= 2 then log("crossed into " .. sc .. " v=" .. vsync); leg = leg + 1; ft = 0 end
    elseif phase == "walk" then
        local s = seq[step]
        poke(s[1], s[2]); stepT = stepT + 1
        if stepT >= HOLD then log("step " .. step .. " done " .. lights()); step = step + 1; stepT = 0 end
        if step > #seq then phase = "after"; doneT = 0 end
    elseif phase == "after" then
        doneT = doneT + 1
        if doneT == AFTER then log("lights " .. lights()); ckpt("chitei2_corridor_pillars_lit"); LOGF:close(); PCSX.quit(0) end
    end
    if vsync > 6000 then log("timeout"); PCSX.quit(2) end
end
PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] = PCSX.Events.createEventListener("GPU::Vsync", on_vsync)
