import { log } from "./log";

const BAR_WIDTH = 20;
const DEFAULT_ESTIMATED_PROGRESS_MS = 30 * 60 * 1000;

export function formatBytes(bytes: number): string {
  return `${(bytes / 1024 / 1024).toFixed(1)}MB`;
}

export function formatProgressBar(label: string, downloaded: number, total: number): string {
  const pct = total <= 0 ? 0 : Math.min(100, Math.floor((downloaded / total) * 100));
  const filled = Math.round((pct / 100) * BAR_WIDTH);
  const empty = BAR_WIDTH - filled;
  const bar = "█".repeat(filled) + "░".repeat(empty);
  return `${label}  [${bar}] ${pct}%  ${formatBytes(downloaded)}/${formatBytes(total)}`;
}

export function formatPercentProgress(label: string, percent: number): string {
  const pct = Math.max(0, Math.min(100, Math.floor(percent)));
  const filled = Math.round((pct / 100) * BAR_WIDTH);
  const empty = BAR_WIDTH - filled;
  const bar = "█".repeat(filled) + "░".repeat(empty);
  return `${label}  [${bar}] ${pct}%`;
}

function estimatePercent(elapsedMs: number, estimatedTotalMs: number): number {
  if (elapsedMs <= 0) return 0;
  const targetMs = Math.max(1, estimatedTotalMs);
  return Math.max(1, Math.min(99, Math.floor((elapsedMs / targetMs) * 99)));
}

export function createPercentProgress(
  label: string,
  options: { estimatedTotalMs?: number; intervalMs?: number } = {},
): {
  finish(finalLabel: string): void;
  interrupt(writeLine: () => void): void;
  stop(): void;
} {
  const isTTY = process.stderr.isTTY;

  if (!isTTY) {
    process.stderr.write(`${formatPercentProgress(label, 0)}\n`);
    return {
      finish(finalLabel: string) {
        process.stderr.write(`${formatPercentProgress(finalLabel, 100)}\n`);
      },
      interrupt(writeLine: () => void) {
        writeLine();
      },
      stop() {},
    };
  }

  const intervalMs = options.intervalMs ?? 250;
  const estimatedTotalMs = options.estimatedTotalMs ?? DEFAULT_ESTIMATED_PROGRESS_MS;
  const startedAt = performance.now();
  let lastLineLength = 0;
  let timer: Timer | undefined;
  let stopped = false;
  let firstRender = true;

  const render = () => {
    if (stopped) return;
    const percent = firstRender ? 0 : estimatePercent(performance.now() - startedAt, estimatedTotalMs);
    firstRender = false;
    const line = formatPercentProgress(label, percent);
    lastLineLength = line.length;
    process.stderr.write(`\r${line}`);
  };

  const clearLine = (stopTimer: boolean) => {
    if (stopTimer) stopped = true;
    if (timer) {
      clearInterval(timer);
      timer = undefined;
    }
    if (lastLineLength > 0) {
      process.stderr.write(`\r${" ".repeat(lastLineLength)}\r`);
      lastLineLength = 0;
    }
  };

  render();
  timer = setInterval(render, intervalMs);

  return {
    finish(finalLabel: string) {
      clearLine(true);
      process.stderr.write(`${formatPercentProgress(finalLabel, 100)}\n`);
    },
    interrupt(writeLine: () => void) {
      if (stopped) {
        writeLine();
        return;
      }
      clearLine(false);
      writeLine();
      render();
      timer = setInterval(render, intervalMs);
    },
    stop() {
      clearLine(true);
    },
  };
}

export async function streamResponseToFile(
  res: Response,
  destPath: string,
  label: string,
): Promise<number> {
  if (!res.body) {
    throw new Error(
      `Download failed: empty response for ${label}\n  Fix: Try again — the server may be temporarily unavailable`,
    );
  }

  const totalBytes = Number(res.headers.get("content-length") || 0);
  const progress = createProgressBar(label, totalBytes);

  const writer = Bun.file(destPath).writer();
  let bytes = 0;
  try {
    for await (const chunk of res.body) {
      writer.write(chunk);
      bytes += chunk.length;
      progress.update(chunk.length);
    }
  } finally {
    writer.end();
  }

  progress.finish();
  return bytes;
}

export function createProgressBar(label: string, totalBytes: number): {
  update(downloadedBytes: number): void;
  finish(): void;
} {
  const isTTY = process.stderr.isTTY;

  if (!isTTY || totalBytes <= 0) {
    const sizeInfo = totalBytes > 0 ? ` (${formatBytes(totalBytes)})` : "";
    log.progress(`Downloading ${label}${sizeInfo}...`);
    return {
      update() {},
      finish() {
        log.success(`Downloaded ${label} ✓`);
      },
    };
  }

  let current = 0;
  let lastPct = -1;
  return {
    update(downloadedBytes: number) {
      current += downloadedBytes;
      const pct = totalBytes > 0 ? Math.floor((current / totalBytes) * 100) : 0;
      if (pct === lastPct) return;
      lastPct = pct;
      const line = formatProgressBar(label, current, totalBytes);
      process.stderr.write(`\r${line}`);
    },
    finish() {
      const line = formatProgressBar(label, totalBytes, totalBytes);
      process.stderr.write(`\r${line}\n`);
    },
  };
}
