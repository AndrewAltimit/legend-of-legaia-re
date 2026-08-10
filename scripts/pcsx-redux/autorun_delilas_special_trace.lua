-- autorun_delilas_special_trace.lua
--
-- Dome-course driver (same forced warp + pokes as
-- autorun_delilas_dome_course.lua) instrumented for the signature-special
-- softlock: a breakpoint on the entry-table tag search `FUN_80050E2C`
-- logs (tag, caller, result-at-return), and a per-tick row tracks each
-- battle actor's anim pair (+0x1D9/+0x1DA), action id (+0x1DF) and HP.
-- A special that tag-searches a dropped entry shows up as the same tag
-- failing (result 0xFF) every tick while the caster's anim pair stops
-- changing and HP freezes.
--
-- Launch (same env as the dome-course probe):
--   LEGAIA_POKES="..." LEGAIA_FRAMES=7200 \
--   bash scripts/pcsx-redux/run_probe.sh --iso patched.bin \
--     --scenario sol_to_karisto_worldmap \
--     --lua scripts/pcsx-redux/autorun_delilas_special_trace.lua
--
-- Output: delilas_special_trace.csv  tick,mode,note

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad = require("probe.pad")

local GAME_MODE   = 0x8007B83C
local WARP_SUB    = 0x8007BA34
local COURSE_WORD = 0x8007BAC0
local HEAP_DESC   = 0x8007BB58
local TAG_SEARCH  = 0x80050E2C
local ACTOR_TABLE = 0x801C9370
local MALLOC_FAIL = 0x800178B8

local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 7200)
local FORCE_AT = probe.getenv_num("LEGAIA_FORCE_AT", 120)
local FORMATION = 0x8007BD0C
-- Seat overrides at battle install (mode 0x14): 0 = leave alone.
local FIRST = probe.getenv_num("LEGAIA_FIRST_SEAT", 0)
local SECOND = probe.getenv_num("LEGAIA_SECOND_SEAT", 0)
-- Re-top the player's HP every N ticks so long fights survive to see many
-- special cycles (0 = off).
local HEAL = probe.getenv_num("LEGAIA_HEAL", 0)
local POKES_RAW = probe.getenv("LEGAIA_POKES", "")

local pokes = {}
for pair in string.gmatch(POKES_RAW, "[^,%s]+") do
    local a, v, w = string.match(pair, "([^:]+):([^:]+):?(.*)")
    if a and v then
        pokes[#pokes + 1] = { addr = tonumber(a), val = tonumber(v), byte = (w == "b") }
    end
end

local CSV = probe.csv_open(probe.out_path("delilas_special_trace.csv"),
    "tick,mode,note")

local function reg(r, name)
    local ok, v = pcall(function() return tonumber(r.GPR.n[name]) % 0x100000000 end)
    if ok then return v end
    return 0
end

local installed = false
local last_mode = -1
-- Dedup consecutive identical tag-search rows (the stuck state repeats the
-- same failing search every frame): log the first hit and then a count.
local last_search = ""
local repeat_n = 0
local search_rows = 0

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE", ""),
    capture_frames = FRAMES,
    on_arm = function(ctx)
        probe.arm_breakpoint(MALLOC_FAIL, "Exec", 4, "malloc_fail", function()
            local r = PCSX.getRegisters()
            CSV:row("0,0,MALLOC FAILED size=0x%X", reg(r, "s1"))
        end)
        probe.arm_breakpoint(TAG_SEARCH, "Exec", 4, "tag_search", function()
            if not installed then return end
            if search_rows >= 4000 then return end
            local r = PCSX.getRegisters()
            local key = string.format("tag=0x%02X count=%d ra=0x%08X",
                reg(r, "a1"), reg(r, "a2"), reg(r, "ra"))
            if key == last_search then
                repeat_n = repeat_n + 1
                return
            end
            if repeat_n > 0 then
                CSV:row("0,0,search-repeat x%d: %s", repeat_n, last_search)
                search_rows = search_rows + 1
            end
            last_search = key
            repeat_n = 0
            search_rows = search_rows + 1
            CSV:row("0,0,search %s", key)
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
            if mode == 0x14 and FIRST ~= 0 then
                probe.write_u8(FORMATION, FIRST)
            end
            if mode == 0x14 and SECOND ~= 0 then
                probe.write_u8(FORMATION + 1, SECOND)
            end
            CSV:row("%d,0x%X,mode-change word=0x%X cells=%02X %02X", elapsed,
                mode, probe.read_u32(COURSE_WORD) or 0,
                probe.read_u8(FORMATION) or 0, probe.read_u8(FORMATION + 1) or 0)
            last_mode = mode
        end
        if installed then
            local phase = elapsed % 40
            if phase == 0 then
                pad.force(pad.BTN.CROSS)
            elseif phase == 6 then
                pad.release(pad.BTN.CROSS)
            end
            if HEAL ~= 0 and elapsed % 300 == 0 and mode == 0x15 then
                local a = probe.read_u32(ACTOR_TABLE) or 0
                if a > 0x80000000 and a < 0x80200000 then
                    probe.write_u16(a + 0x14C, HEAL)
                end
            end
            if elapsed % 60 == 0 and mode == 0x15 then
                local cells = {}
                for slot = 0, 5 do
                    local a = probe.read_u32(ACTOR_TABLE + slot * 4) or 0
                    if a > 0x80000000 and a < 0x80200000 then
                        cells[#cells + 1] = string.format(
                            "s%d anim=%02X/%02X act=%02X hp=%d", slot,
                            probe.read_u8(a + 0x1D9) or 0,
                            probe.read_u8(a + 0x1DA) or 0,
                            probe.read_u8(a + 0x1DF) or 0,
                            probe.read_u16(a + 0x14C) or 0)
                    end
                end
                -- ctx[0x28A] = the shared battle turn counter the Delilas AI
                -- arm keys its special cadence on (AI-retime calibration).
                local bctx = probe.read_u32(0x8007BD24) or 0
                local cnt = -1
                if bctx > 0x80000000 and bctx < 0x80200000 then
                    cnt = probe.read_u8(bctx + 0x28A) or -1
                end
                CSV:row("%d,0x%X,turn=%d actors %s", elapsed, mode, cnt,
                    table.concat(cells, " | "))
            end
        end
    end,
    on_done = function()
        if repeat_n > 0 then
            CSV:row("0,0,search-repeat x%d: %s", repeat_n, last_search)
        end
        CSV:row("0,0x%X,done", last_mode)
        pcall(function()
            local sstate = require("probe.sstate")
            sstate.save(probe.out_path("special_trace_end.sstate"))
        end)
        CSV:close()
    end,
})
