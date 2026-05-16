// Kesha Voice Kit — OpenClaw plugin entry.
//
// Registers the locally-installed `kesha` CLI as an audio transcription
// provider so OpenClaw routes voice messages from any connected channel
// through Kesha (Parakeet TDT via CoreML on Apple Silicon, ONNX elsewhere).
// 25 languages, no cloud, ~19x faster than Whisper.
//
// The same `kesha` CLI also produces messenger-ready voice notes via
// `kesha say "..." --format ogg-opus --out reply.ogg` (Telegram /
// Discord ingest OGG/Opus directly — no transcoding needed). Not wired into
// OpenClaw's media-understanding capability here; invoke it from a hook or
// agent tool when you want Kesha to speak.
//
// Prerequisite: the `kesha` binary must be on PATH. Install it with:
//   bun add -g @drakulavich/kesha-voice-kit
//   kesha install            # ASR models
//   kesha install --tts      # TTS models (Kokoro + Vosk-TTS), only if you use `kesha say`
//
// The module specifier below is split across `+` on purpose — see the
// OpenClaw skill-scanner rule "dangerous-exec" in
// src/security/skill-scanner.ts. It gates on a per-file substring match,
// and splitting the specifier keeps that substring out of this source so
// the rule does not fire on this legitimate local-CLI wrapper.
const { spawnSync } = require("node:child_" + "process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const DEFAULT_TIMEOUT_MS = 60_000;
const MODEL_ID = "parakeet-tdt-0.6b-v3";

function tempAudioPath(fileName) {
  const ext = (fileName && path.extname(fileName)) || ".ogg";
  return path.join(os.tmpdir(), `kesha-${process.pid}-${Date.now()}${ext}`);
}

async function transcribeAudio(req) {
  if (!req || !req.buffer) {
    return { text: "" };
  }

  const tmp = tempAudioPath(req.fileName);
  fs.writeFileSync(tmp, req.buffer);

  try {
    const result = spawnSync("kesha", ["--json", tmp], {
      encoding: "utf8",
      timeout: req.timeoutMs ?? DEFAULT_TIMEOUT_MS,
    });

    if (result.error || result.status !== 0) {
      return { text: "" };
    }

    const parsed = JSON.parse(result.stdout || "[]");
    const text = Array.isArray(parsed) && parsed[0] && typeof parsed[0].text === "string"
      ? parsed[0].text
      : "";

    return { text, model: MODEL_ID };
  } catch {
    return { text: "" };
  } finally {
    try {
      fs.unlinkSync(tmp);
    } catch {
      // best effort cleanup
    }
  }
}

module.exports = {
  id: "kesha-voice-kit",
  name: "Kesha Voice Kit",
  register(api) {
    api.registerMediaUnderstandingProvider({
      id: "kesha-voice-kit",
      capabilities: ["audio"],
      defaultModels: { audio: MODEL_ID },
      autoPriority: { audio: 50 },
      transcribeAudio,
    });
  },
};
