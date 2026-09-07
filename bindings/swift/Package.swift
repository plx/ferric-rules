// swift-tools-version: 6.0
import PackageDescription

let package = Package(
  name: "Ferric",
  platforms: [.macOS(.v15), .iOS(.v18)],
  products: [.library(name: "Ferric", targets: ["Ferric"])],
  targets: [
    .binaryTarget(name: "CFerric", path: "Artifacts/CFerric.xcframework"),
    .target(
      name: "Ferric",
      dependencies: ["CFerric"],
      linkerSettings: [.linkedFramework("Security"), .linkedFramework("CoreFoundation")]
    ),
    .testTarget(name: "FerricTests", dependencies: ["Ferric"], resources: [.copy("Fixtures")]),
  ],
  swiftLanguageModes: [.v6]
)
