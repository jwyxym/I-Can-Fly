mod auth_storage;
mod file_scanner;
mod proxy_routing;
mod vpn;
mod vpn_settings;
#[cfg(target_os = "android")]
mod vpn_jni;
mod xray;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(vpn::init())
        .plugin(file_scanner::init())
        .invoke_handler(tauri::generate_handler![
            auth_storage::auth_load,
            auth_storage::auth_save,
            auth_storage::auth_clear,
			vpn_settings::vpn_settings_load,
			vpn_settings::vpn_settings_save,
            vpn::vpn_connect,
            vpn::vpn_disconnect,
            vpn::vpn_status,
            file_scanner::file_list_applications,
            file_scanner::file_get_application_icon,
            file_scanner::file_scan_executables,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                xray::shutdown();
            }
        });
}
