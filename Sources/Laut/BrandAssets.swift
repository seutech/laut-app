import AppKit

enum BrandAssets {
    static let appIcon = load("LautIcon", source: "app-icon")
    static let menuBarMark: NSImage = {
        let image = load("LautMark", source: "mark")
        image.size = NSSize(width: 18, height: 18)
        image.isTemplate = true
        return image
    }()

    private static func load(_ resource: String, source: String) -> NSImage {
        if let url = Bundle.main.url(forResource: resource, withExtension: "png"), let image = NSImage(contentsOf: url) { return image }
        // Also make the artwork available when running the executable from the checkout.
        let url = URL(fileURLWithPath: FileManager.default.currentDirectoryPath).appendingPathComponent("Resources/Branding/\(source).png")
        return NSImage(contentsOf: url) ?? NSImage(systemSymbolName: "text.bubble.fill", accessibilityDescription: "Laut") ?? NSImage(size: NSSize(width: 18, height: 18))
    }
}
