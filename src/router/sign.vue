<script setup lang="ts">
	import { computed, onMounted, onUnmounted, reactive, ref, watch } from "vue";
	import { Snackbar, type DialogActions } from '@varlet/ui';
	import { useRoute, useRouter } from "vue-router";
	import { api, ApiError, type CaptchaResponse } from "../script/api";

	const route = useRoute();
	const router = useRouter();
	const form = reactive({ user: '', pass: '', pass2: '', email: '', emailCode: '', captchaCode: '' });
	const pending = ref(false);
	const sending = ref(false);
	const captchaDialog = ref(false);
	const dialogEmail = ref('');
	const captcha = ref<CaptchaResponse | null>(null);
	const captchaLoading = ref(false);
	const now = ref(Date.now());
	const mailLimits = reactive(new Map<string, { expiresAt: number; resendAt: number }>());
	const email = computed(() => form.email.trim().toLowerCase());
	const resendSeconds = computed(() => Math.max(0,
		Math.ceil(((mailLimits.get(email.value)?.resendAt ?? 0) - now.value) / 1000)));
	let clock: ReturnType<typeof setInterval> | undefined;
	let captchaVersion = 0;

	function usernameRule(value: string): true | string {
		return /^[A-Za-z0-9_-]{3,64}$/.test(value) || '用户名需为 3–64 位字母、数字、下划线或连字符';
	}

	function passwordRule(value: string): true | string {
		const bytes = new TextEncoder().encode(value).length;
		return (bytes >= 10 && bytes <= 128) || '密码需为 10–128 个 UTF-8 字节';
	}

	function emailRule(value: string): true | string {
		const address = value.trim();
		return (address.length <= 254 && /^[\x21-\x7e]+$/.test(address)
			&& /^[^@<>]+@[^@<>]+$/.test(address)) || '请输入有效的 ASCII 邮箱地址';
	}

	function showError(error: unknown, fallback: string): void {
		const messages: Record<string, string> = {
			captcha_invalid_or_expired: '图片验证码错误或已过期，请输入新验证码。',
			email_code_invalid_or_expired: '邮箱验证码错误、已过期或已使用，请检查或重新发送。',
			username_taken: '用户名已被使用。',
			email_taken: '邮箱已被绑定。',
			invalid_email: '邮箱格式不正确。',
			verification_rate_limited: '验证码请求过于频繁，请稍后重试。',
			auth_busy: '服务器繁忙，请稍后重试。',
			smtp_not_configured: '服务器尚未配置邮件服务。',
			email_delivery_failed: '邮件发送失败，请稍后重试。',
			ECONNABORTED: '请求超时，请稍后重试。',
			ERR_BAD_RESPONSE: '服务器响应读取失败，请稍后重试。',
		};
		const content = error instanceof ApiError
			? messages[error.code ?? ''] ?? (error.status === null ? '无法连接服务器，请检查网络和 API 地址。' : error.message)
			: error instanceof Error ? error.message : fallback;
		Snackbar.error(content);
	}

	async function refreshCaptcha(): Promise<void> {
		if (!captchaDialog.value || captchaLoading.value) return;
		const version = ++captchaVersion;
		captchaLoading.value = true;
		captcha.value = null;
		form.captchaCode = '';
		try {
			const result = await api.getCaptcha();
			if (version === captchaVersion) captcha.value = result;
		} catch (error) {
			if (version === captchaVersion) showError(error, '获取图片验证码失败，请重试。');
		} finally {
			if (version === captchaVersion) captchaLoading.value = false;
		}
	}

	async function openEmailVerification(): Promise<void> {
		if (pending.value || sending.value || captchaDialog.value) return;
		now.value = Date.now();
		if (resendSeconds.value > 0) return;
		const validation = emailRule(form.email);
		if (validation !== true) { Snackbar.error(validation); return; }
		dialogEmail.value = email.value;
		captchaDialog.value = true;
		await refreshCaptcha();
	}

	async function beforeCaptchaClose(action: DialogActions, done: () => void): Promise<void> {
		if (sending.value) return;
		if (action !== 'confirm' || await sendEmailCode()) done();
	}

	async function sendEmailCode(): Promise<boolean> {
		if (!captchaDialog.value || pending.value || sending.value || captchaLoading.value) return false;
		now.value = Date.now();
		if (resendSeconds.value > 0) return false;
		if (dialogEmail.value !== email.value) return false;
		const currentCaptcha = captcha.value;
		if (!currentCaptcha || currentCaptcha.expires_at * 1000 <= Date.now()) {
			Snackbar.error('图片验证码已过期或未加载，请输入新验证码。');
			await refreshCaptcha();
			return false;
		}
		if (!/^[0-9]{6}$/.test(form.captchaCode)) {
			Snackbar.error('请输入图片中的六位数字验证码。');
			return false;
		}
		sending.value = true;
		const address = dialogEmail.value;
		let sent = false;
		try {
			const result = await api.sendEmailCode({ purpose: 'register', email: address,
				captcha_id: currentCaptcha.captcha_id, captcha_code: form.captchaCode });
			now.value = Date.now();
			mailLimits.set(address, { expiresAt: result.expires_at * 1000,
				resendAt: now.value + result.resend_after_seconds * 1000 });
			form.emailCode = '';
			Snackbar.success('邮箱验证码已发送，请检查收件箱。');
			sent = true;
		} catch (error) {
			showError(error, '发送邮箱验证码失败，请重试。');
		} finally {
			// 提交失败后留在弹窗并换一张图片；成功后关闭弹窗。
			if (!sent) await refreshCaptcha();
			sending.value = false;
		}
		return sent;
	}

	async function submit(valid = true): Promise<void> {
		if (!valid || pending.value || sending.value) return;
		for (const validation of [usernameRule(form.user), passwordRule(form.pass), emailRule(form.email)]) {
			if (validation !== true) { Snackbar.error(validation); return; }
		}
		if (form.pass !== form.pass2) { Snackbar.error('两次密码必须一致。'); return; }
		if (!/^[0-9]{6}$/.test(form.emailCode)) { Snackbar.error('请输入六位邮箱验证码。'); return; }
		const limit = mailLimits.get(email.value);
		if (limit && limit.expiresAt <= Date.now()) {
			Snackbar.error('邮箱验证码已过期，请重新发送。');
			return;
		}
		pending.value = true;
		try {
			await api.register({ username: form.user, password: form.pass,
				email: email.value, email_code: form.emailCode });
			form.pass = '';
			form.pass2 = '';
			form.emailCode = '';
			Snackbar.success('注册成功，请登录。');
			const redirect = route.query.redirect;
			await router.replace({ name: 'login', query: typeof redirect === 'string' ? { redirect } : {} });
		} catch (error) {
			showError(error, '注册失败，请稍后重试。');
		} finally {
			pending.value = false;
		}
	}

	watch(email, () => {
		form.emailCode = '';
		if (!sending.value && email.value !== dialogEmail.value) captchaDialog.value = false;
	});
	watch(captchaDialog, show => {
		if (show) return;
		// 丢弃关闭前尚未完成的图片请求，避免重新打开时显示旧图片。
		captchaVersion++;
		captchaLoading.value = false;
		captcha.value = null;
		form.captchaCode = '';
	}, { flush: 'sync' });
	onMounted(() => {
		clock = setInterval(() => { now.value = Date.now(); }, 1000);
	});
	onUnmounted(() => { if (clock !== undefined) clearInterval(clock); });
</script>

<template>
	<var-form @submit="submit">
		<var-input placeholder="请输入用户名" :rules="usernameRule" v-model="form.user" :disabled="pending" />
		<var-input type="password" placeholder="请输入密码" :rules="passwordRule" v-model="form.pass" :disabled="pending" />
		<var-input type="password" placeholder="请再次输入密码"
			:rules="v => v === form.pass && !!v || '两次密码必须一致'" v-model="form.pass2" :disabled="pending" />
		<var-input type="email" placeholder="请输入邮箱" :rules="emailRule" v-model="form.email" :disabled="pending || sending || captchaDialog" />
		<var-button text outline type="primary" native-type="button"
			:disabled="pending || sending || resendSeconds > 0" @click="openEmailVerification">
			{{ resendSeconds > 0 ? `${resendSeconds} 秒后重发` : '验证邮箱' }}
		</var-button>
		<var-input placeholder="请输入邮箱收到的六位验证码" inputmode="numeric" :maxlength="6"
			:rules="v => /^[0-9]{6}$/.test(v) || '请输入六位邮箱验证码'" v-model="form.emailCode" :disabled="pending || sending" />
		<div class="login">
			<var-button text outline type="primary" native-type="submit" :loading="pending" :disabled="pending || sending">注册</var-button>
			<var-button text outline type="primary" native-type="button" :disabled="pending || sending"
				@click="router.replace({ name: 'login', query: route.query })">返回登录</var-button>
		</div>
	</var-form>
	<var-dialog v-model:show="captchaDialog" title="验证邮箱" confirm-button-text="提交验证"
		:confirm-button-loading="sending" :confirm-button-disabled="captchaLoading || !captcha || resendSeconds > 0"
		:cancel-button-disabled="sending" :close-on-click-overlay="!sending" :close-on-key-escape="!sending"
		@before-close="beforeCaptchaClose">
		<div class="captcha-dialog-content">
			<p>验证后将发送邮件到 {{ dialogEmail }}</p>
			<div class="captcha-row">
				<img v-if="captcha" :src="`data:${captcha.mime_type};base64,${captcha.image_base64}`" alt="六位数字验证码" />
				<span v-else>{{ captchaLoading ? '验证码加载中…' : '验证码未加载' }}</span>
				<var-button text type="primary" native-type="button" :disabled="sending || captchaLoading"
					@click="refreshCaptcha">刷新</var-button>
			</div>
			<var-input placeholder="请输入图片中的六位数字" type="text" inputmode="numeric" :maxlength="6"
				v-model="form.captchaCode" :disabled="sending || captchaLoading" @keyup.enter="beforeCaptchaClose('confirm', () => { captchaDialog = false })" />
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
		.var-input {
			min-height: 80px;
		}
		.login {
			height: 80px;
			width: 240px;
			display: flex;
			justify-content: space-between;
			align-items: center;
		}
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
