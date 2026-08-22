import Foundation

public struct MobileAPIStudioConfiguration: Sendable {
    public var appID: String
    public var appName: String
    public var appVersion: String?
    public var appBuild: String?
    public var desktopBaseURL: URL
    public var enabled: Bool

    public init(
        appID: String = Bundle.main.bundleIdentifier ?? "unknown.app",
        appName: String = Bundle.main.object(forInfoDictionaryKey: "CFBundleDisplayName") as? String
            ?? Bundle.main.object(forInfoDictionaryKey: "CFBundleName") as? String
            ?? "iOS App",
        appVersion: String? = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String,
        appBuild: String? = Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String,
        desktopBaseURL: URL = URL(string: "http://127.0.0.1:8182")!,
        enabled: Bool = MobileAPIStudio.defaultEnabled
    ) {
        self.appID = appID
        self.appName = appName
        self.appVersion = appVersion
        self.appBuild = appBuild
        self.desktopBaseURL = desktopBaseURL
        self.enabled = enabled
    }
}

public struct MobileAPIStudioSource: Sendable {
    public var file: String?
    public var function: String?
    public var line: UInt?

    public init(file: String? = nil, function: String? = nil, line: UInt? = nil) {
        self.file = file
        self.function = function
        self.line = line
    }

    internal var wireValue: [String: Any] {
        var value: [String: Any] = [:]
        if let file { value["file"] = file }
        if let function { value["function"] = function }
        if let line { value["line"] = line }
        return value
    }
}

public struct MobileAPIStudioContext: Sendable {
    public var screen: String?
    public var feature: String?
    public var attributes: [String: String]
    public var source: MobileAPIStudioSource?

    public init(
        screen: String? = nil,
        feature: String? = nil,
        attributes: [String: String] = [:],
        source: MobileAPIStudioSource? = nil
    ) {
        self.screen = screen
        self.feature = feature
        self.attributes = attributes
        self.source = source
    }

    internal var wireValue: [String: Any] {
        var value: [String: Any] = ["attributes": attributes]
        if let screen { value["screen"] = screen }
        if let feature { value["feature"] = feature }
        if let source { value["source"] = source.wireValue }
        return value
    }
}

public enum MobileAPIStudioLogLevel: String, Sendable {
    case debug
    case info
    case warning
    case error
}

public struct MobileAPIStudioInstrumentedRequest: Sendable {
    public let requestID: String
    public let request: URLRequest

    public init(requestID: String, request: URLRequest) {
        self.requestID = requestID
        self.request = request
    }
}
