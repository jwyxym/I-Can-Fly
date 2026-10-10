import { fetch } from "@tauri-apps/plugin-http";
import axios, { AxiosError, AxiosHeaders, CanceledError, type AxiosAdapter, type AxiosResponse } from "axios";

// 直接使用已安装的 HTTP 插件，避免第三方适配器内置旧版 IPC 读取协议。
export const tauriHttpAdapter: AxiosAdapter = async config => {
	const controller = new AbortController();
	let timedOut = false;
	let timeout: ReturnType<typeof setTimeout> | undefined;
	const abort = () => controller.abort();
	config.signal?.addEventListener?.("abort", abort);
	config.cancelToken?.subscribe(abort);
	if (config.signal?.aborted || config.cancelToken?.reason) controller.abort();
	if (config.timeout && config.timeout > 0) {
		timeout = setTimeout(() => {
			timedOut = true;
			controller.abort();
		}, config.timeout);
	}
	let response: AxiosResponse | undefined;
	try {
		if (controller.signal.aborted) throw new CanceledError("Request canceled", config);
		const headers = new Headers();
		for (const [name, value] of Object.entries(AxiosHeaders.from(config.headers).toJSON(true))) {
			if (value !== null && value !== undefined) headers.set(name, String(value));
		}
		const result = await fetch(axios.getUri(config), {
			method: config.method?.toUpperCase() ?? "GET",
			headers,
			body: config.data as BodyInit | null | undefined,
			signal: controller.signal,
		});
		response = {
			data: null,
			status: result.status,
			statusText: result.statusText,
			headers: AxiosHeaders.from(Object.fromEntries(result.headers.entries())),
			config,
		};
		// JSON 以文本交给 Axios 的 transformResponse 解析，保留非 JSON 错误正文。
		switch (config.responseType) {
			case "arraybuffer": response.data = await result.arrayBuffer(); break;
			case "blob": response.data = await result.blob(); break;
			default: response.data = await result.text(); break;
		}
		if (config.validateStatus && !config.validateStatus(response.status)) {
			throw new AxiosError(`Request failed with status code ${response.status}`,
				response.status >= 500 ? AxiosError.ERR_BAD_RESPONSE : AxiosError.ERR_BAD_REQUEST,
				config, undefined, response);
		}
		return response;
	} catch (error) {
		if (timedOut) throw new AxiosError(`timeout of ${config.timeout}ms exceeded`, AxiosError.ECONNABORTED, config);
		if (controller.signal.aborted) throw new CanceledError("Request canceled", config);
		if (axios.isAxiosError(error)) throw error;
		throw AxiosError.from(error, response ? AxiosError.ERR_BAD_RESPONSE : AxiosError.ERR_NETWORK,
			config, undefined, response);
	} finally {
		if (timeout !== undefined) clearTimeout(timeout);
		config.signal?.removeEventListener?.("abort", abort);
		config.cancelToken?.unsubscribe(abort);
	}
};
