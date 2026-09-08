-- autorun_battle_teardown_hook.lua
--
-- Battle-teardown capture answering two questions static reads cannot:
--
--   A. Which resident image does SCUS `jal 0x801F7B88` (at 0x800481A0, inside
--      the per-actor battle draw tick FUN_800480D8's scene-teardown preamble,
--      gated on `_DAT_8007BDC0 != 0`) actually reach? Sixty-five of the
--      sixty-eight slot-B overlay images are large enough to host a routine at
--      base+0x11B0, so the `jal` alone names nothing. An exec breakpoint on
--      the call site reads the loader-B tracker `0x8007BC4C` at that instant
--      (extraction index = 895 + tracker) plus the bytes at the target, which
--      together name the image. A run in which the site never fires is also an
--      answer: it says `_DAT_8007BDC0` is never raised on this path.
--
--   B. What shows the (384, 0) 320x256 dome panel still? The arming is settled
--      (`ctx[+0xC] = 1` at 0x800474CC in FUN_80047430, under `gp[+0xA48] & 0x80`),
--      and `ctx[+0xC]` then walks 1 -> 2 while PROT 0978 streams the still in.
--      No image on the disc materialises `0x180` paired with `y = 0` outside
--      the loaders. This probe watches for BOTH shapes a display can take: a
--      textured primitive off the page (tpage sweep) and a DISPENV pointed at
--      (384, 0) - because a full-screen VRAM still is usually SHOWN, not drawn.
--      Framebuffer grabs bracket the teardown so the answer is also visible.
--
-- Launch (an ordinary battle that resolves into the victory tail with no
-- input; the same teardown the dome uses - `gp[+0xA48] |= 0x80` is raised by
-- the ordinary spoils path at 0x8004EDE0):
--   LEGAIA_FRAMES=1800 timeout 3600 bash scripts/pcsx-redux/run_probe.sh \
--     --scenario rim_elm_gimard_victory \
--     --lua scripts/pcsx-redux/autorun_battle_teardown_hook.lua \
--     --out-dir captures/w1e/teardown_gimard
--
-- Outputs (in the run dir):
--   hook_hits.csv   tick,site,pc,ra,loader_b_id,extraction,bdc0,ctx,ctx_0c,target_w0,target_w1
--   teardown.csv    tick,mode,ctx,ctx_0c,ctx_0b,bdc0,gp_a48,gp_9f4,loader_b,note
--   still_hits.csv  tick,kind,detail
--   fb_<tick>.screen / .meta  framebuffer grabs around the teardown

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local scan  = require("probe.prim_scan")

-- ---------------------------------------------------------------- addresses
local GP            = 0x8007B318     -- recovered by find-gp-relative-refs.py --find-gp
local GAME_MODE     = 0x8007B83C     -- u16
local CTX_PTR       = GP + 0xA0C     -- 0x8007BD24, the battle-context pointer
local BDC0          = 0x8007BDC0     -- _DAT_8007BDC0, the hook gate (lw)
local GP_A48        = GP + 0xA48     -- 0x8007BD60, bit 0x80 arms the still load
local GP_9F4        = GP + 0x9F4     -- 0x8007BD0C, per-battle enemy id byte
local LOADER_B_ID   = 0x8007BC4C     -- gp+0x934, loader-B current id
local LOADER_B_BASE = 895            -- extraction index = LOADER_B_BASE + id (capture-pinned)

local SITE_HOOK  = 0x800481A0        -- jal 0x801F7B88
local SITE_ARM   = 0x800474CC        -- sb v0,0xc(v1)  (delay slot of the j)
local HOOK_TARGET = 0x801F7B88
local SLOT_B_BASE = 0x801F69D8

-- The dome panel still: VRAM (384, 0), 320x256. Its tpage ids for the two
-- colour depths a still could use - 4bpp/8bpp/16bpp at x=384, y=0 - plus the
-- semi-transparency-bit variants retail's descriptors add.
local STILL_TPAGES = { 0x0006, 0x0026, 0x0046, 0x0086, 0x00A6, 0x0106, 0x0126 }

-- Optional pad mash. The dome is the one context that shows the panel still,
-- and its match SM will not advance without input, so a capture that only
-- watches a parked dome state answers nothing. LEGAIA_PAD_MASH=<period> cycles
-- Cross / Up / Down / Left / Right, one press per period, holding each 6
-- vsyncs. It is a crude driver: it gets a match moving, it does not play well.
local PAD_MASH   = probe.getenv_num("LEGAIA_PAD_MASH", 0)
-- LEGAIA_PAD_MASH_KEYS overrides the cycle. Default is Cross only: in a battle
-- the command row is chosen with Left/Right, so a mash that includes them
-- eventually lands on Run and escapes the fight instead of finishing it.
local MASH_KEYS  = {}
for k in string.gmatch(probe.getenv("LEGAIA_PAD_MASH_KEYS", "CROSS"), "[^,]+") do
    MASH_KEYS[#MASH_KEYS + 1] = string.upper(k)
end

local FRAMES     = probe.getenv_num("LEGAIA_FRAMES", 1800)
local SCAN_EVERY = probe.getenv_num("LEGAIA_SCAN_EVERY", 30)
local FB_EVERY   = probe.getenv_num("LEGAIA_FB_EVERY", 120)

local hook_csv  = probe.csv_open(probe.out_path("hook_hits.csv"),
    "tick,site,pc,ra,loader_b_id,extraction,bdc0,ctx,ctx_0c,target_w0,target_w1")
local tear_csv  = probe.csv_open(probe.out_path("teardown.csv"),
    "tick,mode,ctx,ctx_0c,ctx_0b,bdc0,gp_a48,gp_9f4,loader_b,note")
local still_csv = probe.csv_open(probe.out_path("still_hits.csv"),
    "tick,kind,detail")

local hook_hits, arm_hits = 0, 0
local first_hook, first_arm = nil, nil
local still_seen = {}
local last_key = nil
local g_tick = 0
local bdc0_nonzero_ticks = 0

local function u32(a) return probe.read_u32(a) or 0 end
local function u8(a)  return probe.read_u8(a) or 0 end

local function ctx_ptr() return u32(CTX_PTR) end

local function grab_fb(tag)
    local ok, ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
    if not ok or ss == nil then return false end
    local bpp = tonumber(ss.bpp) or 0
    local bits = (bpp > 16) and 24 or 16
    local w, h = tonumber(ss.width), tonumber(ss.height)
    local fh = io.open(probe.out_path(string.format("fb_%s.screen", tag)), "wb")
    if fh == nil then return false end
    fh:write(tostring(ss.data)); fh:close()
    local mh = io.open(probe.out_path(string.format("fb_%s.screen.meta", tag)), "w")
    if mh ~= nil then
        mh:write(string.format("width=%d\nheight=%d\nbpp=%d\nbytes_per_pixel=%d\n",
            w, h, bits, bits / 8))
        mh:close()
    end
    return true
end

probe.run({
    sstate         = probe.getenv("LEGAIA_SSTATE", "unset"),
    capture_frames = FRAMES,
    snapshot_path  = probe.out_path("teardown.snapshot.txt"),
    on_arm = function()
        local d_hook = { addr = SITE_HOOK, name = "jal_801F7B88", hits_ref = { n = 0 } }
        local d_arm  = { addr = SITE_ARM,  name = "ctx_0C_arm",   hits_ref = { n = 0 } }

        probe.arm_breakpoint(SITE_HOOK, "Exec", 4, d_hook.name, function()
            local r = PCSX.getRegisters()
            local id = u32(LOADER_B_ID)
            local c  = ctx_ptr()
            hook_csv:row("%d,hook,0x%08X,0x%08X,%d,%d,0x%08X,0x%08X,%d,0x%08X,0x%08X",
                g_tick,
                bit.band(tonumber(r.pc), 0xFFFFFFFF),
                bit.band(tonumber(r.GPR.n.ra), 0xFFFFFFFF),
                id, LOADER_B_BASE + id, u32(BDC0), c,
                (c ~= 0) and u8(c + 0xC) or -1,
                u32(HOOK_TARGET), u32(HOOK_TARGET + 4))
            hook_hits = hook_hits + 1
            d_hook.hits_ref.n = hook_hits
            if first_hook == nil then
                first_hook = g_tick
                PCSX.log(string.format(
                    "[teardown] jal 0x801F7B88 FIRED at tick %d; loader-B id=%d -> extraction %d",
                    g_tick, id, LOADER_B_BASE + id))
                -- Dump the slot-B window head so the image can be byte-matched
                -- offline against the extracted overlay corpus.
                local buf = probe.read_bytes(SLOT_B_BASE, 0x4000)
                if buf ~= nil then
                    local fh = io.open(probe.out_path("slot_b_at_hook.bin"), "wb")
                    if fh then fh:write(tostring(buf)); fh:close() end
                end
            end
        end)

        probe.arm_breakpoint(SITE_ARM, "Exec", 4, d_arm.name, function()
            local r = PCSX.getRegisters()
            local c = ctx_ptr()
            hook_csv:row("%d,arm,0x%08X,0x%08X,%d,%d,0x%08X,0x%08X,%d,0,0",
                g_tick,
                bit.band(tonumber(r.pc), 0xFFFFFFFF),
                bit.band(tonumber(r.GPR.n.ra), 0xFFFFFFFF),
                u32(LOADER_B_ID), LOADER_B_BASE + u32(LOADER_B_ID),
                u32(BDC0), c, (c ~= 0) and u8(c + 0xC) or -1)
            arm_hits = arm_hits + 1
            d_arm.hits_ref.n = arm_hits
            if first_arm == nil then
                first_arm = g_tick
                PCSX.log(string.format("[teardown] ctx[+0xC] = 1 armed at tick %d", g_tick))
            end
        end)

        return { d_hook, d_arm }
    end,

    on_capture = function(ctx, elapsed)
        g_tick = elapsed
        local c    = ctx_ptr()
        local c0c  = (c ~= 0) and u8(c + 0xC) or -1
        local c0b  = (c ~= 0) and u8(c + 0xB) or -1
        local bdc0 = u32(BDC0)
        if bdc0 ~= 0 then bdc0_nonzero_ticks = bdc0_nonzero_ticks + 1 end
        local key = string.format("%d/%d/%d/%d/%d",
            probe.read_u16(GAME_MODE) or -1, c0c, c0b, bdc0, u8(GP_A48))
        if key ~= last_key then
            tear_csv:row("%d,0x%X,0x%08X,%d,%d,0x%08X,0x%02X,0x%02X,%d,change",
                elapsed, probe.read_u16(GAME_MODE) or 0, c, c0c, c0b, bdc0,
                u8(GP_A48), u8(GP_9F4), u32(LOADER_B_ID))
            last_key = key
            -- The teardown machine moving is the interesting window: grab a
            -- frame on every transition as well as on the periodic cadence.
            grab_fb(string.format("%05d_state", elapsed))
        elseif elapsed % 120 == 0 then
            tear_csv:row("%d,0x%X,0x%08X,%d,%d,0x%08X,0x%02X,0x%02X,%d,tick",
                elapsed, probe.read_u16(GAME_MODE) or 0, c, c0c, c0b, bdc0,
                u8(GP_A48), u8(GP_9F4), u32(LOADER_B_ID))
        end

        if FB_EVERY > 0 and (elapsed % FB_EVERY) == 0 then
            grab_fb(string.format("%05d", elapsed))
        end

        if PAD_MASH > 0 then
            local phase = elapsed % PAD_MASH
            local key = MASH_KEYS[(math.floor(elapsed / PAD_MASH) % #MASH_KEYS) + 1]
            if phase == 0 then
                probe.pad_force(probe.BTN[key])
            elseif phase == 6 then
                probe.pad_release(probe.BTN[key])
            end
        end

        if (elapsed % SCAN_EVERY) == 0 then
            local ram = scan.snapshot()
            if ram ~= nil then
                for _, tp in ipairs(STILL_TPAGES) do
                    local hits, total = scan.find_tpage(ram, tp, 4)
                    for _, h in ipairs(hits) do
                        local k = string.format("tpage_0x%04X", tp)
                        if scan.plausible(h) and still_seen[k] == nil then
                            still_seen[k] = elapsed
                            PCSX.log(string.format(
                                "[teardown] first tpage 0x%04X packet at tick %d: 0x%08X (%d,%d)",
                                tp, elapsed, h.packet_va, h.x, h.y))
                        end
                        still_csv:row("%d,tpage,0x%04X packet=0x%08X code=0x%02X x=%d y=%d clut=0x%04X total=%d plausible=%d",
                            elapsed, tp, h.packet_va, h.code, h.x, h.y, h.clut or 0, total,
                            scan.plausible(h) and 1 or 0)
                    end
                end
                -- Anything addressing (384, 0) as a primitive corner or as a
                -- VRAM-to-VRAM blit source.
                local all_codes = {}
                for k in pairs(scan.TEXTURED_CODES) do all_codes[k] = true end
                for k in pairs(scan.MOVE_IMAGE_CODES) do all_codes[k] = true end
                for _, base in ipairs({ 0x20, 0x28, 0x30, 0x38, 0x60, 0x68, 0x70, 0x78 }) do
                    for i = 0, 3 do all_codes[base + i] = true end
                end
                local xy_hits, xy_total = scan.find_at_xy(ram, 384, 0, all_codes, 6)
                for _, h in ipairs(xy_hits) do
                    if still_seen["xy"] == nil then
                        still_seen["xy"] = elapsed
                        PCSX.log(string.format(
                            "[teardown] first (384,0) packet at tick %d: 0x%08X code=0x%02X dst=(%d,%d) %dx%d",
                            elapsed, h.packet_va, h.code,
                            h.dst_x or -1, h.dst_y or -1, h.size_w or -1, h.size_h or -1))
                    end
                    still_csv:row("%d,xy_384_0,packet=0x%08X code=0x%02X dst=(%d;%d) size=%dx%d total=%d",
                        elapsed, h.packet_va, h.code,
                        h.dst_x or -1, h.dst_y or -1, h.size_w or -1, h.size_h or -1,
                        xy_total)
                end

                for _, h in ipairs(scan.find_disp_rect(ram, 384, 0, 8)) do
                    if still_seen["disp"] == nil then
                        still_seen["disp"] = elapsed
                        PCSX.log(string.format(
                            "[teardown] first (384,0) display RECT at tick %d: 0x%08X %dx%d",
                            elapsed, h.va, h.w, h.h))
                    end
                    still_csv:row("%d,disp_rect,va=0x%08X w=%d h=%d", elapsed, h.va, h.w, h.h)
                end
            end
        end
    end,

    on_summary = function()
        PCSX.log("=== probe hits ===")
        PCSX.log(string.format("  jal 0x801F7B88 site 0x800481A0: %d hits (first tick %s)",
            hook_hits, tostring(first_hook)))
        PCSX.log(string.format("  ctx[+0xC]=1 arm 0x800474CC:      %d hits (first tick %s)",
            arm_hits, tostring(first_arm)))
        PCSX.log(string.format("  _DAT_8007BDC0 non-zero on %d of %d sampled vsyncs",
            bdc0_nonzero_ticks, FRAMES))
        for k, v in pairs(still_seen) do
            PCSX.log(string.format("  (384,0) candidate %s first seen tick %d", k, v))
        end
        PCSX.log("=== end ===")
    end,

    on_done = function()
        local fh = io.open(probe.out_path("summary.txt"), "w")
        if fh then
            fh:write(string.format("jal 0x801F7B88 (site 0x800481A0): hits=%d first_tick=%s\n",
                hook_hits, tostring(first_hook)))
            fh:write(string.format("ctx[+0xC]=1 arm (0x800474CC):     hits=%d first_tick=%s\n",
                arm_hits, tostring(first_arm)))
            fh:write(string.format("_DAT_8007BDC0 non-zero vsyncs:   %d / %d\n",
                bdc0_nonzero_ticks, FRAMES))
            if next(still_seen) == nil then
                fh:write("(384,0) still: NO tpage packet and NO display RECT seen\n")
            else
                for k, v in pairs(still_seen) do
                    fh:write(string.format("(384,0) candidate %s first tick %d\n", k, v))
                end
            end
            fh:close()
        end
        hook_csv:close(); tear_csv:close(); still_csv:close()
    end,
})
