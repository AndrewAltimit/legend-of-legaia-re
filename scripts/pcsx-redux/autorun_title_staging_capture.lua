-- autorun_title_staging_capture.lua
--
-- Boot-time LZS-decode trace. Arms an Exec breakpoint at FUN_8001A55C
-- (the universal LZS decoder, signature `(a0=src_len, a1=src_ptr,
-- a2=dst_ptr)`) and for every fire captures:
--   * a CSV row with decode_idx, src_len, src_ptr, dst_ptr, pc, ra
--   * the full compressed source bytes to
--     <OUT_DIR>/decode_<NNN>_src<XXXXXXXX>_dst<XXXXXXXX>_len<NNNNN>.bin
--
-- Purpose: pin the PROT source of the title-screen overlay. The
-- existing overlay_title.bin snapshot is post-mix (see memory
-- project_title_overlay_capture_is_mixed); the compressed source lives
-- in a staging buffer that's reused after each decode, so the only way
-- to recover it is to read src bytes the instant FUN_8001A55C is
-- entered. Each per-decode .bin file is then a candidate to byte-match
-- against PROT entries (or LZS-decode-and-match) by an offline script.
--
-- Env vars:
--   LEGAIA_SSTATE        save state path (default: sstate7). Ignored when
--                        LEGAIA_NO_SSTATE=1 (the recommended cold-boot mode).
--   LEGAIA_NO_SSTATE     if "1", skip save-state load and let Legaia cold-boot
--                        from BIOS. This is the path that produces the
--                        publisher-logo + title-overlay LZS decodes — any
--                        in-game save state is past those decodes.
--   LEGAIA_OUT_DIR       output directory (default: captures/boot_walk/title_staging)
--   LEGAIA_FRAMES        max vsyncs to capture (default: 3600 = ~60s @ 60Hz)
--   LEGAIA_BOOT_DELAY    vsyncs to wait before loading save (default: 60)
--   LEGAIA_MAX_DECODES   stop after this many decodes (default: 256)
--   LEGAIA_MAX_SRC_BYTES per-decode source dump cap (default: 0x40000 = 256 KiB)
--   LEGAIA_TITLE_LO      title-overlay range lo end (default: 0x801C0000)
--   LEGAIA_TITLE_HI      title-overlay range hi end (default: 0x801F0000)
--   LEGAIA_QUIT_AFTER    vsyncs to capture AFTER a title-range decode (default: 60)
--
-- Output:
--   <OUT_DIR>/decodes.csv               one row per LZS decode
--   <OUT_DIR>/decode_<NNN>_*.bin        per-decode source-buffer dump
--   <OUT_DIR>/summary.txt               final summary (incl. title-range hits)

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local LZS_ENTRY   = 0x8001A55C

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate7")
local OUT_DIR     = probe.getenv("LEGAIA_OUT_DIR", "captures/boot_walk/title_staging")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 3600)
local BOOT_DELAY  = probe.getenv_num("LEGAIA_BOOT_DELAY", 60)
local MAX_DECODES = probe.getenv_num("LEGAIA_MAX_DECODES", 256)
local NO_SSTATE   = probe.getenv("LEGAIA_NO_SSTATE", "") == "1"

if NO_SSTATE then
    -- Monkey-patch the save-state loader to a no-op so probe.run leaves
    -- the BIOS-driven cold boot running. probe.run still requires a
    -- non-nil sstate string (it's `assert`-ed), but the value is never
    -- opened.
    probe.load_save_state = function(_)
        PCSX.log("[stage] LEGAIA_NO_SSTATE=1 -- cold-boot mode; sstate ignored")
        return true
    end
end
local MAX_SRC_BYTES = probe.getenv_num("LEGAIA_MAX_SRC_BYTES", 0x40000)
local TITLE_LO    = probe.getenv_num("LEGAIA_TITLE_LO", 0x801C0000)
local TITLE_HI    = probe.getenv_num("LEGAIA_TITLE_HI", 0x801F0000)
local QUIT_AFTER  = probe.getenv_num("LEGAIA_QUIT_AFTER", 60)

-- mkdir -p the output dir
os.execute(string.format("mkdir -p %q", OUT_DIR))

local CSV_PATH = OUT_DIR .. "/decodes.csv"
local SUMMARY_PATH = OUT_DIR .. "/summary.txt"

local csv = probe.csv_open(CSV_PATH,
    "decode_idx,src_len,src_ptr,dst_ptr,dumped_bytes,pc,ra,head_hex")

PCSX.log(string.format(
    "[stage] LZS entry=0x%08X out=%s sstate=%s frames=%d max_decodes=%d",
    LZS_ENTRY, OUT_DIR, SSTATE_PATH, FRAMES, MAX_DECODES))
PCSX.log(string.format(
    "[stage] title range=0x%08X..0x%08X quit_after=%d vsyncs after first hit",
    TITLE_LO, TITLE_HI, QUIT_AFTER))

-- decode_idx is global so the BP callback can increment it.
local decode_idx = 0
local title_hits = {}
local title_hit_at = nil  -- vsync_in_capture when first title-range hit happened
local hit_cap_reached = false

local function n32(v) return bit.band(v, 0xFFFFFFFF) end

-- Render a short hex fingerprint of the first up-to-16 bytes of src.
local function head_hex(buf, n)
    local s = tostring(buf)
    local lim = math.min(n or 16, #s)
    local parts = {}
    for i = 1, lim do parts[#parts + 1] = string.format("%02X", s:byte(i)) end
    return table.concat(parts)
end

local function bp_callback(ctx)
    if hit_cap_reached then return end

    local r = PCSX.getRegisters()
    local src_len = n32(tonumber(r.GPR.n.a0) or 0)
    local src_ptr = n32(tonumber(r.GPR.n.a1) or 0)
    local dst_ptr = n32(tonumber(r.GPR.n.a2) or 0)
    local pc      = n32(tonumber(r.pc) or 0)
    local ra      = n32(tonumber(r.GPR.n.ra) or 0)

    decode_idx = decode_idx + 1
    local idx = decode_idx

    -- Sanity-clamp src_len. The decoder's loop guard is `blez s2`, so a
    -- nonpositive a0 means a no-op decode; record but don't dump.
    -- An out-of-RAM src_ptr could mean BIOS / scratchpad; skip the dump.
    local dump_len = 0
    local in_ram = probe.in_ram(src_ptr, math.min(src_len, MAX_SRC_BYTES))
    if src_len > 0 and in_ram then
        dump_len = math.min(src_len, MAX_SRC_BYTES)
    end

    local fp = ""
    local out_path = ""
    if dump_len > 0 then
        local buf = probe.read_bytes(src_ptr, dump_len)
        if buf ~= nil then
            fp = head_hex(buf, 16)
            out_path = string.format(
                "%s/decode_%03d_src%08X_dst%08X_len%d.bin",
                OUT_DIR, idx, src_ptr, dst_ptr, src_len)
            local fh = io.open(out_path, "wb")
            if fh ~= nil then
                fh:write(tostring(buf))
                fh:close()
            else
                PCSX.log(string.format(
                    "[stage] WARN: cannot open %s for write", out_path))
                dump_len = 0
            end
        else
            PCSX.log(string.format(
                "[stage] WARN: read_bytes failed src=0x%08X len=%d",
                src_ptr, dump_len))
            dump_len = 0
        end
    end

    csv:row("%d,%d,0x%08X,0x%08X,%d,0x%08X,0x%08X,%s",
        idx, src_len, src_ptr, dst_ptr, dump_len, pc, ra, fp)

    local in_title = (dst_ptr >= TITLE_LO and dst_ptr < TITLE_HI)
    if in_title then
        title_hits[#title_hits + 1] = {
            idx = idx, src_len = src_len, src_ptr = src_ptr,
            dst_ptr = dst_ptr, dumped = dump_len, pc = pc, ra = ra,
        }
        if title_hit_at == nil then
            title_hit_at = ctx.last_capture_vsync or 0
            PCSX.log(string.format(
                "[stage] FIRST title-range hit: decode #%d src=0x%08X dst=0x%08X len=%d ra=0x%08X",
                idx, src_ptr, dst_ptr, src_len, ra))
        end
    end

    if idx <= 8 or in_title then
        PCSX.log(string.format(
            "[stage] decode #%d  src=0x%08X dst=0x%08X len=%d  ra=0x%08X  head=%s",
            idx, src_ptr, dst_ptr, src_len, ra, fp))
    end

    if idx >= MAX_DECODES then
        hit_cap_reached = true
        PCSX.log(string.format(
            "[stage] decode cap %d reached; further BP fires ignored",
            MAX_DECODES))
    end
end

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    boot_delay     = BOOT_DELAY,
    snapshot_path  = OUT_DIR .. "/live_snapshot.txt",
    snapshot_every = 120,

    on_arm = function(ctx)
        local d = { addr = LZS_ENTRY, name = "lzs_entry",
                    hits_ref = { n = 0 } }
        probe.arm_breakpoint(LZS_ENTRY, "Exec", 4, "lzs_entry",
            function() pcall(bp_callback, ctx); d.hits_ref.n = d.hits_ref.n + 1 end)
        PCSX.log(string.format(
            "[stage] Exec BP armed at 0x%08X", LZS_ENTRY))
        return { d }
    end,

    on_capture = function(ctx, elapsed)
        -- Cache the vsync ID so the BP callback can mark "first hit at"
        ctx.last_capture_vsync = elapsed
        -- Early-quit: once we've seen a title-range decode AND given the
        -- emulator QUIT_AFTER more vsyncs for context, bail out.
        if title_hit_at ~= nil and (elapsed - title_hit_at) >= QUIT_AFTER then
            PCSX.log(string.format(
                "[stage] early-quit: title-range hit + %d vsyncs context elapsed",
                QUIT_AFTER))
            ctx.request_quit = true
        end
        -- Also bail once decode-cap is hit (give 30 vsyncs of context).
        if hit_cap_reached then
            ctx.request_quit = true
        end
    end,

    on_done = function(_, _)
        csv:close()
        local f = io.open(SUMMARY_PATH, "w")
        if f ~= nil then
            f:write(string.format(
                "# title-staging LZS-decode trace\n"))
            f:write(string.format(
                "sstate         %s\n", SSTATE_PATH))
            f:write(string.format(
                "out_dir        %s\n", OUT_DIR))
            f:write(string.format(
                "decodes_total  %d\n", decode_idx))
            f:write(string.format(
                "title_range    0x%08X..0x%08X\n", TITLE_LO, TITLE_HI))
            f:write(string.format(
                "title_hits     %d\n", #title_hits))
            for _, h in ipairs(title_hits) do
                f:write(string.format(
                    "  #%d  src=0x%08X  dst=0x%08X  len=%d  dumped=%d  ra=0x%08X\n",
                    h.idx, h.src_ptr, h.dst_ptr, h.src_len, h.dumped, h.ra))
            end
            f:close()
        end
        PCSX.log(string.format(
            "[stage] done: %d total decodes, %d in title range; csv=%s summary=%s",
            decode_idx, #title_hits, CSV_PATH, SUMMARY_PATH))
    end,
})
