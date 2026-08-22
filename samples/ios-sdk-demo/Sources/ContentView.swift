import Foundation
import MobileAPIStudio
import SwiftUI

struct ContentView: View {
    @State private var status = "SDK ready. Tap to make a correlated request."
    @State private var sending = false

    var body: some View {
        VStack(spacing: 18) {
            Text("Mobile API Studio")
                .font(.title2.bold())
            Text(status)
                .font(.body)
                .multilineTextAlignment(.center)
            Button(sending ? "Sending…" : "Send sample request") {
                Task { await sendRequest() }
            }
            .buttonStyle(.borderedProminent)
            .disabled(sending)
        }
        .padding(28)
    }

    @MainActor
    private func sendRequest() async {
        sending = true
        defer { sending = false }

        MobileAPIStudio.setContext(
            screen: "Sample Home",
            feature: "Load Demo API",
            attributes: ["trigger": "button"]
        )
        status = "Sending…"

        do {
            let configuration = MobileAPIStudio.instrument(URLSessionConfiguration.default)
            let session = URLSession(configuration: configuration)
            let url = URL(string: "https://httpbin.org/anything/mobile-api-studio")!
            let (_, response) = try await session.data(from: url)
            let code = (response as? HTTPURLResponse)?.statusCode ?? 0
            MobileAPIStudio.log(
                "Sample request completed",
                attributes: ["status": String(code)]
            )
            status = "Completed with HTTP \(code). Open Traffic → App context."
        } catch {
            MobileAPIStudio.log("Sample request failed: \(error)", level: .error)
            status = "Failed: \(error.localizedDescription)"
        }
    }
}
