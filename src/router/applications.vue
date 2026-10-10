<script setup lang="ts">
	import { computed, nextTick, onMounted, ref, watch } from "vue";
	import { Snackbar } from '@varlet/ui';
	import { useRouter } from "vue-router";
	import { file } from "../script/file";
	import ApplicationIcon from "../components/ApplicationIcon.vue";
	import { applicationKey, vpnSettings, type SelectedApplication } from "../script/vpnSettings";

	const router = useRouter();
	const { data, loaded, saving, failed } = vpnSettings;
	const loading = ref(false);
	const listLoading = ref(false);
	const query = ref('');
	const applications = ref<SelectedApplication[]>([]);
	const visibleCount = ref(50);
	const policies = [
		{ label: '关闭应用分流', value: 'all' },
		{ label: '所选应用绕过代理', value: 'bypass' },
		{ label: '仅所选应用走代理', value: 'proxy' },
	];
	const selectedKeys = computed(() => new Set(data.applications.map(applicationKey)));
	const filtered = computed(() => {
		const entries = new Map(applications.value.map(app => [applicationKey(app), app]));
		// 保留已选择但这次未扫描到的应用，用户仍可取消选择。
		for (const app of data.applications) {
			if (app.platform === vpnSettings.platform.value && !entries.has(applicationKey(app))) entries.set(applicationKey(app), app);
		}
		const name = query.value.trim().toLocaleLowerCase();
		return [...entries.values()].filter(app => app.name.toLocaleLowerCase().includes(name))
			.sort((a, b) => a.name.localeCompare(b.name, 'zh-CN'));
	});
	const visibleApplications = computed(() => filtered.value.slice(0, visibleCount.value));

	async function loadApplications(): Promise<void> {
		if (loading.value) return;
		loading.value = true;
		try {
			await vpnSettings.ready();
			if (!vpnSettings.applicationsSupported.value) throw new Error('当前平台暂不支持应用列表。');
			applications.value = (await file.listApplications()).map(app => ({
				platform: app.platform, name: app.name,
				id: app.platform === 'android' ? app.packageName : app.executablePath,
			}));
			visibleCount.value = 50;
		} catch (error) {
			Snackbar.error(`读取应用列表失败：${error instanceof Error ? error.message : String(error)}`);
		} finally {
			loading.value = false;
		}
	}

	function selectApplication(app: SelectedApplication, checked: boolean): void {
		if (!loaded.value) return;
		const key = applicationKey(app);
		const remaining = data.applications.filter(item => applicationKey(item) !== key);
		data.applications = checked ? [...remaining, app] : remaining;
	}

	async function loadMore(): Promise<void> {
		visibleCount.value += 50;
		await nextTick();
		listLoading.value = false;
	}
	watch(query, () => { visibleCount.value = 50; });
	onMounted(() => { void loadApplications(); });
</script>

<template>
	<main class="applications-page">
		<header class="toolbar">
			<var-button text type="primary" @click="router.push({ name: 'settings' })">返回</var-button>
			<h1>应用分流</h1>
			<var-button text type="primary" :loading="loading" :disabled="loading" @click="loadApplications">刷新</var-button>
		</header>
		<div class="controls">
			<var-select v-model="data.applicationMode" :options="policies" :disabled="!loaded" />
			<var-input v-model="query" clearable placeholder="按应用名称筛选" />
			<p>已选 {{ data.applications.length }} 个 · 找到 {{ filtered.length }} 个<span v-if="saving"> · 正在保存…</span></p>
			<var-button v-if="failed" text type="primary" @click="vpnSettings.save">重试保存</var-button>
		</div>
		<div class="application-list no-scrollbar">
			<var-list v-model:loading="listLoading" :immediate-check="false"
				:finished="visibleCount >= filtered.length" finished-text="" @load="loadMore">
				<var-cell v-for="app in visibleApplications" :key="applicationKey(app)" :title="app.name" :description="app.id"
					ripple @click="selectApplication(app, !selectedKeys.has(applicationKey(app)))">
					<template #icon>
						<ApplicationIcon :source="app.id" />
					</template>
					<template #extra>
						<var-checkbox :model-value="selectedKeys.has(applicationKey(app))" :disabled="!loaded" @click.stop
							@update:model-value="value => selectApplication(app, Boolean(value))" />
					</template>
				</var-cell>
			</var-list>
			<p v-if="!loading && !filtered.length" class="empty">没有匹配的应用</p>
		</div>
	</main>
</template>

<style scoped lang="scss">
	.applications-page {
		height: 100%;
		display: flex;
		flex-direction: column;
	}
	.toolbar {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 8px;
		h1 {
			font-size: 20px;
		}
	}
	.controls {
		padding: 0 16px;
		.var-input {
			margin-top: 16px;
		}
	}
	.application-list {
		flex: 1;
		min-height: 0;
		overflow-y: auto;
		:deep(.var-cell__description) {
			overflow-wrap: anywhere;
		}
		:deep(.var-cell__extra) {
			flex: 0 0 auto;
		}
	}
	.empty {
		text-align: center;
	}
</style>
