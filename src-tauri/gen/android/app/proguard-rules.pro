# Add project specific ProGuard rules here.
# You can control the set of applied configuration files using the
# proguardFiles setting in build.gradle.
#
# For more details, see
#   http://developer.android.com/guide/developing/tools/proguard.html

# If your project uses WebView with JS, uncomment the following
# and specify the fully qualified class name to the JavaScript interface
# class:
#-keepclassmembers class fqcn.of.javascript.interface.for.webview {
#   public *;
#}

# Uncomment this to preserve the line number information for
# debugging stack traces.
#-keepattributes SourceFile,LineNumberTable

# If you keep the line number information, uncomment this to
# hide the original source file name.
#-renamesourcefileattribute SourceFile

# Shrink and obfuscate app-side helper classes aggressively; the Rust core
# still owns the sensitive logic, but we should not ship easy-to-read Kotlin
# bridge names and strings in release builds.
-repackageclasses ''
-allowaccessmodification
-adaptclassstrings
-adaptresourcefilenames
-adaptresourcefilecontents

-keep class shop.gooofish.yuguanjia.MainActivity { *; }
-keep class shop.gooofish.yuguanjia.XianyuApp { *; }
-keep class shop.gooofish.yuguanjia.DeviceId { *; }
-keep class shop.gooofish.yuguanjia.NativeBridge { *; }
-keep class shop.gooofish.yuguanjia.WryActivity { *; }
-keep class shop.gooofish.yuguanjia.RustWebView { *; }
-keep class shop.gooofish.yuguanjia.TauriActivity { *; }
