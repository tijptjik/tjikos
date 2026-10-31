plugins { id("com.android.application"); id("org.jetbrains.kotlin.android"); id("org.jetbrains.kotlin.plugin.compose") }
android {
    namespace = "hk.type.notjik"
    compileSdk = 35
    defaultConfig { applicationId = "hk.type.notjik"; minSdk = 26; targetSdk = 35; versionCode = 2; versionName = "0.2.0"; testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner" }
    testBuildType = "release"
    buildFeatures { compose = true }
    compileOptions { sourceCompatibility = JavaVersion.VERSION_17; targetCompatibility = JavaVersion.VERSION_17 }
    kotlinOptions { jvmTarget = "17" }
    signingConfigs {
        create("distribution") {
            System.getenv("NOTJIK_ANDROID_KEYSTORE")?.let { storeFile = file(it) }
            storePassword = System.getenv("NOTJIK_ANDROID_STORE_PASSWORD")
            keyAlias = System.getenv("NOTJIK_ANDROID_KEY_ALIAS") ?: "notjik"
            keyPassword = System.getenv("NOTJIK_ANDROID_KEY_PASSWORD")
        }
    }
    buildTypes { getByName("release") { signingConfig = signingConfigs.getByName("distribution"); isMinifyEnabled = false } }
}
dependencies {
    implementation(platform("androidx.compose:compose-bom:2025.04.01"))
    implementation("androidx.activity:activity-compose:1.10.1")
    implementation("androidx.fragment:fragment-ktx:1.8.6")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.9.0")
    implementation("androidx.work:work-runtime-ktx:2.10.1")
    implementation("com.google.firebase:firebase-messaging:24.1.1")
    implementation("com.squareup.okhttp3:okhttp:4.12.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    androidTestImplementation("androidx.test:runner:1.6.2")
    androidTestImplementation("androidx.test:core:1.6.1")
}
