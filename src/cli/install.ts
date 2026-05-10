import { defineCommand } from "citty";
import { downloadEngine } from "../engine-install";
import { getEngineBinPath } from "../engine";
import { maybeAskForStar } from "../star";
import { log } from "../log";

interface InstallCommandArgs {
  coreml: boolean;
  onnx: boolean;
  "no-cache": boolean;
  tts: boolean;
  vad: boolean;
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

async function performInstall(noCache: boolean, backend?: string, tts = false, vad = false) {
  try {
    await downloadEngine(noCache, backend, { tts, vad });
    const currentVersion = typeof pkg.version === "string" ? pkg.version : null;
    await maybeAskForStar(getEngineBinPath(), currentVersion, log);
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
  },
  async run({ args }: { args: InstallCommandArgs }) {
    const backend = resolveBackendFlag(args.coreml, args.onnx);
    await performInstall(args["no-cache"], backend, args.tts, args.vad);
  },
});
