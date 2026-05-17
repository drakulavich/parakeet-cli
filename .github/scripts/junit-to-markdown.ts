#!/usr/bin/env bun
/**
 * Convert JUnit XML test results to a markdown table.
 * Usage: bun junit-to-markdown.ts [--summary] <title> <xml...>
 */

import { XMLParser } from "fast-xml-parser";

interface TestResult {
  suite: string;
  name: string;
  status: "passed" | "failed" | "skipped";
  timeMs: number;
  message: string;
  file: string;
  line: string;
}

function escapeMd(text: string): string {
  return text.replace(/\|/g, "\\|");
}

export async function parseJunit(path: string): Promise<TestResult[]> {
  try {
    const xml = await Bun.file(path).text();
    const parser = new XMLParser({ ignoreAttributes: false, attributeNamePrefix: "@_" });
    const doc = parser.parse(xml);

    const results: TestResult[] = [];

    function collect(node: any): void {
      if (!node) return;
      const items = Array.isArray(node) ? node : [node];
      for (const item of items) {
        // Process testcases at this level
        if (item.testcase) {
          const cases = Array.isArray(item.testcase) ? item.testcase : [item.testcase];
          for (const tc of cases) {
            const name = tc["@_name"] ?? "unknown";
            const suiteName = tc["@_classname"] ?? item["@_name"] ?? "";
            const timeS = parseFloat(tc["@_time"] ?? "0") || 0;
            const file = tc["@_file"] ?? item["@_file"] ?? "";
            const line = tc["@_line"] ?? "";

            let status: TestResult["status"] = "passed";
            let message = "";
            if (tc.failure) {
              status = "failed";
              const msg = typeof tc.failure === "string" ? tc.failure : tc.failure["@_message"] ?? "";
              message = msg.slice(0, 100);
            } else if (tc.skipped !== undefined) {
              status = "skipped";
            }

            results.push({ suite: suiteName, name, status, timeMs: timeS * 1000, message, file, line });
          }
        }
        // Recurse into nested testsuites
        if (item.testsuite) collect(item.testsuite);
      }
    }

    // Start from root — could be testsuites, testsuite, or bare testcases
    if (doc.testsuites) collect(doc.testsuites.testsuite ?? doc.testsuites);
    else if (doc.testsuite) collect(doc.testsuite);
    else collect(doc);

    return results;
  } catch (e) {
    console.error(`Warning: Could not parse ${path}: ${e}`);
    return [];
  }
}

export function toMarkdown(title: string, results: TestResult[], summaryOnly: boolean): string {
  const passed = results.filter(r => r.status === "passed").length;
  const failed = results.filter(r => r.status === "failed").length;
  const skipped = results.filter(r => r.status === "skipped").length;
  const total = results.length;
  const icon = failed === 0 ? "✅" : "❌";
  const statusIcons = { passed: "✅", failed: "❌", skipped: "⏭️" } as const;

  if (summaryOnly) {
    const totalTime = results.reduce((s, r) => s + r.timeMs, 0);
    const timeStr = totalTime >= 1000 ? `${(totalTime / 1000).toFixed(1)}s` : `${Math.round(totalTime)}ms`;
    return `| ${title} | ${icon} **${passed}** passed · ${failed} failed · ${skipped} skipped | ${total} | ${timeStr} |`;
  }

  const lines: string[] = [
    `### ${title}`,
    "",
    `**${passed} passed** · ${failed} failed · ${skipped} skipped · ${total} total`,
    "",
  ];

  if (!results.length) {
    lines.push("_No test results found._");
    return lines.join("\n");
  }

  const failures = results.filter(r => r.status === "failed");
  const skippedResults = results.filter(r => r.status === "skipped");
  const baseShow = failed > 0 && total > 20 ? failures : results;
  const show = (skippedResults.length > 0 ? baseShow.filter(r => r.status !== "skipped") : baseShow)
    .toSorted((a, b) => b.timeMs - a.timeMs);

  if (show.length > 0) {
    lines.push("| Suite | Test | Status | Time |", "|-------|------|--------|------|");
    for (const r of show) {
      const time = `${Math.round(r.timeMs)}ms`;
      let name = escapeMd(r.name);
      if (r.message) name = `${name} — ${escapeMd(r.message)}`;
      lines.push(`| ${escapeMd(r.suite)} | ${name} | ${statusIcons[r.status]} | ${time} |`);
    }
  }

  if (skippedResults.length > 0) {
    lines.push("", "#### Skipped Tests", "", "| Suite | Test | Location |", "|-------|------|----------|");
    for (const r of skippedResults) {
      const location = r.file ? `${r.file}${r.line ? `:${r.line}` : ""}` : "";
      lines.push(`| ${escapeMd(r.suite)} | ${escapeMd(r.name)} | ${escapeMd(location)} |`);
    }
  }
  return lines.join("\n");
}

// CLI
const summaryOnly = process.argv.includes("--summary");
const args = process.argv.slice(2).filter(a => !a.startsWith("--"));

if (args.length < 2) {
  console.error("Usage: bun junit-to-markdown.ts [--summary] <title> <xml...>");
  process.exit(1);
}

const [title, ...xmlPaths] = args;
const results = (await Promise.all(xmlPaths.map(parseJunit))).flat();
console.log(toMarkdown(title, results, summaryOnly));
