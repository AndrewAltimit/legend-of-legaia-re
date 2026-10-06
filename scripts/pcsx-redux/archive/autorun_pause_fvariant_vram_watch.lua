-- autorun_pause_fvariant_vram_watch.lua
--
-- Stage 1 of pinning the pause-menu-path writer of the extraction-0874 s2
-- (player.lzs) F-variant pixels: VRAM row 271 words at x = 853/856/857
-- flipping 3333->ffff / 3333->fff3 / 1e33->1e3f (each equal to the disc word
-- two rows down at (x,273)).
--
-- Polls the six halfwords (851..858, row 271) via PCSX.getVRAM() every vsync
-- while a scripted SELECT press opens the pause menu from a field state. On
-- the first change: logs the vsync + old/new words, dumps full main RAM, and
-- scans it in-Lua for (a) GP0 A0h/80h image-copy packets whose rect covers
-- the changed words and (b) a 16-byte needle of the new row-271 content, so
-- the staging buffer VA comes out of the same run. Stage 2 write-watches
-- that VA for the builder PC.
--
--   LEGAIA_FRAMES=400 \
--       timeout --kill-after=30s 900s bash scripts/pcsx-redux/run_probe.sh \
--       --scenario field_walled_collision_pin \
--       --lua scripts/pcsx-redux/autorun_pause_fvariant_vram_watch.lua

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad = require("probe.pad")

local SSTATE = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 400)
local OUT_DIR = probe.getenv("LEGAIA_OUT_DIR", "/tmp/pausevram2")
local SELECT_AT = probe.getenv_num("LEGAIA_SELECT_AT", 60)
local SELECT_HOLD = 8

local GAME_MODE_VA = 0x8007b83c
local ROW = 271
local X0, X1 = 851, 859   -- halfword x window [X0, X1)
local ROW_BYTES = 2048

os.execute(string.format("mkdir -p %q", OUT_DIR))

local function get_vram()
    local ok, data = pcall(function()
        if PCSX.getVRAM ~= nil then return PCSX.getVRAM() end
        if PCSX.GPU and PCSX.GPU.getVRAM then return PCSX.GPU.getVRAM() end
        return nil
    end)
    if not ok or data == nil then return nil end
    return tostring(data)
end

local function band(vram, row, x0, x1)
    return vram:sub(row * ROW_BYTES + x0 * 2 + 1, row * ROW_BYTES + x1 * 2)
end

local function hex(s)
    local out = {}
    for i = 1, #s do out[#out + 1] = string.format("%02x", s:byte(i)) end
    return table.concat(out)
end

local function dump_full_ram(path)
    local mf = PCSX.getMemoryAsFile()
    local fh = io.open(path, "wb")
    if fh == nil then return false end
    local CHUNK = 65536
    for off = 0, 0x200000 - CHUNK, CHUNK do
        local s = mf:readAt(CHUNK, off)
        fh:write(tostring(s))
    end
    fh:close()
    return true
end

-- Scan a RAM blob for GP0 A0h/80h packets covering (x=853..858, y=271) and
-- for the needle bytes. Returns log lines.
local function scan_ram(ram, needle_new, needle_src)
    local lines = {}
    local function u16(off)  -- 1-based blob offset
        return ram:byte(off) + ram:byte(off + 1) * 256
    end
    for off = 1, #ram - 16, 4 do
        local cmd = ram:byte(off + 3)
        if cmd == 0xA0 then
            local x, y = u16(off + 4), u16(off + 6)
            local w, h = u16(off + 8), u16(off + 10)
            if y <= ROW and ROW < y + math.max(h, 1)
                and x <= 853 and 858 <= x + math.max(w, 1)
                and w > 0 and w <= 1024 and h > 0 and h <= 512 then
                lines[#lines + 1] = string.format(
                    "A0 packet va=0x%08X dst=(%d,%d) %dx%d",
                    0x80000000 + off - 1, x, y, w, h)
            end
        elseif cmd == 0x80 then
            local sx, sy = u16(off + 4), u16(off + 6)
            local dx, dy = u16(off + 8), u16(off + 10)
            local w, h = u16(off + 12), u16(off + 14)
            if dy <= ROW and ROW < dy + math.max(h, 1)
                and dx <= 853 and 858 <= dx + math.max(w, 1)
                and w > 0 and w <= 1024 and h > 0 and h <= 512 then
                lines[#lines + 1] = string.format(
                    "80 packet va=0x%08X src=(%d,%d) dst=(%d,%d) %dx%d",
                    0x80000000 + off - 1, sx, sy, dx, dy, w, h)
            end
        end
    end
    for label, needle in pairs({ new_row271 = needle_new, src_row273 = needle_src }) do
        local at = 1
        while true do
            local s = ram:find(needle, at, true)
            if s == nil then break end
            lines[#lines + 1] = string.format(
                "needle %s at va=0x%08X", label, 0x80000000 + s - 1)
            at = s + 1
        end
    end
    return lines
end

local baseline = nil
local fired = false
local last_mode = nil
local released = false

probe.run({
    sstate         = SSTATE,
    capture_frames = FRAMES,
    out_path       = OUT_DIR .. "/watch.log",

    on_arm = function() return {} end,

    on_capture = function(_ctx, tick)
        local mode = probe.read_u8(GAME_MODE_VA)
        if mode ~= last_mode then
            PCSX.log(string.format("[fvariant] vsync=%d game_mode=0x%02x",
                tick, mode or 0xFF))
            last_mode = mode
        end
        if tick == SELECT_AT then
            pad.force(pad.BTN.SELECT)
            PCSX.log(string.format("[fvariant] vsync=%d press SELECT", tick))
        elseif tick == SELECT_AT + SELECT_HOLD and not released then
            pad.release(pad.BTN.SELECT)
            released = true
        end
        if fired then return end
        local vram = get_vram()
        if vram == nil then
            if tick == 1 then PCSX.log("[fvariant] getVRAM unavailable") end
            return
        end
        local cur = band(vram, ROW, X0, X1)
        if baseline == nil then
            baseline = cur
            PCSX.log(string.format("[fvariant] vsync=%d baseline row%d x%d..%d = %s",
                tick, ROW, X0, X1 - 1, hex(cur)))
            return
        end
        if cur ~= baseline then
            fired = true
            PCSX.log(string.format("[fvariant] vsync=%d CHANGE row%d: %s -> %s",
                tick, ROW, hex(baseline), hex(cur)))
            local src = band(vram, 273, X0, X1)
            PCSX.log(string.format("[fvariant]   row273 same window = %s", hex(src)))
            local ram_path = OUT_DIR .. string.format("/ram_at_%d.bin", tick)
            if dump_full_ram(ram_path) then
                PCSX.log("[fvariant] RAM dumped to " .. ram_path)
            end
            local ram_fh = io.open(ram_path, "rb")
            local ram = ram_fh and ram_fh:read("*a")
            if ram_fh then ram_fh:close() end
            if ram then
                -- 16-byte needles: new row-271 content x851.., and the
                -- row-273 source window.
                for _, line in ipairs(scan_ram(ram, cur, src)) do
                    PCSX.log("[fvariant] " .. line)
                end
                PCSX.log("[fvariant] RAM scan done")
            end
        end
    end,

    on_done = function()
        pad.release(pad.BTN.SELECT)
        PCSX.log(string.format("=== fvariant_vram_watch: fired=%s ===",
            tostring(fired)))
    end,
})
