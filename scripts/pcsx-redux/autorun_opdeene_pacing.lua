-- autorun_opdeene_pacing.lua
--
-- Per-vsync pacing capture of the zero-input `opdeene` opening record: where
-- the cutscene record stands (`ctx[+0x9E]` PC, `+0x54` wait accumulator,
-- `+0x10` flags), the camera mover's progress (`FUN_801DC0BC` node, `+0x9C`
-- of `+0x9E`), and one vignette actor's clip + walk state (`+0x10` halt bit
-- `0x400`, `+0x5C` / `+0x5E` requested / bound clip, `+0x62` control word,
-- `+0x68` cursor, position, heading) and the narration roller
-- (`FUN_80037174` node: `+0x6A` retired pages, `+0x10` flags). The questions it answers: how long an
-- NPC end-latch spin (`AD <ch> 08`) holds the record, whether a cross-context
-- halt-bit test (`B3 <ch> 0A`) holds it across an NPC walk, and when the
-- scene label flips.
--
--   timeout --kill-after=30s 900s bash scripts/pcsx-redux/run_probe.sh \
--       --fast --lua scripts/pcsx-redux/autorun_opdeene_pacing.lua \
--       --scenario s1_newgame_field --frames 4600
--
-- Env: LEGAIA_RECORD_IDS (comma list of record flat indices `+0x50`; default
--      the three opening legs' records `0x23,0x21,0x32`),
--      LEGAIA_ACTOR_ID (vignette actor `+0x50`, default 5).
-- Output: <out>/opdeene_pacing.csv. Pure RAM observation - integers only.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 4600)
local RECORD_IDS = {}
for tok in string.gmatch(probe.getenv("LEGAIA_RECORD_IDS", "0x23,0x21,0x32"), "[^,]+") do
    RECORD_IDS[tonumber(tok)] = true
end
local ACTOR_ID = probe.getenv_num("LEGAIA_ACTOR_ID", 5)

local FIELD_TICK = 0x8003BC08
local MOVER_TICK = 0x801DC0BC
local ROLLER_TICK = 0x80037174

local function u8(a) return probe.mem.read_u8(a) or 0 end
local function u16(a) return probe.mem.read_u16(a) or 0 end
local function u32(a) return probe.mem.read_u32(a) or 0 end
local function s16(a)
    local v = u16(a)
    if v >= 0x8000 then v = v - 0x10000 end
    return v
end

local out = probe.out_path("opdeene_pacing.csv")
local CSV = io.open(out, "w")
CSV:write("vsync,step393,mode,label,rec_pc,rec_wait,rec_flags,mover_t,mover_d,"
    .. "a_flags,a_5c,a_5e,a_62,a_68,a_6a,a_x,a_z,a_26,roller_retired,roller_flags\n")

local function label()
    local s = ""
    for i = 0, 7 do
        local c = u8(0x8007050C + i)
        if c == 0 then break end
        s = s .. string.char(c)
    end
    return s
end

local function scan()
    local rec, mover, actor, roller = nil, nil, nil, nil
    local seen = {}
    for k = 0, 8 do
        local p = u32(0x8007C34C + 4 * k)
        local n = 0
        while probe.in_ram(p) and not seen[p] and n < 512 do
            seen[p] = true
            local tick = u32(p + 0x0C)
            local id = u16(p + 0x50)
            if tick == MOVER_TICK and bit.band(u32(p + 0x10), 8) == 0 and mover == nil then
                mover = p
            elseif tick == ROLLER_TICK and roller == nil then
                roller = p
            elseif tick == FIELD_TICK then
                if RECORD_IDS[id] and rec == nil and probe.in_ram(u32(p + 0x90)) then
                    rec = p
                elseif id == ACTOR_ID and actor == nil and bit.band(u32(p + 0x10), 0x100) == 0 then
                    actor = p
                end
            end
            p = u32(p)
            n = n + 1
        end
    end
    return rec, mover, actor, roller
end

probe.run({
    sstate = SSTATE_PATH,
    capture_frames = FRAMES,
    on_arm = function() return {} end,
    on_capture = function(ctx, e)
        local rec, mover, actor, roller = scan()
        local rl_retired, rl_flags = -1, 0
        if roller then rl_retired, rl_flags = s16(roller + 0x6A), u32(roller + 0x10) end
        local rp, rw, rf = -1, -1, 0
        if rec then rp, rw, rf = s16(rec + 0x9E), s16(rec + 0x54), u32(rec + 0x10) end
        local mt, md = -1, -1
        if mover then mt, md = s16(mover + 0x9C), s16(mover + 0x9E) end
        local af, a5c, a5e, a62, a68, a6a, ax, az, a26 = 0, -1, -1, 0, -1, -1, 0, 0, 0
        if actor then
            af, a5c, a5e = u32(actor + 0x10), s16(actor + 0x5C), s16(actor + 0x5E)
            a62, a68, a6a = u16(actor + 0x62), u16(actor + 0x68), s16(actor + 0x6A)
            ax, az, a26 = s16(actor + 0x14), s16(actor + 0x18), u16(actor + 0x26)
        end
        CSV:write(string.format("%d,%d,%d,%s,%d,%d,0x%08X,%d,%d,0x%08X,%d,%d,0x%04X,%d,%d,%d,%d,%d,%d,0x%08X\n",
            e, probe.mem.read_scratch_u8(0x1F800393) or -1, u16(0x8007B83C), label(),
            rp, rw, rf, mt, md, af, a5c, a5e, a62, a68, a6a, ax, az, a26, rl_retired, rl_flags))
        if e % 200 == 0 then
            CSV:flush()
            PCSX.log(string.format("[opdeene_pacing] vsync %d pc %d", e, rp))
        end
    end,
    on_done = function()
        CSV:close()
        PCSX.log("[opdeene_pacing] done -> " .. out)
    end,
})
