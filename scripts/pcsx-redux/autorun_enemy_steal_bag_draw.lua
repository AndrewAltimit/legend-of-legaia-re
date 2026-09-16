-- autorun_enemy_steal_bag_draw.lua
--
-- Slot-draw oracle for the **enemy** Steal (action id `0x51`, PROT 0941 body
-- `0x801F730C`). The body picks the item it takes by rolling a raw slot index
-- over the whole 256-slot bag, re-rolling until the slot passes three tests,
-- and then hands the slot's ITEM ID - not the slot - to the SCUS consume
-- helper `FUN_80042310`, which scans only inside the active window
-- ([`inventory.md`](../../docs/subsystems/inventory.md)). The draw space and
-- the removal space are therefore different spans, and a bag with holes is
-- what makes the difference observable.
--
-- Reads, from the body's own disassembly (PROT 0941 at slot-B base
-- `0x801F69D8`):
--
--   0x801F77C4  jal 0x80056798      rand()
--   0x801F77E4  a0 = rand % 0x100   the raw slot index
--   0x801F7800  lh  gp[+0x2D2]      window start - the re-roll floor, taken
--                                   only when 0x8007BD11 == 4
--   0x801F7864  lbu bag[slot].id    test 1 (non-zero id)
--   0x801F7874  lbu bag[slot].count test 2 (non-zero count)
--   0x801F789C  lhu item[id]+2      test 3 (the item record halfword two
--                                   bytes below its name pointer)
--   0x801F78B0  sltiu s1, 0x400     up to 1024 attempts, then the fail arm
--   0x801F79C4  jal 0x80042310      consume(a0 = id, a1 = 1)
--
-- Every rejected draw trips the test-1 breakpoint too, so `draws.csv` is the
-- full roll sequence, not just the accepted one.
--
-- Outputs (probe.out_path):
--   draws.csv    one row per slot draw: attempt index, slot, the bag pair at
--                that slot, and whether it passed each test.
--   consume.csv  the FUN_80042310 call + return, and the window triple.
--   bag.csv      non-empty bag slots before the cast and after it.
--
-- Env:
--   LEGAIA_SSTATE       required (a pre-turn battle state)
--   LEGAIA_MONSTER_SEAT caster seat (default 3)
--   LEGAIA_TARGET_SEAT  value written to caster +0x1DD (default 0)
--   LEGAIA_HOLES        comma-separated bag slots to ZERO before the cast
--                       (the holes); default "" = leave the bag alone
--   LEGAIA_SEED_SLOTS   comma-separated `slot:id:count` triples to write
--                       before the cast, so the draw space has known contents
--   LEGAIA_INJECT_AT    vsync to convert the monster's action (default 20)
--   LEGAIA_PRESS_UNTIL  keep tapping CROSS until this vsync (default 400)
--   LEGAIA_FRAMES       capture vsyncs (default 2400)
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 2400)
local MON_SEAT    = probe.getenv_num("LEGAIA_MONSTER_SEAT", 3)
local TARGET_SEAT = probe.getenv_num("LEGAIA_TARGET_SEAT", 0)
local INJECT_AT   = probe.getenv_num("LEGAIA_INJECT_AT", 20)
local PRESS_UNTIL = probe.getenv_num("LEGAIA_PRESS_UNTIL", 400)
local HOLES       = probe.getenv("LEGAIA_HOLES", "")
local SEED_SLOTS  = probe.getenv("LEGAIA_SEED_SLOTS", "")
local LABEL       = probe.getenv("LEGAIA_LABEL", "steal-draw")

local SPELL       = 0x51
local ACTOR_TABLE = 0x801C9370
local CTX_PTR     = 0x8007BD24
local SEAT_CHAR   = 0x8007BD10
local BAG         = 0x80085958        -- 256 slots x [id:u8][count:u8]
local ITEM_TABLE  = 0x80074368        -- the body's own base; +4 == 0x8007436C
local LOADER_ID   = 0x8007BC4C

local DRAW_TEST1  = 0x801F7864        -- lbu bag[slot].id, v1 = &bag[slot]
local CONSUME     = 0x80042310
local CONSUME_RET = 0x801F79CC        -- the instruction after the jal's slot

local function u8(a)  return probe.read_u8(a)  or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
local function tou32(v)
    v = tonumber(v) or 0
    if v < 0 then v = v + 0x100000000 end
    return v
end
local function regs()
    local r = PCSX.getRegisters()
    return (r.GPR and r.GPR.n) or {}
end
local function ctxp()
    local c = u32(CTX_PTR)
    if c < 0x80000000 or c >= 0x80200000 then return nil end
    return c
end
local function actor(slot)
    local p = u32(ACTOR_TABLE + slot * 4)
    if p < 0x80000000 or p >= 0x80200000 then return nil end
    return p
end
local function split(s)
    local out = {}
    for tok in string.gmatch(s or "", "[^,]+") do out[#out + 1] = tok end
    return out
end

local draws, consume_csv, bag_csv
local injected, band_seen = false, false
local draw_n, consume_n = 0, 0
local elapsed_now = 0
local gp_base = 0
local quit_at = -1
local last_flow = ""

local function dump_bag(tag)
    local n = 0
    for slot = 0, 255 do
        local id = u8(BAG + slot * 2)
        local ct = u8(BAG + slot * 2 + 1)
        if id ~= 0 or ct ~= 0 then
            bag_csv:row("%s,%d,%d,%d", tag, slot, id, ct)
            n = n + 1
        end
    end
    bag_csv:row("%s,count,%d,", tag, n)
    PCSX.log(string.format("[bag %s] %d non-empty slots", tag, n))
end

probe.run({
    sstate = SSTATE_PATH, capture_frames = FRAMES,
    on_arm = function()
        PCSX.log(string.format("== enemy steal bag draw == label=%s seat=%d target=%d",
            LABEL, MON_SEAT, TARGET_SEAT))
        probe.env.write_manifest("autorun_enemy_steal_bag_draw.lua", {
            label = LABEL, sstate = SSTATE_PATH, monster_seat = MON_SEAT,
            target_seat = TARGET_SEAT, holes = HOLES, seed_slots = SEED_SLOTS,
        })
        draws = probe.csv_open(probe.out_path("draws.csv"),
            "attempt,vsync,slot,bag_id,bag_count,item_hw,pass1,pass2,pass3,seat1char,win_start")
        consume_csv = probe.csv_open(probe.out_path("consume.csv"),
            "kind,vsync,a0_id,a1_count,v0,ra,win_start,win_end,win_span,gp")
        bag_csv = probe.csv_open(probe.out_path("bag.csv"), "tag,slot,id,count")

        probe.arm_breakpoint(DRAW_TEST1, "Exec", 4, "draw", function()
            local n = regs()
            local ptr = tou32(n.v1)
            if ptr < BAG or ptr >= BAG + 512 then return end
            local slot = (ptr - BAG) / 2
            local id = u8(ptr)
            local ct = u8(ptr + 1)
            local hw = id ~= 0 and u16(ITEM_TABLE + id * 12 + 2) or 0
            draw_n = draw_n + 1
            draws:row("%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d",
                draw_n, elapsed_now, slot, id, ct, hw,
                id ~= 0 and 1 or 0, ct ~= 0 and 1 or 0, hw ~= 0 and 1 or 0,
                u8(SEAT_CHAR + 1), gp_base ~= 0 and u16(gp_base + 0x2D2) or -1)
        end)

        probe.arm_breakpoint(CONSUME, "Exec", 4, "consume", function()
            local n = regs()
            consume_n = consume_n + 1
            consume_csv:row("call,%d,%d,%d,,0x%08X,%d,%d,%d,0x%08X",
                elapsed_now, tou32(n.a0) % 256, tou32(n.a1), tou32(n.ra),
                gp_base ~= 0 and u16(gp_base + 0x2D2) or -1,
                gp_base ~= 0 and u16(gp_base + 0x2D4) or -1,
                gp_base ~= 0 and u16(gp_base + 0x2D6) or -1, gp_base)
        end)
        probe.arm_breakpoint(CONSUME_RET, "Exec", 4, "consume_ret", function()
            local n = regs()
            consume_csv:row("ret,%d,,,%d,,,,,", elapsed_now, tou32(n.v0))
        end)
        return {}
    end,

    on_capture = function(c, elapsed)
        elapsed_now = elapsed
        if gp_base == 0 then
            local n = regs()
            local g = tou32(n.gp)
            -- The window halfwords are gp-relative and gp is constant for the
            -- whole run; bound it to the .sdata band so a value sampled inside
            -- a BIOS frame cannot be mistaken for it.
            if g >= 0x80070000 and g < 0x80080000 then gp_base = g end
        end
        local cx = ctxp()
        local mon = actor(MON_SEAT)
        if cx == nil or mon == nil then return end

        probe.pad_release(probe.BTN.CROSS)
        if not band_seen and elapsed < PRESS_UNTIL then
            local sub = elapsed % 60
            if sub >= 30 and sub < 34 then probe.pad_force(probe.BTN.CROSS) end
        end

        if not injected and elapsed >= INJECT_AT then
            injected = true
            for _, tok in ipairs(split(SEED_SLOTS)) do
                local s, i, ct = string.match(tok, "(%d+):(%d+):(%d+)")
                if s then
                    probe.write_u8(BAG + tonumber(s) * 2, tonumber(i))
                    probe.write_u8(BAG + tonumber(s) * 2 + 1, tonumber(ct))
                end
            end
            for _, tok in ipairs(split(HOLES)) do
                local s = tonumber(tok)
                if s then
                    probe.write_u8(BAG + s * 2, 0)
                    probe.write_u8(BAG + s * 2 + 1, 0)
                end
            end
            dump_bag("before")
            probe.write_u16(mon + 0x14C, 9999)
            probe.write_u16(mon + 0x14E, 9999)
            for s = 0, 2 do
                local a = actor(s)
                if a ~= nil then
                    probe.write_u16(a + 0x14C, 9999)
                    probe.write_u16(a + 0x172, 9999)
                    probe.write_u16(a + 0x14E, 9999)
                end
            end
            probe.write_u8(mon + 0x1DE, 2)
            probe.write_u8(mon + 0x1DF, SPELL)
            probe.write_u8(mon + 0x1DD, TARGET_SEAT)
            PCSX.log(string.format("[inject t%d] steal queued on seat %d; gp=0x%08X window=[%d,%d) span=%d",
                elapsed, MON_SEAT, gp_base,
                gp_base ~= 0 and u16(gp_base + 0x2D2) or -1,
                gp_base ~= 0 and u16(gp_base + 0x2D4) or -1,
                gp_base ~= 0 and u16(gp_base + 0x2D6) or -1))
        end

        local st7 = u8(cx + 7)
        if st7 >= 0x6E and st7 <= 0x71 then band_seen = true end
        local line = string.format("%02X/%d/%d", st7, u8(cx + 0x279), u8(LOADER_ID))
        if line ~= last_flow then
            last_flow = line
            PCSX.log(string.format("[t%d] st7=0x%02X phase=%d loader=%d draws=%d",
                elapsed, st7, u8(cx + 0x279), u8(LOADER_ID), draw_n))
        end
        if band_seen and consume_n > 0 and quit_at < 0 then quit_at = elapsed + 120 end
        if band_seen and u8(cx + 0x279) == 0xFF and quit_at < 0 then quit_at = elapsed + 120 end
        if quit_at >= 0 and elapsed >= quit_at then
            dump_bag("after")
            c.request_quit = true
        end
    end,

    on_done = function()
        PCSX.log(string.format("[steal-draw] draws=%d consumes=%d band=%s",
            draw_n, consume_n, tostring(band_seen)))
        if draws then draws:close() end
        if consume_csv then consume_csv:close() end
        if bag_csv then bag_csv:close() end
    end,
})
