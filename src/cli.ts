#!/usr/bin/env bun

// Thin re-export shim. The CLI subcommands live in ./cli/*.ts since #180;
// tests and lib.ts keep importing from "./cli" via these re-exports so the
// public surface is unchanged.
export { doctorCommand } from "./cli/doctor";
export { completionsCommand } from "./cli/completions";
export { installCommand } from "./cli/install";
export { manpageCommand } from "./cli/manpage";
export { recordCommand, resolveRecordArgs } from "./cli/record";
export type { ResolvedRecordArgs } from "./cli/record";
export { sayCommand } from "./cli/say";
export { pickVoiceForLang } from "./voice-routing";
export { statusCommand } from "./cli/status";
export { statsCommand } from "./cli/stats";
export { supportBundleCommand } from "./cli/support-bundle";
export {
  mainCommand,
  detectLanguage,
  checkLanguageMismatch,
  estimateTranscriptDurationSeconds,
  resolveOutputFormat,
  shouldRunAudioLanguageDetection,
  shouldReportTranscribeProgress,
} from "./cli/main";
export type { ResolvedOutputFormat } from "./cli/main";
export { runCli } from "./cli/dispatch";

export type {
  TranscribeErrorRecord,
  TranscribeJsonOutput,
  TranscribeResult,
} from "./types";
export {
  formatJsonOutput,
  formatTextOutput,
  formatTranscriptOutput,
  formatVerboseOutput,
} from "./format";
export { formatToonOutput } from "./toon";
export {
  collectDoctorReport,
  formatDoctorReport,
  redactDiagnosticValue,
} from "./doctor";
export type { DoctorReport } from "./doctor";
export { createSupportBundle } from "./support-bundle";
export type { SupportBundleResult } from "./support-bundle";
export { renderInstallPlan } from "./install-plan";
export { keshaCacheDir } from "./paths";

import { runCli } from "./cli/dispatch";

if (import.meta.main) {
  await runCli();
}
