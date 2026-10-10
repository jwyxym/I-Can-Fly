use std::{
    ffi::{c_char, c_int, CStr, CString},
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

use libloading::Library;
use serde::Serialize;
#[cfg(not(target_os = "android"))]
use tauri::Manager;

fn library_path(_path: PathBuf) -> Result<String, String> {
    // 与 Windbot 相同：Android 交给 native linker 搜索 APK 中的库，桌面使用资源目录。
    #[cfg(target_os = "android")]
    return Ok("libxray.so".into());
    #[cfg(not(target_os = "android"))]
    {
        let file = if cfg!(target_os = "windows") {
            "xray.dll"
        } else if cfg!(target_os = "macos") {
            "libxray.dylib"
        } else {
            "libxray.so"
        };
        Ok(_path.join(file).to_string_lossy().into_owned())
    }
}

type StartArgs = unsafe extern "C" fn(*const c_char) -> c_int;
type Stop = unsafe extern "C" fn() -> c_int;
type LastError = unsafe extern "C" fn() -> *mut c_char;
type FreeString = unsafe extern "C" fn(*mut c_char);

#[derive(Clone, Copy)]
struct Api {
    start_args: StartArgs,
    stop: Stop,
    last_error: LastError,
    free_string: FreeString,
}

impl Api {
    fn load(library: &'static Library) -> Result<Self, String> {
        // ABI 与 Xray-core/main/shared/exports.go 一致，库在整个进程生命周期内有效。
        unsafe {
            Ok(Self {
                start_args: *library
                    .get::<StartArgs>(b"XrayStartArgs\0")
                    .map_err(|e| e.to_string())?,
                stop: *library
                    .get::<Stop>(b"XrayStop\0")
                    .map_err(|e| e.to_string())?,
                last_error: *library
                    .get::<LastError>(b"XrayLastError\0")
                    .map_err(|e| e.to_string())?,
                free_string: *library
                    .get::<FreeString>(b"XrayFreeString\0")
                    .map_err(|e| e.to_string())?,
            })
        }
    }

    fn error(self, operation: &str, code: c_int) -> String {
        // 调用方持有宿主锁，错误不会被并发启停操作覆盖。
        unsafe {
            let pointer = (self.last_error)();
            if pointer.is_null() {
                return format!("{operation} failed ({code}), XrayLastError returned NULL");
            }
            let message = CStr::from_ptr(pointer).to_string_lossy().into_owned();
            (self.free_string)(pointer);
            format!("{operation} failed ({code}): {message}")
        }
    }
}

struct LoadedLibrary {
    path: String,
    api: Result<Api, String>,
}

#[derive(Default)]
struct Xray {
    library: Option<LoadedLibrary>,
    running: bool,
    mode: Option<&'static str>,
    last_error: Option<String>,
    #[cfg(target_os = "android")]
    vpn_fd: Option<std::os::fd::OwnedFd>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct XrayStatus {
    pub(crate) loaded: bool,
    pub(crate) ready: bool,
    pub(crate) running: bool,
    pub(crate) mode: Option<&'static str>,
    pub(crate) library_path: Option<String>,
    pub(crate) last_error: Option<String>,
}

impl Xray {
    fn status(&self) -> XrayStatus {
        XrayStatus {
            loaded: self.library.is_some(),
            ready: self
                .library
                .as_ref()
                .is_some_and(|library| library.api.is_ok()),
            running: self.running,
            mode: self.mode,
            library_path: self.library.as_ref().map(|library| library.path.clone()),
            last_error: self.last_error.clone(),
        }
    }

    fn start(&mut self, library_path: String, sets: Vec<String>) -> Result<(), String> {
        if self.running {
            return Err("Xray is already running; stop it before starting again".into());
        }
        if library_path.trim().is_empty() || library_path.contains('\0') {
            return Err(
                "libraryPath must be a non-empty library path without NUL characters".into(),
            );
        }
        if sets.is_empty() {
            return Err("Provide at least one --set assignment".into());
        }
        let mut args = Vec::with_capacity(sets.len() * 2);
        for (index, assignment) in sets.into_iter().enumerate() {
            let (key, _) = assignment
                .split_once('=')
                .ok_or_else(|| format!("sets[{index}] must use path=value syntax"))?;
            if key.trim().is_empty() || assignment.contains('\0') {
                return Err(format!("sets[{index}] has an empty path or NUL character"));
            }
            args.push("--set".to_owned());
            args.push(assignment);
        }
        let json = serde_json::to_string(&args).map_err(|e| e.to_string())?;
        let args = CString::new(json).map_err(|e| e.to_string())?;

        if let Some(library) = &self.library {
            if library.path != library_path {
                return Err(
                    "Xray library is already loaded; restart the application to change libraryPath"
                        .into(),
                );
            }
        } else {
            // Library::new 会执行库初始化代码；路径必须指向可信的本机 Xray 构建产物。
            let library = unsafe { Library::new(&library_path) }
                .map_err(|e| format!("Cannot load Xray library: {e}"))?;
            // Go c-shared 不支持运行时卸载。即使缺失符号，也保留已加载的库。
            let library = Box::leak(Box::new(library));
            self.library = Some(LoadedLibrary {
                path: library_path,
                api: Api::load(library).map_err(|e| format!("Invalid Xray C ABI: {e}")),
            });
        }
        let api = self
            .library
            .as_ref()
            .expect("library was loaded")
            .api
            .clone()?;
        // XrayStartArgs 只在调用期间读取 NUL 结尾的 UTF-8 JSON 数组。
        let result = unsafe { (api.start_args)(args.as_ptr()) };
        if result != 0 {
            return Err(api.error("XrayStartArgs", result));
        }
        self.running = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), String> {
        if !self.running {
            return Ok(());
        }
        let api = self
            .library
            .as_ref()
            .expect("running Xray has a library")
            .api
            .clone()?;
        let result = unsafe { (api.stop)() };
        if result != 0 {
            return Err(api.error("XrayStop", result));
        }
        self.running = false;
        self.mode = None;
        Ok(())
    }
}

// 与 Go 库的进程级单实例保持一致，锁覆盖操作及随后的错误字符串读取。
static XRAY: OnceLock<Mutex<Xray>> = OnceLock::new();

fn instance() -> &'static Mutex<Xray> {
    XRAY.get_or_init(|| Mutex::new(Xray::default()))
}

fn operate(operation: impl FnOnce(&mut Xray) -> Result<(), String>) -> Result<XrayStatus, String> {
    let mut xray = instance()
        .lock()
        .map_err(|_| "Xray state lock is poisoned".to_owned())?;
    match operation(&mut xray) {
        Ok(()) => {
            xray.last_error = None;
            Ok(xray.status())
        }
        Err(error) => {
            xray.last_error = Some(error.clone());
            Err(error)
        }
    }
}

pub(crate) async fn start<R: tauri::Runtime>(
    _app: tauri::AppHandle<R>,
    sets: Vec<String>,
    mode: &'static str,
) -> Result<XrayStatus, String> {
    #[cfg(not(target_os = "android"))]
    let path: PathBuf = _app.path().resource_dir().map_err(|e| e.to_string())?;
    #[cfg(target_os = "android")]
    let path = PathBuf::new();
    tauri::async_runtime::spawn_blocking(move || {
        operate(|xray| {
            xray.start(library_path(path)?, sets)?;
            xray.mode = Some(mode);
            Ok(())
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

pub(crate) async fn stop() -> Result<XrayStatus, String> {
    tauri::async_runtime::spawn_blocking(|| {
        operate(|xray| {
            #[cfg(target_os = "android")]
            if xray.vpn_fd.is_some() {
                // TUN 内核由 Android 服务顺序关闭，统一 disconnect 这里只处理端口模式。
                return Ok(());
            }
            xray.stop()
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

pub(crate) async fn status() -> Result<XrayStatus, String> {
    tauri::async_runtime::spawn_blocking(|| {
        instance()
            .lock()
            .map(|xray| xray.status())
            .map_err(|_| "Xray state lock is poisoned".to_owned())
    })
    .await
    .map_err(|e| e.to_string())?
}

pub fn shutdown() {
    if XRAY.get().is_some() {
        if let Err(error) = operate(Xray::stop) {
            eprintln!("Cannot stop Xray on exit: {error}");
        }
    }
}

#[cfg(target_os = "android")]
pub(crate) fn start_vpn(
    mut sets: Vec<String>,
    fd: i32,
    mtu: i32,
    inbound: usize,
) -> Result<(), String> {
    use std::os::fd::{AsRawFd, BorrowedFd};
    if fd < 0 || !(1280..=65535).contains(&mtu) || inbound > 1024 {
        return Err("Invalid VPN fd, MTU or TUN inbound index".into());
    }
    operate(|xray| {
        if xray.running || xray.vpn_fd.is_some() {
            return Err("Xray is already running; stop the existing session first".into());
        }
        // Android 服务在同步 JNI 调用期间持有原始 fd。内核单独持有副本，
        // 即使 XrayStop 失败，服务关闭原始 fd 也不会让内核读到被复用的 fd。
        let owned = unsafe { BorrowedFd::borrow_raw(fd) }
            .try_clone_to_owned()
            .map_err(|e| e.to_string())?;
        // --set 不支持带点的字面量 key。合并整个 env 对象后设置两个别名，
        // 避免 Go 优先读取旧的 xray.tun.fd，或宿主 setenv 与 Go 环境缓存不一致。
        let mut env = serde_json::Map::new();
        let mut remaining = Vec::new();
        for assignment in sets.drain(..) {
            let (key, value) = assignment
                .split_once('=')
                .ok_or("Expected path=value in Xray sets")?;
            if key == "env" {
                env = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(value)
                    .map_err(|e| format!("Invalid env object: {e}"))?;
            } else if let Some(key) = key.strip_prefix("env.") {
                if key.is_empty() || key.contains(['.', '[', ']']) {
                    return Err(
                        "Use env={...} for environment keys containing dots or brackets".into(),
                    );
                }
                let value = serde_json::from_str(value)
                    .unwrap_or_else(|_| serde_json::Value::String(value.into()));
                env.insert(key.into(), value);
            } else {
                remaining.push(assignment);
            }
        }
        if env.values().any(|value| !value.is_string()) {
            return Err("Xray env values must be strings; quote numeric values".into());
        }
        let fd_value = serde_json::Value::String(owned.as_raw_fd().to_string());
        env.insert("xray.tun.fd".into(), fd_value.clone());
        env.insert("XRAY_TUN_FD".into(), fd_value);
        remaining.push(format!("env={}", serde_json::Value::Object(env)));
        remaining.push(format!("inbounds[{inbound}].protocol=tun"));
        // Android 使用传入的 fd，此名称仅用于内核配置和日志。
        // 显式指定以跳过 GetAvailableTunName 的网卡枚举，避免 netlink 权限错误。
        remaining.push(format!("inbounds[{inbound}].settings.name=android-vpn"));
        remaining.push(format!("inbounds[{inbound}].settings.mtu={mtu}"));
        // Android 路由由 VpnService 管理，宿主应用已排除，无需绑定物理接口。
        remaining.push(format!(
            "inbounds[{inbound}].settings.autoOutboundsInterface=\"\""
        ));
        remaining.push(format!(
            "inbounds[{inbound}].settings.autoSystemRoutingTable=[]"
        ));
        xray.start(library_path(PathBuf::new())?, remaining)?;
        xray.vpn_fd = Some(owned);
        xray.mode = Some("tun");
        Ok(())
    })
    .map(|_| ())
}

#[cfg(target_os = "android")]
pub(crate) fn stop_vpn() -> Result<(), String> {
    operate(|xray| {
        if xray.vpn_fd.is_some() {
            xray.stop()?;
            xray.vpn_fd = None;
        }
        Ok(())
    })
    .map(|_| ())
}
