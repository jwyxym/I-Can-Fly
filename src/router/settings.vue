<script setup lang="ts">
	import { onMounted, ref, watch } from "vue";
	import { Snackbar } from '@varlet/ui';
	import { useRouter } from "vue-router";
	import { vpnSettings } from "../script/vpnSettings";
	const router = useRouter();
	const { data, loaded, platform, forceTun, applicationsSupported, saving, failed } = vpnSettings;
	const proxyPortInput = ref(String(data.proxyPort));
	watch(() => data.proxyPort, port => { proxyPortInput.value = String(port); });
	function updateProxyPort(value: string): void {
		proxyPortInput.value = value;
		if (!loaded.value || !/^\d+$/.test(value)) return;
		const port = Number(value);
		if (port >= 1 && port <= 65535) data.proxyPort = port;
	}
	function validateProxyPort(): void {
		const value = proxyPortInput.value;
		if (!/^\d+$/.test(value) || Number(value) < 1 || Number(value) > 65535) {
			Snackbar.error('Proxy 端口必须是 1–65535 的整数。');
		}
		proxyPortInput.value = String(data.proxyPort);
	}
	const modeOptions = [{ label: 'TUN 模式', value: 'tun' }, { label: 'Proxy 模式', value: 'proxy' }];
	const domainOptions = [
		{ label: '关闭域名分流', value: 'all' },
		{ label: '这些域名绕过代理', value: 'bypass' },
		{ label: '仅这些域名走代理', value: 'proxy' },
	];
	async function load(): Promise<void> {
		try { await vpnSettings.ready(); }
		catch (error) { Snackbar.error(`读取 VPN 设置失败：${String(error)}`); }
	}
	onMounted(() => { void load(); });
</script>

<template>
	<main class="settings-page">
		<var-button text type="primary" @click="router.push({ name: 'home', query: { tab: 'mine' } })">返回</var-button>
		<h1>设置</h1>
		<var-button v-if="!loaded" text type="primary" @click="load">重新加载设置</var-button>
		<section class="settings-section">
			<h2>VPN 模式</h2>
			<var-select v-model="data.mode" :options="modeOptions" :disabled="!loaded || forceTun" />
			<template v-if="data.mode === 'proxy'">
				<var-input :model-value="proxyPortInput" placeholder="Proxy 监听端口" inputmode="numeric"
					:disabled="!loaded" @update:model-value="updateProxyPort" @blur="validateProxyPort" />
				<p class="hint">端口范围为 1–65535；修改后，下次连接生效。</p>
			</template>
			<p v-if="platform === 'android'" class="hint">Android 固定使用 TUN 模式。</p>
		</section>
		<section class="settings-section" :class="{ 'routing-disabled': data.mode === 'proxy' }"
			:aria-disabled="data.mode === 'proxy'">
			<h2>域名分流</h2>
			<var-select v-model="data.domainMode" :options="domainOptions" :disabled="!loaded || data.mode === 'proxy'" />
			<var-input v-if="data.domainMode !== 'all'" v-model="data.domainText" textarea :rows="5"
				placeholder="每行一个域名或 URL，例如 example.com 或 https://example.com" :disabled="!loaded || data.mode === 'proxy'" />
			<p class="hint">URL 按域名匹配，不区分路径；支持 domain:、full: 和 *. 前缀。</p>
			<p v-if="data.mode === 'proxy'" class="hint">域名分流仅支持 TUN 模式。</p>
		</section>
		<section class="settings-section" :class="{ 'routing-disabled': data.mode === 'proxy' }"
			:aria-disabled="data.mode === 'proxy'">
			<h2>应用分流</h2>
			<p>{{ data.applicationMode === 'all' ? '关闭应用分流' : data.applicationMode === 'bypass' ? '所选应用绕过代理' : '仅所选应用走代理' }} · 已选 {{ data.applications.length }} 个</p>
			<var-button text outline type="primary" :disabled="!loaded || !applicationsSupported || data.mode === 'proxy'"
				@click="router.push({ name: 'application-settings' })">选择应用</var-button>
			<p v-if="loaded && !applicationsSupported" class="hint">当前平台暂不支持应用列表。</p>
			<p v-else-if="data.mode === 'proxy'" class="hint">应用分流仅支持 TUN 模式。</p>
		</section>
		<p v-if="saving" class="hint">正在保存…</p>
		<var-button v-if="failed" text type="primary" @click="vpnSettings.save">重试保存</var-button>
	</main>
</template>

<style scoped>
	.settings-page {
		padding: 16px;
		height: 100%;
		box-sizing: border-box;
		overflow-y: auto;
	}
	h1 {
		font-size: 24px;
	}
	h2 {
		font-size: 18px;
	}
	.settings-section {
		margin-bottom: 24px;
		.var-input {
			margin-top: 16px;
		}
	}
	.hint {
		font-size: 14px;
		color: var(--color-text-secondary);
		line-height: 1.6;
	}
	.routing-disabled {
		h2, p {
			color: var(--color-text-disabled, #9e9e9e);
		}
	}
</style>
