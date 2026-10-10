package i.can.fly.vpn

import android.os.Bundle

/** 地址和路由使用数字 IP/CIDR；DNS 必须由接入的内核实际处理或转发。 */
data class VpnConfig(
	val session: String = "I Can Fly",
	val mtu: Int = 1500,
	val addresses: List<String> = listOf("172.19.0.1/30"),
	val routes: List<String> = listOf("0.0.0.0/0"),
	val dnsServers: List<String> = listOf("1.1.1.1"),
	// 宿主应用始终绕过 VPN；允许列表会移除自身，仅含自身时拒绝启动。
	val allowedApplications: List<String> = emptyList(),
	// 额外排除的应用，服务会自动加入宿主包名。
	val disallowedApplications: List<String> = emptyList(),
	val metered: Boolean = false,
) {
	init {
		require(mtu in 1280..65535) { "Invalid MTU" }
		require(addresses.isNotEmpty()) { "At least one TUN address is required" }
		require(routes.isNotEmpty()) { "At least one VPN route is required" }
		require(allowedApplications.isEmpty() || disallowedApplications.isEmpty()) {
			"Allowed and disallowed applications cannot be combined"
		}
	}

	internal fun toBundle() = Bundle().apply {
		putString("session", session)
		putInt("mtu", mtu)
		putStringArrayList("addresses", ArrayList(addresses))
		putStringArrayList("routes", ArrayList(routes))
		putStringArrayList("dns", ArrayList(dnsServers))
		putStringArrayList("allowed", ArrayList(allowedApplications))
		putStringArrayList("disallowed", ArrayList(disallowedApplications))
		putBoolean("metered", metered)
	}

	internal companion object {
		fun fromBundle(bundle: Bundle) = VpnConfig(
			session = requireNotNull(bundle.getString("session")),
			mtu = bundle.getInt("mtu"),
			addresses = requireNotNull(bundle.getStringArrayList("addresses")),
			routes = requireNotNull(bundle.getStringArrayList("routes")),
			dnsServers = requireNotNull(bundle.getStringArrayList("dns")),
			allowedApplications = requireNotNull(bundle.getStringArrayList("allowed")),
			disallowedApplications = requireNotNull(bundle.getStringArrayList("disallowed")),
			metered = bundle.getBoolean("metered"),
		)
	}
}
