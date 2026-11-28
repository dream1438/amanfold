# Minification is off for the beta (see build.gradle.kts). When it is turned
# on, keep JNA and the generated UniFFI bindings, which are reached by name.
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-keep class com.amanfold.core.** { *; }
