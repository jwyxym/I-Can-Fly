package i.can.fly.vpn

import android.app.Activity
import android.net.VpnService
import androidx.activity.result.ActivityResult
import androidx.appcompat.app.AppCompatActivity
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import org.json.JSONObject

@InvokeArg
class VpnConfigArgs {
	var session: String = "I Can Fly"
	var mtu: Int = 1500
	var addresses: List<String> = listOf("172.19.0.1/30")
	var routes: List<String> = listOf("0.0.0.0/0")
	var dnsServers: List<String> = listOf("1.1.1.1")
	var allowedApplications: List<String> = emptyList()
	var disallowedApplications: List<String> = emptyList()
	var metered: Boolean = false

	fun toConfig() = VpnConfig(session, mtu, addresses, routes, dnsServers,
		allowedApplications, disallowedApplications, metered)
}

@InvokeArg
class XrayVpnArgs {
	var sets: List<String> = emptyList()
	var tunInboundIndex: Int = 0
	fun toOptions() = XrayVpnOptions(sets, tunInboundIndex)
}

@InvokeArg
class VpnConnectArgs {
	var vpnConfig: VpnConfigArgs = VpnConfigArgs()
	var xray: XrayVpnArgs? = null
}

@TauriPlugin
class VpnPlugin(private val activity: Activity) : Plugin(activity) {
	private var pendingConnect: Invoke? = null
	private var permissionCancelled = false


	@Command
	fun connect(invoke: Invoke) {
		if (pendingConnect != null) {
			invoke.reject("A VPN permission request is already pending")
			return
		}
		try {
			check(!VpnRuntime.running && !VpnRuntime.starting && !VpnRuntime.stopping) { "VPN is busy" }
			val args = invoke.parseArgs(VpnConnectArgs::class.java)
			args.vpnConfig.toConfig()
			requireNotNull(args.xray) { "Xray options are required" }.toOptions()
			VpnRuntime.lastError = null
			val intent = VpnService.prepare(activity)
			if (intent == null) start(invoke)
			else {
				pendingConnect = invoke
				permissionCancelled = false
				startActivityForResult(invoke, intent, "onVpnPermissionResult")
			}
		} catch (error: Exception) {
			pendingConnect = null
			fail(invoke, error)
		}
	}

	@ActivityCallback
	fun onVpnPermissionResult(invoke: Invoke, result: ActivityResult) {
		// 断开后迟到的系统授权结果不能重新连接。
		if (pendingConnect !== invoke) return
		pendingConnect = null
		if (permissionCancelled) return
		if (result.resultCode != Activity.RESULT_OK) {
			fail(invoke, IllegalStateException("VPN permission denied"))
			return
		}
		start(invoke)
	}

	private fun start(invoke: Invoke) {
		try {
			val args = invoke.parseArgs(VpnConnectArgs::class.java)
			VpnController.start(activity, args.vpnConfig.toConfig(), requireNotNull(args.xray).toOptions())
			val result = JSObject()
			result.put("accepted", true)
			invoke.resolve(result)
		} catch (error: Exception) {
			fail(invoke, error)
		}
	}

	@Command
	fun disconnect(invoke: Invoke) {
		try {
			if (!permissionCancelled) pendingConnect?.reject("VPN connection cancelled")
			// 保留占位，直到系统窗口关闭，避免两个授权结果对应到同一回调。
			permissionCancelled = true
			VpnController.disconnect(activity)
			invoke.resolve()
		} catch (error: Exception) {
			fail(invoke, error)
		}
	}

	@Command
	fun getStatus(invoke: Invoke) {
		try {
			val result = JSObject()
			result.put("running", VpnRuntime.running)
			result.put("starting", VpnRuntime.starting)
			result.put("stopping", VpnRuntime.stopping)
			result.put("backendRegistered", true)
			result.put("permissionGranted", VpnService.prepare(activity) == null)
			result.put("permissionPending", pendingConnect != null)
			result.put("lastError", VpnRuntime.lastError ?: JSONObject.NULL)
			invoke.resolve(result)
		} catch (error: Exception) {
			fail(invoke, error)
		}
	}

	override fun onDestroy(activity: AppCompatActivity) {
		if (!permissionCancelled) pendingConnect?.reject("Activity destroyed before VPN permission completed")
		pendingConnect = null
	}

	private fun fail(invoke: Invoke, error: Exception) {
		VpnRuntime.lastError = error.message ?: error.javaClass.simpleName
		invoke.reject(VpnRuntime.lastError)
	}
}
