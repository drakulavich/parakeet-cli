import { defineCommand } from "citty";
import { collectDoctorReport, formatDoctorReport } from "../doctor";
import { log } from "../log";

interface DoctorCommandArgs {
  json: boolean;
  redact: boolean;
}

export const doctorCommand = defineCommand({
  meta: {
    name: "doctor",
    description: "Collect support diagnostics without changing local state",
  },
  args: {
    json: {
      type: "boolean",
      description: "Output diagnostics as JSON",
      default: false,
    },
    redact: {
      type: "boolean",
      description: "Redact secrets and user-home paths from diagnostic output",
      default: false,
    },
  },
  async run({ args }: { args: DoctorCommandArgs }) {
    const report = await collectDoctorReport({ redact: args.redact });
    if (args.json) {
      console.log(JSON.stringify(report, null, 2));
      return;
    }
    log.info(formatDoctorReport(report));
  },
});
