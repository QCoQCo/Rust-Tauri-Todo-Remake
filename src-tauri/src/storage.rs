use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use rand::RngCore;
use std::fs;
use std::path::{Path, PathBuf};

const DATA_FILENAME: &str = "app_data.enc.json";
const KEY_FILENAME: &str = "key_fallback.b64";
const KEYRING_USERNAME: &str = "data_key_v1";

#[derive(serde::Serialize, serde::Deserialize)]
struct Envelope {
    v: u32,
    nonce_b64: String,
    ct_b64: String,
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

fn get_key_from_keyring(app: &tauri::AppHandle) -> Result<Option<[u8; 32]>, String> {
    let service = service_name(app);
    let entry = keyring::Entry::new(&service, KEYRING_USERNAME)
        .map_err(|e| format!("keyring entry error: {e}"))?;

    match entry.get_password() {
        Ok(b64) => {
            let engine = base64::engine::general_purpose::STANDARD;
            let decoded = engine
                .decode(b64.as_bytes())
                .map_err(|e| format!("keyring key decode error: {e}"))?;
            if decoded.len() != 32 {
                return Err("keyring key has invalid length".to_string());
            }
            let mut key = [0u8; 32];
            key.copy_from_slice(&decoded);
            Ok(Some(key))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("keyring get error: {e}")),
    }
}

fn set_key_to_keyring(app: &tauri::AppHandle, key: &[u8; 32]) -> Result<(), String> {
    let service = service_name(app);
    let entry = keyring::Entry::new(&service, KEYRING_USERNAME)
        .map_err(|e| format!("keyring entry error: {e}"))?;
    let engine = base64::engine::general_purpose::STANDARD;
    let b64 = engine.encode(key);
    entry
        .set_password(&b64)
        .map_err(|e| format!("keyring set error: {e}"))?;
    Ok(())
}

fn get_key_from_fallback_file(app: &tauri::AppHandle) -> Result<Option<[u8; 32]>, String> {
    let dir = app_data_dir(app)?;
    let path = dir.join(KEY_FILENAME);
    if !path.exists() {
        return Ok(None);
    }
    let b64 = fs::read_to_string(&path).map_err(|e| format!("fallback key read error: {e}"))?;
    let engine = base64::engine::general_purpose::STANDARD;
    let decoded = engine
        .decode(b64.trim().as_bytes())
        .map_err(|e| format!("fallback key decode error: {e}"))?;
    if decoded.len() != 32 {
        return Err("fallback key has invalid length".to_string());
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&decoded);
    Ok(Some(key))
}

fn set_key_to_fallback_file(app: &tauri::AppHandle, key: &[u8; 32]) -> Result<(), String> {
    let dir = app_data_dir(app)?;
    let path = dir.join(KEY_FILENAME);
    ensure_parent_dir(&path)?;
    let engine = base64::engine::general_purpose::STANDARD;
    let b64 = engine.encode(key);
    fs::write(&path, b64.as_bytes()).map_err(|e| format!("fallback key write error: {e}"))?;
    Ok(())
}

fn get_existing_key(app: &tauri::AppHandle) -> Result<Option<[u8; 32]>, String> {
    // 로드 시에는 "새 키 생성"을 절대 하지 않음
    if let Some(key) = get_key_from_keyring(app)? {
        return Ok(Some(key));
    }
    if let Some(key) = get_key_from_fallback_file(app)? {
        // 가능하면 키체인에도 복사(실패해도 무시)
        let _ = set_key_to_keyring(app, &key);
        return Ok(Some(key));
    }
    Ok(None)
}

fn get_or_create_key(app: &tauri::AppHandle) -> Result<[u8; 32], String> {
    // 1) OS 키체인 우선
    if let Some(key) = get_key_from_keyring(app)? {
        return Ok(key);
    }

    // 2) fallback 파일
    if let Some(key) = get_key_from_fallback_file(app)? {
        // 가능하면 키체인에도 복사
        let _ = set_key_to_keyring(app, &key);
        return Ok(key);
    }

    // 3) 새 키 생성
    let mut key = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);

    // 키체인 저장을 시도하고, 실패하면 fallback 파일에 저장
    if set_key_to_keyring(app, &key).is_err() {
        set_key_to_fallback_file(app, &key)?;
    }
    Ok(key)
}

pub fn load_encrypted(app: &tauri::AppHandle) -> Result<Option<Vec<u8>>, String> {
    let dir = app_data_dir(app)?;
    let path = dir.join(DATA_FILENAME);
    if !path.exists() {
        return Ok(None);
    }

    let key = get_existing_key(app)?
        .ok_or_else(|| "missing encryption key for existing data (cannot decrypt)".to_string())?;

    let raw = fs::read_to_string(&path).map_err(|e| format!("data read error: {e}"))?;
    let env: Envelope = serde_json::from_str(&raw).map_err(|e| format!("envelope parse error: {e}"))?;
    if env.v != 1 {
        return Err("unsupported data version".to_string());
    }

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
    let nonce = Nonce::from_slice(&nonce_bytes);

    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| format!("cipher init error: {e}"))?;
    let pt = cipher
        .decrypt(nonce, ct.as_ref())
        .map_err(|e| format!("decrypt failed (tampered or wrong key): {e}"))?;
    Ok(Some(pt))
}

pub fn save_encrypted(app: &tauri::AppHandle, plaintext: &[u8]) -> Result<(), String> {
    let dir = app_data_dir(app)?;
    let path = dir.join(DATA_FILENAME);
    ensure_parent_dir(&path)?;

    let key = get_or_create_key(app)?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| format!("cipher init error: {e}"))?;

    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ct = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| format!("encrypt error: {e}"))?;

    let engine = base64::engine::general_purpose::STANDARD;
    let env = Envelope {
        v: 1,
        nonce_b64: engine.encode(nonce_bytes),
        ct_b64: engine.encode(ct),
    };
    let out = serde_json::to_string(&env).map_err(|e| format!("envelope serialize error: {e}"))?;
    fs::write(&path, out.as_bytes()).map_err(|e| format!("data write error: {e}"))?;
    Ok(())
}

pub fn reset_storage(app: &tauri::AppHandle) -> Result<(), String> {
    let dir = app_data_dir(app)?;

    // 1) 데이터 파일 삭제
    let data_path = dir.join(DATA_FILENAME);
    if data_path.exists() {
        fs::remove_file(&data_path).map_err(|e| format!("failed to remove data file: {e}"))?;
    }

    // 2) fallback 키 파일 삭제
    let key_path = dir.join(KEY_FILENAME);
    if key_path.exists() {
        fs::remove_file(&key_path).map_err(|e| format!("failed to remove fallback key file: {e}"))?;
    }

    // 3) 키체인 키 삭제(실패는 무시)
    let service = service_name(app);
    if let Ok(entry) = keyring::Entry::new(&service, KEYRING_USERNAME) {
        let _ = entry.delete_password();
    }

    Ok(())
}

