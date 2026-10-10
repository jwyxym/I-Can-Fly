import { file } from "./file";

export const applicationIconFallback = './icon.jpg';
const cache = new Map<string, Promise<string>>();
const queue: (() => Promise<void>)[] = [];
const concurrency = 4;
const cacheLimit = 256;
let active = 0;

function loadNext(): void {
	while (active < concurrency && queue.length) {
		const task = queue.shift()!;
		active++;
		void task().finally(() => {
			active--;
			loadNext();
		});
	}
}

/** source 为 Android 包名或 Windows EXE 路径，共享缓存并限制原生读取并发。 */
export function getApplicationIcon(source: string): Promise<string> {
	if (!source) return Promise.resolve(applicationIconFallback);
	const cached = cache.get(source);
	if (cached) {
		cache.delete(source);
		cache.set(source, cached);
		return cached;
	}
	const request = new Promise<string>(resolve => {
		queue.push(async () => {
			try {
				resolve(await file.getApplicationIcon(source) || applicationIconFallback);
			} catch {
				resolve(applicationIconFallback);
			}
		});
	});
	cache.set(source, request);
	if (cache.size > cacheLimit) cache.delete(cache.keys().next().value!);
	loadNext();
	return request;
}
