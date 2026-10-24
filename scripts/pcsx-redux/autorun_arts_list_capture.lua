-- autorun_arts_list_capture.lua
--
-- Open the battle arts-entry screen's Triangle "Hyper Arts list" window and
-- save a state per page. Start from any retail state parked in the entry
-- (battle menu flow ctx[+0x06] == 0x50, e.g. the arts_bar_* scenarios).
--
-- Each Triangle press steps the pager FUN_801D3748 (closed -> page 1 ->
-- page 2 -> page 3 -> closed, row offset _DAT_8007B458 = 0 / 5 / 10);
-- FUN_801D3444 then slides the window (SCUS text actor, kind 0xC, rect
-- (-146, 36, 138, 144)) in by the frame step a battle frame until its
-- counter 0x801F4E0A reaches 12. The probe waits for the counter to settle
-- at 12 on each page, then saves arts_list_p<N>.sstate plus a screenshot
-- (.raw + .raw.meta, decode with scripts/pcsx-redux/decode_pcsx_screen.py).
--
-- No memory is poked: the only input is the pad.
--
-- Poll-only (no breakpoints) - safe to run --fast:
--   bash scripts/pcsx-redux/run_probe.sh --fast \
--     --lua scripts/pcsx-redux/autorun_arts_list_capture.lua \
--     --scenario arts_bar_astral_sword_vahn --out-dir <dir> --iso <scratch copy>
-- LEGAIA_PAGES (default 3) caps how many pages are saved.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local sstate = require("probe.sstate")

local CTX_PTR = 0x8007BD24
local LIST_BITS = 0x801F4E09
local SLIDE = 0x801F4E0A
local ROW_OFFSET = 0x8007B458
local PAGES = probe.getenv_num("LEGAIA_PAGES", 3)

local function flow_byte()
    local c = probe.read_u32(CTX_PTR)
    if c == nil or c < 0x80000000 or c >= 0x80200000 then return nil end
    return probe.read_u8(c + 6)
end

local function screenshot(stem)
    local ok, ss = pcall(PCSX.GPU.takeScreenShot)
    if not ok or ss == nil then return end
    local fh = io.open(probe.out_path(stem .. ".raw"), "wb")
    if fh == nil then return end
    fh:write(tostring(ss.data))
    fh:close()
    local mh = io.open(probe.out_path(stem .. ".raw.meta"), "w")
    if mh then
        mh:write(string.format("width=%d\nheight=%d\nbpp=%d\n",
            ss.width or 320, ss.height or 228,
            (ss.bpp == "BPP_24") and 24 or 16))
        mh:close()
    end
end

local page = 0          -- pages saved so far
local press_at = nil    -- vsync of the current Triangle press
local settled_at = nil  -- vsync the counter first read 12 on this page

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE",
        os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1"),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 900),
    on_arm = function() return {} end,
    on_capture = function(ctx, el)
        if press_at ~= nil and el >= press_at + 4 then
            probe.pad_release(probe.BTN.TRIANGLE)
        end
        local f = flow_byte()
        local bits = probe.read_u8(LIST_BITS) or 0
        local slide = probe.read_u8(SLIDE) or 0
        local row = probe.read_u32(ROW_OFFSET) or 0
        if el % 10 == 0 then
            PCSX.log(string.format("[artslist] f=%d flow=%s bits=0x%02X slide=%d row=%d",
                el, tostring(f), bits, slide, row))
        end
        if page >= PAGES then
            ctx.request_quit = true
            return
        end
        -- Press Triangle once per page: the first after the state settles,
        -- each later one after the previous page was saved.
        if press_at == nil and f == 0x50 and el >= 20 then
            probe.pad_force(probe.BTN.TRIANGLE)
            press_at = el
            settled_at = nil
            PCSX.log(string.format("[artslist] f=%d press Triangle (page %d)", el, page + 1))
            return
        end
        if press_at ~= nil and el >= press_at + 8 and bit.band(bits, 1) == 1 and slide == 12 then
            if settled_at == nil then settled_at = el end
            if el >= settled_at + 10 then
                page = page + 1
                local stem = string.format("arts_list_p%d", page)
                sstate.save(probe.out_path(stem .. ".sstate"))
                screenshot(stem)
                PCSX.log(string.format("[artslist] saved %s at f=%d row=%d", stem, el, row))
                press_at = nil
            end
        end
    end,
    on_done = function()
        PCSX.log(string.format("[artslist] done: pages=%d flow=%s", page, tostring(flow_byte())))
    end,
})
