use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
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

// 불러오지 못한 데이터 파일을 옆으로 옮기지 못했을 때, 그 파일을 덮어쓰지 않도록 저장을 막는다
static WRITES_BLOCKED: AtomicBool = AtomicBool::new(false);

#[derive(serde::Serialize, serde::Deserialize)]
struct Envelope {
    v: u32,
    nonce_b64: String,
    ct_b64: String,
    hmac_b64: Option<String>, // 백업 파일용 (로컬 저장에는 없을 수 있음)
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

/// 임시 파일에 쓰고 디스크에 flush한 뒤 rename으로 교체한다.
/// 쓰는 도중 앱이 죽어도 기존 파일은 온전히 남는다.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    ensure_parent_dir(path)?;
    let mut tmp_name = path
        .file_name()
        .ok_or_else(|| format!("invalid file path: {}", path.display()))?
        .to_os_string();
    tmp_name.push(".tmp");
    let tmp = path.with_file_name(tmp_name);

    let result = (|| -> std::io::Result<()> {
        let mut f = fs::File::create(&tmp)?;
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
    write_atomic(&path, engine.encode(key).as_bytes())
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
    write_atomic(&dir.join(DATA_FILENAME), out.as_bytes())
}

pub fn load_encrypted(app: &tauri::AppHandle) -> Result<Option<Vec<u8>>, String> {
    let dir = app_data_dir(app)?;
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

fn compute_hmac(key: &[u8; 32], data: &[u8]) -> [u8; 32] {
    use hmac::Mac;
    let mut mac = <HmacSha256 as Mac>::new_from_slice(key).expect("HMAC can take key of any size");
    mac.update(data);
    mac.finalize().into_bytes().into()
}

pub fn export_backup(app: &tauri::AppHandle, output_path: &Path, plaintext: &[u8]) -> Result<(), String> {
    let key = key_for_write(app, &app_data_dir(app)?)?;
    let (nonce_bytes, ct) = encrypt(&key, plaintext)?;

    let engine = base64::engine::general_purpose::STANDARD;
    let ct_b64 = engine.encode(&ct);

    // HMAC 서명: nonce + ciphertext
    let mut signed_data = Vec::with_capacity(12 + ct.len());
    signed_data.extend_from_slice(&nonce_bytes);
    signed_data.extend_from_slice(&ct);
    let hmac = compute_hmac(&key, &signed_data);
    let hmac_b64 = engine.encode(hmac);

    let env = Envelope {
        v: 1,
        nonce_b64: engine.encode(nonce_bytes),
        ct_b64,
        hmac_b64: Some(hmac_b64),
    };
    let out = serde_json::to_string_pretty(&env).map_err(|e| format!("envelope serialize error: {e}"))?;
    write_atomic(output_path, out.as_bytes())
}

pub fn import_backup(app: &tauri::AppHandle, input_path: &Path) -> Result<Vec<u8>, String> {
    let raw = fs::read_to_string(input_path).map_err(|e| format!("backup read error: {e}"))?;
    let env: Envelope = serde_json::from_str(&raw).map_err(|e| format!("envelope parse error: {e}"))?;
    if env.v != 1 {
        return Err("unsupported backup version".to_string());
    }

    let hmac_b64 = env
        .hmac_b64
        .as_deref()
        .ok_or_else(|| "missing HMAC signature (file may be corrupted)".to_string())?;

    let (nonce_bytes, ct) = decode_envelope(&env)?;
    let engine = base64::engine::general_purpose::STANDARD;
    let expected_hmac = engine
        .decode(hmac_b64.as_bytes())
        .map_err(|e| format!("HMAC decode error: {e}"))?;
    if expected_hmac.len() != 32 {
        return Err("invalid HMAC length".to_string());
    }

    // HMAC 검증
    let key = key_for_write(app, &app_data_dir(app)?)?;
    let mut signed_data = Vec::with_capacity(12 + ct.len());
    signed_data.extend_from_slice(&nonce_bytes);
    signed_data.extend_from_slice(&ct);
    let computed_hmac = compute_hmac(&key, &signed_data);
    if computed_hmac.as_slice() != expected_hmac.as_slice() {
        return Err("HMAC verification failed: file may be tampered or corrupted".to_string());
    }

    // 복호화
    decrypt(&key, &nonce_bytes, &ct)
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
}
