-- autorun_shop_buy_list.lua
--
-- Retail capture of a gold shop's buy list as the SCUS content builder
-- builds it (FUN_80030628 case 0x0B, 0x80030D48..0x80030F98), with and
-- without the Platinum Card (item 0xFF) in the bag. The question it
-- answers: does the builder drop the stock record's last three entries
-- when neither the bag probe FUN_80042F4C(0xFF) nor the 0xFF equipment
-- sweep finds a card (docs/subsystems/shop.md#the-three-row-tail-is-the-platinum-cards)?
--
-- Reaching the merchant is autorun_talk_to_npc.lua's job (walk / poke,
-- face, CROSS until the dialogue byte rises, then CROSS every 20 vsyncs
-- for LEGAIA_TRACE_VSYNCS); this wrapper only adds the observation and
-- the bag poke, then dofile's it:
--
--   * Exec BP 0x80030D5C - after `jal FUN_80042F4C(0xFF)`: v0 = the bag
--     probe's answer (held count of the first 0xFF slot, 0 = none).
--   * Exec BP 0x80030E54 - the `jal 0x80030104` that allocates the list:
--     s4 = the walked row count (record count - 3 + tail, minus ids < 0x1A;
--     it reaches a2 in the delay slot, so a2 is not read here),
--     plus the stock record at *(gp+0x138) (count byte +2, ids from +3).
--   * Exec BP 0x80030F94 - the builder's exit jump: the row words from the
--     list buffer (*(sp+0x10) + 0x28) up to s2, i.e. every row it emitted,
--     class nibble + dim bit + item id each.
--
-- LEGAIA_BAG_CARD=add writes id 0xFF, count 1 into the first empty slot of
-- the bag's active window (gp[+0x2D2]..gp[+0x2D4], bag at 0x80085958,
-- 2-byte [id][count] slots; docs/subsystems/inventory.md); =remove clears
-- every 0xFF slot in the window and every 0xFF byte of the present party's
-- equipment blocks (char +0x196..+0x19D, the builder's second probe). Both
-- run a few vsyncs after the state loads - before the conversation starts,
-- so the builder's probes see the edited party.
--
-- Output: <LEGAIA_OUT_DIR>/shop.log (plus the talk probe's own talk.log).
-- Env: as autorun_talk_to_npc.lua, plus LEGAIA_BAG_CARD.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local env = require("probe.env")
local mem = require("probe.mem")
local bp  = require("probe.bp")

local OUT_DIR = env.getenv("LEGAIA_OUT_DIR", "captures/shop_buy_list")
local CARD = env.getenv("LEGAIA_BAG_CARD", "")
os.execute(string.format("mkdir -p %q", OUT_DIR))
local LOG = io.open(OUT_DIR .. "/shop.log", "w")
local function log(s) PCSX.log("[shop] " .. s); if LOG then LOG:write(s .. "\n"); LOG:flush() end end
local function u32(x) return (tonumber(x) or 0) % 0x100000000 end

local BAG = 0x80085958
local GP = 0x8007B318 -- SCUS $gp (gp+0x728 = 0x8007BA40); the BPs read the register itself

local function bag_window(gp)
    local lo = mem.read_u16(gp + 0x2D2) or 0
    local hi = mem.read_u16(gp + 0x2D4) or 0
    if lo >= 0x8000 then lo = lo - 0x10000 end
    if hi >= 0x8000 then hi = hi - 0x10000 end
    return lo, hi
end
local function dump_bag(tag, gp)
    local lo, hi = bag_window(gp)
    local parts = {}
    for i = lo, hi - 1 do
        local id = mem.read_u8(BAG + i * 2) or 0
        if id ~= 0 then parts[#parts + 1] = string.format("%d:%02X x%d", i, id, mem.read_u8(BAG + i * 2 + 1) or 0) end
    end
    log(string.format("%s bag window [%d,%d) held: %s", tag, lo, hi, table.concat(parts, " ")))
end

local function party_equipment_ff(clear)
    -- 0x80084594 = party size, 0x80084598.. = member ids, record n at
    -- 0x80084708 + n*0x414, equipment block +0x196..+0x19D.
    local found = {}
    local n = mem.read_u8(0x80084594) or 0
    for m = 0, n - 1 do
        local id = mem.read_u8(0x80084598 + m) or 0
        for k = 0, 7 do
            local a = 0x80084708 + id * 0x414 + 0x196 + k
            if (mem.read_u8(a) or 0) == 0xFF then
                found[#found + 1] = string.format("char %d +0x%X", id, 0x196 + k)
                if clear then mem.write_u8(a, 0) end
            end
        end
    end
    return found
end

bp.arm(0x80030D5C, "Exec", 4, "shop_card_probe", function()
    local r = PCSX.getRegisters()
    local gp = u32(r.GPR.n.gp)
    log(string.format("probe FUN_80042F4C(0xFF) -> v0=%d (gp %08X)", u32(r.GPR.n.v0) % 0x10000, gp))
    dump_bag("at probe:", gp)
    local eq = party_equipment_ff(false)
    log("equipment 0xFF bytes at probe: " .. (#eq > 0 and table.concat(eq, ", ") or "none"))
end)
bp.arm(0x80030E54, "Exec", 4, "shop_row_count", function()
    local r = PCSX.getRegisters()
    local gp = u32(r.GPR.n.gp)
    local rec = mem.read_u32(gp + 0x138) or 0
    local cnt = mem.read_u8(rec + 2) or 0
    local ids = {}
    for i = 0, cnt - 1 do ids[#ids + 1] = string.format("%02X", mem.read_u8(rec + 3 + i) or 0) end
    log(string.format("walk count s4=%d; stock record %08X count %d ids %s",
        u32(r.GPR.n.s4), rec, cnt, table.concat(ids, " ")))
end)
bp.arm(0x80030F94, "Exec", 4, "shop_rows_done", function()
    local r = PCSX.getRegisters()
    local sp = u32(r.GPR.n.sp)
    local buf = (mem.read_u32(sp + 0x10) or 0) + 0x28
    local s2 = u32(r.GPR.n.s2)
    local rows = {}
    local a = buf
    while a < s2 and #rows < 40 do
        rows[#rows + 1] = string.format("%04X", mem.read_u16(a) or 0)
        a = a + 2
    end
    log(string.format("emitted %d rows (s5 = %d staged 0x3000 rows): %s", #rows, u32(r.GPR.n.s5), table.concat(rows, " ")))
end)

if CARD == "add" or CARD == "remove" then
    local vv, done = 0, false
    PROBE_LISTENER_ANCHORS = PROBE_LISTENER_ANCHORS or {}
    PROBE_LISTENER_ANCHORS[#PROBE_LISTENER_ANCHORS + 1] =
        PCSX.Events.createEventListener("GPU::Vsync", function()
            vv = vv + 1
            if done or vv < 12 then return end
            done = true
            local lo, hi = bag_window(GP)
            if CARD == "add" then
                for i = lo, hi - 1 do
                    if (mem.read_u8(BAG + i * 2) or 0) == 0 then
                        mem.write_u8(BAG + i * 2, 0xFF)
                        mem.write_u8(BAG + i * 2 + 1, 1)
                        log(string.format("poked Platinum Card (0xFF x1) into bag slot %d", i))
                        break
                    end
                end
            else
                for i = lo, hi - 1 do
                    if (mem.read_u8(BAG + i * 2) or 0) == 0xFF then
                        mem.write_u8(BAG + i * 2, 0)
                        mem.write_u8(BAG + i * 2 + 1, 0)
                        log(string.format("cleared Platinum Card from bag slot %d", i))
                    end
                end
                local eq = party_equipment_ff(true)
                log("cleared equipment 0xFF bytes: " .. (#eq > 0 and table.concat(eq, ", ") or "none"))
            end
            dump_bag("after edit:", GP)
        end)
end
log(string.format("armed (card edit=%q)", CARD))
dofile("scripts/pcsx-redux/autorun_talk_to_npc.lua")
