import { dirname, join } from "path";
import { existsSync, mkdirSync, chmodSync } from "fs";
import { getEngineBinPath, getEngineCapabilities } from "./engine";
import { log } from "./log";
import { streamResponseToFile } from "./progress";
import {
  readInstalledEngineVersion,
  writeInstalledEngineVersion,
} from "./engine-version-marker";

export {
  getVersionMarkerPath,
  readInstalledEngineVersion,
  writeInstalledEngineVersion,
} from "./engine-version-marker";

const GITHUB_REPO = "drakulavich/kesha-voice-kit";

function getEngineBinaryName(): string {
  const platform = process.platform;
  const arch = process.arch;

  if (platform === "darwin" && arch === "arm64") return "kesha-engine-darwin-arm64";
  if (platform === "linux" && arch === "x64") return "kesha-engine-linux-x64";
  if (platform === "win32" && arch === "x64") {
    throw new Error(
      "Windows x64 is temporarily unsupported in v1.5.0 — the Vosk-TTS engine has " +
        "native deps that trip MSVC at link time. Tracked at " +
        "https://github.com/drakulavich/kesha-voice-kit/issues/216. " +
        "Use v1.4.x as a workaround until the fix lands.",
    );
  }

  throw new Error(`Unsupported platform: ${platform} ${arch}`);
}

/**
 * One sidecar's identity. Each shipped Swift sidecar is described by an
 * entry in `SIDECARS`; the helper below loops over them. Centralising the
 * spec keeps the AVSpeech (#141) and diarize (#199) install paths in
 * lockstep — adding a third sidecar is one entry, not a new function.
 */
interface SidecarSpec {
  /** Filename written next to the engine binary. The Rust runtime probes
   * this exact name (sometimes a list — see diarize::sidecar_path). */
  fileBasename: string;
  /** Release asset name. Often equals fileBasename, but AVSpeech writes
   * `say-avspeech` while the asset is `say-avspeech-darwin-arm64`. */
  assetName: string;
  /** Human-readable name in log messages. */
  displayName: string;
  /** Trailing hint on success: "AVSpeech sidecar installed (<hint>)." */
  availableHint: string;
  /** Trailing hint on any failure path: "...; <hint>." */
  unavailableHint: string;
}

const SIDECARS: SidecarSpec[] = [
  {
    fileBasename: "say-avspeech",
    assetName: "say-avspeech-darwin-arm64",
    displayName: "AVSpeech sidecar",
    availableHint: "macOS voices available",
    unavailableHint: "macos-* voices unavailable",
  },
  {
    fileBasename: "kesha-diarize-darwin-arm64",
    assetName: "kesha-diarize-darwin-arm64",
    displayName: "Diarization sidecar",
    availableHint: "--speakers available",
    unavailableHint: "--speakers unavailable",
  },
];

/**
 * Fetch a single Swift sidecar and place it next to the engine binary on
 * darwin-arm64. Best-effort: 404s (older engine versions predate this
 * sidecar) and network errors log a warning and return — the corresponding
 * feature simply won't be available. The user keeps everything else.
 */
async function downloadSidecar(
  spec: SidecarSpec,
  binPath: string,
  engineVersion: string,
): Promise<void> {
  if (process.platform !== "darwin" || process.arch !== "arm64") return;

  const sidecarPath = join(dirname(binPath), spec.fileBasename);
  const url = `https://github.com/${GITHUB_REPO}/releases/download/v${engineVersion}/${spec.assetName}`;

  let res: Response;
  try {
    res = await fetch(url, { redirect: "follow" });
  } catch (e) {
    log.warn(
      `Could not fetch ${spec.displayName} (${e instanceof Error ? e.message : e}); ${spec.unavailableHint}.`,
    );
    return;
  }

  if (!res.ok) {
    log.warn(
      `${spec.displayName} not in release v${engineVersion} (HTTP ${res.status}); ${spec.unavailableHint}.`,
    );
    return;
  }

  // Keep the best-effort contract: streamResponseToFile throws on an empty
  // body and can fail mid-stream, and chmodSync can throw EPERM. Without
  // this catch a stream/chmod failure would propagate through the tail
  // `await Promise.all(sidecarPromises)` in downloadEngine — converting a
  // successful engine install into a thrown exception after log.success
  // already announced it, which is exactly the regression the fetch/404
  // branches above protect against.
  try {
    await streamResponseToFile(res, sidecarPath, spec.displayName);
    chmodSync(sidecarPath, 0o755);
    log.success(`${spec.displayName} installed (${spec.availableHint}).`);
  } catch (e) {
    log.warn(
      `${spec.displayName} install failed (${e instanceof Error ? e.message : e}); ${spec.unavailableHint}.`,
    );
  }
}

export interface InstallOptions {
  /** Also install Kokoro + Vosk-TTS models. */
  tts?: boolean;
  /** Also install Silero VAD model for long-audio preprocessing. */
  vad?: boolean;
  /** Also install the Sortformer streaming-diarization model (~245MB,
   * darwin-arm64 only — see #199). */
  diarize?: boolean;
}

export async function downloadEngine(
  noCache = false,
  backend?: string,
  options: InstallOptions = {},
): Promise<string> {
  const binPath = getEngineBinPath();
  const pkg = await Bun.file(new URL("../package.json", import.meta.url)).json();
  // The engine version is tracked separately from the CLI version so
  // CLI-only patch releases don't require cutting a new GitHub release
  // + Rust rebuild. Fall back to the CLI version for backwards compat.
  const engineVersion =
    typeof pkg.keshaEngine?.version === "string"
      ? pkg.keshaEngine.version
      : typeof pkg.version === "string"
        ? pkg.version
        : "unknown";

  const installedVersion = readInstalledEngineVersion(binPath);
  const cacheValid =
    !noCache && existsSync(binPath) && installedVersion === engineVersion;

  if (cacheValid) {
    log.success(`Engine binary already installed (v${engineVersion}).`);
    // Top up any sidecars missing from this cached install. Pre-#141 / pre-#199
    // engines never shipped them, so a cache-valid binary may still need
    // fetching. Run independent fetches concurrently — same shape as the
    // cold path below.
    const missing = SIDECARS.filter(
      (s) => !existsSync(join(dirname(binPath), s.fileBasename)),
    );
    await Promise.all(missing.map((s) => downloadSidecar(s, binPath, engineVersion)));
  } else {
    // Log why we're downloading — helps diagnose surprising re-downloads.
    if (existsSync(binPath) && installedVersion && installedVersion !== engineVersion) {
      log.progress(
        `Upgrading engine v${installedVersion} → v${engineVersion}...`,
      );
    }
    const binaryName = getEngineBinaryName();
    const url = `https://github.com/${GITHUB_REPO}/releases/download/v${engineVersion}/${binaryName}`;

    mkdirSync(dirname(binPath), { recursive: true });

    // Kick off all sidecar fetches concurrently with the engine fetch. They
    // target independent github.com release assets, so overlapping the HTTP
    // round-trips saves ~15-30s on a cold install. Each sidecar is
    // best-effort (404 on older engines, warn + continue) so a failure
    // doesn't cascade into the engine path.
    const sidecarPromises = SIDECARS.map((s) =>
      downloadSidecar(s, binPath, engineVersion),
    );
    // Defense-in-depth: if the engine fetch throws below, attach no-op
    // rejection handlers so we don't surface unhandledRejection errors
    // from sidecar paths whose internal try/catch ever drifts. Logs from
    // sidecars whose own work is still in flight will print when they
    // complete; the engine error is what the user needs to see now.
    const muteSidecarRejections = () =>
      sidecarPromises.forEach((p) => p.catch(() => {}));

    let res: Response;
    try {
      res = await fetch(url, { redirect: "follow" });
    } catch (e) {
      muteSidecarRejections();
      throw new Error(
        `Failed to fetch engine binary: ${e instanceof Error ? e.message : e}\n  Fix: Check your network connection and try again`,
      );
    }

    if (!res.ok) {
      muteSidecarRejections();
      throw new Error(
        `Failed to download engine binary (HTTP ${res.status})\n  Fix: Check https://github.com/${GITHUB_REPO}/releases for available versions`,
      );
    }

    await streamResponseToFile(res, binPath, "kesha-engine binary");
    chmodSync(binPath, 0o755);
    writeInstalledEngineVersion(binPath, engineVersion);
    log.success(`Engine binary downloaded (v${engineVersion}).`);
    await Promise.all(sidecarPromises);
  }

  if (backend) {
    const caps = await getEngineCapabilities();
    if (caps && caps.backend !== backend) {
      throw new Error(
        `Requested backend "${backend}" is not available: the installed engine for this platform uses "${caps.backend}".\n  Fix: omit --${backend} to use the auto-detected backend, or run on a platform that ships the "${backend}" build.`,
      );
    }
  }

  log.progress("Installing models...");
  const installArgs = [
    "install",
    ...(noCache ? ["--no-cache"] : []),
    ...(options.tts ? ["--tts"] : []),
    ...(options.vad ? ["--vad"] : []),
    ...(options.diarize ? ["--diarize"] : []),
  ];
  const proc = Bun.spawnSync([binPath, ...installArgs], {
    stdout: "pipe",
    stderr: "pipe",
  });

  const stderr = proc.stderr.toString();
  if (stderr) {
    process.stderr.write(stderr);
  }

  if (proc.exitCode !== 0) {
    const detail = stderr.trim();
    throw new Error(detail ? `Failed to install models: ${detail}` : "Failed to install models");
  }

  log.success("Backend installed successfully.");
  return binPath;
}
