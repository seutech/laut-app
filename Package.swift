// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "Laut",
    platforms: [.macOS(.v14)],
    products: [.executable(name: "Laut", targets: ["Laut"])],
    dependencies: [.package(url: "https://github.com/FluidInference/FluidAudio.git", exact: "0.17.5", traits: [])],
    targets: [
        .systemLibrary(name: "CSQLite"),
        .target(name: "LautCore", dependencies: ["CSQLite"], resources: [.process("Localization")]),
        .target(name: "LautAudio", dependencies: ["LautCore", .product(name: "FluidAudio", package: "FluidAudio")]),
        .executableTarget(name: "Laut", dependencies: ["LautCore", "LautAudio"]),
        .executableTarget(name: "LautCoreChecks", dependencies: ["LautCore"], path: "Tests/LautCoreTests"),
        .executableTarget(name: "LautPortableInterop", dependencies: ["LautCore"], path: "Tests/PortableInterop"),
        .executableTarget(name: "LautDiagnostics", dependencies: ["LautCore", "LautAudio"], path: "Tests/Diagnostics")
    ],
    swiftLanguageModes: [.v5]
)
