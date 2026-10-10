<script setup lang="ts">
	import { computed, onMounted, onUnmounted, ref, watch } from "vue";
	import { Snackbar } from '@varlet/ui';
	import { useRouter } from "vue-router";
	import { api, ApiError, type User, type CurrentUser } from "../script/api";
	import { auth } from "../script/auth";
	import { connection } from "../script/connection";

	const { phase, progress, duration, label: connectionLabel, busy: connectionBusy } = connection;
	let stopObserving: (() => void) | undefined;
	onMounted(() => { stopObserving = connection.observe(); });
	onUnmounted(() => { stopObserving?.(); });

	const router = useRouter();
	const active = ref('connect');
	const profile = ref<User | CurrentUser | null>(null);
	const account = computed<User | CurrentUser | null>(() => profile.value ?? auth.session.value?.user ?? null);
	const email = computed(() => {
		const user = account.value;
		return user && 'email' in user ? user.email ?? '未绑定' : '尚未获取';
	});
	const loading = ref(false);
	const activationCode = ref('');
	const activating = ref(false);

	function formatTime(seconds: number | undefined): string {
		if (!seconds) return '未激活';
		return new Date(seconds * 1000).toLocaleString('zh-CN', { hour12: false });
	}

	function errorMessage(error: unknown): string {
		if (error instanceof ApiError) {
			const messages: Record<string, string> = {
				invalid_activation_code: '激活码无效，请检查后重试。',
				activation_code_used_or_expired: '激活码已使用或已过期。',
				unauthorized: '登录已失效，请重新登录。',
				ECONNABORTED: '请求超时，请稍后重试。',
			};
			return messages[error.code ?? ''] ?? (error.status === null ? '无法连接服务器，请检查网络。' : error.message);
		}
		return error instanceof Error ? error.message : '操作失败，请稍后重试。';
	}

	async function loadAccount(): Promise<void> {
		if (loading.value) return;
		const token = auth.session.value?.token;
		loading.value = true;
		try {
			const result = await api.getMe();
			if (auth.session.value?.token === token) profile.value = result;
		} catch (error) {
			Snackbar.error(errorMessage(error));
		} finally {
			loading.value = false;
		}
	}

	async function redeem(valid = true): Promise<void> {
		if (!valid || activating.value || loading.value) return;
		const code = activationCode.value.trim();
		if (!code) { Snackbar.error('请输入激活码。'); return; }
		const token = auth.session.value?.token;
		activating.value = true;
		try {
			const result = await api.redeemActivationCode(code);
			if (auth.session.value?.token !== token) return;
			if (account.value) {
				profile.value = { ...account.value, active_until: result.active_until,
					active: result.active_until * 1000 > Date.now() };
			}
			activationCode.value = '';
			Snackbar.success('激活码兑换成功。');
			await loadAccount();
		} catch (error) {
			Snackbar.error(errorMessage(error));
		} finally {
			activating.value = false;
		}
	}

	watch(active, tab => { if (tab === 'mine' && !activating.value) void loadAccount(); }, { immediate: true });
</script>

<template>
	<main class="home-page">
		<section v-if="active === 'connect'" class="page-content connect-page" aria-label="连接">
			<button class="connection-button" type="button" :disabled="connectionBusy"
				:aria-label="phase === 'connected' ? '断开连接' : phase === 'disconnected' ? '连接 VPN' : connectionLabel" :aria-busy="connectionBusy"
				@click="connection.toggle">
				<var-progress mode="circle" :value="progress" :size="220" :line-width="8" label>
					<span class="connection-label">{{ connectionLabel }}</span>
					<span v-if="phase === 'connected'" class="connection-duration">{{ duration }}</span>
					<span class="connection-hint">{{ phase === 'connected' ? '点击断开' : phase === 'connecting' ? '请稍候' : phase === 'disconnecting' ? '请稍候' : '开启代理' }}</span>
				</var-progress>
			</button>
		</section>
		<section v-else class="page-content mine-page no-scrollbar" aria-label="我的">
			<div class="account-heading">
				<h1>我的</h1>
				<var-button text type="primary" :loading="loading" :disabled="loading || activating" @click="loadAccount">刷新</var-button>
			</div>
			<section class="panel account-panel" aria-label="账号信息">
				<h2>账号信息</h2>
				<var-cell title="账号"><template #extra>{{ account?.username ?? '—' }}</template></var-cell>
				<var-cell title="邮箱" ripple @click="router.push({ name: 'change-email' })">
					<template #extra>
						<div class="email-extra">
							<span>{{ email }}</span>
							<var-button text type="primary" @click.stop="router.push({ name: 'change-email' })">换绑</var-button>
						</div>
					</template>
				</var-cell>
				<var-cell title="账号状态"><template #extra>{{ account?.disabled ? '已禁用' : account ? '正常' : '—' }}</template></var-cell>
				<var-cell title="激活状态"><template #extra>{{ account?.active ? '已激活' : '未激活或已到期' }}</template></var-cell>
				<var-cell title="激活到期"><template #extra>{{ formatTime(account?.active_until) }}</template></var-cell>
				<var-cell title="注册时间"><template #extra>{{ account?.created_at ? formatTime(account.created_at) : '—' }}</template></var-cell>
			</section>
			<section class="panel" aria-label="激活码兑换">
				<h2>激活码</h2>
				<var-form @submit="redeem">
					<var-input v-model="activationCode" placeholder="请输入激活码" :disabled="activating" />
					<var-button type="primary" native-type="submit" :loading="activating" :disabled="activating || loading">兑换激活码</var-button>
				</var-form>
			</section>
			<div class="mine-actions">
				<var-button text outline type="primary" @click="router.push({ name: 'settings' })">设置</var-button>
				<var-button text outline type="primary" @click="router.push({ name: 'change-password' })">修改密码</var-button>
			</div>
		</section>
		<var-bottom-navigation v-model:active="active" bordered safe-area>
			<var-bottom-navigation-item name="connect" label="连接" icon="wifi" />
			<var-bottom-navigation-item name="mine" label="我的" icon="account-circle" />
		</var-bottom-navigation>
	</main>
</template>

<style scoped lang="scss">
	.home-page {
		height: 100%;
		display: flex;
		flex-direction: column;
	}
	.page-content {
		flex: 1;
		min-height: 0;
		overflow-y: auto;
	}
	.mine-page {
		padding: 16px;
	}
	.mine-actions {
		display: flex;
		flex-wrap: wrap;
		gap: 12px;
		padding-bottom: 16px;
	}
	.connect-page {
		display: flex;
		align-items: center;
		justify-content: center;
	}
	.connection-button {
		padding: 0;
		border: 0;
		border-radius: 50%;
		background: transparent;
		color: inherit;
		font: inherit;
		cursor: pointer;
		-webkit-tap-highlight-color: transparent;
		&:disabled {
			cursor: wait;
		}
		&:focus-visible {
			outline: 2px solid var(--color-primary);
			outline-offset: 8px;
		}
		:deep(.var-progress__circle-label) {
			display: flex;
			flex-direction: column;
			align-items: center;
			gap: 10px;
			white-space: nowrap;
		}
	}
	.connection-label {
		font-size: 22px;
		font-weight: 500;
	}
	.connection-duration {
		font-size: 24px;
		font-variant-numeric: tabular-nums;
	}
	.connection-hint {
		font-size: 14px;
		color: var(--color-text-secondary);
	}
	.account-heading {
		display: flex;
		align-items: center;
		justify-content: space-between;
		h1 {
			margin: 0 0 16px;
			font-size: 24px;
		}
	}
	.panel {
		margin-bottom: 20px;
		h2 {
			margin: 0 0 12px;
			font-size: 18px;
		}
		.var-form {
			display: flex;
			flex-direction: column;
			gap: 16px;
		}
	}
	.account-panel {
		.email-extra {
			display: flex;
			align-items: center;
			justify-content: flex-end;
			gap: 8px;
			span {
				min-width: 0;
				overflow-wrap: anywhere;
			}
			.var-button {
				flex-shrink: 0;
			}
		}
		:deep(.var-cell__content) {
			flex: 0 0 5em;
		}
		:deep(.var-cell__extra) {
			flex: 1;
			min-width: 0;
			text-align: right;
			line-height: 1.5;
			white-space: normal;
			overflow-wrap: anywhere;
		}
	}
	.var-bottom-navigation {
		flex-shrink: 0;
	}
</style>
