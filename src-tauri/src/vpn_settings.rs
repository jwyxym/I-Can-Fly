use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::auth_storage::with_database;

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Mode {
	#[default]
	Proxy,
	Tun,
}

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Policy {
	#[default]
	All,
	Bypass,
	Proxy,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ApplicationPlatform {
	Windows,
	Android,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct SelectedApplication {
	platform: ApplicationPlatform,
	id: String,
	name: String,
}

#[derive(Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Settings {
	mode: Mode,
	proxy_port: u16,
	domain_mode: Policy,
	// 保留编辑中的原文；连接时再将 URL 转为域名并校验。
	domain_text: String,
	application_mode: Policy,
	applications: Vec<SelectedApplication>,
}

impl Default for Settings {
	fn default() -> Self {
		Self {
			mode: Mode::default(),
			proxy_port: 10808,
			domain_mode: Policy::default(),
			domain_text: String::new(),
			application_mode: Policy::default(),
			applications: Vec::new(),
		}
	}
}

impl Settings {
	fn normalize(&mut self) -> Result<(), String> {
		if self.proxy_port == 0 {
			return Err("Proxy port must be an integer between 1 and 65535".into());
		}
		if cfg!(target_os = "android") {
			self.mode = Mode::Tun;
		}
		if self.applications.iter().any(|app| app.id.trim().is_empty() || app.name.trim().is_empty()) {
			return Err("Application name and identifier must not be empty".into());
		}
		Ok(())
	}
}

#[derive(Serialize)]
pub(crate) struct SettingsResponse {
	platform: &'static str,
	settings: Settings,
}

#[tauri::command]
pub(crate) async fn vpn_settings_load(app: AppHandle) -> Result<SettingsResponse, String> {
	with_database(app, |connection| {
		let json: Option<String> = connection.query_row(
			"SELECT settings_json FROM vpn_settings WHERE id = 1", [], |row| row.get(0),
		).optional().map_err(|error| error.to_string())?;
		let mut settings = match json {
			Some(json) => serde_json::from_str::<Settings>(&json).map_err(|error| error.to_string())?,
			None => Settings::default(),
		};
		settings.normalize()?;
		Ok(SettingsResponse { platform: std::env::consts::OS, settings })
	}).await
}

#[tauri::command]
pub(crate) async fn vpn_settings_save(app: AppHandle, mut settings: Settings) -> Result<(), String> {
	settings.normalize()?;
	let json = serde_json::to_string(&settings).map_err(|error| error.to_string())?;
	with_database(app, move |connection| {
		connection.execute(
			"INSERT INTO vpn_settings (id, settings_json) VALUES (1, ?1)
			 ON CONFLICT(id) DO UPDATE SET settings_json = excluded.settings_json",
			[json],
		).map_err(|error| error.to_string())?;
		Ok(())
	}).await
}
