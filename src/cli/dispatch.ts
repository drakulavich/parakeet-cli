import { runMain } from "citty";
import { existsSync } from "fs";
import { log } from "../log";
import { suggestCommand } from "../suggest-command";
import { installCommand } from "./install";
import { sayCommand } from "./say";
import { statusCommand } from "./status";
import { mainCommand } from "./main";

const SUBCOMMANDS = ["install", "status", "say"];

function isPathLike(arg: string): boolean {
  return arg.includes(".") || arg.includes("/") || existsSync(arg);
}

export async function runCli(rawArgs = process.argv.slice(2)): Promise<void> {
  const [firstArg, ...restArgs] = rawArgs;

  if (firstArg === "install") {
    await runMain(installCommand, { rawArgs: restArgs });
    return;
  }

  if (firstArg === "status") {
    await runMain(statusCommand, { rawArgs: restArgs });
    return;
  }

  if (firstArg === "say") {
    await runMain(sayCommand, { rawArgs: restArgs });
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
