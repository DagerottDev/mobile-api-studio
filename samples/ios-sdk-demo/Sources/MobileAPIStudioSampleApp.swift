import MobileAPIStudio
import SwiftUI

@main
struct MobileAPIStudioSampleApp: App {
    init() {
        MobileAPIStudio.configure()
        MobileAPIStudio.setContext(
            screen: "Sample Home",
            feature: "SDK Demo",
            attributes: ["platform": "ios"]
        )
        MobileAPIStudio.log("iOS sample launched")
    }

    var body: some Scene {
        WindowGroup {
            ContentView()
        }
    }
}
