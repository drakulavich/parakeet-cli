import { describe, expect, it } from "bun:test";
import { chmodSync, mkdtempSync, writeFileSync } from "fs";
import { tmpdir } from "os";
import { join } from "path";
import { transcribe } from "../../src/lib";
import {
  preflightTranscribeWithSegments,
  transcribe as transcribeWrapper,
  transcribeWithSegments,
} from "../../src/transcribe";

function fakeEngine(features: string[]): string {
  const dir = mkdtempSync(join(tmpdir(), "kesha-transcribe-test-"));
  const path = join(dir, "kesha-engine");
  writeFileSync(
    path,
    `#!/bin/sh
if [ "$1" = "--capabilities-json" ]; then
  printf '%s\\n' '${JSON.stringify({ protocolVersion: 2, backend: "fake", features })}'
  exit 0
fi
if [ "$1" = "transcribe" ]; then
  if [ "$3" = "--json" ] || [ "$2" = "--json" ]; then
    printf '%s\\n' '{"text":"ok","segments":[{"start":0,"end":1,"text":"ok"}]}'
  else
    printf '%s\\n' 'ok'
  fi
  exit 0
fi
exit 2
`,
  );
  chmodSync(path, 0o755);
  return path;
}

async function withEngine<T>(enginePath: string, fn: () => T | Promise<T>): Promise<T> {
  const saved = process.env.KESHA_ENGINE_BIN;
  try {
    process.env.KESHA_ENGINE_BIN = enginePath;
    return await fn();
  } finally {
    if (saved === undefined) delete process.env.KESHA_ENGINE_BIN;
    else process.env.KESHA_ENGINE_BIN = saved;
  }
}

describe("lib API", () => {
  it("rejects missing file", async () => {
    await expect(transcribe("/nonexistent/audio.wav")).rejects.toThrow("File not found");
  });

  it("exports say()", async () => {
    const { say } = await import("../../src/lib");
    expect(typeof say).toBe("function");
  });

  it("exports downloadTts()", async () => {
    const { downloadTts } = await import("../../src/lib");
    expect(typeof downloadTts).toBe("function");
  });

  it("keeps transcribeWithSegments as a compatibility alias", async () => {
    const { transcribeWithSegments, transcribeWithTimestamps } = await import("../../src/lib");
    expect(transcribeWithSegments).toBe(transcribeWithTimestamps);
  });

  it("exports SayError class with code + stderr fields", async () => {
    const { SayError } = await import("../../src/lib");
    const e = new SayError("msg", 1, "stderr");
    expect(e.exitCode).toBe(1);
    expect(e.stderr).toBe("stderr");
  });

  it("uses canonical Bun install commands when transcription backend is missing", async () => {
    const saved = process.env.KESHA_ENGINE_BIN;
    process.env.KESHA_ENGINE_BIN = `/tmp/kesha-missing-engine-${Date.now()}`;
    try {
      let message = "";
      try {
        await transcribeWrapper("audio.wav");
      } catch (err) {
        message = err instanceof Error ? err.message : String(err);
      }
      expect(message).toContain("bun add -g @drakulavich/kesha-voice-kit");
      expect(message).toContain("kesha install");
      expect(message).not.toContain("bunx");
    } finally {
      if (saved === undefined) delete process.env.KESHA_ENGINE_BIN;
      else process.env.KESHA_ENGINE_BIN = saved;
    }
  });

  it("preflights timestamp support before segment transcription", async () => {
    await withEngine(fakeEngine([]), async () => {
      await expect(preflightTranscribeWithSegments({ timestamps: true })).rejects.toThrow(
        "Timestamped segments require",
      );
    });
  });

  it("routes timestamp requests through the JSON segment path", async () => {
    await withEngine(fakeEngine(["transcribe.segments"]), async () => {
      await expect(transcribeWithSegments("audio.wav", { timestamps: true })).resolves.toEqual({
        text: "ok",
        segments: [{ start: 0, end: 1, text: "ok" }],
      });
    });
  });

  it("plain transcription still returns an empty segment list", async () => {
    await withEngine(fakeEngine(["transcribe.segments"]), async () => {
      await expect(transcribeWithSegments("audio.wav")).resolves.toEqual({
        text: "ok",
        segments: [],
      });
    });
  });
});
