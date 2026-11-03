package shop.gooofish.yuguanjia

import android.app.AlarmManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.app.PendingIntent
import android.os.Handler
import android.os.Build
import android.os.IBinder
import android.provider.Settings
import android.util.Log
import androidx.core.app.NotificationManagerCompat
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import okio.ByteString
import org.json.JSONObject
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger

class BackgroundSyncService : Service() {
    private val tag = "XianyuBgSync"
    private val client: OkHttpClient by lazy {
        OkHttpClient.Builder()
            .readTimeout(0, TimeUnit.MILLISECONDS)
            .pingInterval(20, TimeUnit.SECONDS)
            .retryOnConnectionFailure(true)
            .build()
    }

    private var socket: WebSocket? = null
    private var lastSignature: String = ""
    @Volatile private var activeConnectionSignature: String = ""
    @Volatile private var activeConnectionGeneration: Int = 0
    @Volatile private var connectionState: String = "idle"

    override fun onCreate() {
        super.onCreate()
        AppNotifications.ensureChannels(this)
        AppNotifications.clearServiceNotification(this)
        Log.i(tag, "onCreate notificationsEnabled=${NotificationManagerCompat.from(this).areNotificationsEnabled()}")
        connectIfReady()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_TEST_NOTIFICATION -> {
                val title = intent.getStringExtra(EXTRA_TEST_TITLE).orEmpty().ifBlank { "宿主侧测试通知" }
                val text = intent.getStringExtra(EXTRA_TEST_TEXT).orEmpty().ifBlank { "这条通知来自 BackgroundSyncService 测试入口" }
                Log.i(tag, "received ACTION_TEST_NOTIFICATION title=${title.take(80)}")
                AppNotifications.pushMessage(this, title, text, NOTIFY_ID_MESSAGE_BASE + nextMessageId.incrementAndGet())
                return START_STICKY
            }
            ACTION_RECONNECT -> {
                Log.i(tag, "received ACTION_RECONNECT")
                connectIfReady()
                return START_STICKY
            }
            ACTION_RESTART_BACKGROUND_SYNC -> {
                Log.i(tag, "received ACTION_RESTART_BACKGROUND_SYNC")
            }
        }
        logRuntimeState("onStartCommand")
        connectIfReady()
        return START_STICKY
    }

    override fun onTaskRemoved(rootIntent: Intent?) {
        super.onTaskRemoved(rootIntent)
        scheduleRestart(1_500)
    }

    override fun onDestroy() {
        socket?.cancel()
        socket = null
        scheduleRestart(1_500)
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    private fun connectIfReady() {
        val base = NativeBridge.backendBaseUrl()
        val livePath = NativeBridge.backendLivePath()
        if (base.isBlank() || livePath.isBlank()) {
            Log.w(
                tag,
                "connectIfReady skipped: baseBlank=${base.isBlank()} pathBlank=${livePath.isBlank()}",
            )
            return
        }

        val signature = base
        if (signature == activeConnectionSignature && (connectionState == "connecting" || connectionState == "open")) {
            Log.i(tag, "connectIfReady skipped: existing $connectionState connection for current backend")
            return
        }

        activeConnectionSignature = signature
        activeConnectionGeneration += 1
        val generation = activeConnectionGeneration
        connectionState = "connecting"
        socket?.cancel()
        val wsUrl = base.replaceFirst("http", "ws") +
            livePath
        Log.i(tag, "connecting wsBase=${base.replaceFirst("http", "ws")} pathLen=${livePath.length}")
        val request = Request.Builder().url(wsUrl).build()
        val nextSocket = client.newWebSocket(request, object : WebSocketListener() {
            private fun isCurrent(): Boolean {
                return activeConnectionSignature == signature && activeConnectionGeneration == generation
            }

            override fun onOpen(webSocket: WebSocket, response: okhttp3.Response) {
                if (!isCurrent()) {
                    Log.i(tag, "ignore stale ws open for old backend")
                    webSocket.cancel()
                    return
                }
                connectionState = "open"
                Log.i(tag, "ws opened code=${response.code}")
                webSocket.send("""{"cmd":"subscribe"}""")
                lastSignature = ""
            }

            override fun onMessage(webSocket: WebSocket, text: String) {
                if (!isCurrent()) return
                Log.d(tag, "ws text len=${text.length} ${text.take(300)}")
                handlePayload(text)
            }

            override fun onMessage(webSocket: WebSocket, bytes: ByteString) {
                if (!isCurrent()) return
                val utf8 = bytes.utf8()
                Log.d(tag, "ws bytes len=${utf8.length} ${utf8.take(300)}")
                handlePayload(utf8)
            }

            override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
                if (!isCurrent()) {
                    Log.i(tag, "ignore stale ws closed code=$code reason=$reason")
                    return
                }
                connectionState = "closed"
                Log.w(tag, "ws closed code=$code reason=$reason")
                maybeReconnect()
            }

            override fun onFailure(webSocket: WebSocket, t: Throwable, response: okhttp3.Response?) {
                if (!isCurrent()) {
                    Log.i(tag, "ignore stale ws failure msg=${t.message}")
                    return
                }
                connectionState = "failed"
                Log.e(tag, "ws failure code=${response?.code} msg=${t.message}", t)
                maybeReconnect()
            }
        })
        socket = nextSocket
    }

    private fun maybeReconnect() {
        if (NativeBridge.backendBaseUrl().isBlank()) return
        Log.i(tag, "schedule reconnect in 3000ms")
        Handler(mainLooper).postDelayed({ connectIfReady() }, 3_000)
    }

    private fun scheduleRestart(delayMs: Long) {
        val app = applicationContext ?: return
        val alarm = app.getSystemService(Context.ALARM_SERVICE) as? AlarmManager ?: return
        val intent = Intent(app, BackgroundSyncService::class.java).apply {
            action = "shop.gooofish.yuguanjia.ACTION_RESTART_BACKGROUND_SYNC"
        }
        val pendingIntent = PendingIntent.getService(
            app,
            9001,
            intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val triggerAt = System.currentTimeMillis() + delayMs
        val exactAllowed = canScheduleExactAlarm(alarm)
        Log.i(tag, "scheduleRestart delayMs=$delayMs exactAllowed=$exactAllowed triggerAt=$triggerAt")
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M && exactAllowed) {
            alarm.setExactAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, triggerAt, pendingIntent)
        } else if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            Log.w(tag, "exact alarm unavailable; fallback to setAndAllowWhileIdle settings=${Settings.ACTION_REQUEST_SCHEDULE_EXACT_ALARM}")
            alarm.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, triggerAt, pendingIntent)
        } else {
            alarm.setExact(AlarmManager.RTC_WAKEUP, triggerAt, pendingIntent)
        }
    }

    private fun canScheduleExactAlarm(alarm: AlarmManager): Boolean {
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            alarm.canScheduleExactAlarms()
        } else {
            true
        }
    }

    private fun logRuntimeState(source: String) {
        Log.i(
            tag,
            "$source baseUrl=${NativeBridge.backendBaseUrl()} port=${NativeBridge.currentBackendPort()}",
        )
    }

    private fun handlePayload(raw: String) {
        val json = runCatching { JSONObject(raw) }.getOrNull() ?: return
        val eventType = json.optString("type")
        if (eventType.isNotBlank()) {
            Log.i(tag, "handlePayload type=$eventType isSelf=${json.optBoolean("isSelf", false)}")
        }
        if (eventType != "message.received") return
        if (json.optBoolean("isSelf", false)) {
            Log.i(tag, "skip self message session=${json.optString("sessionId")}")
            return
        }

        val sessionId = json.optString("sessionId")
        val ts = json.optLong("ts", 0L)
        val preview = json.optString("rawPreview")
            .ifBlank { json.optString("text") }
            .ifBlank { "发来一条新消息" }
        val account = json.optString("uid")
        val peerNick = json.optString("peerNick")
            .ifBlank { json.optString("senderNick") }
            .ifBlank { "客户" }
        val avatarUrl = json.optString("peerLogo")
            .ifBlank { json.optString("senderLogo") }
            .ifBlank { json.optString("avatar") }
            .ifBlank { json.optString("logo") }
            .ifBlank { json.optString("headImg") }
            .ifBlank { json.optString("headUrl") }
        val sig = "$sessionId|$ts|$preview"
        if (sig == lastSignature) {
            Log.i(tag, "skip duplicate signature=$sig")
            return
        }
        lastSignature = sig

        val title = if (account.isNotBlank()) "$peerNick · $account" else peerNick
        Log.i(
            tag,
            "notify message session=$sessionId ts=$ts avatar=${avatarUrl.isNotBlank()} title=${title.take(80)} preview=${preview.take(120)}",
        )
        AppNotifications.pushMessage(
            this,
            title,
            preview,
            NOTIFY_ID_MESSAGE_BASE + nextMessageId.incrementAndGet(),
            avatarUrl,
            ts,
        )
    }

    companion object {
        private const val NOTIFY_ID_MESSAGE_BASE = 2000
        private const val ACTION_RESTART_BACKGROUND_SYNC =
            "shop.gooofish.yuguanjia.ACTION_RESTART_BACKGROUND_SYNC"
        const val ACTION_TEST_NOTIFICATION =
            "shop.gooofish.yuguanjia.ACTION_TEST_NOTIFICATION"
        const val ACTION_RECONNECT =
            "shop.gooofish.yuguanjia.ACTION_RECONNECT"
        const val EXTRA_TEST_TITLE = "title"
        const val EXTRA_TEST_TEXT = "text"
        private val nextMessageId = AtomicInteger(0)

        fun start(context: Context?) {
            context ?: return
            val intent = Intent(context, BackgroundSyncService::class.java)
            runCatching {
                context.startService(intent)
            }.onFailure { error ->
                Log.e("XianyuBgSync", "startService failed msg=${error.message}", error)
            }
        }

        fun refresh(context: Context?) {
            start(context)
        }
    }
}
