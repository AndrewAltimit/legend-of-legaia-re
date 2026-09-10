-- autorun_gpu_call_census.lua
--
-- Census of every non-ordering-table GPU call retail makes across a run,
-- plus every window-chrome emit. Two questions it answers at once:
--
--   * does ANYTHING sample VRAM `(384, 0)` - the rect PROT 0978 streams the
--     ringside panel stills into - across a whole battle teardown? The
--     earlier negatives came from periodic RAM sweeps and ordering-table
--     walks; this watches the libgpu entry points themselves, so a one-shot
--     blit or a display-origin flip cannot fall between samples.
--   * which arm of the SCUS window emitter `FUN_8002C69C` lays a given
--     window, by logging its rect together with the style selector
--     `gp[+0x14C]` and the kind byte at `0x800732A4 + style*12` that
--     indexes its seven-arm jump table at `0x80010D18`.
--
-- Watched entry points (PsyQ libgpu, statically linked into SCUS):
--   0x800583C8  LoadImage(RECT*, src)          CPU -> VRAM
--   0x8005842C  StoreImage(RECT*, dst)         VRAM -> CPU
--   0x80058490  MoveImage(RECT*, dx, dy)       VRAM -> VRAM
--   0x800589D0  PutDispEnv(DISPENV*)           display origin / size
--   0x8005A094  GP1 command issue (a0 = word)  incl. 0x05 display start
--   0x8005A0D0  direct GP0 FIFO write (a0 = word list, a1 = count)
--   0x8002C69C  window emitter (a0..a3 = x, y, w, h)
--
-- Outputs (LEGAIA_OUT_DIR): gpu_calls.csv, window_calls.csv, summary.txt.
--
-- Env: LEGAIA_SSTATE, LEGAIA_FRAMES, LEGAIA_MAX_ROWS (per-site row cap,
-- default 4000), LEGAIA_HIT_X (VRAM x to flag, default 384).

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE   = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 1800)
local MAX_ROWS = probe.getenv_num("LEGAIA_MAX_ROWS", 4000)
local HIT_X    = probe.getenv_num("LEGAIA_HIT_X", 384)

local WINDOW_PC   = 0x8002C69C
local STYLE_OFF   = 0x14C          -- gp[+0x14C] = window style id
local STYLE_TABLE = 0x800732A4     -- 12-byte per-style descriptor
local TRACKER     = 0x8007BC4C     -- loader-B tracker (extraction - 895)
local MODE_VA     = 0x8007B83C

local IMAGE_SITES = {
    { name = "LoadImage",   addr = 0x800583C8, rect = "a0" },
    { name = "StoreImage",  addr = 0x8005842C, rect = "a0" },
    { name = "MoveImage",   addr = 0x80058490, rect = "a0" },
    { name = "PutDispEnv",  addr = 0x800589D0, rect = "a0" },
}
local GP1_PC = 0x8005A094
local GP0_PC = 0x8005A0D0

local gpu_rows, win_rows = {}, {}
local counts = {}
local hits384 = 0
local vsync = 0

local function bump(k)
    counts[k] = (counts[k] or 0) + 1
end

local function rect_at(ptr)
    if ptr == nil or not probe.in_ram(ptr, 8) then return nil end
    return probe.read_u16(ptr), probe.read_u16(ptr + 2),
           probe.read_u16(ptr + 4), probe.read_u16(ptr + 6)
end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,

    on_arm = function()
        for _, s in ipairs(IMAGE_SITES) do
            local name = s.name
            probe.bp.arm(s.addr, "Exec", 4, name, function()
                bump(name)
                local r = PCSX.getRegisters().GPR.n
                local ptr = bit.band(tonumber(r.a0), 0xFFFFFFFF)
                local x, y, w, h = rect_at(ptr)
                if x == nil then return end
                if x == HIT_X then hits384 = hits384 + 1 end
                if #gpu_rows >= MAX_ROWS then return end
                gpu_rows[#gpu_rows + 1] = string.format(
                    "%d,%s,0x%08X,%d,%d,%d,%d,%d,%d,0x%08X",
                    vsync, name, ptr, x, y, w, h,
                    bit.band(tonumber(r.a1), 0xFFFF),
                    bit.band(tonumber(r.a2), 0xFFFF),
                    bit.band(tonumber(r.ra), 0xFFFFFFFF))
            end)
        end

        probe.bp.arm(GP1_PC, "Exec", 4, "GP1", function()
            bump("GP1")
            local r = PCSX.getRegisters().GPR.n
            local cmd = bit.band(tonumber(r.a0), 0xFFFFFFFF)
            local op = bit.rshift(cmd, 24)
            bump(string.format("GP1_%02X", op))
            -- GP1(0x05) is the display-area start: x = bits 0..9.
            if op == 0x05 then
                local x = bit.band(cmd, 0x3FF)
                local y = bit.band(bit.rshift(cmd, 10), 0x1FF)
                if x == HIT_X then hits384 = hits384 + 1 end
                if #gpu_rows < MAX_ROWS then
                    gpu_rows[#gpu_rows + 1] = string.format(
                        "%d,GP1_DISPSTART,0x%08X,%d,%d,0,0,0,0,0x%08X",
                        vsync, cmd, x, y, bit.band(tonumber(r.ra), 0xFFFFFFFF))
                end
            end
        end)

        probe.bp.arm(GP0_PC, "Exec", 4, "GP0_list", function()
            bump("GP0_list")
            local r = PCSX.getRegisters().GPR.n
            local list = bit.band(tonumber(r.a0), 0xFFFFFFFF)
            local n = bit.band(tonumber(r.a1), 0xFFFF)
            if not probe.in_ram(list, 4) then return end
            -- Flag a VRAM-to-VRAM / VRAM-to-CPU transfer whose source page
            -- is (HIT_X, *): GP0 0x80/0xC0 carry a source (x, y) word next.
            for i = 0, math.min(n, 16) - 1 do
                local w0 = probe.read_u32(list + i * 4) or 0
                local op = bit.rshift(w0, 24)
                if op == 0x80 or op == 0xC0 or op == 0xA0 then
                    local src = probe.read_u32(list + (i + 1) * 4) or 0
                    local x = bit.band(src, 0xFFFF)
                    if x == HIT_X then hits384 = hits384 + 1 end
                    if #gpu_rows < MAX_ROWS then
                        gpu_rows[#gpu_rows + 1] = string.format(
                            "%d,GP0_%02X,0x%08X,%d,%d,0,0,0,0,0x%08X",
                            vsync, op, list, x,
                            bit.band(bit.rshift(src, 16), 0xFFFF),
                            bit.band(tonumber(r.ra), 0xFFFFFFFF))
                    end
                end
            end
        end)

        probe.bp.arm(WINDOW_PC, "Exec", 4, "window_emit", function()
            bump("window_emit")
            local r = PCSX.getRegisters().GPR.n
            local gp = bit.band(tonumber(r.gp), 0xFFFFFFFF)
            local style = probe.read_u32(gp + STYLE_OFF) or 0
            local rec = STYLE_TABLE + style * 12
            local kind = probe.read_u8(rec) or 0xFF
            local tiles = probe.read_u8(rec + 1) or 0xFF
            local clut = probe.read_u8(rec + 3) or 0xFF
            bump(string.format("window_style_%02X", bit.band(style, 0xFF)))
            if #win_rows >= MAX_ROWS then return end
            win_rows[#win_rows + 1] = string.format(
                "%d,%d,%d,%d,%d,0x%02X,%d,%d,0x%02X,0x%08X",
                vsync,
                bit.band(tonumber(r.a0), 0xFFFF),
                bit.band(tonumber(r.a1), 0xFFFF),
                bit.band(tonumber(r.a2), 0xFFFF),
                bit.band(tonumber(r.a3), 0xFFFF),
                bit.band(style, 0xFF), kind, tiles, clut,
                bit.band(tonumber(r.ra), 0xFFFFFFFF))
        end)

        return {}
    end,

    on_capture = function(_, elapsed)
        vsync = elapsed
        if elapsed % 120 ~= 0 then return end
        PCSX.log(string.format(
            "[gpucensus] vsync=%d mode=0x%02X tracker=%d hits(x=%d)=%d"
            .. " window=%d gp1=%d load=%d move=%d store=%d",
            elapsed, probe.read_u8(MODE_VA) or 0,
            probe.read_u32(TRACKER) or 0, HIT_X, hits384,
            counts["window_emit"] or 0, counts["GP1"] or 0,
            counts["LoadImage"] or 0, counts["MoveImage"] or 0,
            counts["StoreImage"] or 0))
    end,

    on_done = function()
        local fh = io.open(probe.out_path("gpu_calls.csv"), "w")
        if fh then
            fh:write("vsync,site,arg0,x,y,w,h,a1,a2,ra\n")
            for _, row in ipairs(gpu_rows) do fh:write(row .. "\n") end
            fh:close()
        end
        local wh = io.open(probe.out_path("window_calls.csv"), "w")
        if wh then
            wh:write("vsync,x,y,w,h,style,kind,tileset,clut_byte,ra\n")
            for _, row in ipairs(win_rows) do wh:write(row .. "\n") end
            wh:close()
        end
        local sh = io.open(probe.out_path("summary.txt"), "w")
        if sh then
            sh:write(string.format("vsyncs=%d\n", vsync))
            sh:write(string.format("vram_x_%d_hits=%d\n", HIT_X, hits384))
            local keys = {}
            for k in pairs(counts) do keys[#keys + 1] = k end
            table.sort(keys)
            for _, k in ipairs(keys) do
                sh:write(string.format("%s=%d\n", k, counts[k]))
            end
            sh:close()
        end
        PCSX.log(string.format("[gpucensus] done: x=%d hits=%d rows=%d/%d",
            HIT_X, hits384, #gpu_rows, #win_rows))
    end,
})
