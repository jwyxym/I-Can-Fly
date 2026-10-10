<script setup lang="ts">
	import { computed, onMounted, onUnmounted, reactive, ref, watch } from "vue";
	import { Snackbar, type DialogActions } from '@varlet/ui';
	import { useRouter } from "vue-router";
	import { api, ApiError, type CaptchaResponse } from "../script/api";
	import { auth } from "../script/auth";

	const router = useRouter();
	const form = reactive({ oldPassword: '', newPassword: '', confirmPassword: '', emailCode: '', captchaCode: '' });
	const currentEmail = ref('');
	const profileLoaded = ref(false);
	const profileLoading = ref(false);
	const pending = ref(false);
	const sending = ref(false);
	const captchaDialog = ref(false);
	const dialogPurpose = ref<'email' | 'password'>('email');
	const captcha = ref<CaptchaResponse | null>(null);
	const captchaLoading = ref(false);
	const now = ref(Date.now());
	const resendAt = ref(0);
	const emailCodeExpiresAt = ref(0);
	const resendSeconds = computed(() => Math.max(0, Math.ceil((resendAt.value - now.value) / 1000)));
	const disabled = computed(() => pending.value || sending.value || captchaDialog.value || profileLoading.value || !currentEmail.value);
	let clock: ReturnType<typeof setInterval> | undefined;
	let captchaVersion = 0;
	let disposed = false;

	function showError(error: unknown): void {
		const messages: Record<string, string> = {
			captcha_invalid_or_expired: '图片验证码错误或已过期，请输入新验证码。',
			email_code_invalid_or_expired: '邮箱验证码错误、已过期或已使用，请检查或重新发送。',
			email_not_bound: '当前账号未绑定邮箱，暂不能修改密码。',
			password_must_be_10_to_128_bytes: '新密码需为 10–128 个 UTF-8 字节。',
			password_changed_retry: '密码已发生变化，请重新登录后操作。',
			unauthorized: '原密码错误或登录已失效，请重新登录后重试。',
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
			if (address !== currentEmail.value) {
				form.emailCode = '';
				emailCodeExpiresAt.value = 0;
				resendAt.value = 0;
			}
			currentEmail.value = address;
			profileLoaded.value = true;
			if (!address) Snackbar.error('当前账号未绑定邮箱，暂不能修改密码。');
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

	async function openEmailVerification(): Promise<void> {
		if (disabled.value) return;
		now.value = Date.now();
		if (resendSeconds.value > 0) return;
		dialogPurpose.value = 'email';
		captchaDialog.value = true;
		await refreshCaptcha();
	}

	function validateForm(): boolean {
		if (!form.oldPassword) { Snackbar.error('请输入当前登录密码。'); return false; }
		const bytes = new TextEncoder().encode(form.newPassword).length;
		if (bytes < 10 || bytes > 128) { Snackbar.error('新密码需为 10–128 个 UTF-8 字节。'); return false; }
		if (form.newPassword !== form.confirmPassword) { Snackbar.error('两次输入的新密码必须一致。'); return false; }
		if (!/^[0-9]{6}$/.test(form.emailCode)) { Snackbar.error('请输入邮箱收到的六位验证码。'); return false; }
		if (emailCodeExpiresAt.value && emailCodeExpiresAt.value <= Date.now()) {
			Snackbar.error('邮箱验证码已过期，请重新发送。');
			return false;
		}
		return true;
	}

	async function submit(valid = true): Promise<void> {
		if (!valid || disabled.value || !validateForm()) return;
		// 发邮件使用的图片已经消费，提交密码必须另取新图片。
		dialogPurpose.value = 'password';
		captchaDialog.value = true;
		await refreshCaptcha();
	}

	async function confirmVerification(): Promise<boolean> {
		if (!captchaDialog.value || pending.value || sending.value || captchaLoading.value) return false;
		if (dialogPurpose.value === 'email' && resendSeconds.value > 0) return false;
		if (dialogPurpose.value === 'password' && !validateForm()) return false;
		const currentCaptcha = captcha.value;
		if (!currentCaptcha || currentCaptcha.expires_at * 1000 <= Date.now()) {
			Snackbar.error('图片验证码已过期或未加载，请输入新验证码。');
			await refreshCaptcha();
			return false;
		}
		if (!/^[0-9]{6}$/.test(form.captchaCode)) { Snackbar.error('请输入图片中的六位数字验证码。'); return false; }
		const purpose = dialogPurpose.value;
		const token = auth.session.value?.token;
		if (purpose === 'email') sending.value = true;
		else pending.value = true;
		let success = false;
		try {
			if (purpose === 'email') {
				// 邮件只能发到当前绑定邮箱，API 不允许携带 email 字段。
				const result = await api.sendEmailCode({ purpose: 'change_password',
					captcha_id: currentCaptcha.captcha_id, captcha_code: form.captchaCode });
				if (disposed) return false;
				now.value = Date.now();
				resendAt.value = now.value + result.resend_after_seconds * 1000;
				emailCodeExpiresAt.value = result.expires_at * 1000;
				form.emailCode = '';
				Snackbar.success('邮箱验证码已发送，请检查收件箱。');
			} else {
				await api.changePassword({ old_password: form.oldPassword, new_password: form.newPassword,
					email_code: form.emailCode, captcha_id: currentCaptcha.captcha_id, captcha_code: form.captchaCode });
				success = true;
				form.oldPassword = '';
				form.newPassword = '';
				form.confirmPassword = '';
				form.emailCode = '';
				if (auth.session.value && auth.session.value.token !== token) return true;
				if (auth.session.value?.token === token) {
					try { await auth.clear(); } catch (error) { showError(error); }
				}
				Snackbar.success('密码修改成功，请重新登录。');
				await router.replace({ name: 'login', query: { redirect: '/?tab=mine' } });
			}
			success = true;
		} catch (error) {
			// 原密码错误的 401 会触发登录拦截器跳转，页面卸载后仍需提示原因。
			if (!disposed || (purpose === 'password' && error instanceof ApiError && error.status === 401 && !auth.session.value)) {
				showError(error);
				if (error instanceof ApiError && error.code === 'database_error') form.emailCode = '';
			}
		} finally {
			if (!success && !disposed) await refreshCaptcha();
			sending.value = false;
			pending.value = false;
		}
		return success;
	}

	async function beforeCaptchaClose(action: DialogActions, done: () => void): Promise<void> {
		if (sending.value || pending.value) return;
		if (action !== 'confirm' || await confirmVerification()) done();
	}

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
		form.oldPassword = '';
		form.newPassword = '';
		form.confirmPassword = '';
		if (clock !== undefined) clearInterval(clock);
	});
</script>

<template>
	<var-form @submit="submit">
		<h1>修改密码</h1>
		<var-input placeholder="当前绑定邮箱" :model-value="currentEmail || (profileLoaded ? '未绑定' : '加载中…')" readonly />
		<var-button v-if="!profileLoaded" text outline type="primary" native-type="button"
			:loading="profileLoading" :disabled="profileLoading" @click="loadCurrentEmail">重新获取邮箱</var-button>
		<var-input type="password" placeholder="请输入当前密码" v-model="form.oldPassword" :disabled="disabled" />
		<var-input type="password" placeholder="请输入新密码" v-model="form.newPassword" :disabled="disabled" />
		<var-input type="password" placeholder="请再次输入新密码" v-model="form.confirmPassword" :disabled="disabled" />
		<var-button text outline type="primary" native-type="button"
			:disabled="disabled || resendSeconds > 0" @click="openEmailVerification">
			{{ resendSeconds > 0 ? `${resendSeconds} 秒后重发` : '验证邮箱' }}
		</var-button>
		<var-input placeholder="请输入邮箱收到的六位验证码" inputmode="numeric" :maxlength="6"
			v-model="form.emailCode" :disabled="disabled" />
		<p class="hint">新密码需为 10–128 个 UTF-8 字节，修改成功后需要重新登录。</p>
		<div class="actions">
			<var-button text outline type="primary" native-type="submit" :loading="pending" :disabled="disabled">确认修改</var-button>
			<var-button text outline type="primary" native-type="button" :disabled="pending || sending"
				@click="router.replace({ name: 'home', query: { tab: 'mine' } })">返回</var-button>
		</div>
	</var-form>
	<var-dialog v-model:show="captchaDialog" :title="dialogPurpose === 'email' ? '验证邮箱' : '验证并修改密码'"
		:confirm-button-text="dialogPurpose === 'email' ? '提交验证' : '确认修改'" :confirm-button-loading="sending || pending"
		:confirm-button-disabled="captchaLoading || !captcha || (dialogPurpose === 'email' && resendSeconds > 0)"
		:cancel-button-disabled="sending || pending" :close-on-click-overlay="!sending && !pending" :close-on-key-escape="!sending && !pending"
		@before-close="beforeCaptchaClose">
		<div class="captcha-dialog-content">
			<p>{{ dialogPurpose === 'email' ? `验证后将发送邮件到 ${currentEmail}` : '输入本次图片验证码，确认修改密码。' }}</p>
			<div class="captcha-row">
				<img v-if="captcha" :src="`data:${captcha.mime_type};base64,${captcha.image_base64}`" alt="六位数字验证码" />
				<span v-else>{{ captchaLoading ? '验证码加载中…' : '验证码未加载' }}</span>
				<var-button text type="primary" native-type="button" :disabled="sending || pending || captchaLoading" @click="refreshCaptcha">刷新</var-button>
			</div>
			<var-input placeholder="请输入图片中的六位数字" inputmode="numeric" :maxlength="6"
				v-model="form.captchaCode" :disabled="sending || pending || captchaLoading"
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
