import { getEngineBinPath, isEngineInstalled, getEngineCapabilities, type EngineCapabilities } from "./engine";
import { log } from "./log";

/**
 * Wire format for the synthesized audio. Matches the engine's `--format` flag.
 * - `wav` (default): RIFF WAV at the engine's native sample rate.
 * - `ogg-opus`: OGG-encapsulated Opus, mono. The format Telegram, WhatsApp,
 *   Signal, and Discord render as native voice messages. See #223.
 */
export type SayFormat = "wav" | "ogg-opus";

export interface SayOptions {
  /**
   * Text to synthesize. Required for programmatic callers — `say()` does not
   * forward the host process's stdin. The CLI (`kesha say` with no positional
   * arg) handles stdin separately before invoking `say()`.
   */
  text?: string;
  /** Voice id, e.g. `en-am_michael`. Defaults to engine default. */
  voice?: string;
  /** Override the voice's default BCP 47 language code (e.g. `en-us`, `ru`). */
  lang?: string;
  /** Write audio to this path instead of returning bytes. */
  out?: string;
  /** Speaking rate 0.5–2.0. */
  rate?: number;
  /** Parse `text` as SSML (`<speak>…<break time="500ms"/>…</speak>`). See issue #122. */
  ssml?: boolean;
  /**
   * Output audio format. Defaults to `wav` (or inferred from the `out`
   * extension when omitted: `.wav` → wav, `.ogg`/`.opus` → ogg-opus).
   */
  format?: SayFormat;
  /** Opus bitrate in bits/second. Only valid with `format: "ogg-opus"`. Default 32000. */
  bitrate?: number;
  /**
   * Encoder sample rate in Hz. Only valid with `format: "ogg-opus"`.
   * Must be one of 8000, 12000, 16000, 24000, 48000. Default 24000.
   */
  sampleRate?: number;
  /**
   * Disable acronym auto-expansion for `ru-vosk-*` and `en-*` voices.
   * When true, passes `--no-expand-abbrev` to the engine (requires engine
   * capability `tts.ru_acronym_expansion` or `tts.en_acronym_expansion`).
   * On older engines that don't advertise the capability, the flag is
   * dropped from argv and `log.warn` surfaces the drop on every
   * invocation (post-#275 D3). `<say-as interpret-as="characters">`
   * still works regardless of this flag.
   */
  noExpandAbbrev?: boolean;
}

/** Build the argv passed to `kesha-engine say` (pure, unit-testable). */
export function buildSayArgs(o: SayOptions, capabilities?: EngineCapabilities | null): string[] {
  const args: string[] = ["say"];
  if (o.voice) args.push("--voice", o.voice);
  if (o.lang) args.push("--lang", o.lang);
  if (o.out) args.push("--out", o.out);
  if (o.rate !== undefined && o.rate !== 1.0) args.push("--rate", String(o.rate));
  if (o.ssml) args.push("--ssml");
  if (o.format) args.push("--format", o.format);
  if (o.bitrate !== undefined) args.push("--bitrate", String(o.bitrate));
  if (o.sampleRate !== undefined) args.push("--sample-rate", String(o.sampleRate));
  if (o.noExpandAbbrev) {
    const supportsExpand = capabilities?.features?.some(
      (f) => f === "tts.ru_acronym_expansion" || f === "tts.en_acronym_expansion",
    ) ?? false;
    if (supportsExpand) {
      args.push("--no-expand-abbrev");
    } else {
      // CLAUDE.md "NEVER SWALLOW ERRORS": the user explicitly passed the flag.
      // Silent drop with only `log.debug` made the flag look effective on old
      // engines (#275 D3). Surface it as a warning so a CI script or human
      // user sees the mismatch on every invocation, not only with --debug.
      log.warn(
        "--no-expand-abbrev requires kesha-engine ≥ 1.10.0 (advertises no tts.ru_acronym_expansion / tts.en_acronym_expansion capability); flag ignored",
      );
    }
  }
  if (o.text !== undefined && o.text.length > 0) args.push(o.text);
  return args;
}

export class SayError extends Error {
  constructor(
    message: string,
    public readonly exitCode: number,
    public readonly stderr: string,
  ) {
    super(message);
    this.name = "SayError";
  }
}

/**
 * Synthesize speech. Returns raw WAV bytes. If `out` is provided in options,
 * the engine writes to the file and this function returns an empty buffer.
 */
export async function say(opts: SayOptions): Promise<Uint8Array> {
  if (!isEngineInstalled()) {
    throw new SayError(
      "kesha-engine not installed. run: kesha install",
      1,
      "",
    );
  }
  const capabilities = opts.noExpandAbbrev ? await getEngineCapabilities() : null;
  const args = buildSayArgs({ ...opts, text: undefined }, capabilities);
  const startedAt = performance.now();
  log.debug(`spawn ${getEngineBinPath()} ${args.join(" ")} (text: ${opts.text?.length ?? 0} chars)`);
  const proc = Bun.spawn([getEngineBinPath(), ...args], {
    stdin: "pipe",
    stdout: "pipe",
    stderr: "pipe",
  });

  if (opts.text !== undefined && opts.text.length > 0) {
    proc.stdin.write(opts.text);
    await proc.stdin.end();
  } else {
    await proc.stdin.end();
  }

  const [stdoutBuf, stderrText, exitCode] = await Promise.all([
    new Response(proc.stdout).arrayBuffer(),
    new Response(proc.stderr).text(),
    proc.exited,
  ]);

  log.debug(`exit=${exitCode} dt=${Math.round(performance.now() - startedAt)}ms bytes=${stdoutBuf.byteLength}`);

  // #275 D4: surface engine stderr on the success path so warnings like
  // `Model mirror active:` and the dtrace lines emitted under
  // KESHA_DEBUG=1 reach the user. Errors keep their existing path
  // through `SayError.stderr` so we don't double-print.
  if (exitCode === 0 && stderrText.length > 0) {
    process.stderr.write(stderrText.endsWith("\n") ? stderrText : stderrText + "\n");
  }
  if (exitCode !== 0) {
    throw new SayError(
      stderrText.trim() || `kesha-engine say exited ${exitCode}`,
      exitCode,
      stderrText,
    );
  }
  return new Uint8Array(stdoutBuf);
}
