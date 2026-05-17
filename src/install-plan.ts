import { existsSync, statSync } from "fs";
import { dirname, join } from "path";
import { getEngineBinPath } from "./engine";
import { readInstalledEngineVersion } from "./engine-version-marker";
import { keshaCacheDir } from "./paths";
import { engineVersion, packageVersion } from "./package-info";

export interface InstallPlanOptions {
  noCache?: boolean;
  backend?: string;
  tts?: boolean;
  vad?: boolean;
  diarize?: boolean;
}

interface PlanFile {
  relPath: string;
  sizeBytes: number;
}

interface PlanComponent {
  name: string;
  source: string;
  sizeBytes: number;
  cached: boolean;
  refresh: boolean;
  note?: string;
}

interface ReleaseAssetSpec {
  assetName: string;
  sizeBytes: number;
}

// Mirrors the pinned runtime manifests so `kesha install --plan` works before
// the engine exists. Keep in sync with rust/src/models.rs and release assets.
const ASR_FILES: PlanFile[] = [
  { relPath: "models/parakeet-tdt-v3/encoder-model.onnx", sizeBytes: 41_770_866 },
  { relPath: "models/parakeet-tdt-v3/encoder-model.onnx.data", sizeBytes: 2_435_420_160 },
  { relPath: "models/parakeet-tdt-v3/decoder_joint-model.onnx", sizeBytes: 72_520_893 },
  { relPath: "models/parakeet-tdt-v3/nemo128.onnx", sizeBytes: 139_764 },
  { relPath: "models/parakeet-tdt-v3/vocab.txt", sizeBytes: 93_939 },
];

const LANG_ID_FILES: PlanFile[] = [
  { relPath: "models/lang-id-ecapa/lang-id-ecapa.onnx", sizeBytes: 759_814 },
  { relPath: "models/lang-id-ecapa/lang-id-ecapa.onnx.data", sizeBytes: 85_327_872 },
  { relPath: "models/lang-id-ecapa/labels.json", sizeBytes: 646 },
];

const VAD_FILES: PlanFile[] = [
  { relPath: "models/silero-vad/silero_vad.onnx", sizeBytes: 2_327_524 },
];

const KOKORO_FILES: PlanFile[] = [
  { relPath: "models/kokoro-82m/model.onnx", sizeBytes: 325_532_387 },
  { relPath: "models/kokoro-82m/voices/am_michael.bin", sizeBytes: 522_240 },
];

const VOSK_RU_FILES: PlanFile[] = [
  { relPath: "models/vosk-ru/model.onnx", sizeBytes: 179_314_533 },
  { relPath: "models/vosk-ru/dictionary", sizeBytes: 101_431_118 },
  { relPath: "models/vosk-ru/config.json", sizeBytes: 1_518 },
  { relPath: "models/vosk-ru/bert/model.onnx", sizeBytes: 654_361_598 },
  { relPath: "models/vosk-ru/bert/vocab.txt", sizeBytes: 1_780_720 },
];

const DIARIZE_FILES: PlanFile[] = [
  { relPath: "models/diarize/SortformerNvidiaLow_v2.mlpackage/Manifest.json", sizeBytes: 617 },
  {
    relPath: "models/diarize/SortformerNvidiaLow_v2.mlpackage/Data/com.apple.CoreML/model.mlmodel",
    sizeBytes: 7_080_357,
  },
  {
    relPath: "models/diarize/SortformerNvidiaLow_v2.mlpackage/Data/com.apple.CoreML/weights/0-weight.bin",
    sizeBytes: 5_930_400,
  },
  {
    relPath: "models/diarize/SortformerNvidiaLow_v2.mlpackage/Data/com.apple.CoreML/weights/1-weight.bin",
    sizeBytes: 232_161_600,
  },
];

const DARWIN_SIDECARS: ReleaseAssetSpec[] = [
  { assetName: "say-avspeech-darwin-arm64", sizeBytes: 63_056 },
  { assetName: "kesha-diarize-darwin-arm64", sizeBytes: 7_228_000 },
  { assetName: "kesha-kokoro-darwin-arm64", sizeBytes: 7_189_648 },
  { assetName: "kesha-textlang-darwin-arm64", sizeBytes: 57_648 },
];

function isDarwinArm64(): boolean {
  return process.platform === "darwin" && process.arch === "arm64";
}

function sumFiles(files: PlanFile[]): number {
  return files.reduce((sum, file) => sum + file.sizeBytes, 0);
}

function humanBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let n = bytes / 1024;
  let i = 0;
  while (n >= 1024 && i < units.length - 1) {
    n /= 1024;
    i++;
  }
  return `${n.toFixed(n >= 100 ? 0 : 1)} ${units[i]}`;
}

function engineAssetForPlatform(): ReleaseAssetSpec | null {
  if (process.platform === "darwin" && process.arch === "arm64") {
    return { assetName: "kesha-engine-darwin-arm64", sizeBytes: 59_621_264 };
  }
  if (process.platform === "linux" && process.arch === "x64") {
    return { assetName: "kesha-engine-linux-x64", sizeBytes: 62_897_808 };
  }
  if (process.platform === "win32" && process.arch === "x64") {
    return { assetName: "kesha-engine-windows-x64.exe", sizeBytes: 63_126_528 };
  }
  return null;
}

function sidecarFilename(assetName: string): string {
  if (assetName === "say-avspeech-darwin-arm64") return "say-avspeech";
  if (assetName === "kesha-diarize-darwin-arm64") return "kesha-diarize-darwin-arm64";
  if (assetName === "kesha-kokoro-darwin-arm64") return "kesha-kokoro";
  if (assetName === "kesha-textlang-darwin-arm64") return "kesha-textlang";
  return assetName;
}

function filesCached(cacheRoot: string, files: PlanFile[]): boolean {
  return files.every((file) => {
    const path = join(cacheRoot, file.relPath);
    return existsSync(path) && statSync(path).size > 0;
  });
}

function bundleComponent(
  cacheRoot: string,
  name: string,
  source: string,
  files: PlanFile[],
  refresh: boolean,
  note?: string,
): PlanComponent {
  return {
    name,
    source,
    sizeBytes: sumFiles(files),
    cached: filesCached(cacheRoot, files),
    refresh,
    note,
  };
}

export async function renderInstallPlan(options: InstallPlanOptions = {}): Promise<string> {
  const cacheRoot = keshaCacheDir();
  const binPath = getEngineBinPath();
  const engineDir = dirname(binPath);
  const noCache = options.noCache === true;
  const components: PlanComponent[] = [];

  const engineAsset = engineAssetForPlatform();
  if (engineAsset) {
    const engineCached =
      existsSync(binPath) && readInstalledEngineVersion(binPath) === engineVersion;
    components.push({
      name: `Engine ${engineAsset.assetName}`,
      source: `GitHub release v${engineVersion}`,
      sizeBytes: engineAsset.sizeBytes,
      cached: engineCached,
      refresh: noCache,
      note:
        process.platform === "win32"
          ? "Windows x64 is currently blocked by the install path; see issue #216."
          : undefined,
    });
  } else {
    components.push({
      name: `Engine for ${process.platform} ${process.arch}`,
      source: "GitHub release",
      sizeBytes: 0,
      cached: false,
      refresh: false,
      note: "unsupported platform",
    });
  }

  if (isDarwinArm64()) {
    for (const sidecar of DARWIN_SIDECARS) {
      components.push({
        name: `Sidecar ${sidecar.assetName}`,
        source: `GitHub release v${engineVersion}`,
        sizeBytes: sidecar.sizeBytes,
        cached: existsSync(join(engineDir, sidecarFilename(sidecar.assetName))),
        refresh: noCache,
      });
    }
  }

  components.push(
    bundleComponent(
      cacheRoot,
      "ASR Parakeet TDT v3",
      "model cache",
      ASR_FILES,
      noCache,
      "required for speech-to-text",
    ),
  );
  components.push(
    bundleComponent(
      cacheRoot,
      "Audio language ID ECAPA",
      "model cache",
      LANG_ID_FILES,
      noCache,
      "required for --json, --toon, --format transcript, --lang, and --verbose language metadata",
    ),
  );

  if (options.tts) {
    if (isDarwinArm64()) {
      components.push(
        bundleComponent(
          cacheRoot,
          "TTS Vosk RU",
          "model cache",
          VOSK_RU_FILES,
          noCache,
          "Russian ru-vosk-* voices",
        ),
      );
      components.push({
        name: "TTS Kokoro EN",
        source: "FluidAudio CoreML sidecar",
        sizeBytes: 0,
        cached: existsSync(join(engineDir, "kesha-kokoro")),
        refresh: false,
        note: "no Kokoro ONNX model is downloaded by Kesha on darwin-arm64; warm-up may compile/update FluidAudio's CoreML cache",
      });
    } else {
      components.push(
        bundleComponent(
          cacheRoot,
          "TTS Kokoro EN",
          "model cache",
          KOKORO_FILES,
          noCache,
          "English en-* voices",
        ),
      );
      components.push(
        bundleComponent(
          cacheRoot,
          "TTS Vosk RU",
          "model cache",
          VOSK_RU_FILES,
          noCache,
          "Russian ru-vosk-* voices",
        ),
      );
    }
  }

  if (options.vad) {
    components.push(
      bundleComponent(
        cacheRoot,
        "VAD Silero v5",
        "model cache",
        VAD_FILES,
        noCache,
        "long-audio preprocessing",
      ),
    );
  }

  if (options.diarize) {
    components.push(
      bundleComponent(
        cacheRoot,
        "Diarization Sortformer",
        "model cache",
        DIARIZE_FILES,
        noCache,
        isDarwinArm64()
          ? "speaker labels for --speakers"
          : "darwin-arm64 only; install will reject this flag on the current platform",
      ),
    );
  }

  const coldBytes = components.reduce((sum, component) => sum + component.sizeBytes, 0);
  const expectedNetworkBytes = components.reduce((sum, component) => {
    if (component.cached && !component.refresh) return sum;
    return sum + component.sizeBytes;
  }, 0);
  const status = (component: PlanComponent) => {
    if (component.refresh) return "refresh";
    return component.cached ? "cached" : "needed";
  };

  const lines = [
    "Kesha install plan",
    "",
    `Package: @drakulavich/kesha-voice-kit ${packageVersion}`,
    `Engine release: v${engineVersion}`,
    `Platform: ${process.platform} ${process.arch}`,
    `Cache: ${cacheRoot}`,
    `Engine binary: ${binPath}`,
    options.backend ? `Requested backend: ${options.backend}` : "Requested backend: auto",
    "",
    "Components:",
  ];

  for (const component of components) {
    lines.push(
      `  - ${component.name}: ${humanBytes(component.sizeBytes)} (${component.sizeBytes} bytes, ${status(component)}, ${component.source})`,
    );
    if (component.note) lines.push(`    ${component.note}`);
  }

  lines.push(
    "",
    "Totals:",
    `  Cold-cache download: ${humanBytes(coldBytes)}`,
    `  Expected network for this run: ${humanBytes(expectedNetworkBytes)}`,
    "",
    "Install behavior:",
    "  - No files are downloaded or changed by --plan.",
    "  - install verifies model SHA-256 hashes and reuses matching cached files unless --no-cache is set.",
    isDarwinArm64()
      ? "  - macOS install signs/unquarantines downloaded binaries for Gatekeeper."
      : "  - No macOS Gatekeeper signing step on this platform.",
    "  - install warms the ASR backend after downloads; CoreML warm-up is typically 20-30 s, ONNX warm-up is about 500 ms.",
  );

  if (options.tts && isDarwinArm64()) {
    lines.push(
      "  - --tts also warms FluidAudio Kokoro CoreML; first en-* synthesis may compile/update a system CoreML cache.",
    );
  }

  const command = [
    "kesha",
    "install",
    options.noCache ? "--no-cache" : "",
    options.backend === "coreml" ? "--coreml" : "",
    options.backend === "onnx" ? "--onnx" : "",
    options.tts ? "--tts" : "",
    options.vad ? "--vad" : "",
    options.diarize ? "--diarize" : "",
  ].filter(Boolean);
  lines.push("", `Run: ${command.join(" ")}`, "");

  return `${lines.join("\n")}\n`;
}
