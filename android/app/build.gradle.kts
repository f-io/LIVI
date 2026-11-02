plugins {
    id("com.android.application")
}

val liviVersion = rootDir.resolve("../native/Cargo.toml").readLines()
    .dropWhile { it != "[workspace.package]" }
    .first { it.startsWith("version = ") }
    .substringAfter('"')
    .substringBefore('"')
val keystore = providers.environmentVariable("LIVI_ANDROID_KEYSTORE")
val minSdkLevel = providers.gradleProperty("livi.minSdk").get().toInt()
val compileSdkLevel = providers.gradleProperty("livi.compileSdk").get().toInt()

android {
    namespace = "dev.fio.livi"
    compileSdk = compileSdkLevel

    defaultConfig {
        applicationId = "dev.fio.livi"
        minSdk = minSdkLevel
        targetSdk = compileSdkLevel
        versionName = liviVersion
        versionCode = liviVersion.split(".").map(String::toInt).reduce { code, part -> code * 100 + part }
        ndk {
            abiFilters += "arm64-v8a"
        }
    }

    signingConfigs {
        create("release") {
            if (keystore.isPresent) {
                storeFile = file(keystore.get())
                storePassword = providers.environmentVariable("LIVI_ANDROID_KEYSTORE_PASSWORD").get()
                keyAlias = providers.environmentVariable("LIVI_ANDROID_KEY_ALIAS").get()
                keyPassword = providers.environmentVariable("LIVI_ANDROID_KEY_PASSWORD").get()
            }
        }
    }

    buildTypes {
        release {
            signingConfig = signingConfigs.getByName(if (keystore.isPresent) "release" else "debug")
        }
    }
}

androidComponents {
    onVariants { variant ->
        variant.sources.jniLibs?.addStaticSourceDirectory("build/generated/livi/jniLibs")
        variant.sources.res?.addStaticSourceDirectory("build/generated/livi/res")
        variant.sources.java?.addStaticSourceDirectory("build/generated/livi/java")
        variant.sources.assets?.addStaticSourceDirectory("build/generated/livi/assets")
    }
}
