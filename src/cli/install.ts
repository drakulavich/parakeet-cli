import { defineCommand } from "citty";
import { downloadEngine } from "../engine-install";
import { getEngineBinPath } from "../engine";
import { readStarSeen, shouldShowStarPrompt, writeStarSeen } from "../star";
import { log } from "../log";

interface InstallCommandArgs {
  coreml: boolean;
  onnx: boolean;
  "no-cache": boolean;
  tts: boolean;
  vad: boolean;
  diarize: boolean;
}

const pkg = await Bun.file(new URL("../../package.json", import.meta.url)).json();

function resolveBackendFlag(coreml: boolean, onnx: boolean): string | undefined {
  if (coreml && onnx) {
    log.error('Choose only one backend: "--coreml" or "--onnx".');
    process.exit(1);
  }
  if (coreml) return "coreml";
  if (onnx) return "onnx";
  return undefined;
}

async function askForStar() {
  // Gate on major-or-minor bump only — patch releases and re-installs of the
  // same version shouldn't nag. First-ever install (no marker) still prompts.
  const currentVersion = typeof pkg.version === "string" ? pkg.version : null;
  if (!currentVersion) return;
  const binPath = getEngineBinPath();
  const seen = readStarSeen(binPath);
  if (!shouldShowStarPrompt(currentVersion, seen)) {
    return;
  }
  // Record the version up front so a single run never prompts twice, even
  // if the gh subprocess below throws.
  try {
    writeStarSeen(binPath, currentVersion);
  } catch {
    // Non-fatal — falling through to the prompt is still OK, just means we
    // may nag again on the next install if the write failed for IO reasons.
  }

  const gh = Bun.which("gh");
  if (!gh) {
    log.info("\nIf you enjoy Kesha Voice Kit, consider starring the repo:");
    log.info("  https://github.com/drakulavich/kesha-voice-kit");
    return;
  }
  const authCheck = Bun.spawnSync([gh, "auth", "status"], { stdout: "ignore", stderr: "ignore" });
  if (authCheck.exitCode !== 0) return;
  const starred = Bun.spawnSync([gh, "api", "user/starred/drakulavich/kesha-voice-kit"], { stdout: "ignore", stderr: "ignore" });
  if (starred.exitCode === 0) return; // already starred
  log.info("\n⭐ If you enjoy Kesha Voice Kit, star it on GitHub:");
  log.info("  https://github.com/drakulavich/kesha-voice-kit");
  log.info('  Or run: gh api -X PUT /user/starred/drakulavich/kesha-voice-kit');
}

async function performInstall(
  noCache: boolean,
  backend?: string,
  tts = false,
  vad = false,
  diarize = false,
) {
  if (diarize && !(process.platform === "darwin" && process.arch === "arm64")) {
    log.error(
      "--diarize is currently darwin-arm64 only " +
        "(see https://github.com/drakulavich/kesha-voice-kit/issues/199).",
    );
    process.exit(1);
  }
  try {
    await downloadEngine(noCache, backend, { tts, vad, diarize });
    await askForStar();
  } catch (err: unknown) {
    const message = err instanceof Error ? err.message : String(err);
    log.error(message);
    process.exit(1);
  }
}

export const installCommand = defineCommand({
  meta: {
    name: "install",
    description: "Download inference engine and models",
  },
  args: {
    coreml: {
      type: "boolean",
      description: "Force CoreML backend (macOS arm64)",
      default: false,
    },
    onnx: {
      type: "boolean",
      description: "Force ONNX backend",
      default: false,
    },
    "no-cache": {
      type: "boolean",
      description: "Re-download even if cached",
      default: false,
    },
    tts: {
      type: "boolean",
      description: "Also install TTS models (Kokoro EN + Vosk-TTS RU, ~990MB)",
      default: false,
    },
    vad: {
      type: "boolean",
      description: "Also install Silero VAD (~2.3MB) for long-audio preprocessing",
      default: false,
    },
    diarize: {
      type: "boolean",
      description: "Also install the Sortformer streaming-diarization model (~245MB, darwin-arm64 only — #199)",
      default: false,
    },
  },
  async run({ args }: { args: InstallCommandArgs }) {
    const backend = resolveBackendFlag(args.coreml, args.onnx);
    await performInstall(args["no-cache"], backend, args.tts, args.vad, args.diarize);
  },
});
