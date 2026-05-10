import { existsSync, readFileSync, writeFileSync } from "fs";

/**
 * Version-bump gate for the "star the repo" prompt in `kesha install`.
 *
 * Prompts are valuable on first install and on meaningful upgrades (new
 * features) but annoying on patch-only bumps. This module persists the
 * last-seen version to a marker file next to the engine binary and only
 * returns true on a major-or-minor bump.
 */
export function starSeenPath(binPath: string): string {
  return `${binPath}.star-seen`;
}

export function readStarSeen(binPath: string): string | null {
  try {
    const v = readFileSync(starSeenPath(binPath), "utf-8").trim();
    return v.length > 0 ? v : null;
  } catch {
    return null;
  }
}

export function writeStarSeen(binPath: string, version: string): void {
  writeFileSync(starSeenPath(binPath), `${version}\n`);
}

function parseMajorMinor(v: string): [number, number] | null {
  const parts = v.split(".");
  if (parts.length < 2) return null;
  const major = Number(parts[0]);
  const minor = Number(parts[1]);
  if (!Number.isFinite(major) || !Number.isFinite(minor)) return null;
  return [major, minor];
}

/**
 * Returns true iff `current` represents a major-or-minor bump over `seen`.
 * - `seen === null` → true (first install, always prompt once).
 * - Same or downgraded version → false.
 * - Patch-only bump → false (annoying on every install).
 * - Unparseable either side → false (don't nag when we can't reason).
 */
export function shouldShowStarPrompt(current: string, seen: string | null): boolean {
  if (seen === null) return true;
  const c = parseMajorMinor(current);
  const s = parseMajorMinor(seen);
  if (!c || !s) return false;
  if (c[0] > s[0]) return true;
  if (c[0] === s[0] && c[1] > s[1]) return true;
  return false;
}

/** True when a star-seen marker already exists for this install. */
export function hasStarMarker(binPath: string): boolean {
  return existsSync(starSeenPath(binPath));
}

/**
 * Prompt the user to star the repo if and only if `shouldShowStarPrompt`
 * agrees (first install + major-or-minor bumps, never on patch). Records
 * the prompt against the current version up front so a single run never
 * prompts twice — failures from the gh subprocess below don't reopen the
 * gate. Shared by `kesha install` and `kesha status` so opt-in installs
 * (`--tts`, `--diarize`) and `status` reuse the same marker; a user who
 * saw the prompt on the base install won't see it again on the opt-in or
 * the status check.
 *
 * No-ops when the gate says skip, when `currentVersion` is null, or when
 * the user has already starred. When `gh` is missing or unauthenticated,
 * a basic prompt is still printed (so the marker write isn't a silent
 * slot consumption).
 *
 * `shims` lets tests inject deterministic `which` / `spawn` implementations.
 * Production callers leave it undefined; the defaults route through Bun's
 * built-ins. Bun.which() caches PATH at process start so swapping
 * process.env.PATH in tests doesn't work — the shim is the supported way
 * to fake gh's presence and authentication state from a unit test.
 */
export async function maybeAskForStar(
  binPath: string,
  currentVersion: string | null,
  log: { info: (msg: string) => void },
  shims?: {
    which?: (name: string) => string | null;
    spawn?: (cmd: string[]) => { exitCode: number | null };
  },
): Promise<void> {
  const which = shims?.which ?? ((n: string) => Bun.which(n));
  const spawn =
    shims?.spawn ??
    ((cmd: string[]) =>
      Bun.spawnSync(cmd, { stdout: "ignore", stderr: "ignore" }));
  if (!currentVersion) return;
  const seen = readStarSeen(binPath);
  if (!shouldShowStarPrompt(currentVersion, seen)) {
    return;
  }
  try {
    writeStarSeen(binPath, currentVersion);
  } catch {
    /* Non-fatal — falling through to the prompt is still OK. */
  }

  // The marker has been recorded — every path from here that reaches
  // a `return` without printing would silently consume the user's
  // once-per-major.minor slot. Only the "already starred" path is
  // allowed to skip the print, since the consumption is the verification
  // that they're already supporting the project.
  const printBasicPrompt = () => {
    log.info("\nIf you enjoy Kesha Voice Kit, consider starring the repo:");
    log.info("  https://github.com/drakulavich/kesha-voice-kit");
  };

  const gh = which("gh");
  if (!gh) {
    printBasicPrompt();
    return;
  }
  const authCheck = spawn([gh, "auth", "status"]);
  if (authCheck.exitCode !== 0) {
    // gh installed but unauthenticated — we can't check star status, but
    // we can still nudge with the same basic prompt the no-gh path uses.
    // Without this, the marker write above silently consumes the slot.
    printBasicPrompt();
    return;
  }
  const starred = spawn([gh, "api", "user/starred/drakulavich/kesha-voice-kit"]);
  if (starred.exitCode === 0) return; // already starred — slot consumed by verification
  log.info("\n⭐ If you enjoy Kesha Voice Kit, star it on GitHub:");
  log.info("  https://github.com/drakulavich/kesha-voice-kit");
  log.info('  Or run: gh api -X PUT /user/starred/drakulavich/kesha-voice-kit');
}
