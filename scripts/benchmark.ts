#!/usr/bin/env bun
/**
 * Benchmark: openai-whisper vs faster-whisper vs Kesha Voice Kit
 * Runs all three engines on Russian + English fixtures.
 * Output: markdown to stdout, JSON to benchmark-results.json.
 */

import { Glob } from "bun";
import { resolve, basename } from "path";
import { existsSync, mkdirSync } from "fs";
import { homedir } from "os";

const CLI = "kesha";
const VENV_DIR = resolve(homedir(), ".cache", "kesha", "benchmark-venv");
const RESULTS_FILE = "benchmark-results.json";

// --- Types ---

interface EngineResult {
  time: number;
  text: string;
}

interface FileResult {
  file: string;
  openaiWhisper: EngineResult;
  fasterWhisper: EngineResult;
  kesha: EngineResult;
  keshaCoreml?: EngineResult;
}

interface GroupResult {
  name: string;
  results: FileResult[];
  totals: { openaiWhisper: number; fasterWhisper: number; kesha: number; keshaCoreml?: number };
}

interface BenchmarkReport {
  date: string;
  platform: { os: string; arch: string; chip: string; ram: string };
  keshaBackend: string;
  whisperModel: string;
  groups: GroupResult[];
}

// --- System detection ---

function getSystemInfo(): BenchmarkReport["platform"] {
  const os = process.platform === "darwin" ? "Darwin" : process.platform === "linux" ? "Linux" : "Windows";
  const arch = process.arch;
  let chip = "Unknown";
  let ram = "Unknown";

  if (os === "Darwin") {
    chip = Bun.spawnSync(["sysctl", "-n", "machdep.cpu.brand_string"], { stdout: "pipe" }).stdout.toString().trim() || "Unknown";
    const profiler = Bun.spawnSync(["system_profiler", "SPHardwareDataType"], { stdout: "pipe" }).stdout.toString();
    ram = profiler.match(/Memory:\s+(.+)/)?.[1] ?? "Unknown";
  } else if (os === "Linux") {
    const lscpu = Bun.spawnSync(["lscpu"], { stdout: "pipe" }).stdout.toString();
    chip = lscpu.match(/Model name:\s+(.*)/)?.[1]?.trim() ?? "Unknown";
    const free = Bun.spawnSync(["free", "-h"], { stdout: "pipe" }).stdout.toString();
    ram = free.match(/Mem:\s+(\S+)/)?.[1] ?? "Unknown";
  }

  return { os, arch, chip, ram };
}

function getKeshaBackend(): string {
  const proc = Bun.spawnSync([CLI, "status"], { stdout: "pipe", stderr: "pipe" });
  const output = proc.stdout.toString();
  if (output.includes("coreml")) return "coreml";
  if (output.includes("onnx")) return "onnx";
  return "unknown";
}

// --- Python venv management ---

function ensureVenv(): string {
  const python = resolve(VENV_DIR, "bin", "python3");
  const pip = resolve(VENV_DIR, "bin", "pip");

  if (existsSync(python)) {
    // Check if packages are installed
    const check = Bun.spawnSync([python, "-c", "import whisper; import faster_whisper"], {
      stdout: "pipe", stderr: "pipe",
    });
    if (check.exitCode === 0) return python;
  }

  console.error("Setting up Python venv for Whisper benchmarks...");
  mkdirSync(VENV_DIR, { recursive: true });

  const venvProc = Bun.spawnSync(["python3", "-m", "venv", VENV_DIR], { stdout: "pipe", stderr: "pipe" });
  if (venvProc.exitCode !== 0) {
    throw new Error(`Failed to create venv: ${venvProc.stderr.toString()}`);
  }

  console.error("Installing openai-whisper...");
  const whisperInstall = Bun.spawnSync([pip, "install", "-q", "openai-whisper"], { stdout: "pipe", stderr: "inherit" });
  if (whisperInstall.exitCode !== 0) throw new Error("Failed to install openai-whisper");

  console.error("Installing faster-whisper...");
  const fasterInstall = Bun.spawnSync([pip, "install", "-q", "faster-whisper"], { stdout: "pipe", stderr: "inherit" });
  if (fasterInstall.exitCode !== 0) throw new Error("Failed to install faster-whisper");

  console.error("Venv ready.\n");
  return python;
}

// --- Fixture scanning ---

function scanFixtures(dir: string): string[] {
  if (!existsSync(dir)) return [];
  return [...new Glob("*.ogg").scanSync(dir)].sort().map((f) => resolve(dir, f));
}

// --- Engine runners ---

function runOpenAIWhisper(python: string, files: string[]): EngineResult[] {
  console.error(`Running openai-whisper (large-v3-turbo) on ${files.length} files...`);

  const script = `
import sys, time, json, whisper

model = whisper.load_model("large-v3-turbo")
results = []
total = len(sys.argv[1:])
for i, f in enumerate(sys.argv[1:], 1):
    name = f.split("/")[-1][:30]
    print(f"  [{i}/{total}] {name}...", end="", flush=True, file=sys.stderr)
    start = time.time()
    result = model.transcribe(f)
    elapsed = time.time() - start
    print(f" {elapsed:.1f}s", file=sys.stderr)
    results.append({"time": round(elapsed, 1), "text": result["text"].strip()})

print(json.dumps(results, ensure_ascii=False))
`;

  const proc = Bun.spawnSync([python, "-c", script, ...files], {
    stdout: "pipe", stderr: "inherit",
  });
  if (proc.exitCode !== 0) throw new Error("openai-whisper benchmark failed");
  return JSON.parse(proc.stdout.toString());
}

function runFasterWhisper(python: string, files: string[]): EngineResult[] {
  console.error(`Running faster-whisper (large-v3-turbo, int8) on ${files.length} files...`);

  const script = `
import sys, time, json
from faster_whisper import WhisperModel

model = WhisperModel("large-v3-turbo", device="cpu", compute_type="int8")
results = []
total = len(sys.argv[1:])
for i, f in enumerate(sys.argv[1:], 1):
    name = f.split("/")[-1][:30]
    print(f"  [{i}/{total}] {name}...", end="", flush=True, file=sys.stderr)
    start = time.time()
    segments, info = model.transcribe(f)
    text = " ".join(s.text.strip() for s in segments)
    elapsed = time.time() - start
    print(f" {elapsed:.1f}s", file=sys.stderr)
    results.append({"time": round(elapsed, 1), "text": text})

print(json.dumps(results, ensure_ascii=False))
`;

  const proc = Bun.spawnSync([python, "-c", script, ...files], {
    stdout: "pipe", stderr: "inherit",
  });
  if (proc.exitCode !== 0) throw new Error("faster-whisper benchmark failed");
  return JSON.parse(proc.stdout.toString());
}

function runKesha(files: string[]): EngineResult[] {
  console.error(`Running Kesha on ${files.length} files...`);
  const results: EngineResult[] = [];

  for (let i = 0; i < files.length; i++) {
    const file = files[i];
    const name = basename(file).slice(0, 30);
    process.stderr.write(`  [${i + 1}/${files.length}] ${name}...`);

    const start = performance.now();
    const proc = Bun.spawnSync([CLI, file], { stdout: "pipe", stderr: "pipe" });
    const elapsed = (performance.now() - start) / 1000;

    if (proc.exitCode !== 0) {
      console.error(" FAILED");
    } else {
      console.error(` ${elapsed.toFixed(1)}s`);
    }

    results.push({
      time: Math.round(elapsed * 10) / 10,
      text: proc.stdout.toString().trim(),
    });
  }

  return results;
}

const COREML_BIN = "/tmp/coreml-bench/parakeet-coreml";

function isCoremlAvailable(): boolean {
  return process.platform === "darwin" && process.arch === "arm64" && existsSync(COREML_BIN);
}

function runKeshaCoreml(files: string[]): EngineResult[] {
  console.error(`Running Kesha CoreML on ${files.length} files...`);
  const results: EngineResult[] = [];

  for (let i = 0; i < files.length; i++) {
    const file = files[i];
    const name = basename(file).slice(0, 30);
    process.stderr.write(`  [${i + 1}/${files.length}] ${name}...`);

    const start = performance.now();
    const proc = Bun.spawnSync([COREML_BIN, file], { stdout: "pipe", stderr: "pipe" });
    const elapsed = (performance.now() - start) / 1000;

    if (proc.exitCode !== 0) {
      console.error(" FAILED");
    } else {
      console.error(` ${elapsed.toFixed(1)}s`);
    }

    results.push({
      time: Math.round(elapsed * 10) / 10,
      text: proc.stdout.toString().trim(),
    });
  }

  return results;
}

// --- Report rendering ---

function round1(n: number): number {
  return Math.round(n * 10) / 10;
}

function sumTimes(results: EngineResult[]): number {
  return round1(results.reduce((sum, r) => sum + r.time, 0));
}

function renderGroup(group: GroupResult, hasCoreml: boolean): string[] {
  const coremlHeader = hasCoreml ? " Kesha CoreML |" : "";
  const coremlSep = hasCoreml ? "---|" : "";
  const lines: string[] = [
    `### ${group.name} (${group.results.length} files)`,
    "",
    `| # | File | openai-whisper | faster-whisper | Kesha ONNX |${coremlHeader} Transcript (Kesha) |`,
    `|---|---|---|---|---|${coremlSep}---|`,
  ];

  for (let i = 0; i < group.results.length; i++) {
    const r = group.results[i];
    const transcript = r.kesha.text.slice(0, 60) + (r.kesha.text.length > 60 ? "..." : "");
    const coremlCol = hasCoreml && r.keshaCoreml ? ` ${r.keshaCoreml.time}s |` : "";
    lines.push(
      `| ${i + 1} | ${r.file} | ${r.openaiWhisper.time}s | ${r.fasterWhisper.time}s | ${r.kesha.time}s |${coremlCol} ${transcript} |`,
    );
  }

  const t = group.totals;
  const coremlTotal = hasCoreml && t.keshaCoreml != null ? ` **${t.keshaCoreml}s** |` : "";
  lines.push(`| **Total** | | **${t.openaiWhisper}s** | **${t.fasterWhisper}s** | **${t.kesha}s** |${coremlTotal} |`);
  lines.push("");

  const bestTime = hasCoreml && t.keshaCoreml != null ? t.keshaCoreml : t.kesha;
  const bestLabel = hasCoreml && t.keshaCoreml != null ? "Kesha CoreML" : "Kesha ONNX";
  const speedVsWhisper = bestTime > 0 ? round1(t.openaiWhisper / bestTime) : 0;
  const speedVsFaster = bestTime > 0 ? round1(t.fasterWhisper / bestTime) : 0;
  lines.push(
    `**Speedup:** ${bestLabel} is ~${speedVsWhisper}x faster than openai-whisper, ~${speedVsFaster}x faster than faster-whisper`,
  );

  return lines;
}

function renderMarkdown(report: BenchmarkReport): string {
  const p = report.platform;
  const lines: string[] = [
    "## Benchmark: Speech-to-Text Engines",
    "",
    `**Date:** ${report.date}`,
    `**Platform:** ${p.os} ${p.arch} (${p.chip}, ${p.ram} RAM)`,
    `**Kesha backend:** ${report.keshaBackend}`,
    `**Whisper model:** ${report.whisperModel}`,
    `**openai-whisper** is the default transcription engine in OpenClaw.`,
    "",
  ];

  const hasCoreml = report.groups.some((g) => g.totals.keshaCoreml != null);
  for (const group of report.groups) {
    lines.push(...renderGroup(group, hasCoreml));
    lines.push("");
  }

  return lines.join("\n");
}

// --- Main ---

async function main(): Promise<void> {
  const repoDir = resolve(import.meta.dir, "..");
  const ruFiles = scanFixtures(resolve(repoDir, "tests/fixtures/benchmark"));
  const enFiles = scanFixtures(resolve(repoDir, "tests/fixtures/benchmark-en"));

  if (ruFiles.length === 0 && enFiles.length === 0) {
    throw new Error("No fixture files found");
  }

  const platform = getSystemInfo();
  const keshaBackend = getKeshaBackend();
  const python = ensureVenv();

  const groups: GroupResult[] = [];

  for (const [name, files] of [["Russian", ruFiles], ["English", enFiles]] as const) {
    if (files.length === 0) continue;

    console.error(`\n--- ${name} (${files.length} files) ---\n`);

    const owResults = runOpenAIWhisper(python, files);
    const fwResults = runFasterWhisper(python, files);
    const kResults = runKesha(files);
    const coremlResults = isCoremlAvailable() ? runKeshaCoreml(files) : null;

    const results: FileResult[] = files.map((f, i) => ({
      file: basename(f),
      openaiWhisper: owResults[i],
      fasterWhisper: fwResults[i],
      kesha: kResults[i],
      ...(coremlResults ? { keshaCoreml: coremlResults[i] } : {}),
    }));

    groups.push({
      name,
      results,
      totals: {
        openaiWhisper: sumTimes(owResults),
        fasterWhisper: sumTimes(fwResults),
        kesha: sumTimes(kResults),
        ...(coremlResults ? { keshaCoreml: sumTimes(coremlResults) } : {}),
      },
    });
  }

  const report: BenchmarkReport = {
    date: new Date().toISOString().split("T")[0],
    platform,
    keshaBackend,
    whisperModel: "large-v3-turbo",
    groups,
  };

  console.log(renderMarkdown(report));
  await Bun.write(RESULTS_FILE, JSON.stringify(report, null, 2));
  console.error(`\nJSON results written to ${RESULTS_FILE}`);
}

main().catch((err) => {
  console.error(`ERROR: ${err instanceof Error ? err.message : err}`);
  process.exit(1);
});
