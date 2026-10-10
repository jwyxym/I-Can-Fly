import { invoke } from "@tauri-apps/api/core";
import { Snackbar } from "@varlet/ui";
import { vpnSettings } from "./vpnSettings";

export type VpnMode = "proxy" | "tun";

/** 可直接使用 file.listApplications / file.scanExecutables 的选中项，也可传包名、EXE 名或路径。 */
export type ProxyApplication = string | { packageName: string } | { executablePath: string };

export type RoutingMode = "proxy" | "bypass";

export interface ApplicationRoutingOptions {
	/** proxy：仅名单内应用参与代理分流；bypass：名单内应用始终直连。 */
	mode: RoutingMode;
	applications: ProxyApplication[];
}

export interface DomainRoutingOptions {
	/** proxy：仅匹配域名走代理；bypass：匹配域名直连，其他域名走代理。 */
	mode: RoutingMode;
	/** 普通域名匹配自身及子域名，也支持 domain:、full: 和 *. 前缀。不传完整 URL。 */
	domains: string[];
}

export interface VpnRoutingOptions {
	mode?: VpnMode;
	/** 仅 Proxy 模式：覆盖第一个 SOCKS/HTTP 监听入站的端口。 */
	proxyPort?: number;
	applicationRouting?: ApplicationRoutingOptions;
	domainRouting?: DomainRoutingOptions;
	/** 代理出站 tag；省略时使用配置中的第一个出站。 */
	proxyOutboundTag?: string;
	/** @deprecated 旧的应用白名单参数；请改用 applicationRouting，二者不能同时传。 */
	applications?: ProxyApplication[];
}

/** @deprecated 使用 VpnRoutingOptions。 */
export type ApplicationProxyOptions = VpnRoutingOptions;

export interface XrayStartOptions {
	/** 每项是 path=value，原生层自动添加 --set。 */
	sets: string[];
}

export interface XrayStatus {
	loaded: boolean;
	ready: boolean;
	running: boolean;
	mode: VpnMode | null;
	libraryPath: string | null;
	lastError: string | null;
}

export interface VpnConfig {
	session?: string;
	mtu?: number;
	addresses?: string[];
	routes?: string[];
	dnsServers?: string[];
	/** 仅 Android：仅代理这些应用；自动移除宿主应用，仅含宿主时拒绝启动。 */
	allowedApplications?: string[];
	/** 仅 Android：额外绕过 VPN 的应用；始终排除宿主应用。 */
	disallowedApplications?: string[];
	/** 仅 Android 支持 true。 */
	metered?: boolean;
}

export interface VpnStatus {
	mode: VpnMode | null;
	running: boolean;
	starting: boolean;
	stopping: boolean;
	backendRegistered: boolean;
	/** Android 系统授权状态；Windows 为 null，不代表已获取管理员权限。 */
	permissionGranted: boolean | null;
	permissionPending: boolean;
	lastError: string | null;
	platform: string;
	/** Android VPN 服务是否正在管理该会话的心跳。 */
	heartbeatManaged?: boolean;
	xray: XrayStatus;
}

export interface VpnHeartbeatOptions {
	baseUrl: string;
	token: string;
	sessionId: string;
	intervalSeconds: number;
	expiresAt: number;
}

export interface VpnConnectOptions {
	/** Android 强制使用 tun；其他平台默认 proxy。tun 创建系统 VPN 并注入 TUN 参数。 */
	mode?: VpnMode;
	/** 仅 Android：由前台服务发送心跳，参数只在内存中传递。 */
	heartbeat?: VpnHeartbeatOptions;
	/** 仅 tun 模式生效。 */
	vpnConfig?: VpnConfig;
	applicationRouting?: {
		mode: RoutingMode;
		applications: string[];
	};
	domainRouting?: DomainRoutingOptions;
	proxyOutboundTag?: string;
	xray: XrayStartOptions & {
		/** 仅 tun 模式：原生层将此 inbound 配为 TUN 并覆盖 MTU，默认索引 0。 */
		tunInboundIndex?: number;
	};
}

/** proxy/Windows TUN 在内核启动后返回；Android TUN 授权后提交服务启动。 */
interface VpnApi {
	connect: (config: Object, routing?: VpnRoutingOptions, heartbeat?: VpnHeartbeatOptions) => Promise<boolean>;
	disconnect: () => Promise<void>;
	getStatus: () => Promise<VpnStatus>;
	status: (snapshot?: VpnStatus) => Promise<boolean>;
}

export const vpn: VpnApi = {
	connect : async (config : Object, routing?: VpnRoutingOptions, heartbeat?: VpnHeartbeatOptions) : Promise<boolean> => {
		try {
			routing ??= await vpnSettings.connectionOptions();
			if (routing?.applicationRouting && routing.applications !== undefined) {
				throw new Error("Use applicationRouting or applications, not both");
			}
			const applicationRouting: ApplicationRoutingOptions | undefined = routing?.applicationRouting ?? (routing?.applications !== undefined
				? { mode: "proxy" as const, applications: routing.applications } : undefined);
			const policies: (ApplicationRoutingOptions | DomainRoutingOptions | undefined)[] = [applicationRouting, routing?.domainRouting];
			policies.forEach((policy: ApplicationRoutingOptions | DomainRoutingOptions | undefined): void => {
				if (policy && policy.mode !== "proxy" && policy.mode !== "bypass") {
					throw new Error("Routing mode must be proxy or bypass");
				}
			});
			if (applicationRouting && applicationRouting.applications.length === 0
				|| routing?.domainRouting && routing.domainRouting.domains.length === 0) {
				throw new Error("Routing lists must not be empty");
			}
			const mode: VpnMode = applicationRouting || routing.domainRouting ? "tun" : routing.mode ?? "proxy";
			const sets: string[] = Object.entries(config).map(([key, value]: [string, unknown]): string => `${key}=${JSON.stringify(value)}`);
			if (mode === 'proxy' && routing.proxyPort !== undefined) {
				if (!Number.isInteger(routing.proxyPort) || routing.proxyPort < 1 || routing.proxyPort > 65535) {
					throw new Error('Proxy 端口必须是 1–65535 的整数。');
				}
				const inbounds: { protocol?: string }[] | undefined = (config as { inbounds?: { protocol?: string }[] }).inbounds;
				const index: number = Array.isArray(inbounds) ? inbounds.findIndex((inbound: { protocol?: string }): boolean =>
					inbound && ['socks', 'http', 'mixed'].includes(inbound.protocol ?? ''),
				) : -1;
				if (index < 0) throw new Error('配置中没有可设置端口的 SOCKS/HTTP 代理入站。');
				sets.push(`inbounds[${index}].port=${routing.proxyPort}`);
			}
			if (await vpn.status()) {
				await vpn.disconnect();
				return false;
			}
			const options: VpnConnectOptions = {
				mode,
				heartbeat,
				applicationRouting: applicationRouting ? {
					mode: applicationRouting.mode,
					applications: applicationRouting.applications.map((application: ProxyApplication): string =>
						typeof application === "string" ? application
							: "packageName" in application ? application.packageName : application.executablePath,
					),
				} : undefined,
				domainRouting: routing?.domainRouting,
				proxyOutboundTag: routing?.proxyOutboundTag,
				vpnConfig: {
					mtu: 1500,
					addresses: ["172.19.0.1/30"],
					routes: ["0.0.0.0/0"],
					dnsServers: ["1.1.1.1"],
				},
				xray: {
					tunInboundIndex: 0,
					sets,
				},
			};
			return (await invoke<{ accepted: boolean }>("vpn_connect", { ...options })).accepted;
		} catch (error) {
			Snackbar.error(error instanceof Error ? error.message : String(error));
			return false;
		}
	},
	disconnect : async () : Promise<void> => {
		await invoke<void>("vpn_disconnect");
	},
	/** 原生完整状态，用于区分启动请求已提交和内核实际运行。 */
	getStatus: (): Promise<VpnStatus> => invoke<VpnStatus>("vpn_status"),
	/** 可复用已查询的原生状态，避免同一次轮询重复调用原生接口。 */
	status : async (snapshot?: VpnStatus) : Promise<boolean> => {
		try {
			const status: VpnStatus = snapshot ?? await vpn.getStatus();
			if (status.lastError)
				return false;
			return status.running;
		} catch {
			return false;
		}
	},
};
