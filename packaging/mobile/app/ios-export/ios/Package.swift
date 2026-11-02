// swift-tools-version:5.7
import PackageDescription

let package = Package(
    name: "tauri-plugin-pdf-export",
    platforms: [.macOS(.v10_13), .iOS(.v16)],
    products: [.library(name: "tauri-plugin-pdf-export", type: .static, targets: ["PdfExport"])],
    dependencies: [.package(name: "Tauri", path: "../.tauri/tauri-api")],
    targets: [.target(name: "PdfExport", dependencies: ["Tauri"], path: "Sources")]
)
