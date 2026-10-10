package i.can.fly.vpn

/**
 * Rust/JNI 或其他内核的适配接口。工厂、start 和 stop 在 VPN 工作线程上执行。
 * start 完成初始化后返回，包处理循环由内核自行调度，不能阻塞 start。
 * tunFd 是借用的非阻塞 fd，禁止关闭它；需要拥有 fd 时先 dup。
 * stop 必须等待包处理退出并关闭内核拥有的 fd，且支持 start 部分失败后的清理。
 */
interface VpnBackend {
	fun start(tunFd: Int, config: VpnConfig, protectSocket: (Int) -> Boolean)
	fun stop()
}

object VpnRuntime {
	/** 在请求连接前注册；服务与 Activity 同进程，不持久化 native 指针。 */
	@Volatile
	var backendFactory: (() -> VpnBackend)? = null

	@Volatile
	var running: Boolean = false
		internal set

	@Volatile
	var starting: Boolean = false
		internal set

	@Volatile
	var stopping: Boolean = false
		internal set

	@Volatile
	var lastError: String? = null
		internal set

	@Volatile
	var heartbeatManaged: Boolean = false
		internal set
}
