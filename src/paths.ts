import { homedir } from "os";
import { join } from "path";

export function keshaCacheDir(): string {
  return process.env.KESHA_CACHE_DIR ?? join(homedir(), ".cache", "kesha");
}

export function defaultEngineBinPath(): string {
  return join(keshaCacheDir(), "engine", "bin", "kesha-engine");
}
