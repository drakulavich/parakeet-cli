import { describe, expect, it } from "bun:test";
import { transcribe } from "../../src/lib";
import { transcribe as transcribeWrapper } from "../../src/transcribe";

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
});
