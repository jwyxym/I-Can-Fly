import axios, { type AxiosRequestConfig } from "axios";
import { tauriHttpAdapter } from "./tauriHttpAdapter";

export const apiClient = axios.create({
	adapter: tauriHttpAdapter,
	baseURL: import.meta.env.VITE_API_BASE_URL?.trim() ?? "",
	timeout: 15_000,
	headers: { Accept: "application/json" },
});

export interface User {
	id: string;
	username: string;
	is_admin: boolean;
	disabled: boolean;
	/** Unix 秒；新用户为 0。 */
	active_until: number;
	active: boolean;
	created_at: number;
}

export interface Credentials {
	username: string;
	password: string;
}

export interface CaptchaVerification {
	captcha_id: string;
	/** 六位数字字符串，保留开头的 0；每次提交后需要获取新图片。 */
	captcha_code: string;
}

export interface CaptchaResponse {
	captcha_id: string;
	image_base64: string;
	mime_type: string;
	/** Unix 秒；有效期 120 秒。 */
	expires_at: number;
}

export interface LoginRequest extends Credentials, CaptchaVerification {}

export interface RegisterRequest extends Credentials {
	email: string;
	/** 用途为 register 的六位邮箱验证码。 */
	email_code: string;
}

export type EmailCodePurpose = "register" | "change_password" | "change_email_old" | "change_email_new";

export type SendEmailCodeRequest = CaptchaVerification & (
	| { purpose: "register"; email: string }
	// 修改密码的邮件只能发到当前绑定邮箱，不允许指定 email。
	| { purpose: "change_password"; email?: never }
	// 换绑两种用途均传目标新邮箱，old 的收件地址由服务端决定。
	| { purpose: "change_email_old" | "change_email_new"; email: string }
);

export interface SendEmailCodeResponse extends OkResponse {
	expires_at: number;
	resend_after_seconds: number;
}

/** 仅 /api/me 返回 email；登录和注册返回的 User 不带此字段。 */
export interface CurrentUser extends User {
	email: string | null;
}

export interface LoginResponse {
	token: string;
	token_type: "Bearer";
	/** Unix 秒，无刷新 Token 接口。 */
	expires_at: number;
	user: User;
}

export interface ChangePasswordRequest extends CaptchaVerification {
	old_password: string;
	new_password: string;
	email_code: string;
}

export interface ChangeEmailRequest {
	password: string;
	new_email: string;
	old_email_code: string;
	new_email_code: string;
}

export interface OkResponse {
	ok: true;
}

export interface ChangePasswordResponse extends OkResponse {
	login_required: true;
}

export interface ChangeEmailResponse extends OkResponse {
	login_required: true;
}

export interface RedeemActivationResponse {
	/** Unix 秒；不会延长已经创建的会话。 */
	active_until: number;
}

export type ConnectionStatus = "provisioning" | "active" | "closing" | "closed";

export interface ConnectionSession {
	id: string;
	status: ConnectionStatus;
	created_at: number;
	expires_at: number;
	heartbeat_at: number;
	/** 曾被观察到在线，不代表当前在线。 */
	seen_online: boolean;
	closed_at: number | null;
	close_reason: string | null;
}

export type JsonValue = string | number | boolean | null | JsonObject | JsonValue[];
export interface JsonObject {
	[key: string]: JsonValue;
}

/** 保留服务端 Xray 配置，不转换字段或自动替换代理地址。 */
export type XrayConfig = JsonObject & {
	inbounds: JsonObject[];
	outbounds: JsonObject[];
};

export interface CreateConnectionResponse {
	session_id: string;
	expires_at: number;
	heartbeat_interval_seconds: number;
	/** true 时调用方需补齐 VLESS vnext / Trojan servers 中的 address。 */
	requires_server_address: boolean;
	config: XrayConfig;
}

export interface ConnectionListResponse {
	/** 最多 100 条，包含历史记录；不包含配置或临时凭据。 */
	items: ConnectionSession[];
}

/** status 为 null 表示没有收到 HTTP 响应；code 保留服务端 error 或 Axios 错误码。 */
export class ApiError extends Error {
	constructor(
		message: string,
		public readonly status: number | null,
		public readonly code: string | null,
		public readonly responseBody: unknown,
	) {
		super(message);
		this.name = "ApiError";
	}
}

// Token 仅存于内存；如由调用方持久化，恢复后通过 setApiToken 设置。
let token: string | null = null;

export function setApiToken(value: string | null): void {
	token = value?.trim() || null;
}

function clearTokenIfCurrent(value: string | null): void {
	// 旧请求完成时，不清除用户在此期间重新登录获得的新 Token。
	if (token === value) token = null;
}

async function request<T>(config: AxiosRequestConfig, authenticated = true): Promise<T> {
	try {
		const response = await apiClient.request<T>({
			...config,
			headers: {
				...config.headers,
				...(authenticated && token ? { Authorization: `Bearer ${token}` } : {}),
			},
		});
		return response.data;
	} catch (error) {
		if (!axios.isAxiosError(error)) throw error;
		const body: unknown = error.response?.data;
		const businessCode = typeof body === "object" && body !== null && "error" in body
			&& typeof body.error === "string" ? body.error : null;
		const message = businessCode ?? (typeof body === "string" && body.trim() ? body : error.message);
		throw new ApiError(message, error.response?.status ?? null, businessCode ?? error.code ?? null, body);
	}
}

function connectionPath(id: string): string {
	if (!id.trim()) throw new Error("sessionId must not be empty");
	return `/api/connections/${encodeURIComponent(id)}`;
}

/** 普通用户接口；返回响应 JSON 本体，不额外包裹 data。所有时间戳及间隔均为秒。 */
export const api = {
	/** 获取一次性数字图片验证码，无需登录。 */
	getCaptcha: () => request<CaptchaResponse>({ method: "GET", url: "/api/auth/captcha" }, false),

	/** 发送邮件会消耗图片验证码。注册、换绑最终提交无需图片；修改密码仍需新图片。 */
	sendEmailCode: (data: SendEmailCodeRequest) => request<SendEmailCodeResponse>({
		method: "POST", url: "/api/auth/email-code", data,
	}, data.purpose !== "register"),

	/** 注册仅提交四个字段，不携带图片验证码，也不会自动登录。 */
	register: (data: RegisterRequest) => request<User>({
		method: "POST", url: "/api/auth/register",
		data: { username: data.username, password: data.password, email: data.email, email_code: data.email_code },
	}, false),

	async login(data: LoginRequest): Promise<LoginResponse> {
		const result = await request<LoginResponse>({ method: "POST", url: "/api/auth/login", data }, false);
		setApiToken(result.token);
		return result;
	},

	/** 只撤销当前 Token；需要断开代理时，先关闭会话和本地内核。 */
	async logout(): Promise<OkResponse> {
		const currentToken = token;
		const result = await request<OkResponse>({ method: "POST", url: "/api/auth/logout" });
		clearTokenIfCurrent(currentToken);
		return result;
	},

	/** 成功后服务端撤销全部 Token，并将代理会话标记为待清理。 */
	async changePassword(data: ChangePasswordRequest): Promise<ChangePasswordResponse> {
		const currentToken = token;
		const result = await request<ChangePasswordResponse>({ method: "POST", url: "/api/auth/password", data });
		clearTokenIfCurrent(currentToken);
		return result;
	},

	/** 最终换绑不提交图片验证码；成功后全部 Token 失效，需要重新登录。 */
	async changeEmail(data: ChangeEmailRequest): Promise<ChangeEmailResponse> {
		const currentToken = token;
		const result = await request<ChangeEmailResponse>({
			method: "POST", url: "/api/auth/change-email",
			data: { password: data.password, new_email: data.new_email,
				old_email_code: data.old_email_code, new_email_code: data.new_email_code },
		});
		clearTokenIfCurrent(currentToken);
		return result;
	},

	getMe: () => request<CurrentUser>({ method: "GET", url: "/api/me" }),

	redeemActivationCode: (code: string) => request<RedeemActivationResponse>({
		method: "POST", url: "/api/activation/redeem", data: { code },
	}),

	/** 不自动重试：超时或响应丢失后先查询会话，凭据只返回一次。 */
	createConnection: () => request<CreateConnectionResponse>({ method: "POST", url: "/api/connections" }),

	listConnections: () => request<ConnectionListResponse>({ method: "GET", url: "/api/connections" }),

	heartbeat: (sessionId: string) => request<ConnectionSession>({
		method: "POST", url: `${connectionPath(sessionId)}/heartbeat`,
	}),

	closeConnection: (sessionId: string) => request<OkResponse>({ method: "DELETE", url: connectionPath(sessionId) }),
};
