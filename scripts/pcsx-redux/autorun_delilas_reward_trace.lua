-- autorun_delilas_reward_trace.lua
--
-- Drive the Delilas dome course to a WIN and instrument the settlement:
-- monsters are weakened to 1 HP once at each battle's start (a one-shot
-- write before any action - blind mid-battle HP writes wedge the settle
-- bookkeeping), Cross-mash clears both rounds, and breakpoints on the
-- reward detour pin what the payout path actually does:
--   - 0x801D1118 (REWARD_HOOK): log v0 (table slot), v1 (counter loaded),
--     a1 (round), plus the course/round globals,
--   - 0x8007AE84 (cave reward routine): detour-entry receipt,
--   - 0x801D1128 (post-store): the stored counter value.
-- The winnings counter 0x80084440 (game-state window +0x300) is logged
-- every 60 ticks - seed it high via LEGAIA_POKES to reproduce the
-- "cheated balance zeroed" report.
--
-- Output: delilas_reward_trace.csv  tick,mode,note

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad = require("probe.pad")

local GAME_MODE   = 0x8007B83C
local WARP_SUB    = 0x8007BA34
local COURSE_WORD = 0x8007BAC0
local ACTOR_TABLE = 0x801C9370
local COUNTER     = 0x80084440
local COURSE_G    = 0x801D1A90
local ROUND_G     = 0x801D1A94
local REWARD_CAVE = 0x8007AE8C

local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 12000)
local FORCE_AT = probe.getenv_num("LEGAIA_FORCE_AT", 120)
local POKES_RAW = probe.getenv("LEGAIA_POKES", "")

local pokes = {}
for pair in string.gmatch(POKES_RAW, "[^,%s]+") do
    local a, v, w = string.match(pair, "([^:]+):([^:]+):?(.*)")
    if a and v then
        pokes[#pokes + 1] = { addr = tonumber(a), val = tonumber(v), byte = (w == "b") }
    end
end

local CSV = probe.csv_open(probe.out_path("delilas_reward_trace.csv"),
    "tick,mode,note")

local function reg(r, name)
    local ok, v = pcall(function() return tonumber(r.GPR.n[name]) % 0x100000000 end)
    if ok then return v end
    return 0
end

local installed = false
local last_mode = -1
local weaken_at = -1

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE", ""),
    capture_frames = FRAMES,
    on_arm = function(ctx)
        -- NB: breakpoints on arena-overlay VAs (0x801Dxxxx) are useless here -
        -- the battle overlay aliases the band and fires them constantly. The
        -- SCUS cave (0x8007xxxx) is always-resident and unambiguous.
        probe.arm_breakpoint(REWARD_CAVE, "Exec", 4, "reward_cave", function()
            local r = PCSX.getRegisters()
            CSV:row("0,0,CAVE reward routine entered course_g=%d round_g=%d",
                probe.read_u32(COURSE_G) or -1, probe.read_u32(ROUND_G) or -1)
        end)
        return {}
    end,
    on_capture = function(ctx, elapsed)
        local mode = probe.read_u16(GAME_MODE) or -1
        if elapsed == FORCE_AT then
            for _, p in ipairs(pokes) do
                if p.byte then
                    probe.write_u8(p.addr, p.val)
                else
                    probe.write_u32(p.addr, p.val)
                end
            end
            probe.write_u32(WARP_SUB, 5)
            probe.write_u32(COURSE_WORD, 0)
            probe.write_u16(GAME_MODE, 0x18)
            installed = true
            CSV:row("%d,0x%X,warp forced (%d pokes)", elapsed, mode, #pokes)
            return
        end
        if mode ~= last_mode then
            CSV:row("%d,0x%X,mode-change word=0x%X counter=%d", elapsed, mode,
                probe.read_u32(COURSE_WORD) or 0, probe.read_u32(COUNTER) or -1)
            if mode == 0x15 then
                weaken_at = elapsed + 20
            end
            last_mode = mode
        end
        if weaken_at > 0 and elapsed == weaken_at then
            weaken_at = -1
            for slot = 3, 5 do
                local a = probe.read_u32(ACTOR_TABLE + slot * 4) or 0
                if a > 0x80000000 and a < 0x80200000 then
                    local hp = probe.read_u16(a + 0x14C) or 0
                    if hp > 1 then
                        probe.write_u16(a + 0x14C, 1)
                    end
                end
            end
            CSV:row("%d,0x%X,monsters weakened to 1 hp", elapsed, mode)
        end
        if installed then
            -- Mash Up then Cross: the intermission continue-menu's first
            -- option is the one that keeps the course going, and a bare
            -- Cross mash has been observed selecting "leave".
            local phase = elapsed % 40
            if phase == 0 then
                pad.force(pad.BTN.UP)
            elseif phase == 4 then
                pad.release(pad.BTN.UP)
            elseif phase == 8 then
                pad.force(pad.BTN.CROSS)
            elseif phase == 14 then
                pad.release(pad.BTN.CROSS)
            end
            if elapsed % 60 == 0 then
                CSV:row("%d,0x%X,counter=%d word=0x%X", elapsed, mode,
                    probe.read_u32(COUNTER) or -1,
                    probe.read_u32(COURSE_WORD) or 0)
            end
        end
    end,
    on_done = function()
        CSV:row("0,0x%X,done counter=%d", last_mode, probe.read_u32(COUNTER) or -1)
        pcall(function()
            local sstate = require("probe.sstate")
            sstate.save(probe.out_path("reward_trace_end.sstate"))
        end)
        CSV:close()
    end,
})
