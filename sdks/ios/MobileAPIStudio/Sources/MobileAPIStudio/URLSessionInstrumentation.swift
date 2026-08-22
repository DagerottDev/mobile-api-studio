import Foundation

public final class MobileAPIStudioURLProtocol: URLProtocol, @unchecked Sendable {
    private static let handledKey = "dev.mobileapistudio.urlprotocol.handled"
    private var task: URLSessionDataTask?
    private var requestID: String?

    public override class func canInit(with request: URLRequest) -> Bool {
        guard MobileAPIStudio.isEnabled,
              let url = request.url,
              let scheme = url.scheme?.lowercased(),
              scheme == "http" || scheme == "https",
              URLProtocol.property(forKey: handledKey, in: request) == nil else {
            return false
        }
        if url.host == "127.0.0.1" && url.port == 8182 {
            return false
        }
        return true
    }

    public override class func canonicalRequest(for request: URLRequest) -> URLRequest {
        request
    }

    public override func startLoading() {
        let mutable = (request as NSURLRequest).mutableCopy() as! NSMutableURLRequest
        URLProtocol.setProperty(true, forKey: Self.handledKey, in: mutable)
        var forwarded = mutable as URLRequest

        if let instrumented = MobileAPIStudio.autoInstrument(forwarded) {
            requestID = instrumented.requestID
            forwarded = instrumented.request
            let marked = (forwarded as NSURLRequest).mutableCopy() as! NSMutableURLRequest
            URLProtocol.setProperty(true, forKey: Self.handledKey, in: marked)
            forwarded = marked as URLRequest
        }

        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = (configuration.protocolClasses ?? []).filter {
            $0 != MobileAPIStudioURLProtocol.self
        }
        let session = URLSession(configuration: configuration)
        task = session.dataTask(with: forwarded) { [weak self] data, response, error in
            guard let self else { return }
            if let requestID = self.requestID {
                MobileAPIStudio.complete(requestID: requestID, response: response, error: error)
            }
            if let error {
                self.client?.urlProtocol(self, didFailWithError: error)
                return
            }
            if let response {
                self.client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
            }
            if let data, !data.isEmpty {
                self.client?.urlProtocol(self, didLoad: data)
            }
            self.client?.urlProtocolDidFinishLoading(self)
        }
        task?.resume()
    }

    public override func stopLoading() {
        task?.cancel()
        task = nil
    }
}
