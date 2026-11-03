package shop.gooofish.yuguanjia

import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import android.util.Log
import androidx.annotation.Keep
import androidx.core.content.FileProvider
import java.io.File
import okhttp3.OkHttpClient
import okhttp3.Request

@Keep
object AppUpdateInstaller {
    private const val TAG = "XianyuAppUpdate"
    private val client = OkHttpClient()

    @JvmStatic
    @Keep
    fun installApkFromUrl(downloadUrl: String, filename: String): String {
        val context = XianyuApp.appContext ?: error("application context unavailable")
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O &&
            !context.packageManager.canRequestPackageInstalls()
        ) {
            val settingsIntent = Intent(
                Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
                Uri.parse("package:${context.packageName}"),
            ).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            context.startActivity(settingsIntent)
            return "请先允许鱼管家安装未知应用，然后再次点击立即更新。"
        }

        val apk = downloadApk(downloadUrl, safeFilename(filename))
        val uri = FileProvider.getUriForFile(
            context,
            "${context.packageName}.fileprovider",
            apk,
        )
        val installIntent = Intent(Intent.ACTION_VIEW)
            .setDataAndType(uri, "application/vnd.android.package-archive")
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        context.startActivity(installIntent)
        return "安装包已下载，已打开系统安装器，请按提示完成安装。"
    }

    private fun downloadApk(downloadUrl: String, filename: String): File {
        val context = XianyuApp.appContext ?: error("application context unavailable")
        val request = Request.Builder().url(downloadUrl).get().build()
        client.newCall(request).execute().use { response ->
            if (!response.isSuccessful) {
                error("下载安装包失败（HTTP ${response.code}）")
            }
            val body = response.body ?: error("下载安装包失败：响应为空")
            val updateDir = File(context.cacheDir, "updates").apply { mkdirs() }
            val apk = File(updateDir, filename)
            body.byteStream().use { input ->
                apk.outputStream().use { output ->
                    input.copyTo(output)
                }
            }
            if (apk.length() <= 0L) {
                apk.delete()
                error("下载安装包失败：文件为空")
            }
            Log.i(TAG, "downloaded apk=${apk.absolutePath} size=${apk.length()}")
            return apk
        }
    }

    private fun safeFilename(raw: String): String {
        val cleaned = raw
            .trim()
            .ifBlank { "yuguanjia-update.apk" }
            .replace(Regex("""[\\/:*?"<>|]"""), "_")
        return if (cleaned.endsWith(".apk", ignoreCase = true)) cleaned else "$cleaned.apk"
    }
}
