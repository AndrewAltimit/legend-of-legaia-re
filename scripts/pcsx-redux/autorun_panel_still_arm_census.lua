-- autorun_panel_still_arm_census.lua
--
-- Which arm of PROT 0978's streamer runs, and which texture pages the frame's
-- draw list selects afterwards.
--
-- PROT 0978 (`field_back_read`, slot-B base `0x801F69D8`, own content
-- `0x1000`) carries TWO streaming families behind one 12-arm phase machine
-- (`FUN_801F6B24`), and its image holds exactly eight `jal 0x800583C8`
-- (`LoadImage`) sites - four per family:
--
--   panel-still family   0x801F6C9C 0x801F6D40 0x801F6DE8 0x801F6E90
--                        each preceded by `addiu a0,s0,0x4c7` at
--                        0x801F6C3C / 0x801F6CCC / 0x801F6D70 / 0x801F6E18
--                        (raw TOC 0x4C7 / 0x4C8 = int.tim / int2.tim)
--   field-restore family 0x801F7078 0x801F7108 0x801F7190 0x801F7224
--                        (raw TOC 0x36C = extraction 874, the party pages)
--
-- A battle teardown was already measured to run the SECOND family. This probe
-- arms both, so "the still never loads here" is a recorded zero rather than an
-- absence of looking, and it reads the phase counter `_DAT_8007B6C8` at every
-- entry to the machine so the arm sequence is visible even when neither
-- family's upload fires.
--
-- The draw-list half answers the other leg: with `LEGAIA_OT_EVERY > 0` the
-- probe walks the ordering table handed to `DrawOTag` (`FUN_80058704`) every
-- N-th call and tallies the GP0 `0xE1` draw-mode words in it, whose low bits
-- are the selected texture page (`x_base = (w & 0xF) * 64`,
-- `y_base = ((w >> 4) & 1) * 256`). Pages 6..9 at `y = 0` are the `(384, 0)`
-- band both families upload into.
--
-- Outputs (probe.out_path):
--   arms.csv     one row per 0978 site hit: which site, the phase counter,
--                the arm VA the jump table holds, the `RECT` and `ra`.
--   images.csv   every LoadImage / StoreImage / MoveImage call's rect + ra.
--   tpages.csv   per sampled frame, the tally of selected texture pages.
--   teardown.csv ctx[+0xB] / ctx[+0xC] / loader tracker on every change.
--
-- Env: LEGAIA_SSTATE, LEGAIA_FRAMES (default 3000), LEGAIA_OT_EVERY
-- (default 0 = off), LEGAIA_OT_MAX_NODES (default 4000), LEGAIA_LABEL.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE   = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 3000)
local OT_EVERY = probe.getenv_num("LEGAIA_OT_EVERY", 0)
local OT_MAX   = probe.getenv_num("LEGAIA_OT_MAX_NODES", 4000)
local LABEL    = probe.getenv("LEGAIA_LABEL", "panel-still")

local CTX_PTR   = 0x8007BD24
local TRACKER   = 0x8007BC4C        -- loader-B tracker (extraction - 895)
local PHASE_CTR = 0x8007B6C8        -- 0978's own 12-arm phase counter
local ARM_TABLE = 0x801F6AA8        -- its jump table
local MACHINE   = 0x801F6B24        -- FUN_801F6B24, the machine entry
local SCUS_DRV  = 0x80025358        -- FUN_80025358, the ctx[+0xC]==2 ticker
local ARM_SITE  = 0x800474CC        -- writes ctx[+0xC] = 1

local SITES = {
    { name = "still_idx0",   addr = 0x801F6C3C },
    { name = "still_idx1",   addr = 0x801F6CCC },
    { name = "still_idx2",   addr = 0x801F6D70 },
    { name = "still_idx3",   addr = 0x801F6E18 },
    { name = "still_load0",  addr = 0x801F6C9C },
    { name = "still_load1",  addr = 0x801F6D40 },
    { name = "still_load2",  addr = 0x801F6DE8 },
    { name = "still_load3",  addr = 0x801F6E90 },
    { name = "restore_load0", addr = 0x801F7078 },
    { name = "restore_load1", addr = 0x801F7108 },
    { name = "restore_load2", addr = 0x801F7190 },
    { name = "restore_load3", addr = 0x801F7224 },
}

local IMAGE_SITES = {
    { name = "LoadImage",  addr = 0x800583C8 },
    { name = "StoreImage", addr = 0x8005842C },
    { name = "MoveImage",  addr = 0x80058490 },
}
local DRAW_OTAG = 0x80058704

local RECT_VA = 0x801F735C          -- the RECT both families share

local function u8(a)  return probe.read_u8(a)  or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
local function tou32(v)
    v = tonumber(v) or 0
    if v < 0 then v = v + 0x100000000 end
    return v
end
local function regs() local r = PCSX.getRegisters() return (r.GPR and r.GPR.n) or {} end
local function ctxp()
    local c = u32(CTX_PTR)
    if c < 0x80000000 or c >= 0x80200000 then return nil end
    return c
end

local arms_csv, images_csv, tpages_csv, teardown_csv
local site_hits = {}
local image_hits = {}
local vsync = 0
local otag_calls = 0
local last_teardown = ""

local function rect_fields(ptr)
    if ptr == nil or not probe.in_ram(ptr, 8) then return -1, -1, -1, -1 end
    return u16(ptr), u16(ptr + 2), u16(ptr + 4), u16(ptr + 6)
end

-- Walk one PSX ordering table and tally GP0 0xE1 draw-mode words. Each node
-- word is `(len << 24) | next24`; the node's `len` payload words follow it.
local function walk_ot(head)
    local pages = {}
    local node = bit.band(head, 0xFFFFFF)
    local seen = 0
    while node ~= 0xFFFFFF and seen < OT_MAX do
        local addr = bit.bor(node, 0x80000000)
        if not probe.in_ram(addr, 4) then break end
        local w = u32(addr)
        local len = bit.band(bit.rshift(w, 24), 0xFF)
        for i = 1, len do
            local pw = u32(addr + i * 4)
            if pw ~= nil and bit.band(bit.rshift(pw, 24), 0xFF) == 0xE1 then
                local px = bit.band(pw, 0xF)
                local py = bit.band(bit.rshift(pw, 4), 1)
                local key = px .. "/" .. py
                pages[key] = (pages[key] or 0) + 1
            end
        end
        node = bit.band(w, 0xFFFFFF)
        seen = seen + 1
    end
    return pages, seen
end

probe.run({
    sstate = SSTATE, capture_frames = FRAMES,
    on_arm = function()
        PCSX.log(string.format("== panel-still arm census == label=%s ot_every=%d", LABEL, OT_EVERY))
        probe.env.write_manifest("autorun_panel_still_arm_census.lua",
            { label = LABEL, sstate = SSTATE, frames = FRAMES, ot_every = OT_EVERY })
        arms_csv = probe.csv_open(probe.out_path("arms.csv"),
            "vsync,site,phase_ctr,arm_va,rect_x,rect_y,rect_w,rect_h,a0,ra,tracker")
        images_csv = probe.csv_open(probe.out_path("images.csv"),
            "vsync,call,rect_ptr,x,y,w,h,ra")
        tpages_csv = probe.csv_open(probe.out_path("tpages.csv"),
            "vsync,otag_call,nodes,page_x,page_y,count")
        teardown_csv = probe.csv_open(probe.out_path("teardown.csv"),
            "vsync,ctx_b,ctx_c,tracker,phase_ctr,slotb0")

        for _, s in ipairs(SITES) do
            site_hits[s.name] = 0
            local nm = s.name
            probe.arm_breakpoint(s.addr, "Exec", 4, nm, function()
                site_hits[nm] = site_hits[nm] + 1
                if site_hits[nm] > 64 then return end
                local n = regs()
                local ctr = u8(PHASE_CTR)
                local x, y, w, h = rect_fields(RECT_VA)
                arms_csv:row("%d,%s,%d,0x%08X,%d,%d,%d,%d,0x%08X,0x%08X,%d",
                    vsync, nm, ctr, u32(ARM_TABLE + ctr * 4), x, y, w, h,
                    tou32(n.a0), tou32(n.ra), u8(TRACKER))
            end)
        end
        probe.arm_breakpoint(MACHINE, "Exec", 4, "machine", function()
            site_hits.machine = (site_hits.machine or 0) + 1
            if site_hits.machine > 256 then return end
            local ctr = u8(PHASE_CTR)
            arms_csv:row("%d,machine,%d,0x%08X,,,,,,0x%08X,%d",
                vsync, ctr, u32(ARM_TABLE + ctr * 4), tou32(regs().ra), u8(TRACKER))
        end)
        probe.arm_breakpoint(SCUS_DRV, "Exec", 4, "scus_drv", function()
            site_hits.scus_drv = (site_hits.scus_drv or 0) + 1
        end)
        probe.arm_breakpoint(ARM_SITE, "Exec", 4, "arm_site", function()
            site_hits.arm_site = (site_hits.arm_site or 0) + 1
            arms_csv:row("%d,arm_site,%d,,,,,,,0x%08X,%d",
                vsync, u8(PHASE_CTR), tou32(regs().ra), u8(TRACKER))
        end)

        for _, s in ipairs(IMAGE_SITES) do
            local nm = s.name
            image_hits[nm] = 0
            probe.arm_breakpoint(s.addr, "Exec", 4, nm, function()
                image_hits[nm] = image_hits[nm] + 1
                if image_hits[nm] > 512 then return end
                local n = regs()
                local ptr = tou32(n.a0)
                local x, y, w, h = rect_fields(ptr)
                images_csv:row("%d,%s,0x%08X,%d,%d,%d,%d,0x%08X",
                    vsync, nm, ptr, x, y, w, h, tou32(n.ra))
            end)
        end

        if OT_EVERY > 0 then
            probe.arm_breakpoint(DRAW_OTAG, "Exec", 4, "drawotag", function()
                otag_calls = otag_calls + 1
                if otag_calls % OT_EVERY ~= 0 then return end
                local pages, nodes = walk_ot(tou32(regs().a0))
                for key, cnt in pairs(pages) do
                    local px, py = string.match(key, "(%d+)/(%d+)")
                    tpages_csv:row("%d,%d,%d,%s,%s,%d", vsync, otag_calls, nodes, px, py, cnt)
                end
            end)
        end
        return {}
    end,

    on_capture = function(_c, elapsed)
        vsync = elapsed
        local cx = ctxp()
        if cx == nil then return end
        local line = string.format("%d/%d/%d/%d",
            u8(cx + 0x0B), u8(cx + 0x0C), u8(TRACKER), u8(PHASE_CTR))
        if line ~= last_teardown then
            last_teardown = line
            teardown_csv:row("%d,%d,%d,%d,%d,0x%08X", elapsed,
                u8(cx + 0x0B), u8(cx + 0x0C), u8(TRACKER), u8(PHASE_CTR),
                u32(0x801F69D8))
        end
    end,

    on_done = function()
        for _, s in ipairs(SITES) do
            PCSX.log(string.format("[census] %-14s %d", s.name, site_hits[s.name] or 0))
        end
        PCSX.log(string.format("[census] machine=%d scus_drv=%d arm_site=%d otag=%d",
            site_hits.machine or 0, site_hits.scus_drv or 0,
            site_hits.arm_site or 0, otag_calls))
        for _, s in ipairs(IMAGE_SITES) do
            PCSX.log(string.format("[census] %-12s %d", s.name, image_hits[s.name] or 0))
        end
        if arms_csv then arms_csv:close() end
        if images_csv then images_csv:close() end
        if tpages_csv then tpages_csv:close() end
        if teardown_csv then teardown_csv:close() end
    end,
})
