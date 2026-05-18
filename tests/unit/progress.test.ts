import { describe, test, expect } from "bun:test";
import {
  createActivityProgress,
  createProgressBar,
  formatActivityProgress,
  formatProgressBar,
  formatBytes,
} from "../../src/progress";

describe("formatBytes", () => {
  test("formats bytes to MB", () => {
    expect(formatBytes(104857600)).toBe("100.0MB");
  });

  test("formats small values", () => {
    expect(formatBytes(1048576)).toBe("1.0MB");
  });

  test("formats zero", () => {
    expect(formatBytes(0)).toBe("0.0MB");
  });
});

describe("formatProgressBar", () => {
  test("renders 0%", () => {
    const bar = formatProgressBar("encoder.onnx", 0, 100);
    expect(bar).toContain("encoder.onnx");
    expect(bar).toContain("0%");
    expect(bar).toContain("░");
  });

  test("renders 50%", () => {
    const bar = formatProgressBar("encoder.onnx", 50, 100);
    expect(bar).toContain("50%");
    expect(bar).toContain("█");
  });

  test("renders 100%", () => {
    const bar = formatProgressBar("encoder.onnx", 100, 100);
    expect(bar).toContain("100%");
  });

  test("includes byte counts in MB", () => {
    const bar = formatProgressBar("file.onnx", 104857600, 209715200);
    expect(bar).toContain("100.0MB");
    expect(bar).toContain("200.0MB");
  });

  test("handles zero total without NaN", () => {
    const bar = formatProgressBar("file.onnx", 0, 0);
    expect(bar).toContain("0%");
    expect(bar).not.toContain("NaN");
  });
});

describe("formatActivityProgress", () => {
  test("renders an indeterminate activity bar with elapsed time", () => {
    const bar = formatActivityProgress("Transcribing workshop.mp4", 65_000, 7);
    expect(bar).toContain("Transcribing workshop.mp4");
    expect(bar).toContain("[");
    expect(bar).toContain("1m05s");
    expect(bar).toContain("█");
    expect(bar).toContain("░");
  });
});

describe("createActivityProgress", () => {
  test("non-TTY mode writes stable log lines", () => {
    const originalIsTTY = process.stderr.isTTY;
    const originalWrite = process.stderr.write;
    const writes: string[] = [];

    try {
      Object.defineProperty(process.stderr, "isTTY", { value: false, configurable: true });
      process.stderr.write = ((chunk: string) => {
        writes.push(chunk);
        return true;
      }) as typeof process.stderr.write;

      const progress = createActivityProgress("Transcribing file.wav");
      progress.finish("Transcribed file.wav (123ms)");

      expect(writes.join("")).toContain("Transcribing file.wav...\n");
      expect(writes.join("")).toContain("Transcribed file.wav (123ms)\n");
    } finally {
      Object.defineProperty(process.stderr, "isTTY", { value: originalIsTTY, configurable: true });
      process.stderr.write = originalWrite;
    }
  });

  test("TTY mode clears the live bar and writes the final line", () => {
    const originalIsTTY = process.stderr.isTTY;
    const originalWrite = process.stderr.write;
    const writes: string[] = [];

    try {
      Object.defineProperty(process.stderr, "isTTY", { value: true, configurable: true });
      process.stderr.write = ((chunk: string) => {
        writes.push(chunk);
        return true;
      }) as typeof process.stderr.write;

      const progress = createActivityProgress("Transcribing file.wav", { intervalMs: 10_000 });
      progress.finish("Transcribed file.wav (123ms)");

      const combined = writes.join("");
      expect(combined).toContain("Transcribing file.wav");
      expect(combined).toContain("Transcribed file.wav (123ms)\n");
      expect(combined).toContain("\r");
    } finally {
      Object.defineProperty(process.stderr, "isTTY", { value: originalIsTTY, configurable: true });
      process.stderr.write = originalWrite;
    }
  });

  test("TTY mode can interrupt the live bar for side-band output", () => {
    const originalIsTTY = process.stderr.isTTY;
    const originalWrite = process.stderr.write;
    const writes: string[] = [];

    try {
      Object.defineProperty(process.stderr, "isTTY", { value: true, configurable: true });
      process.stderr.write = ((chunk: string) => {
        writes.push(chunk);
        return true;
      }) as typeof process.stderr.write;

      const progress = createActivityProgress("Transcribing file.wav", { intervalMs: 10_000 });
      progress.interrupt(() => process.stderr.write("warning: language mismatch\n"));
      progress.finish("Transcribed file.wav (123ms)");

      const combined = writes.join("");
      expect(combined).toContain("warning: language mismatch\n");
      expect(combined).toContain("Transcribing file.wav");
      expect(combined).toContain("Transcribed file.wav (123ms)\n");
    } finally {
      Object.defineProperty(process.stderr, "isTTY", { value: originalIsTTY, configurable: true });
      process.stderr.write = originalWrite;
    }
  });
});

describe("createProgressBar", () => {
  test("non-TTY mode calls log functions", () => {
    const bar = createProgressBar("test.onnx", 0);
    bar.update(100);
    bar.finish();
  });

  test("TTY mode ends with 100% and a newline", () => {
    // Contract: user sees progress culminating in 100% + newline.
    // Intentionally does not assert write count or per-write content —
    // coalescing writes or changing the intermediate-step cadence is a
    // legitimate refactor and shouldn't fail this test (#161, Rossi
    // liability P3).
    const originalIsTTY = process.stderr.isTTY;
    const writes: string[] = [];
    const originalWrite = process.stderr.write;

    try {
      Object.defineProperty(process.stderr, "isTTY", { value: true, configurable: true });
      process.stderr.write = ((chunk: string) => {
        writes.push(chunk);
        return true;
      }) as typeof process.stderr.write;

      const bar = createProgressBar("model.onnx", 200);
      bar.update(100);
      bar.update(50);
      bar.finish();

      const combined = writes.join("");
      expect(combined).toContain("model.onnx");
      expect(combined).toContain("100%");
      expect(combined.endsWith("\n")).toBe(true);
    } finally {
      Object.defineProperty(process.stderr, "isTTY", { value: originalIsTTY, configurable: true });
      process.stderr.write = originalWrite;
    }
  });

  test("non-TTY mode with known size includes size info", () => {
    const originalIsTTY = process.stderr.isTTY;
    try {
      Object.defineProperty(process.stderr, "isTTY", { value: false, configurable: true });
      // Should not throw — exercises the sizeInfo branch
      const bar = createProgressBar("model.onnx", 104857600);
      bar.update(100);
      bar.finish();
    } finally {
      Object.defineProperty(process.stderr, "isTTY", { value: originalIsTTY, configurable: true });
    }
  });
});
