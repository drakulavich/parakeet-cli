import { defineCommand } from "citty";
import { log } from "../log";
import {
  disableStats,
  enableStats,
  exportStats,
  getRecentErrors,
  getStatsStatus,
  getWeekSummary,
  renderErrors,
  renderWeekSummary,
  resetStats,
  setStatsRetentionDays,
  vacuumStats,
  type StatsExportFormat,
} from "../stats";

interface StatsCommandArgs {
  action?: string;
  value?: string;
  format?: string;
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
      description: "Action: enable | disable | status | week | errors | export | reset | vacuum | retention",
    },
    value: {
      type: "positional",
      required: false,
      description: "Action value: export format or retention days",
    },
    format: {
      type: "string",
      description: "Export format: json | csv",
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
        log.info(`Retention: ${formatRetention(status.retentionDays)}`);
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
      case "export": {
        const format = parseExportFormat(args.format ?? args.value ?? "json");
        if (!format) {
          log.error("usage: kesha stats export --format json|csv");
          process.exit(2);
        }
        process.stdout.write(exportStats(format));
        return;
      }
      case "reset": {
        const result = resetStats();
        log.info(
          `Kesha Stats reset: ${result.runs} run(s), ${result.stageTimings} stage timing(s), ` +
            `${result.artifacts} artifact(s), ${result.errors} error(s) deleted`,
        );
        return;
      }
      case "vacuum": {
        const result = vacuumStats();
        log.info(`Kesha Stats vacuumed: ${result.beforeBytes} -> ${result.afterBytes} bytes`);
        log.info(`Database: ${result.dbPath}`);
        return;
      }
      case "retention": {
        if (!args.value) {
          const status = getStatsStatus();
          log.info(`Kesha Stats retention: ${formatRetention(status.retentionDays)}`);
          return;
        }
        const retention = parseRetention(args.value);
        if (retention === undefined) {
          log.error("usage: kesha stats retention <days|off>");
          process.exit(2);
        }
        setStatsRetentionDays(retention);
        log.info(`Kesha Stats retention set to ${formatRetention(retention)}`);
        return;
      }
      default:
        log.error(`unknown stats action '${action}'`);
        log.warn("supported: enable, disable, status, week, errors, export, reset, vacuum, retention");
        process.exit(2);
    }
  },
});

function parseExportFormat(value: string): StatsExportFormat | null {
  return value === "json" || value === "csv" ? value : null;
}

function parseRetention(value: string): number | null | undefined {
  const normalized = value.trim().toLowerCase();
  if (normalized === "off" || normalized === "none" || normalized === "never") return null;
  const days = Number(normalized);
  if (Number.isInteger(days) && days >= 1) return days;
  return undefined;
}

function formatRetention(days: number | null): string {
  return days === null ? "off" : `${days} day(s)`;
}
