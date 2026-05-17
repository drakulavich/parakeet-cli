#!/usr/bin/env bun

// Thin re-export shim. The CLI subcommands live in ./cli/*.ts since #180;
// tests and lib.ts keep importing from "./cli" via these re-exports so the
// public surface is unchanged.
export { doctorCommand } from "./cli/doctor";
export { installCommand } from "./cli/install";
export { sayCommand } from "./cli/say";
export { pickVoiceForLang } from "./voice-routing";
export { statusCommand } from "./cli/status";
export { statsCommand } from "./cli/stats";
export {
  mainCommand,
  detectLanguage,
  checkLanguageMismatch,
  resolveOutputFormat,
} from "./cli/main";
export type { ResolvedOutputFormat } from "./cli/main";
export { runCli } from "./cli/dispatch";

export type { TranscribeResult } from "./types";
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
export { keshaCacheDir } from "./paths";

import { runCli } from "./cli/dispatch";

if (import.meta.main) {
  await runCli();
}
