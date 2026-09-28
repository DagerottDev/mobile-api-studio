import Foundation

internal final class MobileAPIStudioTransport: @unchecked Sendable {
    private let session: URLSession
    private var endpoint: URL
    private let pairingToken: String?

    init(baseURL: URL, pairingToken: String?) {
        self.endpoint = baseURL.appendingPathComponent("v1/events")
        self.pairingToken = pairingToken
        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = []
        configuration.connectionProxyDictionary = [:]
        configuration.timeoutIntervalForRequest = 2
        configuration.timeoutIntervalForResource = 3
        self.session = URLSession(configuration: configuration)
    }

    func send(type: String, payload: [String: Any]) {
        let envelope: [String: Any] = [
            "schemaVersion": 1,
            "eventId": UUID().uuidString.lowercased(),
            "occurredAt": String(Int(Date().timeIntervalSince1970 * 1000)),
            "event": [
                "type": type,
                "payload": payload
            ]
        ]
        guard JSONSerialization.isValidJSONObject(envelope),
              let body = try? JSONSerialization.data(withJSONObject: envelope),
              endpoint.scheme == "http" else {
            return
        }

        var request = URLRequest(url: endpoint)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        if let pairingToken { request.setValue(pairingToken, forHTTPHeaderField: "X-MAS-Pairing-Token") }
        request.setValue(String(body.count), forHTTPHeaderField: "Content-Length")
        request.httpBody = body
        session.dataTask(with: request).resume()
    }
}
