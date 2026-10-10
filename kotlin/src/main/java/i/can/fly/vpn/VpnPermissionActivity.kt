package i.can.fly.vpn

import android.app.Activity
import android.net.VpnService
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.result.contract.ActivityResultContracts

class VpnPermissionActivity : ComponentActivity() {
	private val permission = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) {
		if (it.resultCode == Activity.RESULT_OK) startVpn()
		else {
			VpnRuntime.lastError = "VPN permission denied"
			finish()
		}
	}

	override fun onCreate(savedInstanceState: Bundle?) {
		super.onCreate(savedInstanceState)
		if (savedInstanceState != null) return
		try {
			val request = VpnService.prepare(this)
			if (request == null) startVpn() else permission.launch(request)
		} catch (error: Exception) {
			VpnRuntime.lastError = error.message ?: error.javaClass.simpleName
			finish()
		}
	}

	private fun startVpn() {
		try {
			val config = VpnConfig.fromBundle(requireNotNull(intent.getBundleExtra("config")))
			VpnController.start(this, config)
		} catch (error: Exception) {
			VpnRuntime.lastError = error.message ?: error.javaClass.simpleName
		} finally {
			finish()
		}
	}
}
