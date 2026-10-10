import { computed, readonly, ref } from "vue";
import { Snackbar } from "@varlet/ui";
import { api, apiClient, ApiError, type CreateConnectionResponse, type JsonObject } from "./api";
import { vpn, type VpnStatus } from "./vpn";

type Phase = "disconnected" | "connecting" | "connected" | "disconnecting";
const phase = ref<Phase>("disconnected");
const progress = ref(0);
const connectedAt = ref<number | null>(null);
const now = ref(Date.now());
const checking = ref(true);
let session: CreateConnectionResponse | null = null;
let nextHeartbeat = 0;
let heartbeatEnabled = false;
let heartbeatBusy = false;
const operationBusy = ref(false);
let submitted = false;
let submittedAt = 0;
let observers = 0;
let timer: ReturnType<typeof setInterval> | undefined;
let statusRequest: Promise<VpnStatus> | null = null;
let reportedStatusError = "";
let reportedHeartbeatError = "";

const duration = computed(() => {
	const seconds = connectedAt.value === null ? 0 : Math.max(0, Math.floor((now.value - connectedAt.value) / 1000));
	return [Math.floor(seconds / 3600), Math.floor(seconds / 60) % 60, seconds % 60]
		.map(value => String(value).padStart(2, '0')).join(':');
});
const busy = computed(() => checking.value || operationBusy.value || phase.value === 'connecting' || phase.value === 'disconnecting');
const label = computed(() => ({ disconnected: '已断开', connecting: '正在连接', connected: '已连接', disconnecting: '正在断开' }[phase.value]));

function message(error: unknown): string {
	if (error instanceof ApiError) {
		const messages: Record<string, string> = {
			activation_required: '请先在“我的”页面激活账号。',
			connection_already_exists_or_cleanup_pending: '已有代理会话或正在清理，请稍后重试。',
			xray_unavailable: '服务器代理服务暂不可用。',
			session_expired_or_closed: '代理会话已过期或关闭。',
			unauthorized: '登录已失效，请重新登录。',
		};
		return messages[error.code ?? ''] ?? (error.status === null ? '无法连接服务器，请检查网络。' : error.message);
	}
	return error instanceof Error ? error.message : String(error);
}

function readStatus(): Promise<VpnStatus> {
	return statusRequest ??= vpn.getStatus().finally(() => { statusRequest = null; });
}

function stopIdleTimer(): void {
	if (observers || phase.value !== 'disconnected' || session) return;
	if (timer !== undefined) clearInterval(timer);
	timer = undefined;
}

function reset(): void {
	heartbeatEnabled = false;
	phase.value = 'disconnected';
	progress.value = 0;
	connectedAt.value = null;
	submitted = false;
	reportedHeartbeatError = '';
	stopIdleTimer();
}

async function closeSession(): Promise<void> {
	if (!session) return;
	const current = session;
	try { await api.closeConnection(current.session_id); }
	catch (error) {
		if (!(error instanceof ApiError && error.status === 404)) throw error;
	}
	if (session === current) session = null;
}

function prepareConfig(result: CreateConnectionResponse): void {
	if (!result.requires_server_address) return;
	// API.md：未返回代理地址时使用 API 主机名，保留端口和 TLS/REALITY SNI。
	const settings = result.config.outbounds[0]?.settings;
	if (!settings || typeof settings !== 'object' || Array.isArray(settings)) throw new Error('缺少代理目标配置。');
	const endpoints = settings.vnext ?? settings.servers;
	const endpoint = Array.isArray(endpoints) ? endpoints[0] : null;
	if (!endpoint || typeof endpoint !== 'object' || Array.isArray(endpoint)) throw new Error('缺少代理目标配置。');
	const address = new URL(apiClient.defaults.baseURL ?? '').hostname.replace(/^\[|\]$/g, '');
	(endpoint as JsonObject).address = address;
}

async function disconnect(alreadyStopped = false): Promise<void> {
	if (operationBusy.value) return;
	heartbeatEnabled = false;
	operationBusy.value = true;
	if (alreadyStopped) reset();
	else phase.value = 'disconnecting';
	try {
		// 先停止本地内核和 Android 网卡，远端关闭失败不妨碍本地断开。
		await vpn.disconnect();
		await closeSession();
		const status = await readStatus();
		if (!await vpn.status(status)) reset();
	} catch (error) {
		Snackbar.error(message(error));
		try {
			const status = await readStatus();
			if (await vpn.status(status)) {
				phase.value = 'connected';
				progress.value = 100;
			} else reset();
		} catch { reset(); }
	} finally {
		operationBusy.value = false;
	}
}

async function heartbeat(): Promise<void> {
	if (!heartbeatEnabled || !session || heartbeatBusy || phase.value === 'disconnected' || phase.value === 'disconnecting') return;
	const current = session;
	const interval = Math.max(1, current.heartbeat_interval_seconds) * 1000;
	if (now.value < nextHeartbeat) return;
	nextHeartbeat = now.value + interval;
	heartbeatBusy = true;
	try {
		await api.heartbeat(current.session_id);
		reportedHeartbeatError = '';
	} catch (error) {
		if (!heartbeatEnabled || session !== current) return;
		const text = message(error);
		if (text !== reportedHeartbeatError) Snackbar.error(text);
		reportedHeartbeatError = text;
		if (error instanceof ApiError && [401, 403, 404, 409].includes(error.status ?? 0)) {
			heartbeatEnabled = false;
			// 启动操作完成后由状态轮询处理失效会话，避免与启动并发停止内核。
			current.expires_at = 0;
		}
	} finally {
		heartbeatBusy = false;
	}
}

async function refresh(): Promise<boolean> {
	try {
		const status = await readStatus();
		const running = await vpn.status(status);
		reportedStatusError = '';
		if (!running && (phase.value === 'connected' || phase.value === 'disconnecting')) {
			// 先更新界面和停止心跳，远端清理再慢也不继续显示已连接。
			reset();
			if (!operationBusy.value) await disconnect(true);
			return true;
		}
		if (operationBusy.value || phase.value === 'disconnecting') return true;
		if (phase.value !== 'disconnected' && session && session.expires_at * 1000 <= Date.now()) {
			Snackbar.error('代理会话已到期，连接已停止。');
			await disconnect();
			return true;
		}
		if (running && status.xray.running) {
			connectedAt.value ??= Date.now();
			phase.value = 'connected';
			progress.value = 100;
		} else if (status.starting || status.permissionPending || status.stopping) {
			phase.value = status.stopping ? 'disconnecting' : 'connecting';
		} else if (phase.value === 'connected' || phase.value === 'connecting' || submitted || status.running) {
			// Android 服务提交后可能尚未进入 starting，留出短暂启动窗口。
			if (submitted && !status.lastError && Date.now() - submittedAt < 3000) return true;
			Snackbar.error(status.lastError ?? 'VPN 连接已停止。');
			await disconnect(true);
		}
		return true;
	} catch (error) {
		const text = message(error);
		if (text !== reportedStatusError) Snackbar.error(text);
		reportedStatusError = text;
		if (phase.value === 'connected' || phase.value === 'disconnecting') reset();
		return false;
	} finally {
		checking.value = false;
	}
}

function startTimer(): void {
	if (timer !== undefined) return;
	timer = setInterval(() => {
		now.value = Date.now();
		if (phase.value === 'connecting') progress.value = Math.min(90, progress.value + Math.max(1, (90 - progress.value) / 8));
		if (phase.value === 'disconnecting' && !operationBusy.value) {
			void disconnect();
		} else {
			// 先确认状态，再发送心跳，避免本轮已发现断开后仍补发一次心跳。
			if (!statusRequest) void refresh().then(() => heartbeat());
		}
	}, 1000);
}

async function toggle(): Promise<void> {
	if (busy.value) return;
	// 点击前再次读状态，避免页面显示滞后或外部启动导致重复连接。
	checking.value = true;
	try {
		if (!await refresh()) return;
	} finally {
		checking.value = false;
	}
	if (busy.value) return;
	if (phase.value === 'connected') { await disconnect(); return; }
	operationBusy.value = true;
	phase.value = 'connecting';
	progress.value = 5;
	startTimer();
	try {
		// 上一次远端清理失败时，先重试关闭，避免占用唯一会话名额。
		await closeSession();
		session = await api.createConnection();
		nextHeartbeat = Date.now();
		heartbeatEnabled = true;
		prepareConfig(session);
		progress.value = Math.max(progress.value, 40);
		if (!await vpn.connect(session.config)) {
			// connect 内部会捕获启动错误并返回 false，先停心跳再清理。
			heartbeatEnabled = false;
			await vpn.disconnect();
			await closeSession();
			reset();
			return;
		}
		submitted = true;
		submittedAt = Date.now();
		progress.value = Math.max(progress.value, 65);
	} catch (error) {
		heartbeatEnabled = false;
		Snackbar.error(message(error));
		try { await vpn.disconnect(); await closeSession(); }
		catch (cleanupError) { Snackbar.error(message(cleanupError)); }
		reset();
	} finally {
		operationBusy.value = false;
	}
	if (submitted) await refresh();
}

function observe(): () => void {
	observers++;
	startTimer();
	void refresh();
	return () => { observers--; stopIdleTimer(); };
}

// 状态与心跳独立于页面生命周期，进入设置页后继续连接和计时。
export const connection = {
	phase: readonly(phase), progress: readonly(progress), duration, label, busy, toggle, observe,
};
