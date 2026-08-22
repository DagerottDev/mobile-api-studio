plugins {
    id("com.android.library")
}

android {
    namespace = "dev.mobileapistudio.sdk"
    compileSdk = 37

    defaultConfig {
        minSdk = 23
        consumerProguardFiles("consumer-rules.pro")
    }

    buildFeatures {
        buildConfig = false
    }
}

dependencies {
    api("com.squareup.okhttp3:okhttp:5.4.0")
}
