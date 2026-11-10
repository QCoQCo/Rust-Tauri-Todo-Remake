use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine;
use hmac::Hmac;
use rand::RngCore;
use sha2::Sha256;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

type HmacSha256 = Hmac<Sha256>;

const DATA_FILENAME: &str = "app_data.enc.json";
const KEY_FILENAME: &str = "key_fallback.b64";
const KEYRING_USERNAME: &str = "data_key_v1";

// 백업 v2: 사용자 비밀번호로 암호화 (기기와 무관하게 복원 가능)
const BACKUP_VERSION: u32 = 2;
const BACKUP_KDF: &str = "argon2id";
pub const MIN_BACKUP_PASSWORD_CHARS: usize = 8;
// 가져올 때 허용하는 KDF 비용 상한 (조작된 파일로 메모리를 과하게 쓰지 않도록)
const MAX_KDF_M_COST_KIB: u32 = 256 * 1024;
const MAX_KDF_T_COST: u32 = 10;
const MAX_KDF_P_COST: u32 = 8;

// 불러오지 못한 데이터 파일을 옆으로 옮기지 못했을 때, 그 파일을 덮어쓰지 않도록 저장을 막는다
static WRITES_BLOCKED: AtomicBool = AtomicBool::new(false);

#[derive(serde::Serialize, serde::Deserialize)]
struct Envelope {
    v: u32,
    nonce_b64: String,
    ct_b64: String,
    hmac_b64: Option<String>, // v1 백업 파일용 (로컬 저장에는 없음)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct BackupV2 {
    v: u32,
    kdf: String,
    m_cost_kib: u32,
    t_cost: u32,
    p_cost: u32,
    salt_b64: String,
    nonce_b64: String,
    ct_b64: String,
}

#[derive(serde::Deserialize)]
struct VersionProbe {
    v: u32,
}

enum KeyringLookup {
    Found([u8; 32]),
    NoEntry,     // 키체인에 항목이 없음 (새 키를 저장해도 안전)
    Unavailable, // 접근 실패 또는 값이 깨짐 (덮어쓰지 않음)
}

enum FallbackKey {
    Found([u8; 32]),
    Missing,
    Invalid,
}

fn service_name(app: &tauri::AppHandle) -> String {
    // 가능한 한 안정적인 식별자를 서비스 이름으로 사용
    let id = app.config().tauri.bundle.identifier.clone();
    if id.trim().is_empty() {
        "com.todo-app.app".to_string()
    } else {
        id
    }
}

fn app_data_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path_resolver()
        .app_data_dir()
        .ok_or_else(|| "failed to resolve app data dir".to_string())
}

fn ensure_parent_dir(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("failed to create data dir: {e}"))?;
    }
    Ok(())
}

/// 앱 데이터 폴더와 데이터·키 파일은 본인 계정만 접근하도록 (Unix 권한).
/// 키가 데이터 옆에 있으므로 이 권한이 다른 사용자 계정으로부터의 보호 범위다
#[cfg(unix)]
const PRIVATE_DIR_MODE: u32 = 0o700;
#[cfg(unix)]
const PRIVATE_FILE_MODE: u32 = 0o600;

fn create_private_dir(dir: &Path) -> Result<(), String> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(PRIVATE_DIR_MODE);
    }
    builder.create(dir).map_err(|e| format!("failed to create data dir: {e}"))
}

fn set_private(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if path.is_dir() { PRIVATE_DIR_MODE } else { PRIVATE_FILE_MODE };
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    #[cfg(not(unix))]
    let _ = path; // Windows는 앱 데이터 폴더가 이미 사용자별 ACL로 보호됨
    Ok(())
}

/// 이전 버전이 기본 권한(0644/0755)으로 만든 파일을 바로잡는다
fn restrict_existing_permissions(dir: &Path) {
    for path in [dir.to_path_buf(), dir.join(DATA_FILENAME), dir.join(KEY_FILENAME)] {
        if path.exists() {
            if let Err(e) = set_private(&path) {
                eprintln!("failed to restrict permissions ({}): {e}", path.display());
            }
        }
    }
}

/// 사용자가 고른 위치에 쓰는 파일 (백업 등): 기본 권한
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    write_atomic_with(path, bytes, false)
}

/// 앱 데이터 폴더의 데이터·키 파일: 본인만 읽기/쓰기
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    write_atomic_with(path, bytes, true)
}

/// 임시 파일에 쓰고 디스크에 flush한 뒤 rename으로 교체한다.
/// 쓰는 도중 앱이 죽어도 기존 파일은 온전히 남는다.
fn write_atomic_with(path: &Path, bytes: &[u8], private: bool) -> Result<(), String> {
    match path.parent() {
        Some(parent) if private => create_private_dir(parent)?,
        _ => ensure_parent_dir(path)?,
    }
    let mut tmp_name = path
        .file_name()
        .ok_or_else(|| format!("invalid file path: {}", path.display()))?
        .to_os_string();
    tmp_name.push(".tmp");
    let tmp = path.with_file_name(tmp_name);
    // 지난번에 남은 임시 파일의 권한을 이어받지 않도록 새로 만든다
    let _ = fs::remove_file(&tmp);

    let result = (|| -> std::io::Result<()> {
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        if private {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(PRIVATE_FILE_MODE);
        }
        let mut f = opts.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(&tmp);
        return Err(format!("write error ({}): {e}", path.display()));
    }
    Ok(())
}

/// 파일을 지우지 않고 `<이름>.<label>-<시각>`으로 옮겨 보존한다.
fn quarantine(path: &Path, label: &str) -> Result<PathBuf, String> {
    let name = path
        .file_name()
        .ok_or_else(|| format!("invalid file path: {}", path.display()))?
        .to_string_lossy()
        .to_string();
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let mut target = path.with_file_name(format!("{name}.{label}-{stamp}"));
    let mut n = 1;
    while target.exists() {
        target = path.with_file_name(format!("{name}.{label}-{stamp}-{n}"));
        n += 1;
    }
    fs::rename(path, &target).map_err(|e| format!("failed to preserve {}: {e}", path.display()))?;
    // 보존본에도 키·데이터가 들어 있으므로 본인만 읽을 수 있게
    if let Err(e) = set_private(&target) {
        eprintln!("failed to restrict permissions ({}): {e}", target.display());
    }
    Ok(target)
}

fn decode_key(b64: &str) -> Option<[u8; 32]> {
    let engine = base64::engine::general_purpose::STANDARD;
    let decoded = engine.decode(b64.trim().as_bytes()).ok()?;
    decoded.try_into().ok()
}

fn read_keyring_key(app: &tauri::AppHandle) -> KeyringLookup {
    let entry = match keyring::Entry::new(&service_name(app), KEYRING_USERNAME) {
        Ok(e) => e,
        Err(_) => return KeyringLookup::Unavailable, // 키체인 접근 실패 시 조용히 넘어감
    };
    match entry.get_password() {
        Ok(b64) => decode_key(&b64).map_or(KeyringLookup::Unavailable, KeyringLookup::Found),
        Err(keyring::Error::NoEntry) => KeyringLookup::NoEntry,
        Err(_) => KeyringLookup::Unavailable, // 다른 에러도 조용히 무시 (비밀번호 요구 등)
    }
}

fn write_keyring_key(app: &tauri::AppHandle, key: &[u8; 32]) -> bool {
    let entry = match keyring::Entry::new(&service_name(app), KEYRING_USERNAME) {
        Ok(e) => e,
        Err(_) => return false, // 키체인 접근 실패 시 조용히 실패
    };
    let engine = base64::engine::general_purpose::STANDARD;
    entry.set_password(&engine.encode(key)).is_ok() // 성공 여부만 반환
}

fn read_fallback_key(dir: &Path) -> Result<FallbackKey, String> {
    match fs::read_to_string(dir.join(KEY_FILENAME)) {
        Ok(b64) => Ok(decode_key(&b64).map_or(FallbackKey::Invalid, FallbackKey::Found)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(FallbackKey::Missing),
        Err(e) if e.kind() == ErrorKind::InvalidData => Ok(FallbackKey::Invalid),
        Err(e) => Err(format!("fallback key read error: {e}")),
    }
}

fn replace_fallback_key(dir: &Path, key: &[u8; 32]) -> Result<(), String> {
    let path = dir.join(KEY_FILENAME);
    // 기존 키 파일은 예전 데이터·백업 복구에 필요할 수 있으므로 지우지 않고 보존
    if path.exists() {
        quarantine(&path, "old")?;
    }
    let engine = base64::engine::general_purpose::STANDARD;
    write_private(&path, engine.encode(key).as_bytes())
}

/// 저장에 쓸 키를 찾는다. 키가 없으면 새로 만들되,
/// 데이터 파일이 이미 있으면 그 파일을 영영 못 읽게 되므로 새 키를 만들지 않는다.
fn key_for_write_in_dir(
    dir: &Path,
    keyring_lookup: impl FnOnce() -> KeyringLookup,
    keyring_store: impl FnOnce(&[u8; 32]),
) -> Result<[u8; 32], String> {
    // 1) fallback 파일 우선 (키체인 비밀번호 요구 방지)
    if let FallbackKey::Found(key) = read_fallback_key(dir)? {
        return Ok(key);
    }

    // 2) OS 키체인 시도 (실패해도 에러 없이 넘어감)
    let keyring = keyring_lookup();
    if let KeyringLookup::Found(key) = keyring {
        // 키체인에서 가져온 키를 fallback 파일에도 저장 (다음엔 바로 사용)
        let _ = replace_fallback_key(dir, &key);
        return Ok(key);
    }

    // 3) 새 키 생성
    if dir.join(DATA_FILENAME).exists() {
        return Err("encryption key not found for existing data; refusing to create a new key".to_string());
    }
    let mut key = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);
    replace_fallback_key(dir, &key)?;
    // 키체인에 다른 키가 남아 있을 수 있으면 덮어쓰지 않음
    if matches!(keyring, KeyringLookup::NoEntry) {
        keyring_store(&key);
    }
    Ok(key)
}

fn key_for_write(app: &tauri::AppHandle, dir: &Path) -> Result<[u8; 32], String> {
    key_for_write_in_dir(dir, || read_keyring_key(app), |key| {
        let _ = write_keyring_key(app, key);
    })
}

fn encrypt(key: &[u8; 32], plaintext: &[u8]) -> Result<([u8; 12], Vec<u8>), String> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| format!("cipher init error: {e}"))?;
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext)
        .map_err(|e| format!("encrypt error: {e}"))?;
    Ok((nonce_bytes, ct))
}

fn decrypt(key: &[u8; 32], nonce_bytes: &[u8], ct: &[u8]) -> Result<Vec<u8>, String> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| format!("cipher init error: {e}"))?;
    cipher
        .decrypt(Nonce::from_slice(nonce_bytes), ct)
        .map_err(|e| format!("decrypt failed: {e}"))
}

/// (nonce, ciphertext)
fn decode_envelope(env: &Envelope) -> Result<(Vec<u8>, Vec<u8>), String> {
    let engine = base64::engine::general_purpose::STANDARD;
    let nonce_bytes = engine
        .decode(env.nonce_b64.as_bytes())
        .map_err(|e| format!("nonce decode error: {e}"))?;
    let ct = engine
        .decode(env.ct_b64.as_bytes())
        .map_err(|e| format!("ciphertext decode error: {e}"))?;
    if nonce_bytes.len() != 12 {
        return Err("invalid nonce length".to_string());
    }
    Ok((nonce_bytes, ct))
}

fn load_from_dir(
    dir: &Path,
    keyring_lookup: impl FnOnce() -> KeyringLookup,
) -> Result<Option<Vec<u8>>, String> {
    let path = dir.join(DATA_FILENAME);
    if !path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(&path).map_err(|e| format!("data read error: {e}"))?;
    let env: Envelope = serde_json::from_str(&raw).map_err(|e| format!("envelope parse error: {e}"))?;
    if env.v != 1 {
        return Err("unsupported data version".to_string());
    }
    let (nonce_bytes, ct) = decode_envelope(&env)?;

    // 1) fallback 파일 키
    if let FallbackKey::Found(key) = read_fallback_key(dir)? {
        if let Ok(pt) = decrypt(&key, &nonce_bytes, &ct) {
            return Ok(Some(pt));
        }
    }

    // 2) fallback 키가 없거나 맞지 않으면 키체인 키로 재시도
    if let KeyringLookup::Found(key) = keyring_lookup() {
        if let Ok(pt) = decrypt(&key, &nonce_bytes, &ct) {
            let _ = replace_fallback_key(dir, &key);
            return Ok(Some(pt));
        }
    }

    Err("decrypt failed: no available key matches the stored data (tampered or key lost)".to_string())
}

fn write_data_file(dir: &Path, key: &[u8; 32], plaintext: &[u8]) -> Result<(), String> {
    let (nonce_bytes, ct) = encrypt(key, plaintext)?;
    let engine = base64::engine::general_purpose::STANDARD;
    let env = Envelope {
        v: 1,
        nonce_b64: engine.encode(nonce_bytes),
        ct_b64: engine.encode(ct),
        hmac_b64: None, // 로컬 저장에는 HMAC 불필요 (AES-GCM이 이미 인증 제공)
    };
    let out = serde_json::to_string(&env).map_err(|e| format!("envelope serialize error: {e}"))?;
    write_private(&dir.join(DATA_FILENAME), out.as_bytes())
}

pub fn load_encrypted(app: &tauri::AppHandle) -> Result<Option<Vec<u8>>, String> {
    let dir = app_data_dir(app)?;
    restrict_existing_permissions(&dir);
    load_from_dir(&dir, || read_keyring_key(app))
}

pub fn save_encrypted(app: &tauri::AppHandle, plaintext: &[u8]) -> Result<(), String> {
    if WRITES_BLOCKED.load(Ordering::SeqCst) {
        return Err("saving is disabled: stored data could not be loaded or preserved".to_string());
    }
    let dir = app_data_dir(app)?;
    let key = key_for_write(app, &dir)?;
    write_data_file(&dir, &key, plaintext)
}

/// 불러오지 못한 데이터 파일을 `.corrupt-<시각>`으로 옮겨 보존한다.
/// 옮기지 못하면 그 파일을 덮어쓰지 않도록 이후 저장을 막는다.
pub fn quarantine_data_file(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let result = app_data_dir(app).and_then(|dir| quarantine(&dir.join(DATA_FILENAME), "corrupt"));
    if result.is_err() {
        WRITES_BLOCKED.store(true, Ordering::SeqCst);
    }
    result
}

fn derive_backup_key(password: &str, salt: &[u8], m_cost_kib: u32, t_cost: u32, p_cost: u32) -> Result<[u8; 32], String> {
    let params = Params::new(m_cost_kib, t_cost, p_cost, Some(32)).map_err(|e| format!("invalid KDF params: {e}"))?;
    let mut key = [0u8; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|e| format!("key derivation failed: {e}"))?;
    Ok(key)
}

fn validate_backup_password(password: &str) -> Result<(), String> {
    if password.chars().count() < MIN_BACKUP_PASSWORD_CHARS {
        return Err(format!("backup password must be at least {MIN_BACKUP_PASSWORD_CHARS} characters"));
    }
    Ok(())
}

/// 백업 v2 JSON 생성. 비밀번호에서 Argon2id로 키를 만들고 AES-256-GCM으로 암호화
fn seal_backup(plaintext: &[u8], password: &str) -> Result<String, String> {
    validate_backup_password(password)?;
    let mut salt = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut salt);
    let (m_cost_kib, t_cost, p_cost) = (Params::DEFAULT_M_COST, Params::DEFAULT_T_COST, Params::DEFAULT_P_COST);
    let key = derive_backup_key(password, &salt, m_cost_kib, t_cost, p_cost)?;
    let (nonce_bytes, ct) = encrypt(&key, plaintext)?;

    let engine = base64::engine::general_purpose::STANDARD;
    let backup = BackupV2 {
        v: BACKUP_VERSION,
        kdf: BACKUP_KDF.to_string(),
        m_cost_kib,
        t_cost,
        p_cost,
        salt_b64: engine.encode(salt),
        nonce_b64: engine.encode(nonce_bytes),
        ct_b64: engine.encode(ct),
    };
    serde_json::to_string_pretty(&backup).map_err(|e| format!("backup serialize error: {e}"))
}

fn open_backup_v2(raw: &str, password: &str) -> Result<Vec<u8>, String> {
    let backup: BackupV2 = serde_json::from_str(raw).map_err(|e| format!("backup parse error: {e}"))?;
    if backup.kdf != BACKUP_KDF {
        return Err(format!("unsupported backup KDF: {}", backup.kdf));
    }
    if backup.m_cost_kib > MAX_KDF_M_COST_KIB || backup.t_cost > MAX_KDF_T_COST || backup.p_cost > MAX_KDF_P_COST {
        return Err("backup KDF parameters are out of the allowed range".to_string());
    }
    let engine = base64::engine::general_purpose::STANDARD;
    let salt = engine
        .decode(backup.salt_b64.as_bytes())
        .map_err(|e| format!("salt decode error: {e}"))?;
    let nonce_bytes = engine
        .decode(backup.nonce_b64.as_bytes())
        .map_err(|e| format!("nonce decode error: {e}"))?;
    let ct = engine
        .decode(backup.ct_b64.as_bytes())
        .map_err(|e| format!("ciphertext decode error: {e}"))?;
    if nonce_bytes.len() != 12 {
        return Err("invalid nonce length".to_string());
    }

    let key = derive_backup_key(password, &salt, backup.m_cost_kib, backup.t_cost, backup.p_cost)?;
    // AES-GCM 태그 검증 실패 = 비밀번호가 틀렸거나 파일이 변조/손상됨 (둘은 구분할 수 없음)
    decrypt(&key, &nonce_bytes, &ct).map_err(|_| "wrong password or corrupted backup file".to_string())
}

/// 이전 형식(v1): 이 기기의 데이터 키로 암호화 + HMAC
fn open_backup_v1(raw: &str, key: &[u8; 32]) -> Result<Vec<u8>, String> {
    use hmac::Mac;

    let env: Envelope = serde_json::from_str(raw).map_err(|e| format!("envelope parse error: {e}"))?;
    let hmac_b64 = env
        .hmac_b64
        .as_deref()
        .ok_or_else(|| "missing HMAC signature (file may be corrupted)".to_string())?;
    let (nonce_bytes, ct) = decode_envelope(&env)?;
    let engine = base64::engine::general_purpose::STANDARD;
    let expected_hmac = engine
        .decode(hmac_b64.as_bytes())
        .map_err(|e| format!("HMAC decode error: {e}"))?;

    // HMAC 검증: nonce + ciphertext (상수 시간 비교)
    let mut mac = <HmacSha256 as Mac>::new_from_slice(key).expect("HMAC can take key of any size");
    mac.update(&nonce_bytes);
    mac.update(&ct);
    mac.verify_slice(&expected_hmac).map_err(|_| {
        "HMAC verification failed: v1 backups can only be restored on the device that created them".to_string()
    })?;

    decrypt(key, &nonce_bytes, &ct)
}

fn backup_version(raw: &str) -> Result<u32, String> {
    let probe: VersionProbe = serde_json::from_str(raw).map_err(|e| format!("backup parse error: {e}"))?;
    Ok(probe.v)
}

fn open_backup(
    raw: &str,
    password: Option<&str>,
    device_key: impl FnOnce() -> Result<[u8; 32], String>,
) -> Result<Vec<u8>, String> {
    match backup_version(raw)? {
        1 => open_backup_v1(raw, &device_key()?),
        BACKUP_VERSION => open_backup_v2(raw, password.ok_or_else(|| "backup password required".to_string())?),
        v => Err(format!("unsupported backup version: {v}")),
    }
}

pub fn export_backup(output_path: &Path, plaintext: &[u8], password: &str) -> Result<(), String> {
    let out = seal_backup(plaintext, password)?;
    write_atomic(output_path, out.as_bytes())
}

/// 가져오기 전에 비밀번호를 물어야 하는 백업인지 (v1은 기기 키라 필요 없음)
pub fn backup_needs_password(input_path: &Path) -> Result<bool, String> {
    let raw = fs::read_to_string(input_path).map_err(|e| format!("backup read error: {e}"))?;
    Ok(backup_version(&raw)? >= BACKUP_VERSION)
}

pub fn import_backup(app: &tauri::AppHandle, input_path: &Path, password: Option<&str>) -> Result<Vec<u8>, String> {
    let raw = fs::read_to_string(input_path).map_err(|e| format!("backup read error: {e}"))?;
    open_backup(&raw, password, || {
        let dir = app_data_dir(app)?;
        key_for_write(app, &dir)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("todo-storage-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn files_starting_with(dir: &Path, prefix: &str) -> Vec<PathBuf> {
        fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with(prefix))
            .collect()
    }

    #[test]
    fn write_atomic_replaces_file_and_leaves_no_tmp() {
        let dir = temp_dir("atomic");
        let path = dir.join("data.json");
        write_atomic(&path, b"first").unwrap();
        write_atomic(&path, b"second").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second");
        assert!(!dir.join("data.json.tmp").exists());
    }

    #[test]
    fn round_trip_with_fallback_key() {
        let dir = temp_dir("roundtrip");
        let key = key_for_write_in_dir(&dir, || KeyringLookup::NoEntry, |_| {}).unwrap();
        write_data_file(&dir, &key, b"hello").unwrap();
        let loaded = load_from_dir(&dir, || KeyringLookup::NoEntry).unwrap();
        assert_eq!(loaded.as_deref(), Some(&b"hello"[..]));
    }

    #[test]
    fn lost_key_fails_load_and_does_not_create_new_key() {
        let dir = temp_dir("lostkey");
        let key = key_for_write_in_dir(&dir, || KeyringLookup::NoEntry, |_| {}).unwrap();
        write_data_file(&dir, &key, b"precious").unwrap();
        let original = fs::read(dir.join(DATA_FILENAME)).unwrap();
        fs::remove_file(dir.join(KEY_FILENAME)).unwrap();

        assert!(load_from_dir(&dir, || KeyringLookup::NoEntry).is_err());
        let created = key_for_write_in_dir(&dir, || KeyringLookup::NoEntry, |_| panic!("must not store a new key"));
        assert!(created.is_err());
        assert!(!dir.join(KEY_FILENAME).exists());
        assert_eq!(fs::read(dir.join(DATA_FILENAME)).unwrap(), original);
    }

    #[test]
    fn quarantined_data_is_preserved_and_fresh_start_is_allowed() {
        let dir = temp_dir("quarantine");
        let key = key_for_write_in_dir(&dir, || KeyringLookup::NoEntry, |_| {}).unwrap();
        write_data_file(&dir, &key, b"precious").unwrap();
        let original = fs::read(dir.join(DATA_FILENAME)).unwrap();
        fs::remove_file(dir.join(KEY_FILENAME)).unwrap();

        let preserved = quarantine(&dir.join(DATA_FILENAME), "corrupt").unwrap();
        assert_eq!(fs::read(&preserved).unwrap(), original);
        assert!(!dir.join(DATA_FILENAME).exists());

        let mut stored = false;
        key_for_write_in_dir(&dir, || KeyringLookup::NoEntry, |_| stored = true).unwrap();
        assert!(stored);
    }

    #[test]
    fn keyring_key_is_used_when_fallback_key_does_not_match() {
        let dir = temp_dir("keyring");
        let right = [7u8; 32];
        let wrong = [1u8; 32];
        write_data_file(&dir, &right, b"hi").unwrap();
        replace_fallback_key(&dir, &wrong).unwrap();

        let loaded = load_from_dir(&dir, || KeyringLookup::Found(right)).unwrap();
        assert_eq!(loaded.as_deref(), Some(&b"hi"[..]));
        assert!(matches!(read_fallback_key(&dir).unwrap(), FallbackKey::Found(k) if k == right));
        assert_eq!(files_starting_with(&dir, "key_fallback.b64.old-").len(), 1);
    }

    #[test]
    fn invalid_fallback_key_is_preserved_not_overwritten() {
        let dir = temp_dir("invalidkey");
        fs::write(dir.join(KEY_FILENAME), b"not-a-key").unwrap();

        key_for_write_in_dir(&dir, || KeyringLookup::NoEntry, |_| {}).unwrap();

        let old = files_starting_with(&dir, "key_fallback.b64.old-");
        assert_eq!(old.len(), 1);
        assert_eq!(fs::read(&old[0]).unwrap(), b"not-a-key");
        assert!(matches!(read_fallback_key(&dir).unwrap(), FallbackKey::Found(_)));
    }

    #[test]
    fn existing_keyring_entry_is_not_overwritten_when_unavailable() {
        let dir = temp_dir("keyringunavailable");
        key_for_write_in_dir(&dir, || KeyringLookup::Unavailable, |_| panic!("must not overwrite keyring")).unwrap();
    }

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[cfg(unix)]
    #[test]
    fn data_and_key_files_are_private() {
        let root = temp_dir("private");
        let dir = root.join("app-data"); // 아직 없는 폴더
        let key = key_for_write_in_dir(&dir, || KeyringLookup::NoEntry, |_| {}).unwrap();
        write_data_file(&dir, &key, b"x").unwrap();
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&dir.join(DATA_FILENAME)), 0o600);
        assert_eq!(mode(&dir.join(KEY_FILENAME)), 0o600);

        // 백업처럼 사용자가 고른 위치의 파일은 기본 권한 유지
        let backup = root.join("backup.json");
        write_atomic(&backup, b"x").unwrap();
        assert_ne!(mode(&backup), 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn existing_files_from_old_versions_are_restricted() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("restrict");
        for name in [DATA_FILENAME, KEY_FILENAME] {
            fs::write(dir.join(name), b"x").unwrap();
            fs::set_permissions(dir.join(name), fs::Permissions::from_mode(0o644)).unwrap();
        }
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();

        restrict_existing_permissions(&dir);
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&dir.join(DATA_FILENAME)), 0o600);
        assert_eq!(mode(&dir.join(KEY_FILENAME)), 0o600);

        // 보존본도 본인만
        fs::set_permissions(dir.join(DATA_FILENAME), fs::Permissions::from_mode(0o644)).unwrap();
        let preserved = quarantine(&dir.join(DATA_FILENAME), "corrupt").unwrap();
        assert_eq!(mode(&preserved), 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn stale_tmp_file_permissions_are_not_inherited() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("staletmp");
        let tmp = dir.join(format!("{DATA_FILENAME}.tmp"));
        fs::write(&tmp, b"stale").unwrap();
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o666)).unwrap();
        write_private(&dir.join(DATA_FILENAME), b"fresh").unwrap();
        assert_eq!(mode(&dir.join(DATA_FILENAME)), 0o600);
        assert!(!tmp.exists());
    }

    const PASSWORD: &str = "correct horse";

    fn no_device_key() -> Result<[u8; 32], String> {
        panic!("v2 backups must not need the device key")
    }

    /// 예전 앱이 만들던 v1 백업을 그대로 재현
    fn make_v1_backup(key: &[u8; 32], plaintext: &[u8]) -> String {
        use hmac::Mac;
        let (nonce_bytes, ct) = encrypt(key, plaintext).unwrap();
        let mut mac = <HmacSha256 as Mac>::new_from_slice(key).unwrap();
        mac.update(&nonce_bytes);
        mac.update(&ct);
        let engine = base64::engine::general_purpose::STANDARD;
        serde_json::to_string(&Envelope {
            v: 1,
            nonce_b64: engine.encode(nonce_bytes),
            ct_b64: engine.encode(&ct),
            hmac_b64: Some(engine.encode(mac.finalize().into_bytes())),
        })
        .unwrap()
    }

    fn tamper_ct(raw: &str) -> String {
        let mut v: serde_json::Value = serde_json::from_str(raw).unwrap();
        let engine = base64::engine::general_purpose::STANDARD;
        let mut ct = engine.decode(v["ct_b64"].as_str().unwrap()).unwrap();
        ct[0] ^= 1;
        v["ct_b64"] = engine.encode(ct).into();
        v.to_string()
    }

    #[test]
    fn v2_backup_round_trips_with_password_only() {
        let sealed = seal_backup(b"{\"tasks\":[]}", PASSWORD).unwrap();
        assert!(!sealed.contains("tasks"), "plaintext must not leak");
        assert_eq!(backup_version(&sealed).unwrap(), BACKUP_VERSION);
        let opened = open_backup(&sealed, Some(PASSWORD), no_device_key).unwrap();
        assert_eq!(opened, b"{\"tasks\":[]}");
    }

    #[test]
    fn v2_backup_rejects_wrong_or_missing_password_and_tampering() {
        let sealed = seal_backup(b"data", PASSWORD).unwrap();
        let wrong = open_backup(&sealed, Some("incorrect horse"), no_device_key).unwrap_err();
        assert!(wrong.contains("wrong password"), "{wrong}");
        let missing = open_backup(&sealed, None, no_device_key).unwrap_err();
        assert!(missing.contains("password required"), "{missing}");
        assert!(open_backup(&tamper_ct(&sealed), Some(PASSWORD), no_device_key).is_err());
    }

    #[test]
    fn v2_backups_use_fresh_salt_and_nonce() {
        let a: BackupV2 = serde_json::from_str(&seal_backup(b"same", PASSWORD).unwrap()).unwrap();
        let b: BackupV2 = serde_json::from_str(&seal_backup(b"same", PASSWORD).unwrap()).unwrap();
        assert_ne!(a.salt_b64, b.salt_b64);
        assert_ne!(a.nonce_b64, b.nonce_b64);
        assert_ne!(a.ct_b64, b.ct_b64);
    }

    #[test]
    fn short_backup_password_is_rejected() {
        assert!(seal_backup(b"data", "1234567").is_err());
        assert!(seal_backup(b"data", "12345678").is_ok());
    }

    #[test]
    fn excessive_kdf_params_are_rejected_before_deriving() {
        let mut v: serde_json::Value = serde_json::from_str(&seal_backup(b"data", PASSWORD).unwrap()).unwrap();
        v["m_cost_kib"] = (MAX_KDF_M_COST_KIB + 1).into();
        let err = open_backup(&v.to_string(), Some(PASSWORD), no_device_key).unwrap_err();
        assert!(err.contains("out of the allowed range"), "{err}");
    }

    #[test]
    fn v1_backup_still_imports_with_device_key() {
        let key = [9u8; 32];
        let v1 = make_v1_backup(&key, b"old");
        assert_eq!(open_backup(&v1, None, || Ok(key)).unwrap(), b"old");
        // 다른 기기(다른 키)에서는 실패
        assert!(open_backup(&v1, None, || Ok([8u8; 32])).is_err());
        // 변조
        assert!(open_backup(&tamper_ct(&v1), None, || Ok(key)).is_err());
    }

    #[test]
    fn unknown_backup_version_is_rejected() {
        let err = open_backup(r#"{"v":99}"#, Some(PASSWORD), no_device_key).unwrap_err();
        assert!(err.contains("unsupported backup version"), "{err}");
    }
}
