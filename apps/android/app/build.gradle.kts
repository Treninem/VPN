plugins {
    id("com.android.application")
}

android {
    namespace = "ru.amri.vpn"
    compileSdk = 37

    defaultConfig {
        applicationId = "ru.amri.vpn"
        minSdk = 26
        targetSdk = 37
        versionCode = 1
        versionName = "0.1.0"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}
