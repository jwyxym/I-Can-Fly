use tauri::{
	menu::{Menu, MenuItem},
	tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
	Manager,
};

pub fn init(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
	let show = MenuItem::with_id(app, "tray-show", "显示窗口", true, None::<&str>)?;
	let exit = MenuItem::with_id(app, "tray-exit", "退出", true, None::<&str>)?;
	let menu = Menu::with_items(app, &[&show, &exit])?;
	let icon = app.default_window_icon().ok_or("Application tray icon is missing")?;
	TrayIconBuilder::with_id("i-can-fly")
		.icon(icon.clone())
		.tooltip("I Can Fly")
		.menu(&menu)
		.show_menu_on_left_click(false)
		.on_menu_event(|app, event| match event.id.as_ref() {
			"tray-show" => show_window(app),
			"tray-exit" => app.exit(0),
			_ => {}
		})
		.on_tray_icon_event(|tray, event| {
			if matches!(event, TrayIconEvent::Click {
				button: MouseButton::Left,
				button_state: MouseButtonState::Up,
				..
			} | TrayIconEvent::DoubleClick { button: MouseButton::Left, .. }) {
				show_window(tray.app_handle());
			}
		})
		.build(app)?;
	Ok(())
}

fn show_window(app: &tauri::AppHandle) {
	if let Some(window) = app.get_webview_window("main") {
		if let Err(error) = window.show()
			.and_then(|_| window.unminimize())
			.and_then(|_| window.set_focus())
		{
			eprintln!("Failed to restore window from system tray: {error}");
		}
	}
}
