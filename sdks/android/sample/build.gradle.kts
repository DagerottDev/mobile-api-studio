plugins {
    id("com.android.application")
}

android {
    namespace = "dev.mobileapistudio.sample"
    compileSdk = 37

    defaultConfig {
        applicationId = "dev.mobileapistudio.sample"
        minSdk = 23
        targetSdk = 37
        versionCode = 1
        versionName = "0.1.0"
    }
}

dependencies {
    implementation(project(":mobile-api-studio"))
    implementation("com.squareup.okhttp3:okhttp:5.4.0")
}
