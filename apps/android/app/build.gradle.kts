plugins {
    id("com.android.application")
}

val amriIconSource = rootProject.layout.projectDirectory.file("../../assets/brand/amri-icon.png")
val generatedAmriIconRes = layout.buildDirectory.dir("generated/amri-icon-res")
val generateAmriIconResource by tasks.registering(org.gradle.api.tasks.Copy::class) {
    from(amriIconSource)
    into(generatedAmriIconRes.map { it.dir("drawable-nodpi") })
    rename { "amri_app_icon.png" }
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

android.sourceSets["main"].res.srcDir(generatedAmriIconRes)

tasks.named("preBuild").configure {
    dependsOn(generateAmriIconResource)
}

dependencies {
    testImplementation("junit:junit:4.13.2")
}
