package shop.gooofish.yuguanjia

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.graphics.BitmapShader
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.RectF
import android.graphics.Shader
import android.util.Log
import android.os.Build
import android.os.VibrationEffect
import android.os.VibrationAttributes
import android.os.Vibrator
import android.os.VibratorManager
import android.net.Uri
import android.provider.Settings
import android.media.AudioAttributes
import android.media.RingtoneManager
import android.widget.RemoteViews
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.net.HttpURLConnection
import java.net.URL

object AppNotifications {
    private const val TAG = "XianyuNotify"
    const val CHANNEL_SERVICE = "xianyu_service"
    const val CHANNEL_MESSAGES = "xianyu_messages_v2"
    private var unreadCount = 0

    fun ensureChannels(context: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager

        val service = NotificationChannel(
            CHANNEL_SERVICE,
            "后台常驻",
            NotificationManager.IMPORTANCE_LOW,
        ).apply {
            description = "保持闲鱼工具后台运行"
            setShowBadge(false)
        }

        val messages = NotificationChannel(
            CHANNEL_MESSAGES,
            "新消息提醒",
            NotificationManager.IMPORTANCE_HIGH,
        ).apply {
            description = "闲鱼新客户消息通知"
            enableVibration(true)
            vibrationPattern = longArrayOf(0, 220, 120, 220)
            setSound(
                RingtoneManager.getDefaultUri(RingtoneManager.TYPE_NOTIFICATION),
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_NOTIFICATION)
                    .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
                    .build(),
            )
            lockscreenVisibility = Notification.VISIBILITY_PUBLIC
            setShowBadge(true)
        }

        manager.createNotificationChannel(service)
        manager.createNotificationChannel(messages)
    }

    fun clearServiceNotification(context: Context) {
        val manager = NotificationManagerCompat.from(context)
        listOf(1001, 1501, 1502, 1503, 1504).forEach { manager.cancel(it) }
    }

    fun diagnosticNotification(context: Context, title: String, text: String): Notification {
        ensureChannels(context)
        val appSettingsIntent = PendingIntent.getActivity(
            context,
            1101,
            Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS).apply {
                data = Uri.fromParts("package", context.packageName, null)
                flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP
            },
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        return NotificationCompat.Builder(context, CHANNEL_SERVICE)
            .setSmallIcon(android.R.drawable.stat_sys_warning)
            .setContentTitle(title)
            .setContentText(text)
            .setStyle(NotificationCompat.BigTextStyle().bigText(text))
            .setContentIntent(appSettingsIntent)
            .setAutoCancel(true)
            .setPriority(NotificationCompat.PRIORITY_DEFAULT)
            .build()
    }

    fun canPostNotifications(context: Context): Boolean {
        return if (Build.VERSION.SDK_INT >= 33) {
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED
        } else {
            NotificationManagerCompat.from(context).areNotificationsEnabled()
        }
    }

    fun pushMessage(
        context: Context,
        title: String,
        text: String,
        notificationId: Int,
        avatarUrl: String = "",
        messageTimestamp: Long = System.currentTimeMillis(),
    ) {
        if (!canPostNotifications(context)) {
            Log.w(TAG, "pushMessage skipped: notification permission/state disabled")
            return
        }
        ensureChannels(context)
        val avatar = loadAvatar(avatarUrl)
        val messageViews = messageHeadsUpView(context, title, text, avatar, messageTimestamp)
        unreadCount += 1
        val openAppIntent = PendingIntent.getActivity(
            context,
            notificationId,
            Intent(context, MainActivity::class.java).apply {
                flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
            },
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val notif = NotificationCompat.Builder(context, CHANNEL_MESSAGES)
            .setSmallIcon(R.mipmap.ic_launcher)
            .setLargeIcon(avatar)
            .setCustomContentView(messageViews)
            .setCustomHeadsUpContentView(messageViews)
            .setContentTitle(title)
            .setContentText(text)
            .setStyle(NotificationCompat.BigTextStyle().bigText(text))
            .setContentIntent(openAppIntent)
            .setAutoCancel(true)
            .setCategory(NotificationCompat.CATEGORY_MESSAGE)
            .setWhen(messageTimestamp)
            .setShowWhen(true)
            .setDefaults(NotificationCompat.DEFAULT_ALL)
            .setVibrate(longArrayOf(0, 220, 120, 220))
            .setSound(RingtoneManager.getDefaultUri(RingtoneManager.TYPE_NOTIFICATION))
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            .setNumber(unreadCount)
            .build()
        Log.i(
            TAG,
            "pushMessage notifyId=$notificationId avatar=${avatar != null} title=${title.take(80)} text=${text.take(120)}",
        )
        val manager = NotificationManagerCompat.from(context)
        manager.notify(notificationId, notif)
        vibrateForMessage(context)
    }

    fun showDiagnostic(context: Context, title: String, text: String, notificationId: Int = 1501) {
        ensureChannels(context)
        if (!canPostNotifications(context)) {
            Log.w(TAG, "showDiagnostic skipped: notification permission/state disabled")
            return
        }
        NotificationManagerCompat.from(context).notify(
            notificationId,
            diagnosticNotification(context, title, text),
        )
    }

    private fun loadAvatar(rawUrl: String): Bitmap? {
        val url = normalizeAvatarUrl(rawUrl)
        if (!url.startsWith("http://") && !url.startsWith("https://")) return null
        return runCatching {
            val connection = URL(url).openConnection() as HttpURLConnection
            connection.connectTimeout = 2_000
            connection.readTimeout = 2_000
            connection.instanceFollowRedirects = true
            connection.setRequestProperty("User-Agent", "Mozilla/5.0")
            connection.inputStream.use { stream ->
                BitmapFactory.decodeStream(stream)?.let { circleCrop(it) }
            }
        }.onFailure { error ->
            Log.w(TAG, "loadAvatar failed url=${url.take(120)} msg=${error.message}")
        }.getOrNull()
    }

    private fun messageHeadsUpView(
        context: Context,
        title: String,
        text: String,
        avatar: Bitmap?,
        messageTimestamp: Long,
    ): RemoteViews {
        return RemoteViews(context.packageName, R.layout.notification_message_heads_up).apply {
            setTextViewText(R.id.notification_title, title)
            setTextViewText(R.id.notification_text, text)
            setTextViewText(R.id.notification_app_meta, "鱼管家 · ${formatMessageTime(messageTimestamp)}")
            if (avatar != null) {
                setImageViewBitmap(R.id.notification_avatar, avatar)
            } else {
                setImageViewResource(R.id.notification_avatar, android.R.drawable.stat_notify_chat)
            }
        }
    }

    private fun formatMessageTime(timestamp: Long): String {
        val millis = if (timestamp > 0) timestamp else System.currentTimeMillis()
        return SimpleDateFormat("HH:mm", Locale.getDefault()).format(Date(millis))
    }

    private fun circleCrop(source: Bitmap): Bitmap {
        val size = minOf(source.width, source.height)
        val x = (source.width - size) / 2
        val y = (source.height - size) / 2
        val squared = Bitmap.createBitmap(source, x, y, size, size)
        val output = Bitmap.createBitmap(size, size, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(output)
        val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            shader = BitmapShader(squared, Shader.TileMode.CLAMP, Shader.TileMode.CLAMP)
        }
        canvas.drawOval(RectF(0f, 0f, size.toFloat(), size.toFloat()), paint)
        if (squared != source) squared.recycle()
        return output
    }

    private fun normalizeAvatarUrl(rawUrl: String): String {
        val url = rawUrl.trim()
        if (url.startsWith("//")) return "https:$url"
        if (url.startsWith("http://img.alicdn.com/")) return url.replaceFirst("http://", "https://")
        if (url.startsWith("http://gw.alicdn.com/")) return url.replaceFirst("http://", "https://")
        return url
    }

    private fun vibrateForMessage(context: Context) {
        runCatching {
            val pattern = longArrayOf(0, 260, 120, 260)
            val vibrator = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                val manager = context.getSystemService(VibratorManager::class.java)
                manager.defaultVibrator
            } else {
                @Suppress("DEPRECATION")
                context.getSystemService(Context.VIBRATOR_SERVICE) as Vibrator
            }
            if (!vibrator.hasVibrator()) return
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                val effect = VibrationEffect.createWaveform(pattern, -1)
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                    vibrator.vibrate(
                        effect,
                        VibrationAttributes.Builder()
                            .setUsage(VibrationAttributes.USAGE_NOTIFICATION)
                            .build(),
                    )
                } else {
                    vibrator.vibrate(effect)
                }
            } else {
                @Suppress("DEPRECATION")
                vibrator.vibrate(pattern, -1)
            }
            Log.i(TAG, "message vibration triggered")
        }.onFailure { error ->
            Log.w(TAG, "message vibration failed msg=${error.message}")
        }
    }
}
