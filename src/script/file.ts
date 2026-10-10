import { invoke } from "@tauri-apps/api/core";

export interface ExecutableFile {
	platform: "windows";
	name: string;
	/** 使用正斜杠的可执行文件绝对路径。 */
	executablePath: string;
}

export type Application = {
	platform: "android";
	name: string;
	packageName: string;
} | {
	platform: "windows";
	name: string;
	executablePath: string;
};

export const file = {
	/** Android 传包名，Windows 传 EXE 绝对路径；返回 96 × 96 PNG Data URL，失败时报错。 */
	getApplicationIcon: async (source: string) => {
		try {
			return await invoke<string>(
				"file_get_application_icon", { source },
			);
		} catch {
			return './icon.jpg';
		}
	},
	/** Android 查询桌面应用；Windows 查询开始菜单和桌面的 exe 快捷方式。其他平台暂不支持。 */
	listApplications: () => invoke<Application[]>("file_list_applications"),
	/** 仅 Windows：递归扫描绝对目录中的 .exe（含当前目录），跳过链接；读取失败时报错。 */
	scanExecutables: (directory: string) => invoke<ExecutableFile[]>(
		"file_scan_executables", { directory },
	),
};
