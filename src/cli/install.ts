import { defineCommand } from "citty";
import { downloadEngine } from "../engine-install";
import { getEngineBinPath } from "../engine";
import { renderInstallPlan } from "../install-plan";
import { maybeAskForStar } from "../star";
import { log } from "../log";
import { packageVersion } from "../package-info";

export interface InstallCommandArgs {
  coreml: boolean;
  onnx: boolean;
  "no-cache": boolean;
  noCache?: boolean;
  no_cache?: boolean;
  tts: boolean;
  vad: boolean;
  diarize: boolean;
  plan: boolean;
}

export function resolveNoCacheFlag(
  args: Pick<InstallCommandArgs, "no-cache" | "noCache" | "no_cache">,
  rawArgs: string[] = [],
): boolean {
  return (
    rawArgs.includes("--no-cache") ||
    args["no-cache"] === true ||
    args.noCache === true ||
    args.no_cache === true
  );
}

export function resolveBackendFlag(coreml: boolean, onnx: boolean): string | undefined {
  if (coreml && onnx) {
    log.error('Choose only one backend: "--coreml" or "--onnx".');
    process.exit(1);
  }
  if (coreml) return "coreml";
  if (onnx) return "onnx";
  return undefined;
}

function defaultBackendForPlatform(): string | undefined {
  if (process.platform === "darwin" && process.arch === "arm64") return "coreml";
  if (process.platform === "linux" && process.arch === "x64") return "onnx";
  return undefined;
}

export async function performInstall(
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
  const platformBackend = defaultBackendForPlatform();
  if (backend && !process.env.KESHA_ENGINE_BIN && platformBackend && backend !== platformBackend) {
    log.error(
      `Requested backend "${backend}" is not available on this platform; ` +
        `the release engine uses "${platformBackend}".`,
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
  async run({ args, rawArgs }: { args: InstallCommandArgs; rawArgs: string[] }) {
    const backend = resolveBackendFlag(args.coreml, args.onnx);
    await performInstall(resolveNoCacheFlag(args, rawArgs), backend, args.tts, args.vad, args.diarize, args.plan);
  },
});
