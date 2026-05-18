import { log } from "./log";

const BAR_WIDTH = 20;
const ACTIVITY_PULSE_WIDTH = 5;

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

export function formatActivityProgress(label: string, elapsedMs: number, frame: number): string {
  const range = BAR_WIDTH + ACTIVITY_PULSE_WIDTH;
  const pulseEnd = frame % range;
  const pulseStart = pulseEnd - ACTIVITY_PULSE_WIDTH;
  const bar = Array.from({ length: BAR_WIDTH }, (_, i) =>
    i >= pulseStart && i < pulseEnd ? "█" : "░",
  ).join("");
  return `${label}  [${bar}] ${formatElapsed(elapsedMs)}`;
}

function formatElapsed(elapsedMs: number): string {
  const totalSeconds = Math.max(0, Math.floor(elapsedMs / 1000));
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return minutes > 0 ? `${minutes}m${seconds.toString().padStart(2, "0")}s` : `${seconds}s`;
}

export function createActivityProgress(
  label: string,
  options: { intervalMs?: number } = {},
): {
  finish(finalMessage: string): void;
  interrupt(writeLine: () => void): void;
  stop(): void;
} {
  const isTTY = process.stderr.isTTY;

  if (!isTTY) {
    process.stderr.write(`${label}...\n`);
    return {
      finish(finalMessage: string) {
        process.stderr.write(`${finalMessage}\n`);
      },
      interrupt(writeLine: () => void) {
        writeLine();
      },
      stop() {},
    };
  }

  const intervalMs = options.intervalMs ?? 250;
  const startedAt = performance.now();
  let frame = 1;
  let lastLineLength = 0;
  let timer: Timer | undefined;
  let stopped = false;

  const render = () => {
    if (stopped) return;
    const line = formatActivityProgress(label, performance.now() - startedAt, frame);
    frame += 1;
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
    finish(finalMessage: string) {
      clearLine(true);
      process.stderr.write(`${finalMessage}\n`);
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
