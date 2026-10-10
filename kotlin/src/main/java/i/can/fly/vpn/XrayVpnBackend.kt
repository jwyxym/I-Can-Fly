package i.can.fly.vpn

import android.os.Bundle
import androidx.annotation.Keep
import org.json.JSONArray

data class XrayVpnOptions(val sets: List<String>, val tunInboundIndex: Int = 0) {
	init {
		require(tunInboundIndex in 0..1024) { "Invalid TUN inbound index" }
		require(sets.all { it.substringBefore('=').isNotBlank() && it.contains('=') && !it.contains('\u0000') }) {
			"Xray sets must use path=value syntax"
		}
	}

	fun toBundle() = Bundle().apply {
		putStringArrayList("sets", ArrayList(sets))
		putInt("tunInboundIndex", tunInboundIndex)
	}

	companion object {
		fun fromBundle(bundle: Bundle) = XrayVpnOptions(
			requireNotNull(bundle.getStringArrayList("sets")),
			bundle.getInt("tunInboundIndex"),
		)
	}
}

/** JNI 符号以类名和方法名导出，禁止 R8 重命名。 */
@Keep
class XrayVpnBackend(private val options: XrayVpnOptions) : VpnBackend {
	private var started = false

	override fun start(tunFd: Int, config: VpnConfig, protectSocket: (Int) -> Boolean) {
		// 与 Tauri 使用同一个 Rust 库；这里只增加加载引用，不会重复初始化。
		System.loadLibrary("i_can_fly_lib")
		nativeStart(JSONArray(options.sets).toString(), tunFd, config.mtu, options.tunInboundIndex)
		started = true
	}

	override fun stop() {
		// start 失败时不能误停通过 xray.ts 单独启动的实例。
		if (started) {
			nativeStop()
			started = false
		}
	}

	private external fun nativeStart(setsJson: String, tunFd: Int, mtu: Int, inbound: Int)
	private external fun nativeStop()
}
