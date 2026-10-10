<script setup lang="ts">
	import { computed, onMounted, onUnmounted, reactive, ref, watch } from "vue";
	import { Snackbar, type DialogActions } from '@varlet/ui';
	import { useRouter } from "vue-router";
	import { api, ApiError, type CaptchaResponse } from "../script/api";
	import { auth } from "../script/auth";

	type Purpose = 'change_email_old' | 'change_email_new';
	const router = useRouter();
	const form = reactive({ password: '', email: '', oldCode: '', newCode: '', captchaCode: '' });
	const currentEmail = ref('');
	const profileLoaded = ref(false);
	const profileLoading = ref(false);
	const pending = ref(false);
	const sending = ref(false);
	const captchaDialog = ref(false);
	const dialogEmail = ref('');
	const dialogPurpose = ref<Purpose>('change_email_old');
	const captcha = ref<CaptchaResponse | null>(null);
	const captchaLoading = ref(false);
	const now = ref(Date.now());
	const mailLimits = reactive(new Map<string, { expiresAt: number; resendAt: number }>());
	const oldResendAt = ref(0);
	const email = computed(() => form.email.trim().toLowerCase());
	const disabled = computed(() => pending.value || sending.value || captchaDialog.value || profileLoading.value || !currentEmail.value);
	const oldResendSeconds = computed(() => resendSeconds('change_email_old'));
	const newResendSeconds = computed(() => resendSeconds('change_email_new'));
	const dialogResendSeconds = computed(() => resendSeconds(dialogPurpose.value));
	let clock: ReturnType<typeof setInterval> | undefined;
	let captchaVersion = 0;
	let disposed = false;

	function limitKey(purpose: Purpose, address = email.value): string {
		return `${purpose}:${address}`;
	}

	function resendSeconds(purpose: Purpose): number {
		const resendAt = Math.max(mailLimits.get(limitKey(purpose))?.resendAt ?? 0,
			purpose === 'change_email_old' ? oldResendAt.value : 0);
		return Math.max(0, Math.ceil((resendAt - now.value) / 1000));
	}

	function emailRule(value: string): true | string {
		const address = value.trim();
		return (address.length <= 254 && /^[\x21-\x7e]+$/.test(address)
			&& /^[^@<>]+@[^@<>]+$/.test(address)) || '请输入有效的 ASCII 邮箱地址';
	}

	function validateEmail(): boolean {
		const validation = emailRule(form.email);
		if (validation !== true) { Snackbar.error(validation); return false; }
		if (email.value === currentEmail.value) { Snackbar.error('新邮箱不能与当前绑定邮箱相同。'); return false; }
		return true;
	}

	function showError(error: unknown): void {
		const messages: Record<string, string> = {
			captcha_invalid_or_expired: '图片验证码错误或已过期，请输入新验证码。',
			email_code_invalid_or_expired: '邮箱验证码错误、已过期或已使用，请检查或重新发送。',
			email_taken: '新邮箱已被其他账号绑定。',
			invalid_email: '邮箱格式不正确。',
			email_not_bound: '当前账号未绑定邮箱，暂不能换绑。',
			email_unchanged: '新邮箱不能与当前绑定邮箱相同。',
			email_changed_retry: '当前绑定邮箱已变化，请重新获取两个邮箱的验证码。',
			password_changed_retry: '密码已发生变化，请重新登录后操作。',
			unauthorized: '密码错误或登录已失效，请重新登录后重试。',
			verification_rate_limited: '验证码请求过于频繁，请稍后重试。',
			auth_busy: '服务器繁忙，请稍后重试。',
			smtp_not_configured: '服务器尚未配置邮件服务。',
			email_delivery_failed: '邮件发送失败，请稍后重试。',
			database_error: '服务器保存失败，请重新获取邮箱验证码后重试。',
			ECONNABORTED: '请求超时，请稍后重试。',
			ERR_BAD_RESPONSE: '服务器响应读取失败，请稍后重试。',
		};
		Snackbar.error(error instanceof ApiError
			? messages[error.code ?? ''] ?? (error.status === null ? '无法连接服务器，请检查网络和 API 地址。' : error.message)
			: error instanceof Error ? error.message : String(error));
	}

	async function loadCurrentEmail(): Promise<void> {
		if (profileLoading.value) return;
		profileLoading.value = true;
		try {
			const user = await api.getMe();
			if (disposed) return;
			const address = user.email?.trim().toLowerCase() ?? '';
			if (address !== currentEmail.value) { form.oldCode = ''; form.newCode = ''; }
			currentEmail.value = address;
			profileLoaded.value = true;
			if (!address) Snackbar.error('当前账号未绑定邮箱，暂不能换绑。');
		} catch (error) {
			if (!disposed) showError(error);
		} finally {
			profileLoading.value = false;
		}
	}

	async function refreshCaptcha(): Promise<void> {
		if (!captchaDialog.value || captchaLoading.value) return;
		const version = ++captchaVersion;
		captchaLoading.value = true;
		captcha.value = null;
		form.captchaCode = '';
		try {
			const result = await api.getCaptcha();
			if (!disposed && version === captchaVersion) captcha.value = result;
		} catch (error) {
			if (!disposed && version === captchaVersion) showError(error);
		} finally {
			if (version === captchaVersion) captchaLoading.value = false;
		}
	}

	async function openVerification(purpose: Purpose): Promise<void> {
		if (disabled.value) return;
		now.value = Date.now();
		if (resendSeconds(purpose) > 0 || !validateEmail()) return;
		dialogEmail.value = email.value;
		dialogPurpose.value = purpose;
		captchaDialog.value = true;
		await refreshCaptcha();
	}

	async function sendEmailCode(): Promise<boolean> {
		if (!captchaDialog.value || pending.value || sending.value || captchaLoading.value) return false;
		now.value = Date.now();
		if (dialogResendSeconds.value > 0 || dialogEmail.value !== email.value) return false;
		const currentCaptcha = captcha.value;
		if (!currentCaptcha || currentCaptcha.expires_at * 1000 <= Date.now()) {
			Snackbar.error('图片验证码已过期或未加载，请输入新验证码。');
			await refreshCaptcha();
			return false;
		}
		if (!/^[0-9]{6}$/.test(form.captchaCode)) { Snackbar.error('请输入图片中的六位数字验证码。'); return false; }
		sending.value = true;
		const purpose = dialogPurpose.value;
		const address = dialogEmail.value;
		let sent = false;
		try {
			// 两个用途都传目标新邮箱；原邮箱收件地址由服务端从当前账号读取。
			const result = await api.sendEmailCode({ purpose, email: address,
				captcha_id: currentCaptcha.captcha_id, captcha_code: form.captchaCode });
			if (disposed) return false;
			now.value = Date.now();
			const resendAt = now.value + result.resend_after_seconds * 1000;
			mailLimits.set(limitKey(purpose, address), { expiresAt: result.expires_at * 1000, resendAt });
			if (purpose === 'change_email_old') { oldResendAt.value = resendAt; form.oldCode = ''; }
			else form.newCode = '';
			Snackbar.success(`${purpose === 'change_email_old' ? '原' : '新'}邮箱验证码已发送，请检查收件箱。`);
			sent = true;
		} catch (error) {
			if (!disposed) showError(error);
		} finally {
			if (!sent && !disposed) await refreshCaptcha();
			sending.value = false;
		}
		return sent;
	}

	async function beforeCaptchaClose(action: DialogActions, done: () => void): Promise<void> {
		if (sending.value) return;
		if (action !== 'confirm' || await sendEmailCode()) done();
	}

	async function submit(valid = true): Promise<void> {
		if (!valid || disabled.value) return;
		if (!form.password) { Snackbar.error('请输入当前登录密码。'); return; }
		if (!validateEmail()) return;
		if (!/^[0-9]{6}$/.test(form.oldCode) || !/^[0-9]{6}$/.test(form.newCode)) {
			Snackbar.error('请输入原邮箱和新邮箱收到的六位验证码。');
			return;
		}
		for (const purpose of ['change_email_old', 'change_email_new'] as const) {
			const limit = mailLimits.get(limitKey(purpose));
			if (limit && limit.expiresAt <= Date.now()) { Snackbar.error('邮箱验证码已过期，请重新发送。'); return; }
		}
		pending.value = true;
		const token = auth.session.value?.token;
		try {
			await api.changeEmail({ password: form.password, new_email: email.value,
				old_email_code: form.oldCode, new_email_code: form.newCode });
			form.password = '';
			form.oldCode = '';
			form.newCode = '';
			if (auth.session.value && auth.session.value.token !== token) return;
			if (auth.session.value?.token === token) {
				try { await auth.clear(); } catch (error) { showError(error); }
			}
			Snackbar.success('邮箱换绑成功，请重新登录。');
			await router.replace({ name: 'login', query: { redirect: '/?tab=mine' } });
		} catch (error) {
			if (!disposed) {
				showError(error);
				if (error instanceof ApiError && ['database_error', 'email_changed_retry'].includes(error.code ?? '')) {
					form.oldCode = '';
					form.newCode = '';
					if (error.code === 'email_changed_retry') await loadCurrentEmail();
				}
			}
		} finally {
			pending.value = false;
		}
	}

	watch(email, () => {
		// 原邮箱验证码同样绑定目标新邮箱，修改目标时必须同时清空两份验证码。
		form.oldCode = '';
		form.newCode = '';
		if (!sending.value && email.value !== dialogEmail.value) captchaDialog.value = false;
	}, { flush: 'sync' });
	watch(captchaDialog, show => {
		if (show) return;
		captchaVersion++;
		captchaLoading.value = false;
		captcha.value = null;
		form.captchaCode = '';
	}, { flush: 'sync' });
	onMounted(() => {
		void loadCurrentEmail();
		clock = setInterval(() => { now.value = Date.now(); }, 1000);
	});
	onUnmounted(() => {
		disposed = true;
		captchaVersion++;
		form.password = '';
		if (clock !== undefined) clearInterval(clock);
	});
</script>

<template>
	<var-form @submit="submit">
		<h1>换绑邮箱</h1>
		<var-input placeholder="当前绑定邮箱" :model-value="currentEmail || (profileLoaded ? '未绑定' : '加载中…')" readonly />
		<var-button v-if="!profileLoaded" text outline type="primary" native-type="button"
			:loading="profileLoading" :disabled="profileLoading" @click="loadCurrentEmail">重新获取邮箱</var-button>
		<var-input type="password" placeholder="请输入当前登录密码" v-model="form.password" :disabled="disabled" />
		<var-input type="email" placeholder="请输入新邮箱" v-model="form.email" :disabled="disabled" />
		<var-button text outline type="primary" native-type="button"
			:disabled="disabled || oldResendSeconds > 0" @click="openVerification('change_email_old')">
			{{ oldResendSeconds > 0 ? `${oldResendSeconds} 秒后重发` : '验证原邮箱' }}
		</var-button>
		<var-input placeholder="请输入原邮箱收到的六位验证码" inputmode="numeric" :maxlength="6"
			v-model="form.oldCode" :disabled="disabled" />
		<var-button text outline type="primary" native-type="button"
			:disabled="disabled || newResendSeconds > 0" @click="openVerification('change_email_new')">
			{{ newResendSeconds > 0 ? `${newResendSeconds} 秒后重发` : '验证新邮箱' }}
		</var-button>
		<var-input placeholder="请输入新邮箱收到的六位验证码" inputmode="numeric" :maxlength="6"
			v-model="form.newCode" :disabled="disabled" />
		<p class="hint">换绑成功后需要重新登录。</p>
		<div class="actions">
			<var-button text outline type="primary" native-type="submit" :loading="pending" :disabled="disabled">确认换绑</var-button>
			<var-button text outline type="primary" native-type="button" :disabled="pending || sending"
				@click="router.replace({ name: 'home', query: { tab: 'mine' } })">返回</var-button>
		</div>
	</var-form>
	<var-dialog v-model:show="captchaDialog" :title="dialogPurpose === 'change_email_old' ? '验证原邮箱' : '验证新邮箱'"
		confirm-button-text="提交验证" :confirm-button-loading="sending"
		:confirm-button-disabled="captchaLoading || !captcha || dialogResendSeconds > 0"
		:cancel-button-disabled="sending" :close-on-click-overlay="!sending" :close-on-key-escape="!sending"
		@before-close="beforeCaptchaClose">
		<div class="captcha-dialog-content">
			<p>验证后将发送邮件到 {{ dialogPurpose === 'change_email_old' ? currentEmail : dialogEmail }}</p>
			<div class="captcha-row">
				<img v-if="captcha" :src="`data:${captcha.mime_type};base64,${captcha.image_base64}`" alt="六位数字验证码" />
				<span v-else>{{ captchaLoading ? '验证码加载中…' : '验证码未加载' }}</span>
				<var-button text type="primary" native-type="button" :disabled="sending || captchaLoading" @click="refreshCaptcha">刷新</var-button>
			</div>
			<var-input placeholder="请输入图片中的六位数字" inputmode="numeric" :maxlength="6"
				v-model="form.captchaCode" :disabled="sending || captchaLoading"
				@keyup.enter="beforeCaptchaClose('confirm', () => { captchaDialog = false })" />
		</div>
	</var-dialog>
</template>

<style scoped lang="scss">
	.var-form {
		width: 100%;
		height: 100%;
		display: flex;
		flex-direction: column;
		justify-content: safe center;
		align-items: center;
		overflow-y: auto;
		> * {
			width: 280px;
			flex-shrink: 0;
		}
		h1 {
			font-size: 24px;
		}
		.var-input {
			min-height: 80px;
		}
		.actions {
			height: 80px;
			width: 240px;
			display: flex;
			justify-content: space-between;
			align-items: center;
		}
	}
	.hint {
		font-size: 14px;
		color: var(--color-text-secondary);
	}
	.captcha-dialog-content {
		padding: 0 24px 16px;
		p {
			overflow-wrap: anywhere;
		}
	}
	.captcha-row {
		min-height: 80px;
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		img {
			max-width: 70%;
			max-height: 80px;
			object-fit: contain;
		}
	}
</style>
