package i.can.fly.vpn

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.net.VpnService
import androidx.core.content.ContextCompat

object VpnController {
	/** 从前台 Activity 调用；授权窗口不承载内核配置或 native 指针。 */
	fun connect(activity: Activity, config: VpnConfig = VpnConfig()) {
		check(VpnRuntime.backendFactory != null) { "Register a VpnBackend before connecting" }
		activity.startActivity(Intent(activity, VpnPermissionActivity::class.java).apply {
			putExtra("config", config.toBundle())
		})
	}

	internal fun start(context: Context, config: VpnConfig, xray: XrayVpnOptions? = null, heartbeat: VpnHeartbeatConfig? = null) {
		check(xray != null || VpnRuntime.backendFactory != null) { "VPN backend is not registered" }
		check(VpnService.prepare(context) == null) { "VPN permission is required" }
		check(!VpnRuntime.running && !VpnRuntime.starting && !VpnRuntime.stopping) { "VPN is busy" }
		VpnRuntime.starting = true
		try {
			ContextCompat.startForegroundService(context, Intent(context, FlyVpnService::class.java).apply {
				action = FlyVpnService.ACTION_START
				putExtra("config", config.toBundle())
				xray?.let { putExtra("xray", it.toBundle()) }
				heartbeat?.let { putExtra("heartbeat", it.toBundle()) }
			})
		} catch (error: Exception) {
			VpnRuntime.starting = false
			throw error
		}
	}

	fun disconnect(context: Context) {
		if (VpnRuntime.running || VpnRuntime.starting || VpnRuntime.stopping) {
			VpnRuntime.stopping = true
			// 显式通知服务释放资源，不能依赖 stopService 触发 onDestroy。
			context.startService(Intent(context, FlyVpnService::class.java).setAction(FlyVpnService.ACTION_STOP))
		} else {
			context.stopService(Intent(context, FlyVpnService::class.java))
		}
	}
}
