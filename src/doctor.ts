import { existsSync, readdirSync, statSync } from "fs";
import { dirname, join, sep } from "path";
import { homedir } from "os";
import {
  getEngineBinPath,
  getEngineCapabilities,
  isEngineInstalled,
  type EngineCapabilities,
} from "./engine";
import { readInstalledEngineVersion } from "./engine-version-marker";
import { keshaCacheDir } from "./paths";
import { getStatsStatus, type StatsStatus } from "./stats";

const KNOWN_ENV_KEYS = [
  "KESHA_ENGINE_BIN",
  "KESHA_CACHE_DIR",
  "KESHA_MODEL_MIRROR",
  "KESHA_STATS_DB",
  "KESHA_DEBUG",
  "KESHA_DEBUG_FD",
] as const;

interface DoctorOptions {
  redact?: boolean;
}

interface PathSummary {
  path: string;
  exists: boolean;
  sizeBytes: number;
}

interface CacheComponent extends PathSummary {
  label: string;
}

interface OptionalComponent extends PathSummary {
  name: string;
  note?: string;
}

export interface DoctorReport {
  generatedAt: string;
  redacted: boolean;
  package: {
    name: string;
    version: string;
  };
  runtime: {
    bunVersion: string;
    platform: string;
    arch: string;
  };
  engine: {
    path: string;
    installed: boolean;
    versionMarker: string | null;
    capabilities: EngineCapabilities | null;
    probeError: string | null;
  };
  cache: {
    path: string;
    exists: boolean;
    totalBytes: number;
    components: CacheComponent[];
  };
  optionalComponents: OptionalComponent[];
  stats: StatsStatus | (Partial<StatsStatus> & { error: string });
  env: Record<string, string | null>;
}

function diagnosticHomeDir(): string {
  return process.env.HOME ?? homedir();
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

function dirSizeBytes(path: string): number {
  let total = 0;
  try {
    const st = statSync(path);
    if (st.isFile()) return st.size;
    for (const entry of readdirSync(path, { withFileTypes: true })) {
      const p = join(path, entry.name);
      total += entry.isDirectory() ? dirSizeBytes(p) : statSync(p).size;
    }
  } catch {
    return total;
  }
  return total;
}

function pathSummary(path: string): PathSummary {
  return {
    path,
    exists: existsSync(path),
    sizeBytes: dirSizeBytes(path),
  };
}

function isSecretKey(key: string): boolean {
  const secretParts = ["TOKEN", "KEY", "SECRET", "PASSWORD", "CREDENTIAL", "AUTH"];
  return key
    .split(/[^a-z0-9]+/i)
    .some((part) => secretParts.includes(part.toUpperCase()));
}

function redactUrl(value: string): string | null {
  if (!/^[a-z][a-z0-9+.-]*:\/\//i.test(value)) return null;
  try {
    const url = new URL(value);
    url.username = "";
    url.password = "";
    url.search = "";
    url.hash = "";
    return url.toString().replace(/\/$/, value.endsWith("/") ? "/" : "");
  } catch {
    return null;
  }
}

function redactHomePaths(value: string, homeDir: string): string | null {
  const normalizedValue = value.replaceAll("\\", "/").replace(/\/+$/, "");
  const normalizedHome = homeDir.replaceAll("\\", "/").replace(/\/+$/, "");
  const compareValue = process.platform === "win32" ? normalizedValue.toLowerCase() : normalizedValue;
  const compareHome = process.platform === "win32" ? normalizedHome.toLowerCase() : normalizedHome;

  if (compareValue === compareHome) return "~";
  if (compareValue.startsWith(`${compareHome}/`)) {
    return `~${normalizedValue.slice(normalizedHome.length)}`;
  }
  const escapedHome = normalizedHome.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const homePattern = new RegExp(`${escapedHome}(?=$|/)`, process.platform === "win32" ? "gi" : "g");
  const redacted = normalizedValue.replace(homePattern, (_match, offset: number) => {
    const previous = normalizedValue[offset - 1];
    return previous && !/[\s"'(:=]/.test(previous) ? "/~" : "~";
  });
  return redacted === normalizedValue ? null : redacted;
}

export function redactDiagnosticValue(
  key: string,
  value: string | null,
  homeDir = diagnosticHomeDir(),
): string | null {
  if (value === null) return null;
  if (isSecretKey(key)) return "[REDACTED]";

  const url = redactUrl(value);
  if (url) {
    const redactedUrlPaths = homeDir ? redactHomePaths(url, homeDir) : null;
    return redactedUrlPaths ?? url;
  }

  if (homeDir) {
    const redactedHomePaths = redactHomePaths(value, homeDir);
    if (redactedHomePaths !== null) return redactedHomePaths;
  }
  return value;
}

function redactPath(path: string, redact: boolean): string {
  return redact ? redactDiagnosticValue("path", path) ?? path : path;
}

function redactString(key: string, value: string | null, redact: boolean): string | null {
  return redact ? redactDiagnosticValue(key, value) : value;
}

function redactComponent<T extends PathSummary>(component: T, redact: boolean): T {
  if (!redact) return component;
  return { ...component, path: redactPath(component.path, true) };
}

async function readPackageInfo(): Promise<{ name: string; version: string }> {
  const pkg = await Bun.file(new URL("../package.json", import.meta.url)).json();
  return {
    name: typeof pkg.name === "string" ? pkg.name : "unknown",
    version: typeof pkg.version === "string" ? pkg.version : "unknown",
  };
}

async function collectEngine(redact: boolean): Promise<DoctorReport["engine"]> {
  const binPath = getEngineBinPath();
  const installed = isEngineInstalled();
  let capabilities: EngineCapabilities | null = null;
  let probeError: string | null = null;

  if (installed) {
    try {
      capabilities = await getEngineCapabilities();
      if (!capabilities) probeError = "capabilities probe returned no data";
    } catch (err) {
      probeError = err instanceof Error ? err.message : String(err);
    }
  }

  return {
    path: redactPath(binPath, redact),
    installed,
    versionMarker: readInstalledEngineVersion(binPath),
    capabilities,
    probeError: redactString("probeError", probeError, redact),
  };
}

function collectCache(redact: boolean): DoctorReport["cache"] {
  const cache = keshaCacheDir();
  const binPath = getEngineBinPath();
  const engineDir = dirname(dirname(binPath));
  const components: CacheComponent[] = [
    { label: "Engine", ...pathSummary(engineDir) },
    { label: "ASR (Parakeet)", ...pathSummary(join(cache, "models/parakeet-tdt-v3")) },
    { label: "Language ID", ...pathSummary(join(cache, "models/lang-id-ecapa")) },
    { label: "VAD (Silero)", ...pathSummary(join(cache, "models/silero-vad")) },
    { label: "TTS (Kokoro)", ...pathSummary(join(cache, "models/kokoro-82m")) },
    { label: "TTS (Vosk)", ...pathSummary(join(cache, "models/vosk-ru")) },
  ];
  const engineInsideCache = engineDir === cache || engineDir.startsWith(`${cache}${sep}`);
  const engineOutsideCache = engineInsideCache ? 0 : dirSizeBytes(engineDir);

  return {
    path: redactPath(cache, redact),
    exists: existsSync(cache),
    totalBytes: dirSizeBytes(cache) + engineOutsideCache,
    components: components.map((component) => redactComponent(component, redact)),
  };
}

function collectOptionalComponents(redact: boolean): OptionalComponent[] {
  const cache = keshaCacheDir();
  const sidecarDir = dirname(getEngineBinPath());
  const components: OptionalComponent[] = [
    {
      name: "VAD (Silero)",
      note: "enabled with `kesha install --vad`",
      ...pathSummary(join(cache, "models/silero-vad")),
    },
    {
      name: "TTS (Kokoro)",
      note: "enabled with `kesha install --tts`",
      ...pathSummary(join(cache, "models/kokoro-82m")),
    },
    {
      name: "TTS (Vosk RU)",
      note: "enabled with `kesha install --tts`",
      ...pathSummary(join(cache, "models/vosk-ru")),
    },
    {
      name: "Diarization sidecar",
      note: "darwin-arm64 only",
      ...pathSummary(join(sidecarDir, "kesha-diarize-darwin-arm64")),
    },
    {
      name: "AVSpeech sidecar",
      note: "macOS voices",
      ...pathSummary(join(sidecarDir, "say-avspeech")),
    },
    {
      name: "FluidAudio Kokoro sidecar",
      note: "darwin-arm64 Kokoro",
      ...pathSummary(join(sidecarDir, "kesha-kokoro")),
    },
    {
      name: "Text language sidecar",
      note: "darwin text language fast path",
      ...pathSummary(join(sidecarDir, "kesha-textlang")),
    },
  ];
  return components.map((component) => redactComponent(component, redact));
}

function collectStats(redact: boolean): DoctorReport["stats"] {
  try {
    const status = getStatsStatus();
    return redact
      ? { ...status, dbPath: redactPath(status.dbPath, true) }
      : status;
  } catch (err) {
    return {
      error: redactString("statsError", err instanceof Error ? err.message : String(err), redact) ?? "unknown",
    };
  }
}

function collectEnv(redact: boolean): DoctorReport["env"] {
  const env: Record<string, string | null> = {};
  for (const key of KNOWN_ENV_KEYS) {
    env[key] = redactString(key, process.env[key] ?? null, redact);
  }
  return env;
}

export async function collectDoctorReport(
  options: DoctorOptions = {},
): Promise<DoctorReport> {
  const redact = options.redact === true;
  return {
    generatedAt: new Date().toISOString(),
    redacted: redact,
    package: await readPackageInfo(),
    runtime: {
      bunVersion: Bun.version,
      platform: process.platform,
      arch: process.arch,
    },
    engine: await collectEngine(redact),
    cache: collectCache(redact),
    optionalComponents: collectOptionalComponents(redact),
    stats: collectStats(redact),
    env: collectEnv(redact),
  };
}

function formatInstalled(installed: boolean): string {
  return installed ? "installed" : "missing";
}

export function formatDoctorReport(report: DoctorReport): string {
  const lines = [
    "Kesha Doctor",
    "",
    "Runtime:",
    `  Package: ${report.package.name} ${report.package.version}`,
    `  Bun: ${report.runtime.bunVersion}`,
    `  Platform: ${report.runtime.platform} ${report.runtime.arch}`,
    "",
    "Engine:",
    `  Binary: ${report.engine.path} (${formatInstalled(report.engine.installed)})`,
    `  Version marker: ${report.engine.versionMarker ?? "missing"}`,
    `  Capabilities: ${
      report.engine.capabilities
        ? `${report.engine.capabilities.backend}, protocol v${report.engine.capabilities.protocolVersion}, ${report.engine.capabilities.features.join(", ")}`
        : report.engine.probeError ?? "not available"
    }`,
    "",
    `Cache: ${report.cache.path} (${report.cache.exists ? humanBytes(report.cache.totalBytes) : "missing"})`,
  ];

  for (const component of report.cache.components) {
    lines.push(`  ${component.label}: ${component.exists ? humanBytes(component.sizeBytes) : "missing"}`);
  }

  lines.push("", "Optional components:");
  for (const component of report.optionalComponents) {
    const note = component.note ? ` - ${component.note}` : "";
    lines.push(`  ${component.name}: ${formatInstalled(component.exists)}${note}`);
  }

  lines.push("", "Stats:");
  if ("error" in report.stats) {
    lines.push(`  Error: ${report.stats.error}`);
  } else {
    lines.push(`  Enabled: ${report.stats.enabled ? "yes" : "no"}`);
    lines.push(`  Database: ${report.stats.dbPath}`);
    lines.push(`  Runs: ${report.stats.runCount}`);
  }

  lines.push("", "Environment:");
  for (const [key, value] of Object.entries(report.env)) {
    lines.push(`  ${key}: ${value ?? "unset"}`);
  }

  return `${lines.join("\n")}\n`;
}
