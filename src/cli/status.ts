import { defineCommand } from "citty";
import { showStatus } from "../status";

interface StatusCommandArgs {
  disk: boolean;
}

export const statusCommand = defineCommand({
  meta: {
    name: "status",
    description: "Show backend installation status",
  },
  args: {
    disk: {
      type: "boolean",
      description: "Include recursive cache disk usage",
      default: false,
    },
  },
  async run({ args }: { args: StatusCommandArgs }) {
    await showStatus({ disk: args.disk });
  },
});
