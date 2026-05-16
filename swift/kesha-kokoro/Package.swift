// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "kesha-kokoro",
    platforms: [.macOS(.v14)],
    dependencies: [
        .package(
            url: "https://github.com/FluidInference/FluidAudio.git",
            exact: "0.14.5"
        ),
    ],
    targets: [
        .executableTarget(
            name: "kesha-kokoro",
            dependencies: ["FluidAudio"]
        )
    ]
)
