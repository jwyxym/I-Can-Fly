import { invoke } from "@tauri-apps/api/core";
import { computed, reactive, readonly, ref, watch } from "vue";
import { Snackbar } from "@varlet/ui";
import type { VpnMode, VpnRoutingOptions } from "./vpn";

export type SelectionMode = "all" | "proxy" | "bypass";
export interface SelectedApplication {
	platform: "windows" | "android";
	id: string;
	name: string;
}
export interface VpnSettings {
	mode: VpnMode;
	proxyPort: number;
	domainMode: SelectionMode;
	domainText: string;
	applicationMode: SelectionMode;
	applications: SelectedApplication[];
}

const data = reactive<VpnSettings>({ mode: "proxy", proxyPort: 10808, domainMode: "all", domainText: "", applicationMode: "all", applications: [] });
const platform = ref("");
const loaded = ref(false);
const pending = ref(0);
const failed = ref(false);
const forceTun = computed(() => platform.value === "android");
const applicationsSupported = computed(() => platform.value === "windows" || platform.value === "android");
let readyPromise: Promise<void> | null = null;
let queue: Promise<void> = Promise.resolve();
let version = 0;
let lastError = "";

function save(): Promise<void> {
	if (!loaded.value) return Promise.reject(new Error("VPN 设置尚未加载。"));
	const snapshot = JSON.parse(JSON.stringify(data)) as VpnSettings;
	const current = ++version;
	pending.value++;
	queue = queue.then(async () => {
		try {
			await invoke<void>("vpn_settings_save", { settings: snapshot });
			if (current === version) { failed.value = false; lastError = ""; }
		} catch (error) {
			const message = `保存 VPN 设置失败：${error instanceof Error ? error.message : String(error)}`;
			if (current === version) failed.value = true;
			if (message !== lastError) Snackbar.error(message);
			lastError = message;
		} finally {
			pending.value--;
		}
	});
	return queue;
}

watch(data, () => {
	if (!loaded.value) return;
	if (forceTun.value && data.mode !== "tun") { data.mode = "tun"; return; }
	void save();
}, { deep: true, flush: "sync" });

async function ready(): Promise<void> {
	if (readyPromise) return readyPromise;
	readyPromise = (async () => {
		const result = await invoke<{ platform: string; settings: VpnSettings }>("vpn_settings_load");
		platform.value = result.platform;
		Object.assign(data, result.settings);
		if (forceTun.value) data.mode = "tun";
		loaded.value = true;
	})().catch(error => { readyPromise = null; throw error; });
	return readyPromise;
}

export function applicationKey(application: SelectedApplication): string {
	return `${application.platform}:${application.platform === "windows" ? application.id.replace(/\\/g, "/").toLowerCase() : application.id}`;
}

export function parseDomains(text: string): string[] {
	return [...new Set(text.split(/\r?\n/).map(value => {
		const entry = value.trim();
		if (!entry) return "";
		if (entry.includes("://")) {
			const url = new URL(entry);
			if (url.protocol !== "http:" && url.protocol !== "https:") throw new Error("域名设置只接受 HTTP/HTTPS URL 或域名。");
			return url.hostname.toLowerCase();
		}
		return entry.toLowerCase();
	}).filter(Boolean))];
}

async function connectionOptions(): Promise<VpnRoutingOptions> {
	await ready();
	await queue;
	if (failed.value) throw new Error(lastError);
	const result: VpnRoutingOptions = { mode: forceTun.value ? "tun" : data.mode };
	if (result.mode === 'proxy') {
		if (!Number.isInteger(data.proxyPort) || data.proxyPort < 1 || data.proxyPort > 65535) {
			throw new Error('Proxy 端口必须是 1–65535 的整数。');
		}
		result.proxyPort = data.proxyPort;
		// Proxy 模式只监听本地端口；保留分流设置，切回 TUN 后再应用。
		return result;
	}
	if (data.domainMode !== "all") {
		const domains = parseDomains(data.domainText);
		if (!domains.length) throw new Error("请填写分流域名，或关闭域名分流。");
		result.domainRouting = { mode: data.domainMode, domains };
	}
	if (data.applicationMode !== "all") {
		const applications = data.applications.filter(app => app.platform === platform.value).map(app => app.id);
		if (!applications.length) throw new Error("请选择分流应用，或关闭应用分流。");
		result.applicationRouting = { mode: data.applicationMode, applications };
	}
	return result;
}

export const vpnSettings = {
	data, platform: readonly(platform), loaded: readonly(loaded),
	saving: computed(() => pending.value > 0), failed: readonly(failed),
	forceTun, applicationsSupported, ready, save, connectionOptions,
};
