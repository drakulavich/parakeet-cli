import pkg from "../package.json" with { type: "json" };

export const packageName = typeof pkg.name === "string" ? pkg.name : "unknown";
export const packageVersion = typeof pkg.version === "string" ? pkg.version : "unknown";
export const engineVersion =
  typeof pkg.keshaEngine?.version === "string" ? pkg.keshaEngine.version : packageVersion;
