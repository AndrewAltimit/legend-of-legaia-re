-- autorun_dance_texpage_upload.lua
--
-- Which upload stages the VRAM page at `(960, 256)` that the dance hall's
-- widget records 27 / 28 / 29 draw through (`texpage = 0x001F`)?
--
-- The page is not a member of PROT 1230, the minigame's own `prot::timpack`.
-- The hypothesis the bytes suggest is that it is not staged by the minigame at
-- all: the 4bpp TIM at raw `PROT.DAT` offset `0x11218` declares image origin
-- `(960, 256)`, 64 x 256 halfwords - exactly that page - and the three widget
-- CLUT ids `0x443D` / `0x447D` / `0x44BD` decode to `(976, 272/273/274)`,
-- i.e. rows 16 / 17 / 18 INSIDE that same image rectangle rather than a CLUT
-- block of their own.
--
-- This probe decides it by watching the whole libgpu upload band across the
-- minigame's load and asking whether anything writes into the page at all:
--
--   FUN_800583C8  LoadImage(RECT* a0, u_long* a1)   RAM -> VRAM (ra = caller)
--   FUN_80058490  MoveImage(RECT* a0, int a1, int a2) VRAM -> VRAM
--   FUN_80059BD4  the queue handler that performs the transfer
--
-- Hits are aggregated per (kind, ra, rect) - the overworld CLUT cycle and the
-- per-frame HUD refills would otherwise bury the load window - and every rect
-- that intersects x >= 960 && y >= 256 is called out separately.
--
-- Env vars:
--   LEGAIA_SSTATE      save state (run_probe.sh --scenario <label>)
--   LEGAIA_FRAMES      capture vsyncs (default 1800)
--   LEGAIA_MASH_BTN    button tapped to drive the transition (default CROSS)
--   LEGAIA_MASH_EVERY  vsyncs between taps (default 20)
--   LEGAIA_OUT_DIR     output directory
--
-- Outputs: dance_texpage_upload.csv, dance_texpage_upload.log

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE     = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES     = probe.getenv_num("LEGAIA_FRAMES", 1800)
local MASH_BTN   = probe.getenv("LEGAIA_MASH_BTN", "CROSS")
local MASH_EVERY = probe.getenv_num("LEGAIA_MASH_EVERY", 20)

local OUT_CSV = probe.out_path("dance_texpage_upload.csv")
local OUT_LOG = probe.out_path("dance_texpage_upload.log")

local LOAD_IMAGE = 0x800583C8
local MOVE_IMAGE = 0x80058490
local VRAM_WRITE = 0x80059BD4
local SCENE_NAME = 0x8007050C
local GAME_MODE  = 0x8007B83C
local LOADER_B   = 0x8007BC4C   -- loader-B in-flight tracker

-- Page of interest: the 4bpp texpage 0x1F.
local PAGE_X, PAGE_Y = 960, 256

local TAPS = {
    { addr = LOAD_IMAGE, want = 0x27BDFFE0, kind = "LoadImage" },
    { addr = MOVE_IMAGE, want = 0x27BDFFE0, kind = "MoveImage" },
    { addr = VRAM_WRITE, want = 0x27BDFFB0, kind = "VramWrite" },
}

local lines = {}
local function logf(fmt, ...)
    local s = string.format(fmt, ...)
    lines[#lines + 1] = s
    PCSX.log("[texpage] " .. s)
end

local function u8(a) return probe.read_u8(a) or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
local function n32(v) return bit.band(tonumber(v) or 0, 0xFFFFFFFF) end
local function hex8(v) return string.upper(bit.tohex(n32(v))) end
local function s16(v)
    v = bit.band(tonumber(v) or 0, 0xFFFF)
    if v >= 0x8000 then v = v - 0x10000 end
    return v
end

local function scene_name()
    local out = {}
    for i = 0, 7 do
        local b = probe.read_u8(SCENE_NAME + i)
        if b == nil or b < 0x20 or b >= 0x7F then break end
        out[#out + 1] = string.char(b)
    end
    return table.concat(out)
end

local csv
local g_elapsed = 0
local agg = {}         -- key -> { n, first, last, kind, ra, x, y, w, h, src, hit }
local n_total, n_page = 0, 0
local scenes = {}

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,

    on_arm = function()
        csv = probe.csv_open(OUT_CSV,
            "kind,ra,x,y,w,h,src,count,first_vsync,last_vsync," ..
            "touches_page_960_256,first_scene,first_mode")
        probe.env.write_manifest("autorun_dance_texpage_upload.lua", {
            sstate = SSTATE, frames = FRAMES, mash_btn = MASH_BTN,
            mash_every = MASH_EVERY,
        })
        local descs = {}
        for _, t in ipairs(TAPS) do
            local d = { addr = t.addr, hits_ref = { n = 0 }, name = t.kind }
            probe.arm_breakpoint(t.addr, "Exec", 4, t.kind, function()
                d.hits_ref.n = d.hits_ref.n + 1
                local r = PCSX.getRegisters()
                local rect = n32(r.GPR.n.a0)
                if rect < 0x80000000 or rect >= 0x80200000 then return end
                local x, y = s16(u16(rect)), s16(u16(rect + 2))
                local w, h = s16(u16(rect + 4)), s16(u16(rect + 6))
                local src = n32(r.GPR.n.a1)
                local ra  = n32(r.GPR.n.ra)
                n_total = n_total + 1
                local touches = (x + w > PAGE_X) and (y + h > PAGE_Y) and
                                (x < PAGE_X + 64) and (y < PAGE_Y + 256)
                if touches then n_page = n_page + 1 end
                local key = string.format("%s|%s|%d|%d|%d|%d|%s",
                    t.kind, hex8(ra), x, y, w, h, hex8(src))
                local e = agg[key]
                if e then
                    e.n = e.n + 1
                    e.last = g_elapsed
                else
                    agg[key] = { n = 1, first = g_elapsed, last = g_elapsed,
                                 kind = t.kind, ra = ra, x = x, y = y,
                                 w = w, h = h, src = src, touches = touches,
                                 scene = scene_name(),
                                 mode = u8(GAME_MODE) }
                    if touches then
                        logf("PAGE HIT: %s ra=0x%s rect=(%d,%d,%d,%d) src=0x%s "
                             .. "vsync=%d scene=%s mode=0x%02X loaderB=0x%s",
                             t.kind, hex8(ra), x, y, w, h, hex8(src),
                             g_elapsed, scene_name(), u8(GAME_MODE),
                             hex8(u32(LOADER_B)))
                    end
                end
            end)
            descs[#descs + 1] = d
        end
        return descs
    end,

    on_capture = function(ctx, elapsed)
        g_elapsed = elapsed
        if elapsed == 2 then
            for _, t in ipairs(TAPS) do
                logf("fingerprint [0x%08X] = 0x%s (want 0x%s, %s) %s",
                     t.addr, hex8(u32(t.addr)), hex8(t.want), t.kind,
                     u32(t.addr) == t.want and "OK" or "MISMATCH")
            end
            logf("start scene=%s mode=0x%02X", scene_name(), u8(GAME_MODE))
        end
        local sc = scene_name()
        if sc ~= "" and not scenes[sc] then
            scenes[sc] = elapsed
            logf("scene %s at vsync %d mode 0x%02X", sc, elapsed, u8(GAME_MODE))
        end
        probe.pad_release(probe.BTN[MASH_BTN])
        local sub = elapsed % MASH_EVERY
        if sub >= math.floor(MASH_EVERY / 2) and
           sub < math.floor(MASH_EVERY / 2) + 3 then
            probe.pad_force(probe.BTN[MASH_BTN])
        end
    end,

    on_summary = function()
        probe.pad_release(probe.BTN[MASH_BTN])
        local keys = {}
        for k in pairs(agg) do keys[#keys + 1] = k end
        table.sort(keys)
        for _, k in ipairs(keys) do
            local e = agg[k]
            csv:row("%s,0x%s,%d,%d,%d,%d,0x%s,%d,%d,%d,%s,%s,0x%02X",
                e.kind, hex8(e.ra), e.x, e.y, e.w, e.h, hex8(e.src),
                e.n, e.first, e.last, e.touches and "yes" or "no",
                e.scene, e.mode)
        end
        logf("uploads seen=%d distinct=%d ; rects touching (960,256)=%d",
             n_total, #keys, n_page)
        local sl = {}
        for s, v in pairs(scenes) do sl[#sl + 1] = string.format("%s@%d", s, v) end
        table.sort(sl)
        logf("scenes: %s", table.concat(sl, " "))
        local fh = io.open(OUT_LOG, "w")
        if fh then fh:write(table.concat(lines, "\n") .. "\n"); fh:close() end
        if csv then csv:close() end
    end,
})
