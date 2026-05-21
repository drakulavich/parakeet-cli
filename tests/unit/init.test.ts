import { describe, expect, test } from "bun:test";
import {
  canInstallDiarizeOnPlatform,
  initInstallArgs,
  renderInitOverview,
  resolveInitSelection,
  type InitCommandArgs,
} from "../../src/cli";

function initArgs(overrides: Partial<InitCommandArgs> = {}): InitCommandArgs {
  return {
    coreml: false,
    onnx: false,
    "no-cache": false,
    tts: false,
    vad: false,
    diarize: false,
    plan: false,
    yes: false,
    ...overrides,
  };
}

describe("init onboarding", () => {
  test("defaults to base install only", () => {
    const selection = resolveInitSelection(initArgs(), undefined);
    expect(selection).toEqual({
      noCache: false,
      backend: undefined,
      tts: false,
      vad: false,
      diarize: false,
    });
    expect(initInstallArgs(selection)).toEqual(["kesha", "install"]);
  });

  test("preselected feature flags map to install flags", () => {
    const selection = resolveInitSelection(
      initArgs({ "no-cache": true, tts: true, vad: true, diarize: true }),
      "coreml",
    );
    expect(initInstallArgs(selection)).toEqual([
      "kesha",
      "install",
      "--no-cache",
      "--coreml",
      "--tts",
      "--vad",
      "--diarize",
    ]);
  });

  test("diarization availability is darwin-arm64 only", () => {
    expect(canInstallDiarizeOnPlatform("darwin", "arm64")).toBe(true);
    expect(canInstallDiarizeOnPlatform("darwin", "x64")).toBe(false);
    expect(canInstallDiarizeOnPlatform("linux", "x64")).toBe(false);
  });

  test("overview explains base install and optional features", () => {
    const overview = renderInitOverview(false);
    expect(overview).toContain("The base install downloads the engine");
    expect(overview).toContain("Text-to-speech");
    expect(overview).toContain("VAD");
    expect(overview).toContain("darwin-arm64 only");
    expect(overview).toContain("Nothing downloads until you confirm");
  });
});
