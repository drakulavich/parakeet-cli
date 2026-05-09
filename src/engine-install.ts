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
 * Fetch the AVSpeechSynthesizer sidecar (#141) and place it next to the
 * engine binary on darwin-arm64. The Rust side (`avspeech::helper_path`)
 * looks for a `say-avspeech` file adjacent to the running executable, so
 * the filename on disk is always `say-avspeech` regardless of the release
 * asset name.
 *
 * Best-effort: 404s (older engine versions predate the sidecar) and
 * network errors log a warning and return — macos-* voices simply won't
 * be available, which is a graceful degradation. The user keeps Kokoro +
 * Vosk-TTS.
 */
async function downloadAVSpeechSidecar(binPath: string, engineVersion: string): Promise<void> {
  if (process.platform !== "darwin" || process.arch !== "arm64") return;

  const sidecarPath = join(dirname(binPath), "say-avspeech");
  const url = `https://github.com/${GITHUB_REPO}/releases/download/v${engineVersion}/say-avspeech-darwin-arm64`;

  let res: Response;
  try {
    res = await fetch(url, { redirect: "follow" });
  } catch (e) {
    log.warn(
      `Could not fetch AVSpeech sidecar (${e instanceof Error ? e.message : e}); macos-* voices unavailable.`,
    );
    return;
  }

  if (!res.ok) {
    log.warn(
      `AVSpeech sidecar not in release v${engineVersion} (HTTP ${res.status}); macos-* voices unavailable.`,
    );
    return;
  }

  // Keep the best-effort contract: streamResponseToFile throws on an empty
  // body and can fail mid-stream, and chmodSync can throw EPERM. Without
  // this catch a stream/chmod failure would propagate through the tail
  // `await sidecarPromise` in downloadEngine — converting a successful
  // engine install into a thrown exception after log.success already
  // announced it, which is exactly the regression the fetch/404 branches
  // above protect against.
  try {
    await streamResponseToFile(res, sidecarPath, "say-avspeech sidecar");
    chmodSync(sidecarPath, 0o755);
    log.success("AVSpeech sidecar installed (macOS voices available).");
  } catch (e) {
    log.warn(
      `AVSpeech sidecar install failed (${e instanceof Error ? e.message : e}); macos-* voices unavailable.`,
    );
  }
}

/**
 * Fetch the kesha-diarize Swift sidecar (#199) and place it next to the
 * engine binary on darwin-arm64. The Rust side
 * (`transcribe::diarize::sidecar_path`) probes for `kesha-diarize-darwin-arm64`
 * (and `kesha-diarize`) adjacent to the running executable.
 *
 * Best-effort, mirroring AVSpeech: 404 (older release predates the sidecar)
 * and network errors warn and return — `--speakers` simply won't be available,
 * which the TS-side capability gate surfaces with a #199 link.
 */
async function downloadDiarizeSidecar(binPath: string, engineVersion: string): Promise<void> {
  if (process.platform !== "darwin" || process.arch !== "arm64") return;

  const sidecarPath = join(dirname(binPath), "kesha-diarize-darwin-arm64");
  const url = `https://github.com/${GITHUB_REPO}/releases/download/v${engineVersion}/kesha-diarize-darwin-arm64`;

  let res: Response;
  try {
    res = await fetch(url, { redirect: "follow" });
  } catch (e) {
    log.warn(
      `Could not fetch diarization sidecar (${e instanceof Error ? e.message : e}); --speakers unavailable.`,
    );
    return;
  }

  if (!res.ok) {
    log.warn(
      `Diarization sidecar not in release v${engineVersion} (HTTP ${res.status}); --speakers unavailable.`,
    );
    return;
  }

  try {
    await streamResponseToFile(res, sidecarPath, "kesha-diarize sidecar");
    chmodSync(sidecarPath, 0o755);
    log.success("Diarization sidecar installed (--speakers available).");
  } catch (e) {
    log.warn(
      `Diarization sidecar install failed (${e instanceof Error ? e.message : e}); --speakers unavailable.`,
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
    // Cover the upgrade path from pre-#141 engines that never had a
    // sidecar: if the cached engine is current but the sibling sidecar
    // is missing, fetch it now so macos-* voices start working.
    const sidecarPath = join(dirname(binPath), "say-avspeech");
    if (!existsSync(sidecarPath)) {
      await downloadAVSpeechSidecar(binPath, engineVersion);
    }
    // Same upgrade-from-older-engine path for the #199 diarization sidecar:
    // engines prior to v1.12.0 didn't ship it, so a cached but otherwise
    // current install can still be missing it.
    const diarizePath = join(dirname(binPath), "kesha-diarize-darwin-arm64");
    if (!existsSync(diarizePath)) {
      await downloadDiarizeSidecar(binPath, engineVersion);
    }
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

    // Kick off the sidecar fetch concurrently with the engine fetch. Both
    // target github.com release assets on independent paths with no data
    // dependency, so overlapping the HTTP round-trips saves ~15-30s on a
    // cold install. Sidecar is best-effort (404 on older engines, warn +
    // continue) so a failure doesn't cascade into the engine path.
    const sidecarPromise = downloadAVSpeechSidecar(binPath, engineVersion);
    const diarizePromise = downloadDiarizeSidecar(binPath, engineVersion);

    let res: Response;
    try {
      res = await fetch(url, { redirect: "follow" });
    } catch (e) {
      // Attach a no-op rejection handler to the sidecar promise as
      // defense-in-depth. Today it can't reject (downloadAVSpeechSidecar
      // catches all of its own errors), but if that contract ever drifts
      // a bare dangling promise would surface as an unhandledRejection
      // while we're throwing the engine error. Not waiting — the engine
      // error is what the user needs to see now; sidecar's own logs
      // will print whenever they finish.
      sidecarPromise.catch(() => {});
      diarizePromise.catch(() => {});
      throw new Error(
        `Failed to fetch engine binary: ${e instanceof Error ? e.message : e}\n  Fix: Check your network connection and try again`,
      );
    }

    if (!res.ok) {
      sidecarPromise.catch(() => {});
      diarizePromise.catch(() => {});
      throw new Error(
        `Failed to download engine binary (HTTP ${res.status})\n  Fix: Check https://github.com/${GITHUB_REPO}/releases for available versions`,
      );
    }

    await streamResponseToFile(res, binPath, "kesha-engine binary");
    chmodSync(binPath, 0o755);
    writeInstalledEngineVersion(binPath, engineVersion);
    log.success(`Engine binary downloaded (v${engineVersion}).`);
    await sidecarPromise;
    await diarizePromise;
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
