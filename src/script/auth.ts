import axios from "axios";
import { invoke } from "@tauri-apps/api/core";
import { readonly, shallowRef } from "vue";
import { api, apiClient, setApiToken, type LoginRequest, type LoginResponse } from "./api";

const session = shallowRef<LoginResponse | null>(null);
const storageError = shallowRef("");
let restorePromise: Promise<void> | null = null;
let storageQueue: Promise<void> = Promise.resolve();

function storageOperation<T>(operation: () => Promise<T>): Promise<T> {
	const result = storageQueue.then(operation);
	storageQueue = result.then(() => {}, () => {});
	return result;
}

function reportStorageError(error: unknown): void {
	storageError.value = `无法访问登录数据库：${error instanceof Error ? error.message : String(error)}`;
}

function isLoginResponse(value: unknown): value is LoginResponse {
	if (typeof value !== "object" || value === null) return false;
	const data = value as Partial<LoginResponse>;
	const user = data.user;
	return typeof data.token === "string" && data.token.trim().length > 0
		&& data.token_type === "Bearer"
		&& typeof data.expires_at === "number" && Number.isFinite(data.expires_at)
		&& typeof user === "object" && user !== null
		&& typeof user.id === "string" && typeof user.username === "string"
		&& typeof user.is_admin === "boolean" && typeof user.disabled === "boolean"
		&& typeof user.active_until === "number" && typeof user.active === "boolean"
		&& typeof user.created_at === "number";
}

async function clear(): Promise<void> {
	const token = session.value?.token ?? null;
	session.value = null;
	setApiToken(null);
	try {
		await storageOperation(() => invoke<void>("auth_clear", { token }));
		storageError.value = "";
	} catch (error) {
		reportStorageError(error);
		throw error;
	}
}

function isAuthenticated(): boolean {
	if (!session.value) return false;
	if (session.value.expires_at * 1000 <= Date.now()) {
		void clear().catch(() => {});
		return false;
	}
	return true;
}

async function restore(): Promise<void> {
	try {
		const data: unknown = await storageOperation(() => invoke<LoginResponse | null>("auth_load"));
		storageError.value = "";
		if (data === null) return;
		if (!isLoginResponse(data) || data.expires_at * 1000 <= Date.now()) {
			await clear();
			return;
		}
		session.value = data;
		setApiToken(data.token);
	} catch (error) {
		session.value = null;
		setApiToken(null);
		reportStorageError(error);
	}
}

function ready(): Promise<void> {
	return restorePromise ??= restore();
}

async function login(credentials: LoginRequest): Promise<void> {
	await ready();
	const result = await api.login(credentials);
	if (!isLoginResponse(result) || result.expires_at * 1000 <= Date.now()) {
		setApiToken(null);
		throw new Error("登录响应无效或已过期，请重新登录。");
	}
	// 仅保存服务端返回的 Token、有效期和用户信息，不保存密码。
	const data: LoginResponse = {
		token: result.token,
		token_type: result.token_type,
		expires_at: result.expires_at,
		user: result.user,
	};
	try {
		await storageOperation(() => invoke<void>("auth_save", { data }));
		storageError.value = "";
	} catch (error) {
		setApiToken(null);
		reportStorageError(error);
		throw new Error(storageError.value);
	}
	session.value = data;
}

apiClient.interceptors.response.use(
	response => {
		const url = response.config.url;
		if (session.value && response.config.headers.Authorization === `Bearer ${session.value.token}`
			&& (url === "/api/auth/logout"
				|| ((url === "/api/auth/password" || url === "/api/auth/change-email")
					&& response.data?.login_required === true))) {
			void clear().catch(() => {});
		}
		return response;
	},
	error => {
		// 只清除本次请求所使用的会话，避免旧请求覆盖重新登录后的状态。
		if (axios.isAxiosError(error) && error.response?.status === 401 && session.value
			&& error.config?.headers.Authorization === `Bearer ${session.value.token}`) {
			void clear().catch(() => {});
		}
		return Promise.reject(error);
	},
);

export const auth = {
	session: readonly(session),
	storageError: readonly(storageError),
	ready,
	login,
	clear,
	isAuthenticated,
};
