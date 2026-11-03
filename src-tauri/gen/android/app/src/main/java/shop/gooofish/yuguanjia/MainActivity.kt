package shop.gooofish.yuguanjia

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.util.Log
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.enableEdgeToEdge
import androidx.core.content.ContextCompat

class MainActivity : TauriActivity() {
  private val tag = "XianyuMainActivity"
  private val requestNotifications = registerForActivityResult(
    ActivityResultContracts.RequestPermission()
  ) { granted ->
    Log.i(tag, "POST_NOTIFICATIONS result granted=$granted")
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    val verdict = SecurityGate.evaluate(this)
    if (verdict.blocked) {
      super.onCreate(savedInstanceState)
      AppNotifications.showDiagnostic(this, "安全校验未通过", verdict.reason, 1510)
      finishAffinity()
      return
    }
    super.onCreate(savedInstanceState)
    BackgroundSyncService.start(this)
    maybeRequestNotificationPermission()
  }

  override fun onResume() {
    super.onResume()
    BackgroundSyncService.refresh(this)
  }

  private fun maybeRequestNotificationPermission() {
    if (Build.VERSION.SDK_INT >= 33 &&
      ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
    ) {
      requestNotifications.launch(Manifest.permission.POST_NOTIFICATIONS)
    }
  }
}
