use serde::{Deserialize, Serialize};
use tauri::{
    plugin::{Builder, TauriPlugin},
    Runtime,
};

#[cfg(target_os = "windows")]
mod windows_icon;

#[cfg(target_os = "android")]
struct FileHandle<R: Runtime>(tauri::plugin::PluginHandle<R>);

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("file")
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            {
                use tauri::Manager;
                let handle = _api.register_android_plugin("i.can.fly.file", "FilePlugin")?;
                _app.manage(FileHandle(handle));
            }
            Ok(())
        })
        .build()
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "platform", rename_all = "lowercase")]
pub enum Application {
    Android {
        name: String,
        #[serde(rename = "packageName")]
        package_name: String,
    },
    Windows {
        name: String,
        #[serde(rename = "executablePath")]
        executable_path: String,
    },
}

#[tauri::command]
pub async fn file_list_applications(app: tauri::AppHandle) -> Result<Vec<Application>, String> {
    #[cfg(target_os = "android")]
    {
        #[derive(Deserialize)]
        struct ApplicationList {
            applications: Vec<Application>,
        }
        let result = tauri::async_runtime::spawn_blocking(move || {
            use tauri::Manager;
            app.state::<FileHandle<tauri::Wry>>()
                .0
                .run_mobile_plugin("listApplications", serde_json::json!({}))
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())??;
        serde_json::from_value::<ApplicationList>(result)
            .map(|result| result.applications)
            .map_err(|error| format!("Invalid application list: {error}"))
    }
    #[cfg(target_os = "windows")]
    {
        let _ = app;
        tauri::async_runtime::spawn_blocking(|| {
            use std::os::windows::process::CommandExt;
            let system_root = std::env::var_os("SystemRoot").ok_or("SystemRoot is not set")?;
            let powershell = std::path::PathBuf::from(system_root)
                .join("System32/WindowsPowerShell/v1.0/powershell.exe");
            let output = std::process::Command::new(powershell)
                .args([
                    "-NoLogo",
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    include_str!("desktop_applications.ps1"),
                ])
                .creation_flags(0x08000000) // CREATE_NO_WINDOW
                .output()
                .map_err(|error| format!("Cannot list desktop applications: {error}"))?;
            if !output.status.success() {
                return Err(format!(
                    "Cannot list desktop applications: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
            }
            serde_json::from_slice::<Vec<Application>>(&output.stdout)
                .map_err(|error| format!("Invalid desktop application list: {error}"))
        })
        .await
        .map_err(|error| error.to_string())?
    }
    #[cfg(not(any(target_os = "android", target_os = "windows")))]
    {
        let _ = app;
        Err("Application listing is supported on Android and Windows only".into())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutableFile {
    platform: &'static str,
    name: String,
    executable_path: String,
}

#[tauri::command]
pub async fn file_get_application_icon(
    app: tauri::AppHandle,
    source: String,
) -> Result<String, String> {
    #[cfg(target_os = "android")]
    {
        if source.trim().is_empty() {
            return Err("packageName must not be empty".into());
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct ApplicationIcon {
            data_url: String,
        }
        tauri::async_runtime::spawn_blocking(move || {
            use tauri::Manager;
            let icon: ApplicationIcon = app
                .state::<FileHandle<tauri::Wry>>()
                .0
                .run_mobile_plugin(
                    "getApplicationIcon",
                    serde_json::json!({ "packageName": source }),
                )
                .map_err(|error| error.to_string())?;
            Ok(icon.data_url)
        })
        .await
        .map_err(|error| error.to_string())?
    }
    #[cfg(target_os = "windows")]
    {
        let _ = app;
        tauri::async_runtime::spawn_blocking(move || windows_icon::load(&source))
            .await
            .map_err(|error| error.to_string())?
    }
    #[cfg(not(any(target_os = "android", target_os = "windows")))]
    {
        let _ = (app, source);
        Err("Application icons are only supported on Android and Windows".into())
    }
}

#[tauri::command]
pub async fn file_scan_executables(directory: String) -> Result<Vec<ExecutableFile>, String> {
    #[cfg(target_os = "windows")]
    {
        tauri::async_runtime::spawn_blocking(move || scan_executables(&directory))
            .await
            .map_err(|error| error.to_string())?
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = directory;
        Err("Executable folder scanning is only supported on Windows".into())
    }
}

#[cfg(target_os = "windows")]
fn scan_executables(directory: &str) -> Result<Vec<ExecutableFile>, String> {
    use std::{collections::BTreeMap, fs, os::windows::fs::MetadataExt, path::Path};
    let root = Path::new(directory);
    if !root.is_absolute() {
        return Err("directory must be an absolute folder path".into());
    }
    let root = fs::canonicalize(root)
        .map_err(|error| format!("Cannot open folder {directory}: {error}"))?;
    if !root.is_dir() {
        return Err("directory must point to a folder".into());
    }
    let mut pending = vec![root];
    let mut files = BTreeMap::new();
    while let Some(folder) = pending.pop() {
        let entries = fs::read_dir(&folder)
            .map_err(|error| format!("Cannot scan folder {}: {error}", folder.display()))?;
        for entry in entries {
            let entry = entry
                .map_err(|error| format!("Cannot read entry in {}: {error}", folder.display()))?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)
                .map_err(|error| format!("Cannot inspect {}: {error}", path.display()))?;
            // FILE_ATTRIBUTE_REPARSE_POINT：不跟随符号链接、junction 等重解析点。
            if metadata.file_attributes() & 0x400 != 0 {
                continue;
            }
            if metadata.is_dir() {
                pending.push(path);
            } else if metadata.is_file()
                && path
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
            {
                let raw_path = path.to_str().ok_or_else(|| {
                    format!("Executable path is not valid Unicode: {}", path.display())
                })?;
                // canonicalize 返回扩展路径；转换为使用正斜杠的普通绝对路径。
                let executable_path = if let Some(unc) = raw_path.strip_prefix(r"\\?\UNC\") {
                    format!("//{}", unc.replace('\\', "/"))
                } else {
                    raw_path
                        .strip_prefix(r"\\?\")
                        .unwrap_or(raw_path)
                        .replace('\\', "/")
                };
                files.insert(
                    executable_path.clone(),
                    ExecutableFile {
                        platform: "windows",
                        name: entry.file_name().to_string_lossy().into_owned(),
                        executable_path,
                    },
                );
            }
        }
    }
    Ok(files.into_values().collect())
}
