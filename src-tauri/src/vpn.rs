use crate::proxy_routing::{ApplicationRouting, DomainRouting, RoutingMode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{
    plugin::{Builder, TauriPlugin},
    Runtime,
};

#[cfg(target_os = "android")]
struct VpnHandle<R: Runtime>(tauri::plugin::PluginHandle<R>);

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("vpn")
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            {
                use tauri::Manager;
                let handle = _api.register_android_plugin("i.can.fly.vpn", "VpnPlugin")?;
                _app.manage(VpnHandle(handle));
            }
            Ok(())
        })
        .build()
}

#[cfg(target_os = "android")]
fn call<R: Runtime>(
    app: &tauri::AppHandle<R>,
    command: &str,
    args: Value,
) -> Result<Value, String> {
    use tauri::Manager;
    app.state::<VpnHandle<R>>()
        .0
        .run_mobile_plugin(command, args)
        .map_err(|error| error.to_string())
}

#[derive(Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct VpnConfig {
    session: String,
    mtu: u32,
    addresses: Vec<String>,
    routes: Vec<String>,
    dns_servers: Vec<String>,
    allowed_applications: Vec<String>,
    disallowed_applications: Vec<String>,
    metered: bool,
}

impl Default for VpnConfig {
    fn default() -> Self {
        Self {
            session: "I Can Fly".into(),
            mtu: 1500,
            addresses: vec!["172.19.0.1/30".into()],
            routes: vec!["0.0.0.0/0".into()],
            dns_servers: vec!["1.1.1.1".into()],
            allowed_applications: Vec::new(),
            disallowed_applications: Vec::new(),
            metered: false,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VpnMode {
    Proxy,
    Tun,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationProxy {
    applications: Vec<String>,
    proxy_outbound_tag: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct XrayOptions {
    sets: Vec<String>,
    #[serde(default)]
    tun_inbound_index: usize,
}

#[cfg(target_os = "windows")]
fn windows_sets(config: VpnConfig, mut xray: XrayOptions) -> Result<Vec<String>, String> {
    use std::net::IpAddr;
    if config.session.trim().is_empty() || !(1280..=65535).contains(&config.mtu) {
        return Err("Invalid VPN session name or MTU".into());
    }
    if config.addresses.is_empty() || config.routes.is_empty() || xray.tun_inbound_index > 1024 {
        return Err(
            "VPN addresses/routes cannot be empty and TUN inbound index must be 0..1024".into(),
        );
    }
    if !config.allowed_applications.is_empty()
        || !config.disallowed_applications.is_empty()
        || config.metered
    {
        return Err(
            "Application allowlists, denylists and metered VPN are only supported on Android"
                .into(),
        );
    }
    for cidr in config.addresses.iter().chain(&config.routes) {
        let (address, prefix) = cidr
            .split_once('/')
            .ok_or_else(|| format!("Invalid CIDR: {cidr}"))?;
        let address: IpAddr = address
            .parse()
            .map_err(|_| format!("Invalid CIDR: {cidr}"))?;
        let prefix: u8 = prefix
            .parse()
            .map_err(|_| format!("Invalid CIDR: {cidr}"))?;
        if prefix > if address.is_ipv4() { 32 } else { 128 } {
            return Err(format!("Invalid CIDR: {cidr}"));
        }
    }
    for address in &config.dns_servers {
        address
            .parse::<IpAddr>()
            .map_err(|_| format!("Invalid DNS address: {address}"))?;
    }
    // 用 /1 路由覆盖默认路由，保留物理默认路由供 Xray 出站接口检测。
    let routes: Vec<String> = config
        .routes
        .into_iter()
        .flat_map(|route| match route.as_str() {
            "0.0.0.0/0" => vec!["0.0.0.0/1".into(), "128.0.0.0/1".into()],
            "::/0" => vec!["::/1".into(), "8000::/1".into()],
            _ => vec![route],
        })
        .collect();
    let inbound = format!("inbounds[{}]", xray.tun_inbound_index);
    for (path, value) in [
        ("protocol", serde_json::json!("tun")),
        ("settings.name", serde_json::json!(config.session)),
        ("settings.mtu", serde_json::json!(config.mtu)),
        ("settings.gateway", serde_json::json!(config.addresses)),
        ("settings.dns", serde_json::json!(config.dns_servers)),
        ("settings.autoSystemRoutingTable", serde_json::json!(routes)),
        ("settings.autoOutboundsInterface", serde_json::json!("auto")),
    ] {
        xray.sets.push(format!("{inbound}.{path}={value}"));
    }
    Ok(xray.sets)
}

#[tauri::command]
pub async fn vpn_connect(
    app: tauri::AppHandle,
    mode: Option<VpnMode>,
    vpn_config: Option<VpnConfig>,
    xray: XrayOptions,
    application_proxy: Option<ApplicationProxy>,
    mut application_routing: Option<ApplicationRouting>,
    mut domain_routing: Option<DomainRouting>,
    mut proxy_outbound_tag: Option<String>,
) -> Result<Value, String> {
    // 兼容旧的应用白名单参数，同时禁止重复定义应用策略。
    if let Some(legacy) = application_proxy {
        if application_routing.is_some() {
            return Err("Use applicationRouting or applicationProxy, not both".into());
        }
        if let Some(tag) = legacy.proxy_outbound_tag {
            if proxy_outbound_tag
                .as_ref()
                .is_some_and(|current| current != &tag)
            {
                return Err("Conflicting proxyOutboundTag values".into());
            }
            proxy_outbound_tag = Some(tag);
        }
        application_routing = Some(ApplicationRouting {
            mode: RoutingMode::Proxy,
            applications: legacy.applications,
        });
    }
    if proxy_outbound_tag
        .as_ref()
        .is_some_and(|tag| tag.trim().is_empty() || tag.contains('\0'))
    {
        return Err("proxyOutboundTag must not be empty or contain NUL characters".into());
    }
    if let Some(selection) = &mut application_routing {
        selection.normalize()?;
    }
    if let Some(selection) = &mut domain_routing {
        selection.normalize()?;
    }
    let mode =
        if cfg!(target_os = "android") || application_routing.is_some() || domain_routing.is_some()
        {
            VpnMode::Tun
        } else {
            mode.unwrap_or(VpnMode::Proxy)
        };
    if matches!(mode, VpnMode::Proxy) {
        #[cfg(target_os = "android")]
        {
            let handle = app.clone();
            let status = tauri::async_runtime::spawn_blocking(move || {
                call(&handle, "getStatus", serde_json::json!({}))
            })
            .await
            .map_err(|error| error.to_string())??;
            if ["running", "starting", "stopping", "permissionPending"]
                .iter()
                .any(|key| status[*key].as_bool().unwrap_or(false))
            {
                return Err("VPN is busy; disconnect before switching modes".into());
            }
        }
        // 保留前端 --set 的原始顺序和值，不注入 TUN、DNS、路由或 fd。
        crate::xray::start(app, xray.sets, "proxy").await?;
        return Ok(serde_json::json!({ "accepted": true }));
    }
    #[cfg(any(target_os = "android", target_os = "windows"))]
    let mut xray = xray;
    #[cfg(any(target_os = "android", target_os = "windows"))]
    if application_routing.is_some() || domain_routing.is_some() {
        xray.sets = crate::proxy_routing::configure(
            xray.sets,
            xray.tun_inbound_index,
            application_routing.as_ref(),
            domain_routing.as_ref(),
            proxy_outbound_tag.as_deref(),
            cfg!(target_os = "windows"),
        )?;
    }
    #[cfg(target_os = "android")]
    {
        let mut config = vpn_config.unwrap_or_default();
        if let Some(selection) = application_routing {
            config.allowed_applications.clear();
            config.disallowed_applications.clear();
            match selection.mode {
                RoutingMode::Proxy => config.allowed_applications = selection.applications,
                RoutingMode::Bypass => config.disallowed_applications = selection.applications,
            }
        }
        tauri::async_runtime::spawn_blocking(move || {
            call(
                &app,
                "connect",
                serde_json::json!({ "vpnConfig": config, "xray": xray }),
            )
        })
        .await
        .map_err(|error| error.to_string())?
    }
    #[cfg(target_os = "windows")]
    {
        crate::xray::start(
            app,
            windows_sets(vpn_config.unwrap_or_default(), xray)?,
            "tun",
        )
        .await?;
        Ok(serde_json::json!({ "accepted": true }))
    }
    #[cfg(not(any(target_os = "windows", target_os = "android")))]
    {
        let _ = (app, vpn_config, xray);
        Err("VPN is currently supported on Windows and Android only".into())
    }
}

#[tauri::command]
pub async fn vpn_disconnect(app: tauri::AppHandle) -> Result<Value, String> {
    #[cfg(target_os = "android")]
    {
        tauri::async_runtime::spawn_blocking(move || {
            call(&app, "disconnect", serde_json::json!({}))
        })
        .await
        .map_err(|error| error.to_string())??;
    }
    #[cfg(not(target_os = "android"))]
    let _ = app;
    crate::xray::stop().await?;
    Ok(Value::Null)
}

#[tauri::command]
pub async fn vpn_status(app: tauri::AppHandle) -> Result<Value, String> {
    #[cfg(target_os = "android")]
    let mut status = tauri::async_runtime::spawn_blocking(move || {
        call(&app, "getStatus", serde_json::json!({}))
    })
    .await
    .map_err(|error| error.to_string())??;
    #[cfg(not(target_os = "android"))]
    let mut status = {
        let _ = app;
        serde_json::json!({
            "starting": false, "stopping": false,
            "backendRegistered": cfg!(target_os = "windows"),
            "permissionGranted": null, "permissionPending": false,
        })
    };
    let core = crate::xray::status().await?;
    #[cfg(target_os = "android")]
    if core.mode == Some("proxy") {
        status["running"] = serde_json::json!(core.running);
        status["lastError"] = serde_json::json!(core.last_error);
    } else if core.last_error.is_some() && status["lastError"].is_null() {
        status["lastError"] = serde_json::json!(core.last_error);
    }
    #[cfg(not(target_os = "android"))]
    {
        status["running"] = serde_json::json!(core.running);
        status["lastError"] = serde_json::json!(core.last_error);
    }
    status["platform"] = serde_json::json!(std::env::consts::OS);
    status["mode"] = serde_json::json!(core.mode);
    #[cfg(target_os = "android")]
    if core.mode.is_none()
        && ["starting", "stopping", "permissionPending"]
            .iter()
            .any(|key| status[*key].as_bool().unwrap_or(false))
    {
        status["mode"] = serde_json::json!("tun");
    }
    status["xray"] = serde_json::to_value(core).map_err(|error| error.to_string())?;
    Ok(status)
}
