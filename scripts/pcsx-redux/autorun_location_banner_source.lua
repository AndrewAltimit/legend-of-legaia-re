-- autorun_location_banner_source.lua
--
-- Pin the SOURCE of the on-entry location-name banner (the place name that
-- flashes at the top of the screen when the player walks into a town /
-- dungeon from the overworld).
--
-- Three candidate data sites carry a place name on this disc:
--   * SCUS 0x80073B18 - the 16 quick-travel landmark cells (static);
--   * the world-map location table the scene-5 pointer _DAT_80073EE0 walks
--     (29 x 0x20 records, kingdom-MAN resident, so 0x8010xxxx+);
--   * the per-scene MAN section 2 pointer _DAT_801C6EA0 (also 0x8010xxxx+).
-- The renderer FUN_80036888 draws EVERY on-screen string, so breaking on it
-- across a scene transition and classifying each a0 against those three
-- ranges answers which one the banner reads - and the two MAN-resident
-- candidates are told apart by comparing a0 against the live pointers.
--
-- Scenario: overworld_into_town_man_load (walkable area; a ~0.75s Down press
-- walks into a town entrance and loads a standalone town scene).

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local DRAW = 0x80036888
local NAME_TABLE = 0x80073B18 -- SCUS landmark cells
local SEC2_PTR = 0x801C6EA0 -- MAN section 2 (per-scene name)
local WM_TABLE_PTR = 0x80073EE0 -- MAN section 5 (world-map location table)
local LOC_BUF = 0x80084340 -- save-block location-name scratch

local OUT = probe.out_path("location_banner_source.txt")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 900)
local HOLD = probe.getenv_num("LEGAIA_HOLD", 55)
-- Which direction walks into the transition differs per scenario (the town
-- entrance is Down from the overworld anchor, the castle exit is Up).
local HOLD_BTN = probe.BTN[probe.getenv("LEGAIA_HOLD_BTN", "DOWN")] or probe.BTN.DOWN

local function str_at(p, n)
    local asc = {}
    for i = 0, n - 1 do
        local b = probe.read_u8(p + i) or 0
        if b == 0 then break end
        asc[#asc + 1] = (b >= 0x20 and b < 0x7F) and string.char(b) or "."
    end
    return table.concat(asc)
end

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE",
        os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1"),
    capture_frames = FRAMES,
    hold_button = HOLD_BTN,
    hold_frames = HOLD,

    on_arm = function(ctx)
        ctx.frame = 0
        ctx.seen = {}
        ctx.f = assert(io.open(OUT, "w"))
        ctx.f:write("# location-banner source trace\n")
        ctx.f:write("# cols: frame a0 class text\n")

        probe.arm_breakpoint(DRAW, "Exec", 4, "draw", function()
            local r = PCSX.getRegisters()
            local a0 = (tonumber(r.GPR.n.a0) or 0) % 0x100000000
            if a0 < 0x80000000 or a0 >= 0x80200000 then return end
            local text = str_at(a0, 24)
            if text == "" then return end
            -- One row per (string, pointer) pair; a banner redraws every frame.
            local key = string.format("%08X|%s", a0, text)
            if ctx.seen[key] then return end
            ctx.seen[key] = true

            local sec2 = probe.read_u32(SEC2_PTR) or 0
            local wmt = probe.read_u32(WM_TABLE_PTR) or 0
            local class = "other"
            if a0 >= NAME_TABLE and a0 < NAME_TABLE + 16 * 0x20 then
                class = string.format("SCUS_LANDMARK[%d]", (a0 - NAME_TABLE) / 0x20)
            elseif sec2 ~= 0 and a0 >= sec2 and a0 < sec2 + 0x20 then
                class = string.format("MAN_SEC2+%d", a0 - sec2)
            elseif wmt ~= 0 and a0 > wmt and a0 < wmt + 1 + 29 * 0x20 then
                local rel = a0 - (wmt + 1)
                class = string.format("WORLDMAP_TABLE rec=%d off=%d", rel / 0x20, rel % 0x20)
            elseif a0 >= LOC_BUF and a0 < LOC_BUF + 0x24 then
                class = "LOC_BUF"
            end
            ctx.f:write(string.format("%5d  0x%08X  %-28s %q\n",
                ctx.frame, a0, class, text))
            ctx.f:flush()
        end)
        return {}
    end,

    on_capture = function(ctx, elapsed)
        ctx.frame = elapsed
        if elapsed >= FRAMES - 2 then ctx.request_quit = true end
    end,

    on_done = function(ctx)
        local sec2 = probe.read_u32(SEC2_PTR) or 0
        local wmt = probe.read_u32(WM_TABLE_PTR) or 0
        ctx.f:write(string.format("\n# final _DAT_801C6EA0 = 0x%08X (%q)\n", sec2,
            sec2 ~= 0 and str_at(sec2, 24) or ""))
        local count = wmt ~= 0 and (probe.read_u8(wmt) or 0) or 0
        ctx.f:write(string.format("# final _DAT_80073EE0 = 0x%08X count=%d\n", wmt, count))
        -- Dump the live world-map location table: this is the label pass's own
        -- data, so a rename shows up here even in a state where no marker is
        -- discovered enough to be drawn.
        for i = 0, count - 1 do
            local rec = wmt + 1 + i * 0x20
            ctx.f:write(string.format("#   [%2d] region=%d (%3d,%3d) flag=%#06x %q\n",
                i, probe.read_u8(rec) or 0, probe.read_u8(rec + 1) or 0,
                probe.read_u8(rec + 2) or 0, probe.read_u16(rec + 3) or 0,
                str_at(rec + 8, 24)))
        end
        ctx.f:write(string.format("# final 0x80084340 = %q\n", str_at(LOC_BUF, 24)))
        ctx.f:close()
    end,
})
