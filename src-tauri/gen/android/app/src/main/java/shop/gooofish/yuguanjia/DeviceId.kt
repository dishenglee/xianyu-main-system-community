package shop.gooofish.yuguanjia

import android.annotation.SuppressLint
import android.media.MediaDrm
import android.os.Build
import android.provider.Settings
import androidx.annotation.Keep
import java.security.MessageDigest
import java.util.UUID

/**
 * Android-side primary-hardware-ID collector for the Rust fingerprint
 * module. Two static methods are called from Rust via JNI:
 *
 *   - `widevineId()` returns a hex string derived from the device's
 *     Widevine DRM provisioning certificate (`PROPERTY_DEVICE_UNIQUE_ID`).
 *     Available on every Android 6+ device that ships with Google Play
 *     services / Widevine L1 or L3. This is the recommended modern
 *     replacement for `Build.SERIAL` / IMEI which are gated behind
 *     privileged permissions on Android 10+.
 *   - `androidId()` returns `Settings.Secure.ANDROID_ID` as a hex string
 *     lowercased. Weaker — resets on factory wipe and on some ROMs is
 *     per-app-signing-key — but works as a fallback when Widevine fails.
 *
 * Both return `""` on failure so the JNI bridge can fall through cleanly.
 *
 * We also expose `buildModel()` / `buildManufacturer()` for admin review.
 */
@Keep
object DeviceId {
    private const val WIDEVINE_SCHEME_UUID_MSB = -0x121074568629b532L
    private const val WIDEVINE_SCHEME_UUID_LSB = -0x5c37d8232ae2de13L

    @JvmStatic
    @Keep
    fun widevineId(): String {
        return try {
            val uuid = UUID(WIDEVINE_SCHEME_UUID_MSB, WIDEVINE_SCHEME_UUID_LSB)
            val drm = MediaDrm(uuid)
            val bytes = drm.getPropertyByteArray(MediaDrm.PROPERTY_DEVICE_UNIQUE_ID)
            try {
                drm.close()
            } catch (_: Throwable) {
                // MediaDrm.close() introduced in API 28; ignore on older.
            }
            if (bytes.isEmpty()) "" else sha256Hex(bytes)
        } catch (_: Throwable) {
            // Widevine unavailable (rare OEM ROM, emulator without Play
            // services, etc.). Caller falls through to androidId().
            ""
        }
    }

    @SuppressLint("HardwareIds")
    @JvmStatic
    @Keep
    fun androidId(): String {
        return try {
            val ctx = XianyuApp.appContext ?: return ""
            val raw = Settings.Secure.getString(ctx.contentResolver, Settings.Secure.ANDROID_ID)
            if (raw.isNullOrBlank()) "" else raw.lowercase()
        } catch (_: Throwable) {
            ""
        }
    }

    @JvmStatic
    @Keep
    fun buildModel(): String = Build.MODEL ?: ""

    @JvmStatic
    @Keep
    fun buildManufacturer(): String = Build.MANUFACTURER ?: ""

    private fun sha256Hex(bytes: ByteArray): String {
        val md = MessageDigest.getInstance("SHA-256")
        val digest = md.digest(bytes)
        val sb = StringBuilder(digest.size * 2)
        for (b in digest) {
            sb.append(String.format("%02x", b))
        }
        return sb.toString()
    }
}
