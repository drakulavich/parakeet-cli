import { runMain } from "citty";
import { existsSync } from "fs";
import { log } from "../log";
import { suggestCommand } from "../suggest-command";
import { completionsCommand } from "./completions";
import { doctorCommand } from "./doctor";
import { installCommand } from "./install";
import { manpageCommand } from "./manpage";
import { recordCommand } from "./record";
import { sayCommand } from "./say";
import { statsCommand } from "./stats";
import { statusCommand } from "./status";
import { supportBundleCommand } from "./support-bundle";
import { mainCommand } from "./main";

const SUBCOMMANDS = [
  "doctor",
  "install",
  "status",
  "record",
  "say",
  "stats",
  "support-bundle",
  "completions",
  "manpage",
];

function isPathLike(arg: string): boolean {
  return arg.includes(".") || arg.includes("/") || existsSync(arg);
}

export async function runCli(rawArgs = process.argv.slice(2)): Promise<void> {
  const [firstArg, ...restArgs] = rawArgs;

  if (firstArg === "doctor") {
    await runMain(doctorCommand, { rawArgs: restArgs });
    return;
  }

  if (firstArg === "completions") {
    await runMain(completionsCommand, { rawArgs: restArgs });
    return;
  }

  if (firstArg === "install") {
    await runMain(installCommand, { rawArgs: restArgs });
    return;
  }

  if (firstArg === "manpage") {
    await runMain(manpageCommand, { rawArgs: restArgs });
    return;
  }

  if (firstArg === "status") {
    await runMain(statusCommand, { rawArgs: restArgs });
    return;
  }

  if (firstArg === "record") {
    await runMain(recordCommand, { rawArgs: restArgs });
    return;
  }

  if (firstArg === "say") {
    await runMain(sayCommand, { rawArgs: restArgs });
    return;
  }

  if (firstArg === "stats") {
    await runMain(statsCommand, { rawArgs: restArgs });
    return;
  }

  if (firstArg === "support-bundle") {
    await runMain(supportBundleCommand, { rawArgs: restArgs });
    return;
  }

  // Check for unknown subcommands (non-flag, non-file-path args).
  // Extensionless existing files remain valid transcription inputs; missing
  // bare tokens are more likely command typos and should not start the engine.
  if (firstArg && !firstArg.startsWith("-") && !isPathLike(firstArg)) {
    const suggestion = suggestCommand(firstArg, SUBCOMMANDS);
    log.error(`unknown command '${firstArg}'`);
    if (suggestion && suggestion !== firstArg) {
      log.warn(`(Did you mean ${suggestion}?)`);
    }
    log.warn(`If this is an audio file, pass a path like './${firstArg}'.`);
    process.exit(1);
  }

  await runMain(mainCommand, { rawArgs });
}
