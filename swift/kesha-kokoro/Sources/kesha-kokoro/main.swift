import Foundation
import FluidAudio

func usage() -> Never {
    FileHandle.standardError.write(Data(
        "usage: kesha-kokoro [--voice <voice>] [--speed <0.5-2.0>] [text]\n".utf8
    ))
    exit(2)
}

var voice = "am_michael"
var speed: Float = 1.0
var positional: [String] = []
var i = 1
let args = CommandLine.arguments

while i < args.count {
    switch args[i] {
    case "--voice":
        guard i + 1 < args.count else { usage() }
        voice = args[i + 1]
        i += 2
    case "--speed":
        guard i + 1 < args.count, let parsed = Float(args[i + 1]) else { usage() }
        speed = parsed
        i += 2
    case "--help", "-h":
        usage()
    default:
        positional.append(args[i])
        i += 1
    }
}

let text: String
if !positional.isEmpty {
    text = positional.joined(separator: " ")
} else {
    let data = FileHandle.standardInput.readDataToEndOfFile()
    text = String(data: data, encoding: .utf8) ?? ""
}

let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
guard !trimmed.isEmpty else {
    FileHandle.standardError.write(Data("empty text\n".utf8))
    exit(2)
}

let clampedSpeed = min(max(speed, 0.5), 2.0)
let manager = KokoroTtsManager(defaultVoice: voice)

do {
    try await manager.initialize(preloadVoices: [voice])
    let wav = try await manager.synthesize(
        text: trimmed,
        voice: voice,
        voiceSpeed: clampedSpeed
    )
    guard !wav.isEmpty else {
        FileHandle.standardError.write(Data("error: FluidAudio Kokoro returned no audio\n".utf8))
        exit(1)
    }
    FileHandle.standardOutput.write(wav)
} catch {
    FileHandle.standardError.write(Data("error: FluidAudio Kokoro failed: \(error)\n".utf8))
    exit(1)
}
