<script setup lang="ts">
	import { onMounted, reactive, ref } from "vue";
	import { Snackbar } from '@varlet/ui';
	import { useRoute, useRouter } from "vue-router";
	import { api, ApiError, type CaptchaResponse } from "../script/api";
	import { auth } from "../script/auth";

	const route = useRoute();
	const router = useRouter();

	const form = reactive({
		user : '',
		pass : '',
		captchaCode: '',
	})

	const pending = ref(false);
	const captcha = ref<CaptchaResponse | null>(null);
	const captchaLoading = ref(false);

	async function refreshCaptcha(): Promise<void> {
		if (captchaLoading.value) return;
		captchaLoading.value = true;
		captcha.value = null;
		form.captchaCode = '';
		try {
			captcha.value = await api.getCaptcha();
		} catch (error) {
			Snackbar.error(error instanceof Error ? `获取验证码失败：${error.message}` : '获取验证码失败，请重试。')
		} finally {
			captchaLoading.value = false;
		}
	}

	onMounted(() => { void refreshCaptcha(); });

	async function submit(): Promise<void> {
		if (pending.value || captchaLoading.value) return;
		const username = form.user.trim();
		const password = form.pass;
		if (!username || !password) return;
		const currentCaptcha = captcha.value;
		if (!currentCaptcha || currentCaptcha.expires_at * 1000 <= Date.now()) {
			Snackbar.error('验证码已过期或未加载，请输入新验证码。');
			await refreshCaptcha();
			return;
		}
		if (!/^[0-9]{6}$/.test(form.captchaCode)) {
			Snackbar.error('请输入图片中的六位数字验证码。');
			return;
		}
		pending.value = true;
		try {
			await auth.login({ username, password,
				captcha_id: currentCaptcha.captcha_id, captcha_code: form.captchaCode });
			form.pass = '';
			const redirect = route.query.redirect;
			const target = typeof redirect === "string" && redirect.startsWith("/")
				&& !redirect.startsWith("//") && router.resolve(redirect).name !== "login"
				? redirect : "/";
			await router.replace(target);
		} catch (error) {
			let errorMessage;
			if (error instanceof ApiError && error.code === 'captcha_invalid_or_expired') {
				errorMessage = '验证码错误或已过期，请输入新验证码。';
			} else if (error instanceof ApiError && error.status === 401) {
				errorMessage = "用户名或密码错误。";
			} else if (error instanceof ApiError && error.code === "ECONNABORTED") {
				errorMessage = "登录请求超时，请稍后重试。";
			} else if (error instanceof ApiError && error.code === "ERR_BAD_RESPONSE") {
				errorMessage = "服务器响应读取失败，请稍后重试。";
			} else if (error instanceof ApiError && error.status === null) {
				errorMessage = "无法连接服务器，请检查网络和 API 地址。";
			} else {
				errorMessage = error instanceof Error ? error.message : "登录失败，请稍后重试。";
			}
			Snackbar.error(errorMessage)
		} finally {
			// 每次尝试都会消耗图片；失败或超时后必须刷新。成功登录后页面会离开。
			if (!auth.isAuthenticated()) await refreshCaptcha();
			pending.value = false;
		}
	}
</script>

<template>
	<var-form 
		@submit="submit"
	>
		<var-input
			placeholder="请输入用户名"
			:rules="v => !!v.trim() || '用户名不能为空'"
			v-model="form.user"
		/>
		<var-input
			type="password"
			placeholder="请输入密码"
			:rules="v => !!v.trim() || '密码不能为空'"
			v-model="form.pass"
		/>
		<var-input
			type="number"
			placeholder="请输入验证码"
			:maxlength="6"
			v-model="form.captchaCode"
			:disabled="pending || captchaLoading"
		/>
		<div class="captcha-row">
			<img v-if="captcha" :src="`data:${captcha.mime_type};base64,${captcha.image_base64}`" alt="六位数字验证码" />
			<span v-else>{{ captchaLoading ? '验证码加载中…' : '验证码未加载' }}</span>
			<var-button text type="primary" :disabled="pending || captchaLoading" @click="refreshCaptcha">刷新</var-button>
		</div>
		<div class="login">
			<var-button
				text outline
				type="primary"
				native-type="submit"
				:loading="pending" :disabled="pending || captchaLoading || !captcha"
			>
				登录
			</var-button>
			<var-button
				text outline
				type="primary"
				:loading="pending" :disabled="pending"
				@click="router.replace('sign')"
			>
				注册
			</var-button>
		</div>
	</var-form>
</template>

<style scoped lang="scss">
	.var-form {
		width: 100%;
		height: 100%;
		display: flex;
		flex-direction: column;
		justify-content: center;
		align-items: center;
		> * {
			width: 280px;
		}
		.var-input {
			height: 80px;
		}
		.captcha-row {
			height: 80px;
			display: flex;
			align-items: center;
			justify-content: space-between;
			gap: 12px;
			img {
				max-width: 200px;
				max-height: 80px;
				object-fit: contain;
			}
			.var-button {
				width: auto;
			}
		}
		.login {
			height: 80px;
			width: 220px;
			display: flex;
			justify-content: space-between;
			align-items: center;
		}
	}
</style>
