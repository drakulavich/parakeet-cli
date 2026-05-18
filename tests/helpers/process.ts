import { existsSync, readFileSync } from "fs";

const PID_FILE_POLL_INTERVAL_MS = 25;
const PID_FILE_POLL_ATTEMPTS = 80;
const PID_EXIT_POLL_ATTEMPTS = 120;

export async function waitForPidFile(path: string): Promise<number> {
  for (let i = 0; i < PID_FILE_POLL_ATTEMPTS; i++) {
    if (existsSync(path)) return Number(readFileSync(path, "utf8"));
    await Bun.sleep(PID_FILE_POLL_INTERVAL_MS);
  }
  throw new Error(`timed out waiting for pid file: ${path}`);
}

export function pidIsAlive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

export async function waitForPidExit(pid: number): Promise<boolean> {
  for (let i = 0; i < PID_EXIT_POLL_ATTEMPTS; i++) {
    if (!pidIsAlive(pid)) return true;
    await Bun.sleep(PID_FILE_POLL_INTERVAL_MS);
  }
  return false;
}
