-- probe/version.lua  -- USA-build (SCUS_942.54) version guard.
--
-- Every probe in this tree hard-codes RAM addresses (flag bank, gold,
-- scene name, mode byte, the FUN_8003CE08 flag-set entry) that are
-- specific to the USA retail build `SCUS_942.54`. Point one at a JP /
-- EU / PAL disc or a different revision and it arms exec-breakpoints on
-- the wrong code and diffs the wrong bytes - handing back a CSV of
-- plausible-looking GARBAGE with no error. For a one-time,
-- human-hours capture (a community playthrough) that is the worst
-- failure mode: silent, and you only find out weeks later.
--
-- This module fingerprints a few words of ALWAYS-RESIDENT SCUS code at
-- the exact function entries the probes depend on. That region never
-- pages out to an overlay and is byte-identical across every USA game
-- state, but differs on any other region/revision (different code, or
-- the string/layout simply isn't there). So a matching fingerprint
-- confirms both "this is Legaia" AND "this is the USA layout my data
-- addresses assume".
--
-- Usage (see autorun_state_poll.lua for a live example):
--   local version = require("probe.version")
--   local ok, msg, sig = version.check(version.USA_FINGERPRINT)
--   if not ok then <log FATAL, do NOT arm> end
--
-- Locking it down for a handoff:
--   1. Run any probe with LEGAIA_FP_RECORD=1 on your KNOWN-GOOD USA
--      disc. It logs `[version] fingerprint = <hex>` and refuses to arm.
--   2. Paste that hex into USA_FINGERPRINT below (or export
--      LEGAIA_FP_EXPECTED=<hex> to override without editing).
--   3. Ship it. Any volunteer on a non-USA disc now gets a hard refusal
--      instead of silent garbage.
--
-- Until USA_FINGERPRINT is set the guard runs in UNLOCKED mode: it still
-- rejects a clearly-not-loaded / not-Legaia state, but only WARNS on a
-- fingerprint it can't verify, so the author can iterate on their own
-- box before locking the value in.

local mem = require("probe.mem")
local bit = require("bit")

local M = {}

-- Function entries the probes key off. All in the static SCUS load
-- region (0x80010000..0x800FFFFF) -> always resident, never overlaid.
--   0x8003CE08  FUN_8003CE08  story-flag SET
--   0x8003CE34  FUN_8003CE34  story-flag CLEAR
--   0x8003CE64  FUN_8003CE64  story-flag TEST
M.SIG_ADDRS = { 0x8003CE08, 0x8003CE34, 0x8003CE64 }
M.SIG_WORDS = 6  -- instruction words hashed per address

-- Set this to the hex string printed by a LEGAIA_FP_RECORD=1 run on your
-- USA disc to lock the guard. Left empty = UNLOCKED (warn-only).
-- Override at runtime with LEGAIA_FP_EXPECTED.
--
-- Locked value = 6 words each at 0x8003CE08 / _CE34 / _CE64, recorded off
-- the USA SCUS_942.54 build (Legend of Legaia (USA), GAME ID SCUS94254).
-- The SET/CLEAR prologues share their first 6 words, hence the repeat.
M.USA_FINGERPRINT = "3C02800824424140000428C300A2282130840007240200803C02800824424140000428C300A2282130840007240200803C03800824634140000410C3004310219043161830840007"

local function u32(v)
    v = bit.band(tonumber(v) or 0, 0xFFFFFFFF)
    if v < 0 then v = v + 4294967296 end
    return v
end

-- Read SIG_WORDS words at each SIG_ADDR and concatenate as uppercase
-- hex. Order-sensitive; returns nil if any word is unmapped (RAM not up
-- yet). The raw-hex form is deliberately transparent - no hashing, so a
-- mismatch can be eyeballed word-by-word.
function M.signature()
    local parts = {}
    for _, a in ipairs(M.SIG_ADDRS) do
        for i = 0, M.SIG_WORDS - 1 do
            local w = mem.read_u32(a + i * 4)
            if w == nil then return nil end
            parts[#parts + 1] = string.format("%08X", u32(w))
        end
    end
    return table.concat(parts)
end

-- Coarse "is this even Legaia" signal, independent of the exact
-- fingerprint: scan the SCUS load window for a known dev string. Cheap
-- one-shot (a single bulk read + find). Returns true/false.
function M.has_legaia_anchor()
    local blob = mem.read_bytes(0x80010000, 0xF0000)  -- ~960 KiB of SCUS
    if blob == nil then return false end
    local s = tostring(blob)
    return s:find("h:\\prot\\cdname.dat", 1, true) ~= nil
        or s:find("FIELD PROGRAM", 1, true) ~= nil
end

-- "Is SCUS_942.54 actually loaded yet?" - LATCHED anchor presence. This is
-- the residency signal: during BIOS boot (or a cold NO_SSTATE start) the
-- SCUS region is NOT yet in RAM, and 0x8003CE08 holds unrelated garbage
-- that is neither all-0 nor all-F. So "no anchor" means NOT-BOOTED, which
-- callers MUST treat as transient (keep waiting), never as "wrong game" -
-- that false-positive killed a whole capture. Once seen, we latch true and
-- stop rescanning (the ~960 KiB read only happens during boot).
-- A signature is meaningful only once the fingerprinted CODE is loaded.
-- During boot the SCUS rodata (anchor strings) can land BEFORE the .text at
-- 0x8003CE08, so an anchor hit alone is not enough - a zero/all-F signature
-- means the code region isn't in yet.
function M.signature_valid(sig)
    return sig ~= nil and not sig:match("^0+$") and not sig:match("^F+$")
end

M._resident = false
function M.scus_resident()
    if M._resident then return true end
    -- Resident once BOTH the fingerprinted code is actually loaded (valid,
    -- non-trivial signature) AND a Legaia anchor string is present (rules out
    -- non-Legaia garbage that merely happens to be non-zero at 0x8003CE08).
    if M.signature_valid(M.signature()) and M.has_legaia_anchor() then
        M._resident = true
    end
    return M._resident
end

-- Verify the current state. Returns (ok, message, terminal):
--   ok       - true only when it is safe to capture on this build.
--   terminal - true ONLY on a real wrong-revision (anchor present but the
--              fingerprint differs). A non-terminal false = "wait, not
--              booted yet"; the caller keeps polling. This split is the fix
--              for the boot-time false-positive.
--
--   RAM not readable                     -> (false, "not mapped", false)
--   SCUS not resident (no anchor yet)     -> (false, "waiting...", false)
--   UNLOCKED + resident                   -> (true,  "UNLOCKED ...", false)
--   locked + fingerprint matches          -> (true,  "OK ...", false)
--   locked + fingerprint differs          -> (false, "MISMATCH ...", TRUE)
function M.check(expected)
    expected = expected or ""
    local env_exp = os.getenv("LEGAIA_FP_EXPECTED")
    if env_exp ~= nil and env_exp ~= "" then expected = env_exp end

    -- Residency requires the fingerprinted code loaded AND a Legaia anchor
    -- (see scus_resident). Until then it's booting => transient wait, never
    -- terminal. This gates out the partial-load window where the signature
    -- reads all-zero.
    if not M.scus_resident() then
        return false, "SCUS not resident yet (code+anchor not both in - booting?)", false
    end
    local sig = M.signature()

    if expected == "" then
        return true,
            "UNLOCKED (no fingerprint set; set LEGAIA_FP_EXPECTED or " ..
            "version.USA_FINGERPRINT to lock) sig=" .. sig,
            false
    end
    if sig == expected then
        return true, "OK (fingerprint matched USA build) sig=" .. sig, false
    end
    -- Anchor present (this IS Legaia) but fingerprint differs => genuinely a
    -- different region/revision. Terminal.
    return false,
        string.format("MISMATCH - not the expected USA SCUS_942.54 build.\n" ..
            "  expected = %s\n  observed = %s", expected, sig),
        true
end

-- Record mode: driven by LEGAIA_FP_RECORD=1.
function M.record_mode()
    return os.getenv("LEGAIA_FP_RECORD") == "1"
end

-- Fingerprint for a record run, but ONLY once SCUS is resident (so a record
-- run started cold doesn't capture pre-boot garbage). nil until resident.
function M.record_fingerprint()
    if not M.scus_resident() then return nil end
    return M.signature()
end

return M
