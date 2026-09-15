plugins {
    id("com.android.application")
}

val amriBrandDir = rootProject.layout.projectDirectory.dir("../../assets/brand")
val amriIconSource = amriBrandDir.file("amri-icon.png")
val generatedAmriIconRes = layout.buildDirectory.dir("generated/amri-icon-res")
val generateAmriIconResource by tasks.registering(org.gradle.api.tasks.Copy::class) {
    from(amriIconSource)
    into(generatedAmriIconRes.map { it.dir("drawable-nodpi") })
    rename { "amri_app_icon.png" }
}

val generatedAmriUiRes = layout.buildDirectory.dir("generated/amri-ui-res")
val generateAmriUiResources by tasks.registering(org.gradle.api.tasks.Copy::class) {
    into(generatedAmriUiRes.map { it.dir("raw") })
    from(amriBrandDir.file("background-mobile.svg")) {
        rename { "amri_background_mobile.svg" }
    }
    from(amriBrandDir.file("vpn-power-on.svg")) {
        rename { "amri_vpn_power_on.svg" }
    }
    from(amriBrandDir.file("vpn-power-off.svg")) {
        rename { "amri_vpn_power_off.svg" }
    }
    from(amriBrandDir.file("settings-button.svg")) {
        rename { "amri_settings_button.svg" }
    }
    from(amriBrandDir.file("language-button.svg")) {
        rename { "amri_language_button.svg" }
    }
}

val amriRustWorkspace = rootProject.layout.projectDirectory.dir("../..")
val generatedAmriNativeLibs = layout.buildDirectory.dir("generated/amri-native-jni")
val buildAmriRustNative by tasks.registering(org.gradle.api.tasks.Exec::class) {
    val enabled = providers.environmentVariable("AMRI_BUILD_NATIVE")
        .map { value -> value == "1" || value.equals("true", ignoreCase = true) }
        .orElse(false)

    onlyIf { enabled.get() }
    workingDir(amriRustWorkspace.asFile)

    doFirst {
        val outputDir = generatedAmriNativeLibs.get().asFile
        project.delete(outputDir)
        outputDir.mkdirs()
        commandLine(
            "cargo",
            "ndk",
            "-p",
            "26",
            "-t",
            "arm64-v8a",
            "-t",
            "x86_64",
            "-o",
            outputDir.absolutePath,
            "build",
            "-p",
            "amri-android-ffi",
            "--release",
        )
    }
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

android.sourceSets["main"].res.srcDir(generatedAmriIconRes.get().asFile)
android.sourceSets["main"].res.srcDir(generatedAmriUiRes.get().asFile)
android.sourceSets["main"].jniLibs.srcDir(generatedAmriNativeLibs.get().asFile)

tasks.named("preBuild").configure {
    dependsOn(generateAmriIconResource)
    dependsOn(generateAmriUiResources)
    dependsOn(buildAmriRustNative)
}

dependencies {
    implementation("com.caverock:androidsvg-aar:1.4")
    testImplementation("junit:junit:4.13.2")
}
