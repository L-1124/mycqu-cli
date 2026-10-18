//! 本地状态存储：会话快照、待完成挑战、验证码图片与状态文件锁。
//!
//! # 平台与安全边界
//! - Windows 上会话快照通过 DPAPI（`CRYPTPROTECT_UI_FORBIDDEN`）加密后落盘；
//! - 其他平台**不支持**持久化凭据，相关操作直接报错，绝不静默退回明文存储；
//! - 所有“读取-修改-保存”会话的操作必须持有 [`StateStore::acquire_lock`] 返回的
//!   状态锁，防止多进程基于过期快照互相覆盖。

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use directories::ProjectDirs;
use rsmycqu::errors::session::SessionError;
use rsmycqu::session::Session;

use crate::challenge::{self, StoredChallenge};
use crate::output;

#[cfg(windows)]
mod dpapi {
    use std::ptr;
    use std::slice;
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    };

    pub fn protect(data: &[u8]) -> Result<Vec<u8>, String> {
        let in_blob = CRYPT_INTEGER_BLOB {
            cbData: u32::try_from(data.len()).map_err(|e| e.to_string())?,
            pbData: data.as_ptr().cast_mut(),
        };
        let mut out_blob = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: ptr::null_mut(),
        };

        unsafe {
            CryptProtectData(
                &in_blob,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out_blob,
            )
            .map_err(|e| format!("DPAPI CryptProtectData failed: {e}"))?;

            let protected_bytes = slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec();
            let _ = LocalFree(Some(HLOCAL(out_blob.pbData.cast())));
            Ok(protected_bytes)
        }
    }

    pub fn unprotect(data: &[u8]) -> Result<Vec<u8>, String> {
        let in_blob = CRYPT_INTEGER_BLOB {
            cbData: u32::try_from(data.len()).map_err(|e| e.to_string())?,
            pbData: data.as_ptr().cast_mut(),
        };
        let mut out_blob = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: ptr::null_mut(),
        };

        unsafe {
            CryptUnprotectData(
                &in_blob,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out_blob,
            )
            .map_err(|e| format!("DPAPI CryptUnprotectData failed: {e}"))?;

            let plain_bytes = slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec();
            let _ = LocalFree(Some(HLOCAL(out_blob.pbData.cast())));
            Ok(plain_bytes)
        }
    }
}

/// 非 Windows 平台上拒绝持久化凭据的说明。
const PLAINTEXT_REFUSED: &str =
    "Secure credential storage is only implemented for Windows (DPAPI); refusing to store session data in plaintext";

/// 状态锁等待上限；超过后报告可重试的状态冲突。
const LOCK_WAIT: Duration = Duration::from_secs(120);
/// 锁竞争时的轮询间隔。
const LOCK_POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug)]
pub enum StoreError {
    NotFound,
    /// 快照中的待完成认证流程已过期：需要重新登录，不是状态损坏。
    Expired(String),
    Corrupted(String),
    Io(String),
    /// 状态锁被其他进程持有。
    Locked(String),
}

#[derive(Debug, Clone)]
pub struct StateStore {
    base_dir: PathBuf,
}

/// 排他的会话状态锁守卫。
///
/// 锁由操作系统维护（Windows `LockFileEx` / Unix `flock`）：进程退出或崩溃时
/// 由内核自动释放，因此**不需要**时间戳回收，也不会出现“删除他人锁文件”的竞态。
/// 守卫析构即关闭句柄并解锁；锁文件本身不保存任何凭据。
#[derive(Debug)]
pub struct StateLock {
    /// 持有打开且已加锁的文件句柄；字段不直接读取。
    #[allow(dead_code)]
    file: File,
}

impl StateStore {
    pub fn new() -> Result<Self, String> {
        if !cfg!(windows) {
            return Err(PLAINTEXT_REFUSED.to_string());
        }
        let proj_dirs = ProjectDirs::from("cn", "edu.cqu", "mycqu-cli")
            .ok_or_else(|| "Could not determine local project directories".to_string())?;
        let base_dir = proj_dirs.data_local_dir().to_path_buf();
        fs::create_dir_all(&base_dir).map_err(|e| format!("Failed to create local data directory: {e}"))?;
        Ok(Self { base_dir })
    }

    /// 使用指定数据目录构造存储；供测试与未来多 profile 使用。
    pub fn with_base_dir(base_dir: PathBuf) -> Self {
        let _ = fs::create_dir_all(&base_dir);
        Self { base_dir }
    }

    fn session_file_path(&self) -> PathBuf {
        self.base_dir.join("session.bin")
    }

    pub fn captcha_path(&self) -> PathBuf {
        self.base_dir.join("captcha.png")
    }

    fn challenge_file_path(&self) -> PathBuf {
        self.base_dir.join("challenge.json")
    }

    fn lock_file_path(&self) -> PathBuf {
        self.base_dir.join("session.lock")
    }

    pub fn load_session(&self) -> Result<Session, StoreError> {
        let path = self.session_file_path();
        if !path.exists() {
            return Err(StoreError::NotFound);
        }
        if !cfg!(windows) {
            return Err(StoreError::Corrupted(
                "session snapshot is protected with Windows DPAPI and cannot be read on this platform"
                    .to_string(),
            ));
        }

        let mut file =
            File::open(&path).map_err(|e| StoreError::Io(format!("Failed to open session: {e}")))?;
        let mut encrypted = Vec::new();
        file.read_to_end(&mut encrypted)
            .map_err(|e| StoreError::Io(format!("Failed to read session: {e}")))?;

        #[cfg(windows)]
        let decrypted = dpapi::unprotect(&encrypted)
            .map_err(|e| StoreError::Corrupted(format!("Failed to decrypt session via DPAPI: {e}")))?;
        #[cfg(not(windows))]
        let decrypted = encrypted;

        let contents = String::from_utf8(decrypted)
            .map_err(|e| StoreError::Corrupted(format!("Session is not valid UTF-8: {e}")))?;

        match Session::from_snapshot(&contents) {
            Ok(session) => Ok(session),
            // 待完成流程过期是认证状态，不是本地状态损坏
            Err(SessionError::ExpiredAuthentication) => Err(StoreError::Expired(
                "pending authentication in the saved session has expired".to_string(),
            )),
            Err(e) => Err(StoreError::Corrupted(format!("Snapshot decode failed: {e}"))),
        }
    }

    pub fn save_session(&self, session: &Session) -> Result<(), String> {
        if !cfg!(windows) {
            return Err(PLAINTEXT_REFUSED.to_string());
        }

        let snapshot = session
            .export_snapshot()
            .map_err(|e| format!("Failed to export snapshot: {e}"))?;

        #[cfg(windows)]
        let payload = dpapi::protect(snapshot.as_bytes())?;
        #[cfg(not(windows))]
        let payload = snapshot.into_bytes();

        self.atomic_write(&self.session_file_path(), &payload)
    }

    /// 保存待完成认证流程的会话，并把挑战 ID 与之一同持久化。
    pub fn save_pending_session(&self, session: &Session, challenge_id: &str) -> Result<(), String> {
        self.save_session(session)?;
        let stored = StoredChallenge::new(challenge_id.to_string(), challenge::now_unix());
        self.save_challenge(&stored)
    }

    pub fn load_challenge(&self) -> Result<Option<StoredChallenge>, String> {
        let path = self.challenge_file_path();
        if !path.exists() {
            return Ok(None);
        }
        let text = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read challenge state: {e}"))?;
        let stored: StoredChallenge = serde_json::from_str(&text)
            .map_err(|e| format!("Challenge state is corrupted: {e}"))?;
        Ok(Some(stored))
    }

    pub fn save_challenge(&self, stored: &StoredChallenge) -> Result<(), String> {
        let payload = serde_json::to_string(stored)
            .map_err(|e| format!("Failed to serialize challenge state: {e}"))?;
        self.atomic_write(&self.challenge_file_path(), payload.as_bytes())
    }

    pub fn clear_challenge(&self) -> Result<(), String> {
        let path = self.challenge_file_path();
        if path.exists() {
            fs::remove_file(&path)
                .map_err(|e| format!("Failed to remove challenge state: {e}"))?;
        }
        Ok(())
    }

    pub fn clear_session(&self) -> Result<(), String> {
        // 注意：不得删除 session.lock，锁文件由操作系统句柄维持
        for path in [self.session_file_path(), self.challenge_file_path()] {
            if path.exists() {
                fs::remove_file(&path).map_err(|e| {
                    format!("Failed to remove state file {}: {e}", path.display())
                })?;
            }
        }
        self.clear_captcha()?;
        self.clear_stale_temp_files();
        Ok(())
    }

    /// 清理上次写入崩溃残留的临时文件。
    fn clear_stale_temp_files(&self) {
        let Ok(entries) = fs::read_dir(&self.base_dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.to_string_lossy().ends_with(".tmp") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    pub fn clear_captcha(&self) -> Result<(), String> {
        let captcha = self.captcha_path();
        if captcha.exists() {
            fs::remove_file(&captcha)
                .map_err(|e| format!("Failed to remove captcha file: {e}"))?;
        }
        Ok(())
    }

    pub fn save_captcha(&self, bytes: &[u8]) -> Result<PathBuf, String> {
        let path = self.captcha_path();
        self.atomic_write(&path, bytes)?;
        Ok(path)
    }

    /// 临时文件写入 + `sync_all` + 重命名，避免半写入状态。
    fn atomic_write(&self, path: &Path, payload: &[u8]) -> Result<(), String> {
        let stem = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "state".to_string());
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let tmp_path = self
            .base_dir
            .join(format!("{stem}.{}.{nonce}.tmp", std::process::id()));

        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp_path)
            .map_err(|e| format!("Failed to open temp state file: {e}"))?;

        file.write_all(payload)
            .map_err(|e| format!("Failed to write temp state file: {e}"))?;
        file.sync_all()
            .map_err(|e| format!("Failed to sync temp state file: {e}"))?;
        drop(file);

        fs::rename(&tmp_path, path)
            .map_err(|e| format!("Failed to atomic rename state file: {e}"))?;
        Ok(())
    }

    /// 获取排他的会话状态锁，覆盖整个“读取-修改-保存”临界区。
    pub fn acquire_lock(&self) -> Result<StateLock, StoreError> {
        self.acquire_lock_for(LOCK_WAIT)
    }

    /// 同 [`Self::acquire_lock`]，但允许指定等待上限（测试用）。
    ///
    /// 使用操作系统文件锁（`File::try_lock`）：锁的归属与进程存活由内核保证，
    /// 崩溃或退出会自动释放，因此无需时间戳回收，也不会误删他人的锁。
    /// 锁文件本身可以长期存在，删除它并不会破坏互斥（锁属于句柄而非路径）。
    pub fn acquire_lock_for(&self, wait: Duration) -> Result<StateLock, StoreError> {
        let path = self.lock_file_path();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| StoreError::Io(format!("Failed to open state lock file: {e}")))?;

        let deadline = Instant::now() + wait;
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(StateLock { file }),
                Err(TryLockError::WouldBlock) => {
                    if Instant::now() >= deadline {
                        return Err(StoreError::Locked(format!(
                            "another mycqu process has held the session state lock for over {}s",
                            wait.as_secs()
                        )));
                    }
                    std::thread::sleep(LOCK_POLL_INTERVAL);
                }
                Err(TryLockError::Error(e)) => {
                    return Err(StoreError::Io(format!(
                        "Failed to acquire state lock: {e}"
                    )));
                }
            }
        }
    }
}

/// 把状态错误映射为输出契约字段：退出码、错误码、消息、是否可重试。
///
/// 纯函数，便于测试与复用；报告由 [`report_store_error`] 负责。
pub fn describe_store_error(err: &StoreError) -> (i32, &'static str, String, bool) {
    match err {
        StoreError::NotFound => (
            output::EXIT_AUTH_REQUIRED,
            "AUTH_REQUIRED",
            "No active session found. Please login first.".to_string(),
            false,
        ),
        StoreError::Expired(msg) => (
            output::EXIT_AUTH_REQUIRED,
            "AUTH_FLOW_EXPIRED",
            format!("{msg}; run 'auth login' again"),
            false,
        ),
        StoreError::Corrupted(msg) => (
            output::EXIT_STATE_INVALID,
            "STATE_INVALID",
            format!("Session state corrupted: {msg}"),
            false,
        ),
        StoreError::Io(msg) => (
            output::EXIT_STATE_INVALID,
            "STATE_INVALID",
            format!("Failed to read session state: {msg}"),
            false,
        ),
        // 锁竞争是暂时性的，适合重试
        StoreError::Locked(msg) => (
            output::EXIT_STATE_INVALID,
            "STATE_INVALID",
            format!("Session state is locked: {msg}"),
            true,
        ),
    }
}

/// 按输出契约报告状态错误并返回退出码。
pub fn report_store_error(err: StoreError) -> i32 {
    let (code, err_code, msg, retryable) = describe_store_error(&err);
    output::print_err(err_code, &msg, retryable, None, None);
    code
}

/// 获取会话状态锁；失败时按输出契约直接报告并返回退出码。
pub fn lock_or_report(store: &StateStore) -> Result<StateLock, i32> {
    store.acquire_lock().map_err(|err| {
        // 锁文件创建失败等 IO 问题也统一走状态错误映射
        let err = match err {
            StoreError::Io(msg) => StoreError::Io(format!("Failed to acquire state lock: {msg}")),
            other => other,
        };
        report_store_error(err)
    })
}

/// 读取会话；失败时按输出契约报告并返回退出码。
pub fn load_session_or_report(store: &StateStore) -> Result<Session, i32> {
    store.load_session().map_err(report_store_error)
}

/// 持久化会话；失败时按输出契约报告并返回退出码。
pub fn save_session_or_report(store: &StateStore, session: &Session) -> Result<(), i32> {
    store.save_session(session).map_err(|e| {
        output::print_err(
            "STATE_INVALID",
            &format!("Failed to persist updated session: {e}"),
            false,
            None,
            None,
        );
        output::EXIT_STATE_INVALID
    })
}
