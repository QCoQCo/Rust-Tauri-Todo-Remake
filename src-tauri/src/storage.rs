use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use hmac::Hmac;
use rand::RngCore;
use sha2::Sha256;
use std::fs;
use std::path::{Path, PathBuf};

type HmacSha256 = Hmac<Sha256>;

const DATA_FILENAME: &str = "app_data.enc.json";
const KEY_FILENAME: &str = "key_fallback.b64";
const KEYRING_USERNAME: &str = "data_key_v1";

#[derive(serde::Serialize, serde::Deserialize)]
struct Envelope {
    v: u32,
    nonce_b64: String,
    ct_b64: String,
    hmac_b64: Option<String>, // 백업 파일용 (로컬 저장에는 없을 수 있음)
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

fn get_key_from_keyring(app: &tauri::AppHandle) -> Option<[u8; 32]> {
    let service = service_name(app);
    let entry = match keyring::Entry::new(&service, KEYRING_USERNAME) {
        Ok(e) => e,
        Err(_) => return None, // 키체인 접근 실패 시 조용히 None 반환
    };

    match entry.get_password() {
        Ok(b64) => {
            let engine = base64::engine::general_purpose::STANDARD;
            let decoded = match engine.decode(b64.as_bytes()) {
                Ok(d) => d,
                Err(_) => return None,
            };
            if decoded.len() != 32 {
                return None;
            }
            let mut key = [0u8; 32];
            key.copy_from_slice(&decoded);
            Some(key)
        }
        Err(keyring::Error::NoEntry) => None,
        Err(_) => None, // 다른 에러도 조용히 무시 (비밀번호 요구 등)
    }
}

fn set_key_to_keyring(app: &tauri::AppHandle, key: &[u8; 32]) -> bool {
    let service = service_name(app);
    let entry = match keyring::Entry::new(&service, KEYRING_USERNAME) {
        Ok(e) => e,
        Err(_) => return false, // 키체인 접근 실패 시 조용히 실패
    };
    let engine = base64::engine::general_purpose::STANDARD;
    let b64 = engine.encode(key);
    entry.set_password(&b64).is_ok() // 성공 여부만 반환
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

fn get_or_create_key(app: &tauri::AppHandle) -> Result<[u8; 32], String> {
    // 1) fallback 파일 우선 (키체인 비밀번호 요구 방지)
    if let Some(key) = get_key_from_fallback_file(app)? {
        return Ok(key);
    }

    // 2) OS 키체인 시도 (실패해도 에러 없이 넘어감)
    if let Some(key) = get_key_from_keyring(app) {
        // 키체인에서 가져온 키를 fallback 파일에도 저장 (다음엔 바로 사용)
        let _ = set_key_to_fallback_file(app, &key);
        return Ok(key);
    }

    // 3) 새 키 생성
    let mut key = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);

    // fallback 파일에 저장 (키체인은 시도만 하고 실패해도 무시)
    set_key_to_fallback_file(app, &key)?;
    let _ = set_key_to_keyring(app, &key); // 키체인 저장 시도 (실패해도 무시)
    Ok(key)
}

pub fn load_encrypted(app: &tauri::AppHandle) -> Result<Option<Vec<u8>>, String> {
    let dir = app_data_dir(app)?;
    let path = dir.join(DATA_FILENAME);
    if !path.exists() {
        return Ok(None);
    }

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

    let key = get_or_create_key(app)?;
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
        hmac_b64: None, // 로컬 저장에는 HMAC 불필요 (AES-GCM이 이미 인증 제공)
    };
    let out = serde_json::to_string(&env).map_err(|e| format!("envelope serialize error: {e}"))?;
    fs::write(&path, out.as_bytes()).map_err(|e| format!("data write error: {e}"))?;
    Ok(())
}

fn compute_hmac(key: &[u8; 32], data: &[u8]) -> [u8; 32] {
    use hmac::Mac;
    let mut mac = <HmacSha256 as Mac>::new_from_slice(key).expect("HMAC can take key of any size");
    mac.update(data);
    mac.finalize().into_bytes().into()
}

pub fn export_backup(app: &tauri::AppHandle, output_path: &Path, plaintext: &[u8]) -> Result<(), String> {
    ensure_parent_dir(output_path)?;

    let key = get_or_create_key(app)?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| format!("cipher init error: {e}"))?;

    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ct = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| format!("encrypt error: {e}"))?;

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
    fs::write(output_path, out.as_bytes()).map_err(|e| format!("backup write error: {e}"))?;
    Ok(())
}

pub fn import_backup(app: &tauri::AppHandle, input_path: &Path) -> Result<Vec<u8>, String> {
    let raw = fs::read_to_string(input_path).map_err(|e| format!("backup read error: {e}"))?;
    let env: Envelope = serde_json::from_str(&raw).map_err(|e| format!("envelope parse error: {e}"))?;
    if env.v != 1 {
        return Err("unsupported backup version".to_string());
    }

    let hmac_b64 = env.hmac_b64.ok_or_else(|| "missing HMAC signature (file may be corrupted)".to_string())?;

    let engine = base64::engine::general_purpose::STANDARD;
    let nonce_bytes = engine
        .decode(env.nonce_b64.as_bytes())
        .map_err(|e| format!("nonce decode error: {e}"))?;
    let ct = engine
        .decode(env.ct_b64.as_bytes())
        .map_err(|e| format!("ciphertext decode error: {e}"))?;
    let expected_hmac = engine
        .decode(hmac_b64.as_bytes())
        .map_err(|e| format!("HMAC decode error: {e}"))?;

    if nonce_bytes.len() != 12 {
        return Err("invalid nonce length".to_string());
    }
    if expected_hmac.len() != 32 {
        return Err("invalid HMAC length".to_string());
    }

    // HMAC 검증
    let key = get_or_create_key(app)?;
    let mut signed_data = Vec::with_capacity(12 + ct.len());
    signed_data.extend_from_slice(&nonce_bytes);
    signed_data.extend_from_slice(&ct);
    let computed_hmac = compute_hmac(&key, &signed_data);
    if computed_hmac.as_slice() != expected_hmac.as_slice() {
        return Err("HMAC verification failed: file may be tampered or corrupted".to_string());
    }

    // 복호화
    let nonce = Nonce::from_slice(&nonce_bytes);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| format!("cipher init error: {e}"))?;
    let pt = cipher
        .decrypt(nonce, ct.as_ref())
        .map_err(|e| format!("decrypt failed: {e}"))?;
    Ok(pt)
}

