// swift-tools-version:5.9
// The iOS app's logic that is not interface: server API, session, sync,
// storage, autofill matching. Shared by the app and its AutoFill extension,
// and tested with `swift test` on macOS against a real amanfold-server.
//
// Run ../scripts/build-core.sh first: it builds the Rust core into
// Frameworks/AmanfoldFFI.xcframework and generates Sources/Amanfold.
import PackageDescription

let package = Package(
    name: "AmanfoldKit",
    defaultLocalization: "en",
    platforms: [.iOS(.v17), .macOS(.v14)],
    products: [
        .library(name: "AmanfoldKit", targets: ["AmanfoldKit"]),
    ],
    targets: [
        .binaryTarget(name: "AmanfoldFFI", path: "Frameworks/AmanfoldFFI.xcframework"),
        // Generated UniFFI bindings of the Rust core.
        .target(
            name: "Amanfold",
            dependencies: ["AmanfoldFFI"],
            path: "Sources/Amanfold",
            linkerSettings: [.linkedFramework("Security")]
        ),
        .target(
            name: "AmanfoldKit",
            dependencies: ["Amanfold"],
            path: "Sources/AmanfoldKit",
            resources: [.process("Resources")]
        ),
        .testTarget(name: "AmanfoldKitTests", dependencies: ["AmanfoldKit"], path: "Tests/AmanfoldKitTests"),
    ]
)
