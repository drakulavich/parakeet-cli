import { describe, test, expect, beforeEach, afterEach } from "bun:test";
import { join } from "path";
import { parseLangResult, getEngineBinPath, spawnStdioWithDebugFd } from "../../src/engine";

describe("engine", () => {
  test("getEngineBinPath returns path under .cache kesha", () => {
    const path = getEngineBinPath();
    expect(path).toMatch(/\.cache[/\\]kesha/);
    expect(path).toContain("kesha-engine");
  });

  test("getEngineBinPath follows KESHA_CACHE_DIR", () => {
    const savedCacheDir = process.env.KESHA_CACHE_DIR;
    const savedEngineBin = process.env.KESHA_ENGINE_BIN;
    try {
      delete process.env.KESHA_ENGINE_BIN;
      process.env.KESHA_CACHE_DIR = "/tmp/kesha-cache";
      expect(getEngineBinPath()).toBe(join("/tmp/kesha-cache", "engine", "bin", "kesha-engine"));
    } finally {
      if (savedCacheDir === undefined) delete process.env.KESHA_CACHE_DIR;
      else process.env.KESHA_CACHE_DIR = savedCacheDir;
      if (savedEngineBin === undefined) delete process.env.KESHA_ENGINE_BIN;
      else process.env.KESHA_ENGINE_BIN = savedEngineBin;
    }
  });

  test("parseLangResult parses valid JSON", () => {
    expect(parseLangResult('{"code":"ru","confidence":0.94}')).toEqual({ code: "ru", confidence: 0.94 });
  });

  test("parseLangResult returns null for invalid JSON", () => {
    expect(parseLangResult("not json")).toBeNull();
  });

  test("parseLangResult returns null for empty string", () => {
    expect(parseLangResult("")).toBeNull();
  });

  test("parseLangResult returns null for missing code field", () => {
    expect(parseLangResult('{"confidence":0.94}')).toBeNull();
  });
});

describe("spawnStdioWithDebugFd", () => {
  let savedFd: string | undefined;
  beforeEach(() => {
    savedFd = process.env.KESHA_DEBUG_FD;
    delete process.env.KESHA_DEBUG_FD;
  });
  afterEach(() => {
    if (savedFd === undefined) {
      delete process.env.KESHA_DEBUG_FD;
    } else {
      process.env.KESHA_DEBUG_FD = savedFd;
    }
  });

  test("returns base unchanged when KESHA_DEBUG_FD is unset", () => {
    expect(spawnStdioWithDebugFd(["ignore", "pipe", "pipe"])).toEqual(["ignore", "pipe", "pipe"]);
  });

  test("returns base unchanged on empty value (env exists but blank)", () => {
    process.env.KESHA_DEBUG_FD = "";
    expect(spawnStdioWithDebugFd(["ignore", "pipe", "pipe"])).toEqual(["ignore", "pipe", "pipe"]);
  });

  test("returns base unchanged on non-numeric value (garbage)", () => {
    process.env.KESHA_DEBUG_FD = "abc";
    expect(spawnStdioWithDebugFd(["ignore", "pipe", "pipe"])).toEqual(["ignore", "pipe", "pipe"]);
  });

  test("returns base unchanged for stdio range fd (0/1/2)", () => {
    for (const fd of ["0", "1", "2"]) {
      process.env.KESHA_DEBUG_FD = fd;
      expect(spawnStdioWithDebugFd(["ignore", "pipe", "pipe"])).toEqual(["ignore", "pipe", "pipe"]);
    }
  });

  test("forwards parent fd 3 as child fd 3 (no padding needed)", () => {
    process.env.KESHA_DEBUG_FD = "3";
    expect(spawnStdioWithDebugFd(["ignore", "pipe", "pipe"])).toEqual(["ignore", "pipe", "pipe", 3]);
  });

  test("pads with ignore up to the target fd and identity-maps it", () => {
    process.env.KESHA_DEBUG_FD = "5";
    // fd 5 needs ignore at slots 3, 4 then identity-map at 5.
    expect(spawnStdioWithDebugFd(["ignore", "pipe", "pipe"])).toEqual([
      "ignore",
      "pipe",
      "pipe",
      "ignore",
      "ignore",
      5,
    ]);
  });

  test("preserves alternative base stdin choices (e.g. say-side 'pipe')", () => {
    process.env.KESHA_DEBUG_FD = "3";
    // `kesha say` opens stdin to write the text payload through.
    expect(spawnStdioWithDebugFd(["pipe", "pipe", "pipe"])).toEqual(["pipe", "pipe", "pipe", 3]);
  });

  test("returns base unchanged on negative fd", () => {
    process.env.KESHA_DEBUG_FD = "-1";
    expect(spawnStdioWithDebugFd(["ignore", "pipe", "pipe"])).toEqual(["ignore", "pipe", "pipe"]);
  });

  test("returns base unchanged on decimal fd (Number.isInteger rejects)", () => {
    process.env.KESHA_DEBUG_FD = "3.5";
    expect(spawnStdioWithDebugFd(["ignore", "pipe", "pipe"])).toEqual(["ignore", "pipe", "pipe"]);
  });

  test("returns base unchanged on fd above MAX_FORWARDED_FD (#323 P2)", () => {
    // 1024 is the cap; anything higher would allocate a giant `ignore`
    // padding array. Legitimate users never have an fd this high.
    process.env.KESHA_DEBUG_FD = "1000000";
    expect(spawnStdioWithDebugFd(["ignore", "pipe", "pipe"])).toEqual(["ignore", "pipe", "pipe"]);
  });
});
