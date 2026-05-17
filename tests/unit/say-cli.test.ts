import { describe, expect, test } from "bun:test";
import { shouldRejectMissingSayText } from "../../src/cli/say";

describe("say CLI input guard (#324 P1)", () => {
  test("rejects missing text only when stdin is a TTY", () => {
    expect(shouldRejectMissingSayText(undefined, true)).toBe(true);
    expect(shouldRejectMissingSayText("", true)).toBe(true);
  });

  test("allows piped stdin when text is omitted", () => {
    expect(shouldRejectMissingSayText(undefined, false)).toBe(false);
    expect(shouldRejectMissingSayText(undefined, undefined)).toBe(false);
  });

  test("allows explicit positional text even from a TTY", () => {
    expect(shouldRejectMissingSayText("Hello", true)).toBe(false);
  });
});
