-- autorun_boot_warning_screen.lua
--
-- Cold-boot capture answering three questions no static read closes:
--
--   1. What draws the `init.pak` WARNING screen? PROT 0895 uploads the health
--      warning TIM to VRAM (704, 0) and gives it sprite descriptor 1 in the
--      six-record table at 0x801F369C, but none of the five FUN_801CFBB8 call
--      sites in 0895 passes id 1. An exec breakpoint on FUN_801CFBB8 logs
--      every (z, cx, cy, desc, level, scale) the boot actually issues, and a
--      RAM primitive sweep looks for a packet carrying the WARNING CLUT
--      (0x7E80) whoever built it.
--   2. Which title sub-mode does a cold boot show? FUN_801DD35C's init stores
--      0x02 into the title state word and overwrites it with 0x11 only when
--      the entry word at 0x8007BB00 is non-zero. Both are polled per vsync.
--      NB the sub-mode word is at 0x801F0204 (`lui a2,0x801f` +
--      `sw v0,0x204(a2)` at instruction 0x801DD920) - 0x801DD920 is the
--      INSTRUCTION address, not the data address.
--   3. Does anything sample the card-screen kanji page? Its eight bit-plane
--      CBAs (0x76C0 + plane*0x40) are swept in the same pass.
--
-- The logo CLUTs are swept alongside the hunted ones as a POSITIVE CONTROL:
-- SCEA / Contrail / PROKION are known to draw, so a run in which they appear
-- and WARNING does not is evidence of absence rather than a broken scanner.
--
-- Launch (cold boot - no save state):
--   LEGAIA_NO_SSTATE=1 LEGAIA_FRAMES=3400 \
--   LEGAIA_PAD_SCRIPT=1300:CROSS,1520:DOWN,1620:CROSS \
--   timeout 3600 bash scripts/pcsx-redux/run_probe.sh \
--     --isolate-config \
--     --lua scripts/pcsx-redux/autorun_boot_warning_screen.lua \
--     --out-dir captures/w1e/boot_warning
--
-- Outputs (in the run dir):
--   sprite_calls.csv   tick,pc,ra,z,cx,cy,desc,level,scale,mode,resident
--   timeline.csv       tick,mode,submode,entry_word_8007bb00,note
--   prim_hits.csv      tick,label,clut,packet_va,code,x,y,u,v,tpage,total,plausible
--   summary.txt        per-descriptor call counts + per-CLUT first-seen ticks

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local scan  = require("probe.prim_scan")

-- ---------------------------------------------------------------- addresses
local GAME_MODE      = 0x8007B83C   -- u16 master game mode
local TITLE_SUBMODE  = 0x801F0204   -- title state +0x204 (see header note)
local ENTRY_WORD     = 0x8007BB00   -- _DAT_8007BB00, read with `lw`
local SPRITE_EMIT    = 0x801CFBB8   -- FUN_801CFBB8(z, cx, cy, desc, level, scale)
local DESC_TABLE     = 0x801F369C   -- the six 20-byte sprite descriptors
-- The menu overlay's two MoveImage sites that PARK the card-screen kanji page:
-- (0,475) 256x1 -> (0,492) and (320,256) 128x256 -> (704, 0). The second lands
-- the kanji page exactly on top of the WARNING TIM's VRAM rect, so if these
-- fire the health warning's pixels are gone from that point on.
local PARK_SITES = { 0x801DDD0C, 0x801DDD34 }

-- Descriptor 1's CLUT is 0x7E80 and its tpage 0x000B - read from the bytes at
-- PROT 0895 file +0x24E84, NOT from the boot doc's upload table (0x9A/0x7ED4
-- is descriptor 0/5, PROKION).
local CLUTS = {
    { label = "WARNING",  clut = 0x7E80 },   -- descriptor 1, tpage 0x0B
    { label = "PROKION",  clut = 0x7ED4 },   -- descriptors 0 + 5, tpage 0x9A  (control)
    { label = "SCEA",     clut = 0x7F14 },   -- descriptors 2 + 3, tpage 0x0A  (control)
    { label = "Contrail", clut = 0x7F54 },   -- descriptor 4,     tpage 0x9C  (control)
}
-- Card-screen kanji font: one CBA per bit-plane, CLUT rows 475..482.
for plane = 0, 7 do
    CLUTS[#CLUTS + 1] = { label = string.format("kanji_p%d", plane),
                          clut = 0x76C0 + plane * 0x40 }
end

local FRAMES     = probe.getenv_num("LEGAIA_FRAMES", 5400)
local SCAN_EVERY = probe.getenv_num("LEGAIA_SCAN_EVERY", 20)
-- Pad schedule. LEGAIA_PAD_SCRIPT is a comma-separated "tick:BUTTON" list, e.g.
--   LEGAIA_PAD_SCRIPT=1300:CROSS,1500:DOWN,1600:CROSS
-- Each press is held HOLD_FRAMES vsyncs. The title front-end needs a LADDER,
-- not one press: sub-mode 0x10 is the Press-Start wait, confirming there fades
-- through 0x18 into 0x14 (the two-row NEW GAME / CONTINUE menu), and only a
-- second Down + confirm from 0x14 opens the memory-card screen. Empty script =
-- a pure no-input boot (the attract FMV then fires on the idle countdown).
local PAD_SCRIPT_RAW = probe.getenv("LEGAIA_PAD_SCRIPT", "")
local HOLD_FRAMES    = probe.getenv_num("LEGAIA_PAD_HOLD", 8)

local calls_csv = probe.csv_open(probe.out_path("sprite_calls.csv"),
    "tick,pc,ra,z,cx,cy,desc,level,scale,mode,resident")
local time_csv  = probe.csv_open(probe.out_path("timeline.csv"),
    "tick,mode,submode,entry_8007bb00,note")
local prim_csv  = probe.csv_open(probe.out_path("prim_hits.csv"),
    "tick,label,clut,packet_va,code,x,y,u,v,tpage,total,plausible")

local desc_counts   = {}          -- desc id -> call count
local desc_first    = {}          -- desc id -> first tick seen
local call_hits     = 0
local clut_first    = {}          -- label -> first tick seen
local clut_total    = {}          -- label -> peak candidate count
local last_mode, last_sub, last_entry = -1, -1, -1

-- Cold boot: neuter the save-state loader (the autorun_boot_watch idiom).
require("probe.sstate").load = function(_)
    PCSX.log("[warn-screen] cold boot; sstate load skipped")
    return true
end

-- Is PROT 0895 the image currently at 0x801CE818? Its descriptor table's
-- record 0 begins with the size scale 0x1000 and carries tpage 0x9A /
-- clut 0x7ED4 - a 3-word fingerprint no other slot-A image reproduces.
local function init_pak_resident()
    local w0 = probe.read_u32(DESC_TABLE)
    local w1 = probe.read_u32(DESC_TABLE + 4)
    if w0 == nil or w1 == nil then return 0 end
    if w0 == 0x1000 and w1 == 0x7ED4009A then return 1 end
    return 0
end

-- Parse LEGAIA_PAD_SCRIPT into { tick, bit, name } steps.
local PAD_SCRIPT = {}
for entry in string.gmatch(PAD_SCRIPT_RAW, "[^,]+") do
    local tick, name = string.match(entry, "^%s*(%d+)%s*:%s*(%a+)%s*$")
    if tick ~= nil and probe.BTN[string.upper(name)] ~= nil then
        PAD_SCRIPT[#PAD_SCRIPT + 1] = { tick = tonumber(tick),
                                        bit = probe.BTN[string.upper(name)],
                                        name = string.upper(name) }
    else
        PCSX.log(string.format("[warn-screen] ignoring bad pad step '%s'", entry))
    end
end

local g_tick = 0

-- Framebuffer grab, so "the run was parked on the memory-card screen" is a
-- picture rather than a sub-mode number. Decode with
-- scripts/pcsx-redux/decode_pcsx_screen.py.
local FB_EVERY = probe.getenv_num("LEGAIA_FB_EVERY", 0)
local function grab_fb(tag)
    local ok, ss = pcall(function() return PCSX.GPU.takeScreenShot() end)
    if not ok or ss == nil then return false end
    local bpp = tonumber(ss.bpp) or 0
    local bits = (bpp > 16) and 24 or 16
    local fh = io.open(probe.out_path(string.format("fb_%s.screen", tag)), "wb")
    if fh == nil then return false end
    fh:write(tostring(ss.data)); fh:close()
    local mh = io.open(probe.out_path(string.format("fb_%s.screen.meta", tag)), "w")
    if mh ~= nil then
        mh:write(string.format("width=%d\nheight=%d\nbpp=%d\nbytes_per_pixel=%d\n",
            tonumber(ss.width), tonumber(ss.height), bits, bits / 8))
        mh:close()
    end
    return true
end

probe.run({
    sstate         = "unused",
    capture_frames = FRAMES,
    snapshot_path  = probe.out_path("boot_warning.snapshot.txt"),
    on_arm = function()
        local d = { addr = SPRITE_EMIT, name = "FUN_801CFBB8",
                    hits_ref = { n = 0 } }
        probe.arm_breakpoint(SPRITE_EMIT, "Exec", 4, d.name, function()
            local r  = PCSX.getRegisters()
            local gp = r.GPR.n
            local sp = bit.band(tonumber(gp.sp), 0xFFFFFFFF)
            -- MIPS o32: args 5 and 6 land at sp+0x10 / sp+0x14.
            local level = probe.read_u32(sp + 0x10) or 0
            local scale = probe.read_u32(sp + 0x14) or 0
            local desc  = bit.band(tonumber(gp.a3), 0xFFFFFFFF)
            calls_csv:row("%d,0x%08X,0x%08X,%d,%d,%d,%d,%d,%d,0x%X,%d",
                g_tick,
                bit.band(tonumber(r.pc), 0xFFFFFFFF),
                bit.band(tonumber(gp.ra), 0xFFFFFFFF),
                bit.band(tonumber(gp.a0), 0xFFFFFFFF),
                bit.band(tonumber(gp.a1), 0xFFFFFFFF),
                bit.band(tonumber(gp.a2), 0xFFFFFFFF),
                desc, level, scale,
                probe.read_u16(GAME_MODE) or 0,
                init_pak_resident())
            desc_counts[desc] = (desc_counts[desc] or 0) + 1
            if desc_first[desc] == nil then desc_first[desc] = g_tick end
            d.hits_ref.n = d.hits_ref.n + 1
            call_hits = call_hits + 1
        end)

        local descs = { d }

        -- Kanji-page park sites: does the (704, 0) WARNING rect get overwritten?
        for _, site in ipairs(PARK_SITES) do
            local ds = { addr = site, name = string.format("kanji_park_%08X", site),
                         hits_ref = { n = 0 } }
            probe.arm_breakpoint(site, "Exec", 4, ds.name, function()
                local r = PCSX.getRegisters()
                ds.hits_ref.n = ds.hits_ref.n + 1
                if ds.hits_ref.n <= 8 then
                    time_csv:row("%d,0x%X,0x%X,0x%X,park-0x%08X-ra-0x%08X",
                        g_tick, probe.read_u16(GAME_MODE) or 0,
                        probe.read_u32(TITLE_SUBMODE) or 0,
                        probe.read_u32(ENTRY_WORD) or 0, site,
                        bit.band(tonumber(r.GPR.n.ra), 0xFFFFFFFF))
                    PCSX.log(string.format("[warn-screen] kanji park site 0x%08X at tick %d",
                        site, g_tick))
                end
            end)
            descs[#descs + 1] = ds
        end

        -- Who raises _DAT_8007BB00? The cold boot finds it non-zero long before
        -- the title, and the sub-mode the player sees turns on that word.
        local dw = { addr = ENTRY_WORD, name = "write_8007BB00", hits_ref = { n = 0 } }
        probe.arm_breakpoint(ENTRY_WORD, "Write", 4, dw.name, function()
            local r = PCSX.getRegisters()
            dw.hits_ref.n = dw.hits_ref.n + 1
            if dw.hits_ref.n <= 24 then
                time_csv:row("%d,0x%X,0x%X,0x%X,write-8007BB00-pc-0x%08X-ra-0x%08X",
                    g_tick, probe.read_u16(GAME_MODE) or 0,
                    probe.read_u32(TITLE_SUBMODE) or 0,
                    probe.read_u32(ENTRY_WORD) or 0,
                    bit.band(tonumber(r.pc), 0xFFFFFFFF),
                    bit.band(tonumber(r.GPR.n.ra), 0xFFFFFFFF))
            end
        end)
        descs[#descs + 1] = dw

        return descs
    end,

    on_capture = function(ctx, elapsed)
        g_tick = elapsed

        local mode  = probe.read_u16(GAME_MODE) or -1
        local sub   = probe.read_u32(TITLE_SUBMODE) or -1
        local entry = probe.read_u32(ENTRY_WORD) or -1
        if mode ~= last_mode or sub ~= last_sub or entry ~= last_entry then
            time_csv:row("%d,0x%X,0x%X,0x%X,change", elapsed, mode, sub, entry)
            last_mode, last_sub, last_entry = mode, sub, entry
            if FB_EVERY > 0 then grab_fb(string.format("%05d_state", elapsed)) end
        elseif elapsed % 300 == 0 then
            time_csv:row("%d,0x%X,0x%X,0x%X,tick", elapsed, mode, sub, entry)
        end

        -- Pad schedule.
        for _, step in ipairs(PAD_SCRIPT) do
            if elapsed == step.tick then
                probe.pad_force(step.bit)
                time_csv:row("%d,0x%X,0x%X,0x%X,pad-%s", elapsed, mode, sub, entry, step.name)
            elseif elapsed == step.tick + HOLD_FRAMES then
                probe.pad_release(step.bit)
            end
        end

        if FB_EVERY > 0 and (elapsed % FB_EVERY) == 0 then
            grab_fb(string.format("%05d", elapsed))
        end

        if (elapsed % SCAN_EVERY) == 0 then
            local ram = scan.snapshot()
            if ram ~= nil then
                scan.sweep(ram, CLUTS, function(label, clut, hit, total)
                    if hit == nil then return end
                    local ok = scan.plausible(hit)
                    if ok and clut_first[label] == nil then
                        clut_first[label] = elapsed
                        PCSX.log(string.format(
                            "[warn-screen] first PLAUSIBLE %s packet at tick %d: 0x%08X code=0x%02X (%d,%d)",
                            label, elapsed, hit.packet_va, hit.code, hit.x, hit.y))
                    end
                    if ok and (clut_total[label] or 0) < total then clut_total[label] = total end
                    prim_csv:row("%d,%s,0x%04X,0x%08X,0x%02X,%d,%d,%d,%d,%s,%d,%d",
                        elapsed, label, clut, hit.packet_va, hit.code,
                        hit.x, hit.y, hit.u or 0, hit.v or 0,
                        hit.tpage and string.format("0x%04X", hit.tpage) or "-",
                        total, ok and 1 or 0)
                end, 6)
            end
        end
    end,

    on_summary = function()
        PCSX.log("=== probe hits ===")
        PCSX.log(string.format("  FUN_801CFBB8 calls: %d", call_hits))
        for desc = 0, 5 do
            PCSX.log(string.format("    desc %d: %d calls, first tick %s",
                desc, desc_counts[desc] or 0, tostring(desc_first[desc])))
        end
        for _, w in ipairs(CLUTS) do
            PCSX.log(string.format("  CLUT %-10s 0x%04X first_plausible_tick=%s peak_candidates=%d",
                w.label, w.clut, tostring(clut_first[w.label]),
                clut_total[w.label] or 0))
        end
        PCSX.log("=== end ===")
    end,

    on_done = function()
        local fh = io.open(probe.out_path("summary.txt"), "w")
        if fh then
            fh:write(string.format("FUN_801CFBB8 total calls: %d\n", call_hits))
            for desc = 0, 5 do
                fh:write(string.format("  desc %d: calls=%d first_tick=%s\n",
                    desc, desc_counts[desc] or 0, tostring(desc_first[desc])))
            end
            fh:write("\nCLUT sweep (packet word-3 high halfword):\n")
            for _, w in ipairs(CLUTS) do
                fh:write(string.format("  %-10s 0x%04X first_plausible_tick=%s peak_candidates=%d\n",
                    w.label, w.clut, tostring(clut_first[w.label]),
                    clut_total[w.label] or 0))
            end
            fh:write(string.format("\nlast mode=0x%X submode=0x%X entry_8007bb00=0x%X\n",
                last_mode, last_sub, last_entry))
            fh:close()
        end
        calls_csv:close(); time_csv:close(); prim_csv:close()
    end,
})
