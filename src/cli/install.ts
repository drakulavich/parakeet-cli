import { defineCommand } from "citty";
import { downloadEngine } from "../engine-install";
import { getEngineBinPath } from "../engine";
import { renderInstallPlan } from "../install-plan";
import { maybeAskForStar } from "../star";
import { log } from "../log";
import { packageVersion } from "../package-info";

interface InstallCommandArgs {
  coreml: boolean;
  onnx: boolean;
  "no-cache": boolean;
  tts: boolean;
  vad: boolean;
  diarize: boolean;
  plan: boolean;
}

function resolveBackendFlag(coreml: boolean, onnx: boolean): string | undefined {
  if (coreml && onnx) {
    log.error('Choose only one backend: "--coreml" or "--onnx".');
    process.exit(1);
  }
  if (coreml) return "coreml";
  if (onnx) return "onnx";
  return undefined;
}

async function performInstall(
  noCache: boolean,
  backend?: string,
  tts = false,
  vad = false,
  diarize = false,
  plan = false,
) {
  if (plan) {
    log.info(await renderInstallPlan({ noCache, backend, tts, vad, diarize }));
    return;
  }
  if (diarize && !(process.platform === "darwin" && process.arch === "arm64")) {
    log.error(
      "--diarize is currently darwin-arm64 only " +
        "(see https://github.com/drakulavich/kesha-voice-kit/issues/199).",
    );
    process.exit(1);
  }
  try {
    await downloadEngine(noCache, backend, { tts, vad, diarize });
    await maybeAskForStar(getEngineBinPath(), packageVersion, log);
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
    plan: {
      type: "boolean",
      description: "Show download, disk, and warm-up plan without changing local state",
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
    await performInstall(args["no-cache"], backend, args.tts, args.vad, args.diarize, args.plan);
  },
});
