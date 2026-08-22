// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "MobileAPIStudio",
    platforms: [
        .iOS(.v15),
        .macOS(.v12)
    ],
    products: [
        .library(name: "MobileAPIStudio", targets: ["MobileAPIStudio"])
    ],
    targets: [
        .target(
            name: "MobileAPIStudio",
            path: "Sources/MobileAPIStudio"
        )
    ]
)
