package shop.gooofish.yuguanjia

import android.content.Context
import android.content.pm.ApplicationInfo
import android.os.Build
import android.os.Debug
import java.io.File

object SecurityGate {
    data class Verdict(
        val blocked: Boolean,
        val reason: String,
    )

    fun evaluate(context: Context): Verdict {
        if (BuildConfig.DEBUG) {
            return Verdict(blocked = false, reason = "")
        }
        if (Debug.isDebuggerConnected() || Debug.waitingForDebugger()) {
            return Verdict(true, "检测到调试器连接，已拒绝启动")
        }
        if (isAppDebuggable(context)) {
            return Verdict(true, "检测到可调试构建，已拒绝启动")
        }
        if (looksRooted()) {
            return Verdict(true, "检测到高风险 root 环境，已拒绝启动")
        }
        return Verdict(blocked = false, reason = "")
    }

    private fun isAppDebuggable(context: Context): Boolean {
        return (context.applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE) != 0
    }

    private fun looksRooted(): Boolean {
        val suspiciousPaths = listOf(
            "/system/app/Superuser.apk",
            "/sbin/su",
            "/system/bin/su",
            "/system/xbin/su",
            "/data/local/xbin/su",
            "/data/local/bin/su",
            "/system/sd/xbin/su",
            "/system/bin/failsafe/su",
            "/data/local/su",
        )
        return suspiciousPaths.any { File(it).exists() }
    }
}
