-- autorun_monster_record_source.lua
--
-- Pins the monster stat-record SOURCE + validates the record FORMAT against
-- live battle data.
--
-- Background (static trace, docs/subsystems/battle.md):
--   FUN_800542C8 (battle archive loader) streams per-monster 0x14000-byte
--   LZS blocks from a monster archive; the decoded block's head is the stat
--   record. It then calls FUN_80054CB0(record_ptr, slot) to populate the
--   battle actor. The retail-vs-debug source transport is gated by
--   _DAT_8007B8C2: ==0 -> FUN_800608F0("data\battle\<name>") host trap;
--   !=0 -> FUN_8003E8A8(0x365=869) disc seek (in-RAM PROT TOC). The on-disc
--   archive bytes are NOT located statically (PROT 869 is a stub); this
--   probe captures them live.
--
-- Captures (battle auto-starts a few seconds after sstate8 loads):
--   1. _DAT_8007B8C2 (which transport runs).
--   2. Exec BP @ FUN_8003E8A8 + FUN_800608F0: the archive open/seek (index +
--      resulting CdlLOC at 0x8007BC5C -> disc LBA).
--   3. Exec BP @ FUN_800542C8: loader entry (caller RA).
--   4. Exec BP @ FUN_80054CB0: per-monster record. Logs the parsed record
--      (name string, HP, MP, stat u16s, magic count) and dumps the record
--      bytes. This is the real stat data + format validation.
--
-- Run (slot 8 = boss-ambush battle; ~15s, no game over):
--   LEGAIA_SSTATE=$HOME/Tools/pcsx-redux/SCUS94254.sstate8 \
--   LEGAIA_FRAMES=600 \
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_monster_record_source.lua \
--       timeout --kill-after=30s 600s bash scripts/pcsx-redux/run_probe.sh

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate8")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 600)
local OUT_PATH = probe.out_path("monster_record_source.csv")
local DETAIL   = OUT_PATH:gsub("%.csv$", ".detail.txt")
local DUMP_DIR = OUT_PATH:gsub("monster_record_source%.csv$", "")

local FUN_80054CB0 = 0x80054CB0 -- monster init (record -> actor)
local FUN_800542C8 = 0x800542C8 -- battle archive loader
local FUN_8003E8A8 = 0x8003E8A8 -- debug disc-seek (sets CdlLOC, absolute index)
local FUN_8003E964 = 0x8003E964 -- relative disc-seek (a0 = sector delta)
local FUN_8003E800 = 0x8003E800 -- generic disc read (a0=dest, a1=sector count)
local FUN_800608F0 = 0x800608F0 -- retail host-trap file open
local DAT_8007B8C2 = 0x8007B8C2 -- retail(0)/debug(!=0) gate
local DAT_8007BC5C = 0x8007BC5C -- CdlLOC (BCD MSF)
local DAT_8007B728 = 0x8007B728 -- decoded-block base ptr

-- BCD MSF (3 bytes at CdlLOC) -> absolute disc LBA.
local function cdloc_lba()
    local b = probe.read_bytes(DAT_8007BC5C, 3)
    if b == nil then return -1, "??:??:??" end
    local s = tostring(b)
    local function bcd(x) return math.floor(x / 16) * 10 + (x % 16) end
    local m = bcd(string.byte(s, 1))
    local sec = bcd(string.byte(s, 2))
    local f = bcd(string.byte(s, 3))
    local lba = (m * 60 + sec) * 75 + f - 150
    return lba, string.format("%02X:%02X:%02X", string.byte(s, 1), string.byte(s, 2), string.byte(s, 3))
end

local csv = probe.csv_open(OUT_PATH,
    "tick,slot,rec_ptr,name,hp,mp,s0e,s12,s14,s16,s18,s1a,magic")

local function rd_u16(addr)
    local b = probe.read_bytes(addr, 2)
    if b == nil then return 0 end
    local s = tostring(b)
    return string.byte(s, 1) + string.byte(s, 2) * 256
end

local function rd_str(addr, maxn)
    if addr == 0 or not probe.in_ram(addr, 1) then return "" end
    local b = probe.read_bytes(addr, maxn)
    if b == nil then return "" end
    return tostring(b):gsub("%z.*$", "")
end

local function dump_bytes(path, addr, len)
    if not probe.in_ram(addr, 1) then return false end
    local fh = io.open(path, "wb")
    if not fh then return false end
    local off = 0
    while off < len do
        local n = math.min(0x4000, len - off)
        local chunk = probe.read_bytes(addr + off, n)
        if chunk == nil then break end
        fh:write(tostring(chunk))
        off = off + n
    end
    fh:close()
    return true
end

local rec_hits, loader_hits, seek_hits, trap_hits = 0, 0, 0, 0
local logged_gate = false

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    out_path       = OUT_PATH,
    snapshot_path  = OUT_PATH:gsub("%.csv$", ".hits.txt"),

    on_arm = function()
        local bps = {}

        -- Monster record -> actor (the record bytes + real stats).
        table.insert(bps, { addr = FUN_80054CB0, name = "monster_init" })
        probe.arm_breakpoint(FUN_80054CB0, "Exec", 4, "monster_init", function()
            local r = PCSX.getRegisters()
            local rec  = bit.band(tonumber(r.GPR.n.a0) or 0, 0xFFFFFFFF)
            local slot = bit.band(tonumber(r.GPR.n.a1) or 0, 0xFFFFFFFF)
            if not probe.in_ram(rec, 0x20) then return end
            rec_hits = rec_hits + 1
            local name_ptr = probe.read_u32(rec)
            local name = rd_str(name_ptr, 24)
            local hp   = rd_u16(rec + 0x0C)
            local s0e  = rd_u16(rec + 0x0E)
            local mp   = rd_u16(rec + 0x10)
            local s12  = rd_u16(rec + 0x12)
            local s14  = rd_u16(rec + 0x14)
            local s16  = rd_u16(rec + 0x16)
            local s18  = rd_u16(rec + 0x18)
            local s1a  = rd_u16(rec + 0x1A)
            local magic = string.byte(tostring(probe.read_bytes(rec + 0x4A, 1) or "\0"), 1)
            csv:row("%d,%d,0x%08X,%s,%d,%d,%d,%d,%d,%d,%d,%d,%d",
                rec_hits, slot, rec, name, hp, mp, s0e, s12, s14, s16, s18, s1a, magic)
            PCSX.log(string.format(
                "[mon] rec #%d slot=%d ptr=0x%08X name='%s' HP=%d MP=%d stats=[%d,%d,%d,%d,%d,%d] magic=%d",
                rec_hits, slot, rec, name, hp, mp, s0e, s12, s14, s16, s18, s1a, magic))
            dump_bytes(string.format("%smonster_rec_%02d.bin", DUMP_DIR, rec_hits), rec, 0x120)
        end)

        -- Battle archive loader entry.
        table.insert(bps, { addr = FUN_800542C8, name = "battle_loader" })
        probe.arm_breakpoint(FUN_800542C8, "Exec", 4, "battle_loader", function()
            local r = PCSX.getRegisters()
            local ra = bit.band(tonumber(r.GPR.n.ra) or 0, 0xFFFFFFFF)
            loader_hits = loader_hits + 1
            local gate = string.byte(tostring(probe.read_bytes(DAT_8007B8C2, 1) or "\0"), 1)
            PCSX.log(string.format(
                "[mon] battle_loader #%d ra=0x%08X _DAT_8007b8c2=%d (%s)",
                loader_hits, ra, gate, (gate == 0) and "retail/host-trap" or "debug/PROT-index"))
        end)

        -- Disc seek (debug path) -> capture index + resulting CdlLOC.
        table.insert(bps, { addr = FUN_8003E8A8, name = "disc_seek" })
        probe.arm_breakpoint(FUN_8003E8A8, "Exec", 4, "disc_seek", function()
            local r = PCSX.getRegisters()
            local idx = bit.band(tonumber(r.GPR.n.a0) or 0, 0xFFFFFFFF)
            seek_hits = seek_hits + 1
            local loc = probe.read_bytes(DAT_8007BC5C, 4)
            local locs = loc and tostring(loc) or ""
            local m, s, f = 0, 0, 0
            if #locs >= 3 then
                m, s, f = string.byte(locs, 1), string.byte(locs, 2), string.byte(locs, 3)
            end
            PCSX.log(string.format(
                "[mon] disc_seek #%d idx=0x%X (%d) CdlLOC=%02X:%02X:%02X",
                seek_hits, idx, idx, m, s, f))
        end)

        -- Relative disc-seek: a0 = sector delta = (id-1)*40 for monster
        -- blocks (0x14000 bytes = 40 sectors). So id = a0/40 + 1. ra in the
        -- battle-loader range marks the monster-block seeks.
        table.insert(bps, { addr = FUN_8003E964, name = "rel_seek" })
        probe.arm_breakpoint(FUN_8003E964, "Exec", 4, "rel_seek", function()
            local r = PCSX.getRegisters()
            local delta = bit.band(tonumber(r.GPR.n.a0) or 0, 0xFFFFFFFF)
            local ra = bit.band(tonumber(r.GPR.n.ra) or 0, 0xFFFFFFFF)
            local id_guess = math.floor(delta / 40) + 1
            PCSX.log(string.format(
                "[mon] rel_seek delta=%d sectors (id~%d, byte_off (id-1)*0x14000=0x%X) ra=0x%08X",
                delta, id_guess, (id_guess - 1) * 0x14000, ra))
        end)

        -- Generic disc read: log CdlLOC (disc LBA) for 40-sector reads
        -- (0x28 = monster-block size). Pins the per-block disc source.
        table.insert(bps, { addr = FUN_8003E800, name = "disc_read" })
        probe.arm_breakpoint(FUN_8003E800, "Exec", 4, "disc_read", function()
            local r = PCSX.getRegisters()
            local count = bit.band(tonumber(r.GPR.n.a1) or 0, 0xFFFFFFFF)
            if count ~= 0x28 then return end -- only 40-sector (monster-block) reads
            local ra = bit.band(tonumber(r.GPR.n.ra) or 0, 0xFFFFFFFF)
            local lba, msf = cdloc_lba()
            local prot_off = (lba - 242) * 2048
            PCSX.log(string.format(
                "[mon] disc_read 0x28sec CdlLOC=%s lba=%d prot.dat_off=0x%X ra=0x%08X",
                msf, lba, prot_off, ra))
        end)

        -- Retail host-trap open (filename source).
        table.insert(bps, { addr = FUN_800608F0, name = "host_open" })
        probe.arm_breakpoint(FUN_800608F0, "Exec", 4, "host_open", function()
            local r = PCSX.getRegisters()
            local namep = bit.band(tonumber(r.GPR.n.a0) or 0, 0xFFFFFFFF)
            trap_hits = trap_hits + 1
            PCSX.log(string.format("[mon] host_open #%d name='%s'",
                trap_hits, rd_str(namep, 40)))
        end)

        return bps
    end,

    on_capture = function(_ctx, elapsed)
        if logged_gate or elapsed < 1 then return end
        logged_gate = true
        local gate = string.byte(tostring(probe.read_bytes(DAT_8007B8C2, 1) or "\0"), 1)
        local blk = probe.read_u32(DAT_8007B728)
        PCSX.log(string.format(
            "[mon] start: _DAT_8007b8c2=%d  DAT_8007b728=0x%08X (decoded block base+0x12800=0x%08X)",
            gate, blk, (blk ~= 0) and (blk + 0x12800) or 0))
    end,

    on_done = function()
        csv:close()
        PCSX.log(string.format(
            "=== monster-record probe: %d record(s), %d loader, %d seek, %d host-open ===",
            rec_hits, loader_hits, seek_hits, trap_hits))
    end,
})
