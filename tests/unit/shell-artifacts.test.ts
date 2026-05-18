import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { generateShellArtifacts } from "../../src/shell-artifacts";

function normalizeLineEndings(value: string): string {
  return value.replace(/\r\n/g, "\n");
}

describe("shell artifacts (#344 P2)", () => {
  test("generated completions and manpage match checked-in package files", async () => {
    const artifacts = await generateShellArtifacts();
    expect(artifacts.map((artifact) => artifact.path).sort()).toEqual([
      "completions/kesha.bash",
      "completions/kesha.fish",
      "completions/kesha.zsh",
      "man/kesha.1",
    ]);

    for (const artifact of artifacts) {
      expect(normalizeLineEndings(readFileSync(artifact.path, "utf8"))).toBe(artifact.content);
    }
  });

  test("completions cover the command and installer commands", async () => {
    const bash = readFileSync("completions/kesha.bash", "utf8");
    expect(bash).toContain("complete -F _kesha_completion kesha");
    expect(bash).toContain("completions doctor install manpage record say stats status support-bundle");

    const fish = readFileSync("completions/kesha.fish", "utf8");
    expect(fish).toContain("complete -c kesha");
  });

  test("manpage documents generated completion files", () => {
    const manpage = readFileSync("man/kesha.1", "utf8");
    expect(manpage).toContain(".TH KESHA 1");
    expect(manpage).not.toContain(".BR kesha (1)");
    expect(manpage).toContain("completions/kesha.bash");
    expect(manpage).toContain("completions/kesha.zsh");
    expect(manpage).toContain("completions/kesha.fish");
  });
});
