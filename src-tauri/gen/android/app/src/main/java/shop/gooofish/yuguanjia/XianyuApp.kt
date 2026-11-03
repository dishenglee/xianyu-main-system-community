package shop.gooofish.yuguanjia

import android.app.Application
import android.content.Context
import android.util.Log

/**
 * Lightweight Application subclass whose only job is to expose a
 * process-wide `Context` to helpers that can't easily grab the Activity
 * (JNI-invoked statics like `DeviceId.androidId()`).
 */
class XianyuApp : Application() {
    override fun onCreate() {
        super.onCreate()
        appContext = applicationContext
        Log.i("XianyuApp", "onCreate notificationsEnabled=${AppNotifications.canPostNotifications(this)}")
        AppNotifications.ensureChannels(this)
    }

    companion object {
        @Volatile
        @JvmStatic
        var appContext: Context? = null
            internal set
    }
}
