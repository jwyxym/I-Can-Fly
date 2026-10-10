package i.can.fly.vpn

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.VpnService
import android.os.Build
import android.os.ParcelFileDescriptor
import android.util.Log
import java.util.concurrent.Executors

class FlyVpnService : VpnService() {
	// 跨服务实例共用队列，保证旧内核退出后才启动新内核。
	private var tun: ParcelFileDescriptor? = null
	private var backend: VpnBackend? = null
	@Volatile private var closing = false
	@Volatile private var cleanupQueued = false

	override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
		if (intent?.action == ACTION_STOP) {
			requestStop()
			return START_NOT_STICKY
		}
		// 系统绑定可能让已停止的服务实例被复用。
		if (cleanupQueued) {
			if (VpnRuntime.stopping) return START_NOT_STICKY
			cleanupQueued = false
			closing = false
		}
		try {
			// 始终先满足前台服务时限；不支持系统无配置重启。
			showNotification()
			require(intent?.action == ACTION_START) { "Start VPN from the application" }
			val config = VpnConfig.fromBundle(requireNotNull(intent?.getBundleExtra("config")))
			val xray = intent?.getBundleExtra("xray")?.let { XrayVpnOptions.fromBundle(it) }
			worker.execute {
				if (closing || VpnRuntime.stopping) {
					requestStop()
					return@execute
				}
				if (tun != null) return@execute
				try {
					VpnRuntime.lastError = null
					check(prepare(this) == null) { "VPN permission was revoked" }
					backend = if (xray != null) XrayVpnBackend(xray)
						else checkNotNull(VpnRuntime.backendFactory) { "VPN backend is not registered" }.invoke()
					val builder = Builder().setSession(config.session).setMtu(config.mtu)
					config.addresses.forEach { cidr -> withCidr(cidr) { ip, prefix -> builder.addAddress(ip, prefix) } }
					config.routes.forEach { cidr -> withCidr(cidr) { ip, prefix -> builder.addRoute(ip, prefix) } }
					config.dnsServers.forEach { builder.addDnsServer(it) }
					configureApplications(builder, config)
					if (Build.VERSION.SDK_INT >= 29) builder.setMetered(config.metered)
					builder.setBlocking(false)
					tun = checkNotNull(builder.establish()) { "Cannot establish VPN interface" }
					check(!closing && !VpnRuntime.stopping) { "VPN connection cancelled" }
					backend!!.start(tun!!.fd, config) { fd -> !closing && protect(fd) }
					VpnRuntime.running = !closing
					VpnRuntime.starting = false
				} catch (error: Throwable) {
					report(error)
					requestStop()
				}
			}
		} catch (error: Throwable) {
			report(error)
			requestStop()
		}
		return START_NOT_STICKY
	}

	private fun configureApplications(builder: Builder, config: VpnConfig) {
		// 排除宿主应用，让同进程 Rust/Xray 的出站 socket 绕过 VPN。
		// Android 不允许同时设置允许列表和排除列表。
		if (config.allowedApplications.isNotEmpty()) {
			val allowed = config.allowedApplications.distinct().filter { it != packageName }
			require(allowed.isNotEmpty()) {
				"VPN allowlist must contain an application other than the host application"
			}
			allowed.forEach { builder.addAllowedApplication(it) }
		} else {
			(config.disallowedApplications + packageName).distinct().forEach {
				builder.addDisallowedApplication(it)
			}
		}
	}

	private fun showNotification() {
		val manager = getSystemService(NotificationManager::class.java)
		if (Build.VERSION.SDK_INT >= 26) {
			manager.createNotificationChannel(NotificationChannel(CHANNEL, "VPN", NotificationManager.IMPORTANCE_LOW))
		}
		val stop = PendingIntent.getService(this, 0,
			Intent(this, FlyVpnService::class.java).setAction(ACTION_STOP),
			PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
		val builder = if (Build.VERSION.SDK_INT >= 26) Notification.Builder(this, CHANNEL) else Notification.Builder(this)
		val notification = builder.setSmallIcon(android.R.drawable.stat_sys_warning)
			.setContentTitle("I Can Fly VPN")
			.setContentText("VPN 服务运行中")
			.setOngoing(true)
			.addAction(Notification.Action.Builder(null, "断开", stop).build())
			.build()
		if (Build.VERSION.SDK_INT >= 34) {
			startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_SYSTEM_EXEMPTED)
		} else startForeground(NOTIFICATION_ID, notification)
	}

	override fun onRevoke() {
		requestStop()
	}

	override fun onDestroy() {
		requestStop()
		super.onDestroy()
	}

	@Synchronized
	private fun requestStop() {
		if (cleanupQueued) return
		cleanupQueued = true
		closing = true
		VpnRuntime.stopping = true
		// 系统可能仍绑定服务；不等待 onDestroy 才释放 TUN。
		// 与启动共用队列，确保 nativeStart 返回后才停止内核、关闭 fd。
		worker.execute {
			try {
				cleanup()
			} finally {
				stopForeground(STOP_FOREGROUND_REMOVE)
				stopSelf()
				VpnRuntime.running = false
				VpnRuntime.starting = false
				VpnRuntime.stopping = false
			}
		}
	}

	private fun cleanup() {
		try {
			backend?.stop()
		} catch (error: Throwable) {
			report(error)
		} finally {
			backend = null
			try {
				tun?.close()
			} catch (error: Exception) {
				report(error)
			} finally {
				tun = null
			}
		}
	}

	private fun report(error: Throwable) {
		VpnRuntime.lastError = error.message ?: error.javaClass.simpleName
		Log.e("FlyVpnService", "VPN operation failed", error)
	}

	private fun withCidr(cidr: String, apply: (String, Int) -> Unit) {
		val parts = cidr.split('/')
		require(parts.size == 2) { "Invalid CIDR: $cidr" }
		apply(parts[0], parts[1].toInt())
	}

	companion object {
		internal const val ACTION_START = "i.can.fly.vpn.START"
		internal const val ACTION_STOP = "i.can.fly.vpn.STOP"
		private const val CHANNEL = "vpn"
		private const val NOTIFICATION_ID = 1001
		private val worker = Executors.newSingleThreadExecutor { task -> Thread(task, "vpn-lifecycle") }
	}
}
