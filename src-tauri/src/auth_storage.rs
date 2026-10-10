use std::{
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::{path::BaseDirectory, AppHandle, Manager};

// 首次成功选定后，本次进程的所有读写均使用同一个数据库。
static DATABASE_PATH: OnceLock<PathBuf> = OnceLock::new();
static DATABASE_INIT: Mutex<()> = Mutex::new(());

#[derive(Deserialize, Serialize)]
pub(crate) struct LoginUser {
    id: String,
    username: String,
    is_admin: bool,
    disabled: bool,
    active_until: i64,
    active: bool,
    created_at: i64,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct LoginData {
    token: String,
    token_type: String,
    expires_at: i64,
    user: LoginUser,
}

impl LoginData {
    fn valid(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or(i64::MAX);
        !self.token.trim().is_empty() && self.token_type == "Bearer" && self.expires_at > now
    }
}

fn open(path: &Path) -> Result<Connection, String> {
	let connection = Connection::open(path)
		.map_err(|error| format!("Cannot open login database {}: {error}", path.display()))?;
	connection.busy_timeout(Duration::from_secs(5)).map_err(|error| error.to_string())?;
	// 全局只缓存路径；每次重新打开都检查表，兼容空文件或被重新创建的数据库。
	connection.execute_batch(
		"CREATE TABLE IF NOT EXISTS auth_session (
			id INTEGER PRIMARY KEY CHECK (id = 1),
			token TEXT NOT NULL,
			token_type TEXT NOT NULL,
			expires_at INTEGER NOT NULL,
			user_json TEXT NOT NULL
		);
		CREATE TABLE IF NOT EXISTS vpn_settings (
			id INTEGER PRIMARY KEY CHECK (id = 1),
			settings_json TEXT NOT NULL
		);",
	).map_err(|error| format!("Cannot initialize login database {}: {error}", path.display()))?;
	Ok(connection)
}

fn initialize(directory: PathBuf) -> Result<PathBuf, String> {
	std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
	let path = directory.join("db.db");
	let mut connection = open(&path)?;
    // 实际尝试写入并回滚，验证已有数据库的写权限，同时保留原有登录数据。
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "INSERT INTO auth_session (id, token, token_type, expires_at, user_json)
		 VALUES (1, '', 'Bearer', 0, 'null')
		 ON CONFLICT(id) DO UPDATE SET token = excluded.token",
            [],
        )
        .map_err(|error| error.to_string())?;
    transaction.rollback().map_err(|error| error.to_string())?;
    Ok(path)
}

fn database_path(app: &AppHandle) -> Result<PathBuf, String> {
    if let Some(path) = DATABASE_PATH.get() {
        return Ok(path.clone());
    }
    let _guard = DATABASE_INIT.lock().map_err(|error| error.to_string())?;
    if let Some(path) = DATABASE_PATH.get() {
        return Ok(path.clone());
    }
    #[cfg(target_os = "android")]
    let path = {
        let public: PathBuf = app
            .path()
            .resolve("./", BaseDirectory::Public)
            .map_err(|error| error.to_string())?;
        let directory = public
            .parent()
            .ok_or("Public directory has no parent")?
            .to_path_buf();
        initialize(directory)?
    };
    #[cfg(not(target_os = "android"))]
    let path = {
        let resource = app
            .path()
            .resolve("./", BaseDirectory::Resource)
            .map_err(|error| error.to_string())
            .and_then(initialize);
        match resource {
            Ok(path) => path,
            Err(resource_error) => {
                let local = app
                    .path()
                    .resolve("./", BaseDirectory::AppLocalData)
                    .map_err(|error| error.to_string())
                    .and_then(initialize);
                local.map_err(|error| {
                    format!(
					"Resource database unavailable: {resource_error}; AppLocalData database unavailable: {error}"
				)
                })?
            }
        }
    };
    DATABASE_PATH
        .set(path.clone())
        .map_err(|_| "Database path already initialized")?;
    Ok(path)
}

pub(crate) async fn with_database<T, F>(app: AppHandle, operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(Connection) -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let path = database_path(&app)?;
		operation(open(&path)?)
			.map_err(|error| format!("Login database {}: {error}", path.display()))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub(crate) async fn auth_load(app: AppHandle) -> Result<Option<LoginData>, String> {
    with_database(app, |connection| {
        let row = connection
            .query_row(
                "SELECT token, token_type, expires_at, user_json FROM auth_session WHERE id = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| error.to_string())?;
        let Some((token, token_type, expires_at, user_json)) = row else {
            return Ok(None);
        };
        if let Ok(user) = serde_json::from_str::<LoginUser>(&user_json) {
            let data = LoginData {
                token,
                token_type,
                expires_at,
                user,
            };
            if data.valid() {
                return Ok(Some(data));
            }
        }
        connection
            .execute("DELETE FROM auth_session WHERE id = 1", [])
            .map_err(|error| error.to_string())?;
        Ok(None)
    })
    .await
}

#[tauri::command]
pub(crate) async fn auth_save(app: AppHandle, data: LoginData) -> Result<(), String> {
    if !data.valid() {
        return Err("Invalid or expired login data".into());
    }
    with_database(app, move |connection| {
        let user_json = serde_json::to_string(&data.user).map_err(|error| error.to_string())?;
        connection
            .execute(
                "INSERT INTO auth_session (id, token, token_type, expires_at, user_json)
			 VALUES (1, ?1, ?2, ?3, ?4)
			 ON CONFLICT(id) DO UPDATE SET token = excluded.token, token_type = excluded.token_type,
			 expires_at = excluded.expires_at, user_json = excluded.user_json",
                params![data.token, data.token_type, data.expires_at, user_json],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    })
    .await
}

#[tauri::command]
pub(crate) async fn auth_clear(app: AppHandle, token: Option<String>) -> Result<(), String> {
    with_database(app, move |connection| {
        // Token 条件防止旧会话的清理请求删除新登录数据。
        connection
            .execute(
                "DELETE FROM auth_session WHERE id = 1 AND (?1 IS NULL OR token = ?1)",
                params![token],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    })
    .await
}
