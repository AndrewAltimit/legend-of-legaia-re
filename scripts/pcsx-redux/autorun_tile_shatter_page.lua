-- autorun_tile_shatter_page.lua
--
-- Pin the ONE missing input of the battle-intro tile shatter (style 2):
-- the 4bpp shade page at VRAM (448, 0) the four semi-transparent side
-- faces sample (tpage 0x0027, CLUT 0x7641 = (16, 473)). The page is live
-- only DURING a field-to-battle transition, and no catalogued state is
-- mid-transition - so this probe manufactures one:
--
--   1. load the karisto_sol_pre_encounter state (overworld, one step
--      from a random encounter),
--   2. hold RIGHT so the walk rolls a battle,
--   3. exec-BP on FUN_801D0D24 (the style-2 per-frame tick, overlay
--      0979 field_battle_intro) - the first hit IS a mid-transition
--      frame - and write raw save states on hits 1 / 8 / 24,
--   4. in parallel, exec-BP the libgpu uploaders (LoadImage
--      FUN_800583C8, MoveImage FUN_80058490) from load onward, logging
--      every rect; when a LoadImage lands inside (448..463, 0..63) or
--      the CLUT row (y == 473), ALSO dump the source RAM bytes to a
--      .bin so the page content and its RAM source address are both
--      captured even if the save-state timing missed the upload.
--
-- The states decode offline with extract_vram_from_sstate.py +
-- decode_vram.py; the (448,0) 64x64 4bpp page is VRAM halfword columns
-- 448..463, rows 0..63, and the CLUT is the 16 halfwords at (16, 473).
--
--   bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_tile_shatter_page.lua \
--     --scenario karisto_sol_pre_encounter --frames 3600
--
-- Interpreter mode required (exec BPs). Kill on timeout from the
-- caller; PCSX-Redux does not exit on its own if the probe wedges.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 3600)

-- SCUS (always resident)
local GMODE      = 0x8007B83C   -- game mode; battle = 0x15
local LOADIMAGE  = 0x800583C8   -- libgpu LoadImage(rect*, src)
local MOVEIMAGE  = 0x80058490   -- libgpu MoveImage(rect*, dx, dy)
-- overlay 0979 field_battle_intro (resident only during the transition)
local TILE_TICK  = 0x801D0D24   -- FUN_801D0D24 - style-2 per-frame tick
local STYLE_SEL  = 0x801D2460   -- DAT_801D2460 - style selector 0..4
local SUB_STYLE  = 0x801D2464   -- DAT_801D2464 - tile sub-style

local function tou32(v)
    v = tonumber(v) or 0
    if v < 0 then v = v + 0x100000000 end
    return v
end
local function toi16(v) if v >= 0x8000 then return v - 0x10000 end return v end

local csv = probe.csv_open(probe.out_path("tile_shatter_uploads.csv"),
    "t,kind,x,y,w,h,src,ra,gmode")

local elapsed_now = 0
local tick_hits = 0
local dumps = 0

local function write_rawsstate(label)
    local ok = pcall(function()
        local w = PCSX.createSaveState()
        local path = probe.out_path(label .. ".rawsstate")
        local fh = Support.File.open(path, "CREATE")
        fh:writeMoveSlice(w)
        fh:close()
        PCSX.log("[shatter] state written: " .. path)
    end)
    if not ok then PCSX.log("[shatter] state write FAILED: " .. label) end
end

-- Dump `n` bytes of guest RAM at `addr` to a raw .bin next to the CSV.
local function dump_ram(label, addr, n)
    local bytes = probe.read_bytes(addr, n)
    if bytes == nil then
        PCSX.log(string.format("[shatter] dump_ram FAILED at 0x%08X", addr))
        return
    end
    local path = probe.out_path(label .. ".bin")
    local fh = io.open(path, "wb")
    if fh == nil then return end
    fh:write(bytes)
    fh:close()
    PCSX.log(string.format("[shatter] dumped %d bytes @0x%08X -> %s",
        n, addr, path))
end

local function rect_at(ptr)
    local x = probe.read_u16(ptr)
    if x == nil then return nil end
    return toi16(x),
        toi16(probe.read_u16(ptr + 2) or 0),
        toi16(probe.read_u16(ptr + 4) or 0),
        toi16(probe.read_u16(ptr + 6) or 0)
end

local function arm()
    probe.arm_breakpoint(LOADIMAGE, "Exec", 4, "loadimage", function()
        local r = PCSX.getRegisters()
        local a0 = tou32(r.GPR.n.a0)
        local a1 = tou32(r.GPR.n.a1)
        local ra = tou32(r.GPR.n.ra)
        local x, y, w, h = rect_at(a0)
        if x == nil then return end
        csv:row("%d,load,%d,%d,%d,%d,0x%08X,0x%08X,0x%02X",
            elapsed_now, x, y, w, h, a1, ra, probe.read_u8(GMODE) or 0)
        -- The shade page: 4bpp 64x64 = 16 halfword columns at (448,0).
        -- Any LoadImage overlapping columns 448..463, rows 0..63.
        if x >= 448 and x < 464 and y < 64 and w > 0 and h > 0 then
            dumps = dumps + 1
            dump_ram(string.format("shade_page_%02d_%dx%d_at_%d_%d",
                dumps, w, h, x, y), a1, w * h * 2)
        end
        -- The CLUT row band around (16, 473).
        if y >= 470 and y <= 476 then
            dumps = dumps + 1
            dump_ram(string.format("clut_row_%02d_%dx%d_at_%d_%d",
                dumps, w, h, x, y), a1, w * h * 2)
        end
    end)

    probe.arm_breakpoint(MOVEIMAGE, "Exec", 4, "moveimage", function()
        local r = PCSX.getRegisters()
        local a0 = tou32(r.GPR.n.a0)
        local a1 = tou32(r.GPR.n.a1)
        local a2 = tou32(r.GPR.n.a2)
        local ra = tou32(r.GPR.n.ra)
        local x, y, w, h = rect_at(a0)
        if x == nil then return end
        csv:row("%d,move,%d,%d,%d,%d,dst=%d:%d,0x%08X,0x%02X",
            elapsed_now, x, y, w, h, a1, a2, ra, probe.read_u8(GMODE) or 0)
    end)

    probe.arm_breakpoint(TILE_TICK, "Exec", 4, "tile_tick", function()
        tick_hits = tick_hits + 1
        if tick_hits <= 24 then
            -- The per-tile view matrix (scratch 0x1F8003C8: 3x3 i16
            -- rotation + pad + 3 i32 translation at +0x14) and the near
            -- cutoff halfword at 0x1F80037E, per hit - the matrix is
            -- rewritten per frame, so hit 1 may still be showing the
            -- field camera's last value.
            local vm = {}
            for i = 0, 7 do
                vm[#vm + 1] = string.format("%08X",
                    probe.mem.read_scratch_u32(0x1F8003C8 + i * 4))
            end
            PCSX.log(string.format(
                "[shatter] hit%d viewmat: %s near37E=%04X",
                tick_hits, table.concat(vm, " "),
                probe.mem.read_scratch_u32(0x1F80037C) or 0))
        end
        if tick_hits == 1 then
            PCSX.log(string.format(
                "[shatter] FIRST tile tick: style=%d sub=%d gmode=0x%02X t=%d",
                probe.read_u32(STYLE_SEL) or -1,
                probe.read_u32(SUB_STYLE) or -1,
                probe.read_u8(GMODE) or 0, elapsed_now))
            local bm = {}
            for i = 0, 7 do
                bm[#bm + 1] = string.format("%08X",
                    probe.read_u32(0x8007BF10 + i * 4) or 0)
            end
            PCSX.log("[shatter] base matrix @8007BF10: " ..
                table.concat(bm, " "))
            write_rawsstate("mid_shatter_hit1")
        elseif tick_hits == 8 then
            write_rawsstate("mid_shatter_hit8")
        elseif tick_hits == 24 then
            write_rawsstate("mid_shatter_hit24")
        end
    end)
    -- One probe INSIDE the per-tile loop, right after the MVMVA wrapper
    -- (FUN_8003D344) stored the transformed record position into
    -- 0x1F800348..0x350: the actual per-tile GTE translation. Settles
    -- what the view matrix contributes without decoding the wrapper.
    local mvmva_logged = 0
    probe.arm_breakpoint(0x801D0DBC, "Exec", 4, "tile_mvmva_out", function()
        if mvmva_logged >= 6 then return end
        mvmva_logged = mvmva_logged + 1
        PCSX.log(string.format(
            "[shatter] mvmva out #%d: t=(%d, %d, %d)",
            mvmva_logged,
            (probe.mem.read_scratch_u32(0x1F800348) or 0) - ((probe.mem.read_scratch_u32(0x1F800348) or 0) >= 0x80000000 and 0x100000000 or 0),
            (probe.mem.read_scratch_u32(0x1F80034C) or 0) - ((probe.mem.read_scratch_u32(0x1F80034C) or 0) >= 0x80000000 and 0x100000000 or 0),
            (probe.mem.read_scratch_u32(0x1F800350) or 0) - ((probe.mem.read_scratch_u32(0x1F800350) or 0) >= 0x80000000 and 0x100000000 or 0)))
    end)
    PCSX.log("[shatter] armed LoadImage/MoveImage/tile-tick; walking RIGHT")
end

probe.run({
    sstate = SSTATE_PATH,
    capture_frames = FRAMES,
    on_arm = function(c)
        PCSX.log("== tile-shatter shade-page capture ==")
        probe.write_manifest("autorun_tile_shatter_page.lua", {
            sstate = SSTATE_PATH, frames = FRAMES,
            tile_tick = string.format("0x%08X", TILE_TICK),
        })
        arm()
        return {}
    end,
    on_capture = function(c, elapsed)
        elapsed_now = elapsed
        -- Walk right into the encounter. Keep holding through the
        -- transition; the pad does not affect the intro.
        if elapsed == 10 then probe.pad_force(probe.BTN.RIGHT) end
        -- All three states written -> done.
        if tick_hits >= 24 then
            c.request_quit = true
        end
        if elapsed % 300 == 0 then
            PCSX.log(string.format(
                "[diag t%d] gmode=0x%02X ticks=%d dumps=%d",
                elapsed, probe.read_u8(GMODE) or 0, tick_hits, dumps))
        end
    end,
    on_done = function(c)
        csv:close()
        PCSX.log(string.format(
            "[shatter] done: tick_hits=%d dumps=%d", tick_hits, dumps))
    end,
})
