import { defineCommand } from "citty";
import { log } from "../log";
import {
  disableStats,
  enableStats,
  getRecentErrors,
  getStatsStatus,
  getWeekSummary,
  renderErrors,
  renderWeekSummary,
} from "../stats";

interface StatsCommandArgs {
  action?: string;
}

export const statsCommand = defineCommand({
  meta: {
    name: "stats",
    description: "Manage local anonymous Kesha Stats",
  },
  args: {
    action: {
      type: "positional",
      required: false,
      description: "Action: enable | disable | status | week | errors",
    },
  },
  run({ args }: { args: StatsCommandArgs }) {
    const action = args.action ?? "status";
    switch (action) {
      case "enable": {
        enableStats();
        const status = getStatsStatus();
        log.success("Kesha Stats enabled");
        log.info(`Database: ${status.dbPath}`);
        return;
      }
      case "disable": {
        disableStats();
        const status = getStatsStatus();
        log.info("Kesha Stats disabled");
        log.info(`Database: ${status.dbPath}`);
        return;
      }
      case "status": {
        const status = getStatsStatus();
        log.info(`Kesha Stats: ${status.enabled ? "enabled" : "disabled"}`);
        log.info(`Database: ${status.dbPath}`);
        log.info(`Runs: ${status.runCount}`);
        return;
      }
      case "week": {
        log.info(renderWeekSummary(getWeekSummary()));
        return;
      }
      case "errors": {
        log.info(renderErrors(getRecentErrors()));
        return;
      }
      default:
        log.error(`unknown stats action '${action}'`);
        log.warn("supported: enable, disable, status, week, errors");
        process.exit(2);
    }
  },
});
