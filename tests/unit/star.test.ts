import { describe, test, expect, afterEach } from "bun:test";
import {
  shouldShowStarPrompt,
  starSeenPath,
  readStarSeen,
  writeStarSeen,
  hasStarMarker,
  maybeAskForStar,
} from "../../src/star";
import { mkdtempSync, writeFileSync, rmSync } from "fs";
import { join } from "path";
import { tmpdir } from "os";

const tmpDirs: string[] = [];

function mkTmpBinPath(): string {
  const dir = mkdtempSync(join(tmpdir(), "kesha-star-test-"));
  tmpDirs.push(dir);
  return join(dir, "kesha-engine");
}

afterEach(() => {
  while (tmpDirs.length > 0) {
    rmSync(tmpDirs.pop()!, { recursive: true, force: true });
  }
});

describe("shouldShowStarPrompt — version-bump gate", () => {
  test("first install (null seen) → show", () => {
    expect(shouldShowStarPrompt("1.2.0", null)).toBe(true);
  });

  test("same version → skip", () => {
    expect(shouldShowStarPrompt("1.2.0", "1.2.0")).toBe(false);
  });

  test("patch bump → skip", () => {
    expect(shouldShowStarPrompt("1.2.1", "1.2.0")).toBe(false);
    expect(shouldShowStarPrompt("1.2.99", "1.2.0")).toBe(false);
  });

  test("minor bump → show", () => {
    expect(shouldShowStarPrompt("1.3.0", "1.2.99")).toBe(true);
    expect(shouldShowStarPrompt("1.2.0", "1.1.3")).toBe(true);
  });

  test("major bump → show", () => {
    expect(shouldShowStarPrompt("2.0.0", "1.99.99")).toBe(true);
  });

  test("downgrade → skip", () => {
    expect(shouldShowStarPrompt("1.1.0", "1.2.0")).toBe(false);
    expect(shouldShowStarPrompt("1.0.0", "2.0.0")).toBe(false);
  });

  test("unparseable version → skip (don't nag on garbage)", () => {
    expect(shouldShowStarPrompt("not-a-version", "1.2.0")).toBe(false);
    expect(shouldShowStarPrompt("1.2.0", "garbage")).toBe(false);
    expect(shouldShowStarPrompt("1", "1.0.0")).toBe(false); // too few parts
  });

  test("npm-style prerelease still parses major/minor correctly", () => {
    // `1.3.0-rc.1`.split(".") → ["1", "3", "0-rc", "1"]; major/minor parse ok.
    expect(shouldShowStarPrompt("1.3.0-rc.1", "1.2.0")).toBe(true);
  });
});

describe("star-seen marker file", () => {
  test("starSeenPath appends .star-seen", () => {
    expect(starSeenPath("/bin/kesha-engine")).toBe("/bin/kesha-engine.star-seen");
  });

  test("round-trip write/read", () => {
    const binPath = mkTmpBinPath();
    expect(hasStarMarker(binPath)).toBe(false);
    writeStarSeen(binPath, "1.2.0");
    expect(hasStarMarker(binPath)).toBe(true);
    expect(readStarSeen(binPath)).toBe("1.2.0");
    rmSync(starSeenPath(binPath));
  });

  test("read returns null when missing", () => {
    const binPath = mkTmpBinPath();
    expect(readStarSeen(binPath)).toBeNull();
  });

  test("read returns null on empty / whitespace", () => {
    const binPath = mkTmpBinPath();
    writeFileSync(starSeenPath(binPath), "\n\n  ");
    expect(readStarSeen(binPath)).toBeNull();
    rmSync(starSeenPath(binPath));
  });

  test("overwrite replaces previous", () => {
    const binPath = mkTmpBinPath();
    writeStarSeen(binPath, "1.2.0");
    writeStarSeen(binPath, "1.3.0");
    expect(readStarSeen(binPath)).toBe("1.3.0");
    rmSync(starSeenPath(binPath));
  });
});

describe("maybeAskForStar — orchestration", () => {
  function captureLog() {
    const lines: string[] = [];
    return { log: { info: (m: string) => lines.push(m) }, lines };
  }

  // gh shim factory — `auth` is the first argv when checking auth status,
  // `api` is the first argv when probing star state. Tests configure
  // exitCode for each so we can simulate every branch deterministically
  // without spawning a real subprocess (Bun.which caches PATH at process
  // start, so PATH manipulation in-test does not work).
  function shimsWithGh(opts: { authExit: number; apiExit: number }) {
    return {
      which: () => "/fake/gh",
      spawn: (cmd: string[]) => ({
        exitCode: cmd[1] === "auth" ? opts.authExit : opts.apiExit,
      }),
    };
  }
  const shimsNoGh = { which: () => null };

  test("null currentVersion → no-op (no marker, no log)", async () => {
    const binPath = mkTmpBinPath();
    const { log, lines } = captureLog();
    await maybeAskForStar(binPath, null, log, shimsNoGh);
    expect(hasStarMarker(binPath)).toBe(false);
    expect(lines).toEqual([]);
  });

  test("gate says skip (already seen current major.minor) → no-op", async () => {
    const binPath = mkTmpBinPath();
    writeStarSeen(binPath, "1.2.0");
    const { log, lines } = captureLog();
    await maybeAskForStar(binPath, "1.2.0", log, shimsNoGh);
    expect(readStarSeen(binPath)).toBe("1.2.0");
    expect(lines).toEqual([]);
  });

  test("no `gh` on PATH → marker written + basic prompt printed", async () => {
    const binPath = mkTmpBinPath();
    const { log, lines } = captureLog();
    await maybeAskForStar(binPath, "1.2.0", log, shimsNoGh);
    expect(hasStarMarker(binPath)).toBe(true);
    expect(lines.join("\n")).toContain("consider starring the repo");
  });

  test("gh present but unauthenticated → marker written + basic prompt printed", async () => {
    // Regression: previously the marker write consumed the major.minor
    // slot but the unauthenticated branch returned without any user-visible
    // output. With the fix, the user sees the same basic prompt as the
    // no-gh path.
    const binPath = mkTmpBinPath();
    const { log, lines } = captureLog();
    await maybeAskForStar(binPath, "1.2.0", log, shimsWithGh({ authExit: 1, apiExit: 0 }));
    expect(hasStarMarker(binPath)).toBe(true);
    expect(lines.join("\n")).toContain("consider starring the repo");
  });

  test("gh authenticated + already starred → marker consumed, no prompt", async () => {
    const binPath = mkTmpBinPath();
    const { log, lines } = captureLog();
    await maybeAskForStar(binPath, "1.2.0", log, shimsWithGh({ authExit: 0, apiExit: 0 }));
    expect(hasStarMarker(binPath)).toBe(true);
    expect(lines).toEqual([]);
  });

  test("gh authenticated + not starred → marker written + rich prompt", async () => {
    const binPath = mkTmpBinPath();
    const { log, lines } = captureLog();
    await maybeAskForStar(binPath, "1.2.0", log, shimsWithGh({ authExit: 0, apiExit: 1 }));
    expect(hasStarMarker(binPath)).toBe(true);
    const out = lines.join("\n");
    expect(out).toContain("⭐ If you enjoy Kesha Voice Kit");
    expect(out).toContain("gh api -X PUT /user/starred");
  });
});
