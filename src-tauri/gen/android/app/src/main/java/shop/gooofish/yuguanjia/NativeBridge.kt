package shop.gooofish.yuguanjia

import android.util.Log
import java.util.concurrent.atomic.AtomicInteger

/**
 * Process-wide bridge updated by the Rust shell once the embedded backend is ready.
 * Android-side foreground services read these values to connect to the local
 * loopback backend without depending on the WebView lifecycle.
 */
object NativeBridge {
    private const val TAG = "XianyuNativeBridge"
    private val backendPort = AtomicInteger(0)

    @Volatile
    private var backendLivePath: String = ""

    @JvmStatic
    fun updateBackend(port: Int, livePath: String) {
        backendPort.set(port)
        backendLivePath = livePath
        Log.i(
            TAG,
            "updateBackend port=$port baseUrl=${backendBaseUrl()} pathLen=${livePath.length}",
        )
        BackgroundSyncService.refresh(XianyuApp.appContext)
    }

    @JvmStatic
    fun currentBackendPort(): Int = backendPort.get()

    @JvmStatic
    fun backendBaseUrl(): String {
        val port = currentBackendPort()
        return if (port > 0) "http://127.0.0.1:$port" else ""
    }

    @JvmStatic
    fun backendLivePath(): String = backendLivePath
}
