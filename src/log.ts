import pc from "picocolors";

/**
 * Debug mode (#148): when `KESHA_DEBUG` is truthy OR the caller has flipped
 * `log.debugEnabled = true` (via `--debug`), `log.debug()` writes structured
 * trace lines to stderr. Otherwise it's a no-op. Stdout is never touched.
 *
 * Grammar (#275 D9): values that turn debug OFF — empty, `"0"`, `"false"`,
 * `"no"`, `"off"`, all matched **case-insensitively** after trimming. Any
 * other non-empty value turns debug ON. The Rust engine mirrors this list
 * verbatim in `rust/src/debug.rs` so `KESHA_DEBUG=False` flips both sides
 * the same direction.
 */
const KESHA_DEBUG_OFF_VALUES = new Set(["", "0", "false", "no", "off"]);

function envDebug(): boolean {
  const v = process.env.KESHA_DEBUG;
  if (v === undefined) return false;
  return !KESHA_DEBUG_OFF_VALUES.has(v.trim().toLowerCase());
}

/**
 * Module-load timestamp for relative-since-start prefixes on debug lines.
 * The CLI runs nothing of substance before this file is imported, so this
 * is effectively process-start. Recorded once.
 */
const PROCESS_T0_MS = performance.now();

export const log = {
  info: (msg: string) => console.log(msg),
  success: (msg: string) => console.log(pc.green(msg)),
  progress: (msg: string) => console.log(pc.cyan(msg)),
  warn: (msg: string) => console.error(pc.yellow(msg)),
  error: (msg: string) => console.error(pc.red(msg)),

  debugEnabled: false,
  debug(msg: string): void {
    if (this.debugEnabled || envDebug()) {
      // `[debug +Nms]` prefix sits on the CLI process's own timeline so
      // the reader can see when each line fired. The Rust engine emits
      // the same `+Nms` shape from `rust/src/debug.rs::trace_fmt`, but
      // anchored to its own process start — the two axes are
      // independent. For "duration between two events on the same
      // process", read the prefix difference; for cross-process spans,
      // the spawn→exit `dt=Nms` inside the message remains authoritative.
      const t = Math.round(performance.now() - PROCESS_T0_MS);
      console.error(pc.dim(`[debug +${t}ms] ${msg}`));
    }
  },
};
