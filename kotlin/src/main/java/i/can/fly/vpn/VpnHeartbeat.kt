package i.can.fly.vpn

import android.content.Context
import android.os.Bundle
import android.os.PowerManager
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URI
import java.net.URLEncoder
import java.util.concurrent.Executors
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

internal data class VpnHeartbeatConfig(
	val baseUrl: String,
	val token: String,
	val sessionId: String,
	val intervalSeconds: Long,
	val expiresAt: Long,
) {
	init {
		val uri = URI(baseUrl)
		require(uri.scheme in listOf("http", "https") && !uri.host.isNullOrBlank()
			&& uri.rawUserInfo == null && uri.rawQuery == null && uri.rawFragment == null) { "Invalid heartbeat API URL" }
		require(token.isNotBlank() && token.none { it == '\r' || it == '\n' }) { "Invalid heartbeat token" }
		require(sessionId.isNotBlank() && intervalSeconds in 1..3600 && expiresAt > 0) { "Invalid heartbeat session" }
	}

	fun toBundle() = Bundle().apply {
		putString("baseUrl", baseUrl)
		putString("token", token)
		putString("sessionId", sessionId)
		putLong("intervalSeconds", intervalSeconds)
		putLong("expiresAt", expiresAt)
	}

	fun endpoint() = URI(baseUrl.trimEnd('/') + "/api/connections/"
		+ URLEncoder.encode(sessionId, "UTF-8").replace("+", "%20")).toURL()

	companion object {
		fun fromBundle(bundle: Bundle) = VpnHeartbeatConfig(
			requireNotNull(bundle.getString("baseUrl")), requireNotNull(bundle.getString("token")),
			requireNotNull(bundle.getString("sessionId")), bundle.getLong("intervalSeconds"), bundle.getLong("expiresAt"),
		)
	}
}

/** 由 VPN 服务持有；不依赖 Activity、WebView 或 JavaScript 定时器。 */
internal class VpnHeartbeat(
	context: Context,
	private val config: VpnHeartbeatConfig,
	private val onInvalid: (VpnHeartbeat, String) -> Unit,
) {
	private val stopped = AtomicBoolean(false)
	private val scheduler = Executors.newSingleThreadScheduledExecutor { task -> Thread(task, "vpn-heartbeat") }
	private val wakeLock = context.getSystemService(PowerManager::class.java)
		.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "i.can.fly:vpn-heartbeat").apply { setReferenceCounted(false) }
	private var scheduled: ScheduledFuture<*>? = null
	private var request: HttpURLConnection? = null
	@Volatile private var expiresAt = config.expiresAt

	@Synchronized
	fun start() {
		check(!stopped.get() && scheduled == null) { "Heartbeat already started or stopped" }
		// 随服务释放；设置期限以防异常路径留下无限期的锁。
		val remaining = (expiresAt - System.currentTimeMillis() / 1000).coerceIn(1, 7 * 24 * 3600)
		wakeLock.acquire(TimeUnit.SECONDS.toMillis(remaining))
		scheduled = scheduler.scheduleWithFixedDelay({ tick() }, 0, config.intervalSeconds, TimeUnit.SECONDS)
	}

	private fun tick() {
		if (stopped.get()) return
		if (System.currentTimeMillis() / 1000 >= expiresAt) {
			invalidate("代理会话已到期，连接已停止。")
			return
		}
		var connection: HttpURLConnection? = null
		try {
			connection = open("POST", true)
			synchronized(this) {
				if (stopped.get()) { connection.disconnect(); return }
				request = connection
			}
			val code = connection.responseCode
			if (stopped.get()) return
			if (code in 200..299) {
				val body = connection.inputStream.bufferedReader().use { it.readText() }
				val result = JSONObject(body)
				if (result.optString("status") == "closed") {
					invalidate("代理会话已关闭，连接已停止。")
				} else {
					val deadline = result.optLong("expires_at", expiresAt)
					if (deadline > 0) expiresAt = deadline
				}
			} else if (code in listOf(401, 403, 404, 409)) {
				invalidate(if (code == 401) "登录已失效，VPN 已停止。" else "代理会话已失效或关闭，VPN 已停止。")
			}
		} catch (_: Exception) {
			// 网络切换、超时和服务端临时故障在下个周期重试，不能当成会话失效。
		} finally {
			synchronized(this) { if (request === connection) request = null }
			connection?.disconnect()
		}
	}

	private fun open(method: String, heartbeat: Boolean): HttpURLConnection {
		val url = if (heartbeat) URI(config.endpoint().toString() + "/heartbeat").toURL() else config.endpoint()
		return (url.openConnection() as HttpURLConnection).apply {
			requestMethod = method
			connectTimeout = 10_000
			readTimeout = 10_000
			instanceFollowRedirects = false
			useCaches = false
			setRequestProperty("Authorization", "Bearer ${config.token}")
			setRequestProperty("Accept", "application/json")
		}
	}

	private fun invalidate(reason: String) {
		if (!stopped.get()) onInvalid(this, reason)
	}

	@Synchronized
	fun stop() {
		if (!stopped.compareAndSet(false, true)) return
		scheduled?.cancel(true)
		scheduler.shutdownNow()
		runCatching { request?.disconnect() }
		request = null
		runCatching { if (wakeLock.isHeld) wakeLock.release() }
		// 通知栏断开、系统撤销授权时，WebView 可能不在运行，服务自行关闭远端会话。
		cleanup.execute {
			var connection: HttpURLConnection? = null
			try {
				connection = open("DELETE", false)
				connection.responseCode
			} catch (_: Exception) {
				// 网络不可用时由服务端的心跳超时回收。
			} finally { connection?.disconnect() }
		}
	}

	companion object {
		private val cleanup = Executors.newSingleThreadExecutor { task -> Thread(task, "vpn-session-cleanup") }
	}
}
