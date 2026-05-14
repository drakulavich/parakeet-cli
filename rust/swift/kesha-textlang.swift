// CLI helper for kesha-engine: text on stdin → JSON `{code, confidence}` on stdout.
//
// Replaces a per-call `swift -e <inline-code>` shell-out that was paying the
// Swift JIT compiler startup tax on every `kesha detect-text-lang ...` (200 ms
// warm, up to 35 s cold on macOS 15+ Sequoia after the toolchain cache evicts).
// Precompiled, this completes in ~30-50 ms (binary startup + NaturalLanguage
// framework load) regardless of Xcode state.
//
// Usage:
//   echo "Hello, world" | kesha-textlang
//
// Output:
//   {"code":"en","confidence":0.9998549}
//
// Exit codes: 0 success, 1 empty stdin, 2 internal error (recognition failed,
// JSON marshal failed, etc.).
//
// Invoked by `rust/src/text_lang.rs::detect_text_language` via the same
// sidecar-resolution pattern as say-avspeech and kesha-diarize: sibling-of-exe
// first, then build-time `$OUT_DIR/kesha-textlang` baked by build.rs.

import Foundation
import NaturalLanguage

// Read all of stdin. Language detection wants the whole sample — short snippets
// reduce `NLLanguageRecognizer` confidence noticeably.
let data = FileHandle.standardInput.readDataToEndOfFile()
guard let text = String(data: data, encoding: .utf8), !text.isEmpty else {
  FileHandle.standardError.write("kesha-textlang: empty stdin\n".data(using: .utf8)!)
  exit(1)
}

let recognizer = NLLanguageRecognizer()
recognizer.processString(text)

var code = ""
var confidence: Double = 0.0
if let dominant = recognizer.dominantLanguage {
  code = dominant.rawValue
  confidence = recognizer.languageHypotheses(withMaximum: 1)[dominant] ?? 0.0
}

let payload: [String: Any] = ["code": code, "confidence": confidence]
guard let json = try? JSONSerialization.data(withJSONObject: payload, options: [.sortedKeys])
else {
  FileHandle.standardError.write("kesha-textlang: JSON marshal failed\n".data(using: .utf8)!)
  exit(2)
}
FileHandle.standardOutput.write(json)
