import { readdirSync, statSync } from "fs";
import { join } from "path";
import { isEngineInstalled, getEngineBinPath, getEngineCapabilities } from "./engine";
import { log } from "./log";
import { keshaCacheDir } from "./paths";
import pc from "picocolors";

function humanBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
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
    /* missing path — component not installed */
  }
  return total;
}

export function formatStatusLine(
  label: string,
  path: string | null,
  installed: boolean,
  missingLabel = "not installed",
): string {
  const status = installed ? pc.green("✓") : pc.red(`✗ ${missingLabel}`);
  const pathStr = path ?? "";
  const padding = " ".repeat(Math.max(1, 50 - label.length - pathStr.length));
  return `  ${label}:${pathStr ? `   ${pathStr}` : ""}${padding}${status}`;
}

export async function showStatus(): Promise<void> {
  const binPath = getEngineBinPath();
  const installed = isEngineInstalled();

  log.info("Engine:");
  log.info(formatStatusLine("Binary", installed ? binPath : null, installed));

  if (installed) {
    let caps: Awaited<ReturnType<typeof getEngineCapabilities>> = null;
    try {
      caps = await getEngineCapabilities();
    } catch {
      caps = null;
    }
    if (caps) {
      log.info(formatStatusLine("Backend", caps.backend, true));
      log.info(formatStatusLine("Protocol", `v${caps.protocolVersion}`, true));
      log.info(formatStatusLine("Features", caps.features.join(", "), true));
    } else {
      log.info(formatStatusLine("Capabilities", null, false, "probe failed"));
    }
  }
  log.info("");

  log.info(formatStatusLine("Runtime", `Bun ${Bun.version}`, true));
  log.info(formatStatusLine("Platform", `${process.platform} ${process.arch}`, true));
  const mirror = activeModelMirror();
  if (mirror) {
    log.info(formatStatusLine("Mirror", mirror, true));
  }
  log.info("");

  if (installed) {
    const voices = listInstalledVoices();
    if (voices.length > 0) {
      log.info("TTS voices:");
      for (const v of voices) {
        log.info(`  ${v}`);
      }
      log.info("");
    }

    showDiskUsage(binPath);
  }

  if (!installed) {
    log.warn('Run "kesha install" to download the engine and models.');
    return;
  }
}

function showDiskUsage(binPath: string): void {
  const cache = keshaCacheDir();
  // Engine binary lives under `<cache>/engine/bin/` (managed by the TS CLI's
  // engine-install) while all models live under `<cache>/models/` (managed by
  // the Rust engine). Point at `engine/` (two levels up from the binary) so
  // any future sibling files under that root (metadata, hooks, etc.) are
  // counted too.
  const engineDir = join(binPath, "..", "..");

  const components: Array<{ label: string; path: string }> = [
    { label: "Engine", path: engineDir },
    { label: "ASR (Parakeet)", path: join(cache, "models/parakeet-tdt-v3") },
    { label: "Language ID", path: join(cache, "models/lang-id-ecapa") },
    { label: "VAD (Silero)", path: join(cache, "models/silero-vad") },
    { label: "TTS (Kokoro)", path: join(cache, "models/kokoro-82m") },
    { label: "TTS (Vosk)", path: join(cache, "models/vosk-ru") },
  ];

  const rows: Array<{ label: string; size: number }> = [];
  for (const c of components) {
    const size = dirSizeBytes(c.path);
    if (size > 0) rows.push({ label: c.label, size });
  }

  if (rows.length === 0) return;

  // Total counts everything under both the model cache root AND the engine
  // dir (which may live outside the cache when `KESHA_ENGINE_BIN` overrides
  // the default layout). That way the number matches what the `rm -rf` hint
  // below would actually free, including any temp downloads or future
  // components not in the per-row list.
  const componentTotal = rows.reduce((n, r) => n + r.size, 0);
  const cacheTotal = dirSizeBytes(cache);
  const engineOutsideCache = engineDir.startsWith(cache)
    ? 0
    : dirSizeBytes(engineDir);
  const total = cacheTotal + engineOutsideCache;

  log.info(`Disk usage (${cache}):`);
  const labelWidth = Math.max(...rows.map((r) => r.label.length), "Total".length);
  for (const r of rows) {
    const pad = " ".repeat(labelWidth - r.label.length + 2);
    log.info(`  ${r.label}:${pad}${humanBytes(r.size)}`);
  }
  const totalPad = " ".repeat(labelWidth - "Total".length + 2);
  log.info(`  ${pc.bold("Total")}:${totalPad}${pc.bold(humanBytes(total))}`);
  if (total > componentTotal) {
    const other = total - componentTotal;
    log.info(pc.dim(`  (includes ${humanBytes(other)} of other cache files)`));
  }
  log.info("");
  log.info(pc.dim(`  To reset cache: rm -rf ${cache} — next \`kesha install\` re-downloads.`));
  log.info("");
}

/**
 * Read the effective `KESHA_MODEL_MIRROR` base URL (#121). Returns null when
 * unset, empty, or whitespace. Matches the Rust side's `model_mirror()` in
 * `rust/src/models.rs` — keeping them in lockstep lets `kesha status`
 * surface the exact URL the engine will hit on the next `kesha install`.
 */
export function activeModelMirror(): string | null {
  const raw = process.env.KESHA_MODEL_MIRROR ?? "";
  const trimmed = raw.trim().replace(/\/+$/, "");
  return trimmed.length > 0 ? trimmed : null;
}

function listInstalledVoices(): string[] {
  const cache = keshaCacheDir();
  const voices: string[] = [];
  try {
    const kokoro = readdirSync(join(cache, "models", "kokoro-82m", "voices"));
    for (const f of kokoro) {
      if (f.endsWith(".bin")) voices.push(`en-${f.replace(/\.bin$/, "")}`);
    }
  } catch {
    /* Kokoro not installed */
  }
  try {
    // Vosk-TTS Russian is a single multi-speaker model. Mirror the Rust-side
    // gate (models::is_vosk_ru_cached) — checking model.onnx + bert/model.onnx
    // avoids advertising voices that would fail to load on a partial install.
    statSync(join(cache, "models", "vosk-ru", "model.onnx"));
    statSync(join(cache, "models", "vosk-ru", "bert", "model.onnx"));
    for (const id of ["f01", "f02", "f03", "m01", "m02"]) {
      voices.push(`ru-vosk-${id}`);
    }
  } catch {
    /* Vosk not installed */
  }
  return voices.sort();
}
