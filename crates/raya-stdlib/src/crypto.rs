//! Crypto module implementation (std:crypto)
//!
//! Native implementation using the SDK for hashing, HMAC,
//! secure random, encoding, and constant-time comparison.

use raya_sdk::{NativeCallResult, NativeContext, NativeValue};

use sha2::{Digest, Sha256, Sha384, Sha512};

// ============================================================================
// Public API
// ============================================================================

/// Handle crypto method calls
pub fn call_crypto_method(
    ctx: &dyn NativeContext,
    method_id: u16,
    args: &[NativeValue],
) -> NativeCallResult {
    match method_id {
        0x4000 => hash(ctx, args),           // HASH
        0x4001 => hash_bytes(ctx, args),     // HASH_BYTES
        0x4002 => hmac(ctx, args),           // HMAC
        0x4003 => hmac_bytes(ctx, args),     // HMAC_BYTES
        0x4004 => random_bytes(ctx, args),   // RANDOM_BYTES
        0x4005 => random_int(args),          // RANDOM_INT
        0x4006 => random_uuid(ctx, args),    // RANDOM_UUID
        0x4007 => to_hex(ctx, args),         // TO_HEX
        0x4008 => from_hex(ctx, args),       // FROM_HEX
        0x4009 => to_base64(ctx, args),      // TO_BASE64
        0x400A => from_base64(ctx, args),    // FROM_BASE64
        0x400B => timing_safe_equal(ctx, args),   // TIMING_SAFE_EQUAL
        0x400C => encrypt(ctx, args),             // ENCRYPT
        0x400D => decrypt(ctx, args),             // DECRYPT
        0x400E => generate_key(ctx, args),        // GENERATE_KEY
        0x400F => sign(ctx, args),                // SIGN
        0x4010 => verify(ctx, args),              // VERIFY
        0x4011 => generate_key_pair(ctx, args),   // GENERATE_KEY_PAIR
        0x4012 => hkdf(ctx, args),                // HKDF
        0x4013 => pbkdf2(ctx, args),              // PBKDF2
        _ => NativeCallResult::Unhandled,
    }
}

// ============================================================================
// Method Implementations
// ============================================================================

/// crypto.hash(algorithm, data): string — hex-encoded digest
fn hash(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error("crypto.hash requires 2 arguments".to_string());
    }

    let algorithm = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid algorithm: {}", e)),
    };

    let data = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid data: {}", e)),
    };

    match hash_bytes_internal(&algorithm, data.as_bytes()) {
        Ok(digest) => {
            let hex = hex::encode(digest);
            NativeCallResult::Value(ctx.create_string(&hex))
        }
        Err(e) => NativeCallResult::Error(e),
    }
}

/// crypto.hashBytes(algorithm, data: Buffer): Buffer
fn hash_bytes(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error("crypto.hashBytes requires 2 arguments".to_string());
    }

    let algorithm = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid algorithm: {}", e)),
    };

    let data = match ctx.read_buffer(args[1]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid buffer: {}", e)),
    };

    match hash_bytes_internal(&algorithm, &data) {
        Ok(digest) => NativeCallResult::Value(ctx.create_buffer(&digest)),
        Err(e) => NativeCallResult::Error(e),
    }
}

/// crypto.hmac(algorithm, key, data): string — hex-encoded MAC
fn hmac(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 3 {
        return NativeCallResult::Error("crypto.hmac requires 3 arguments".to_string());
    }

    let algorithm = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid algorithm: {}", e)),
    };

    let key = match ctx.read_string(args[1]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid key: {}", e)),
    };

    let data = match ctx.read_string(args[2]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid data: {}", e)),
    };

    match hmac_compute_internal(&algorithm, key.as_bytes(), data.as_bytes()) {
        Ok(mac) => {
            let hex = hex::encode(mac);
            NativeCallResult::Value(ctx.create_string(&hex))
        }
        Err(e) => NativeCallResult::Error(e),
    }
}

/// crypto.hmacBytes(algorithm, key: Buffer, data: Buffer): Buffer
fn hmac_bytes(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 3 {
        return NativeCallResult::Error("crypto.hmacBytes requires 3 arguments".to_string());
    }

    let algorithm = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid algorithm: {}", e)),
    };

    let key = match ctx.read_buffer(args[1]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid key: {}", e)),
    };

    let data = match ctx.read_buffer(args[2]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid data: {}", e)),
    };

    match hmac_compute_internal(&algorithm, &key, &data) {
        Ok(mac) => NativeCallResult::Value(ctx.create_buffer(&mac)),
        Err(e) => NativeCallResult::Error(e),
    }
}

/// crypto.randomBytes(size): Buffer
fn random_bytes(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("crypto.randomBytes requires 1 argument".to_string());
    }

    let size = match args[0].as_i32() {
        Some(n) => n as usize,
        None => return NativeCallResult::Error("Expected number for size".to_string()),
    };

    if size > 65536 {
        return NativeCallResult::Error(
            "crypto.randomBytes: size too large (max 65536)".to_string(),
        );
    }

    let mut bytes = vec![0u8; size];
    use rand::RngCore;
    rand::thread_rng().fill_bytes(&mut bytes);

    NativeCallResult::Value(ctx.create_buffer(&bytes))
}

/// crypto.randomInt(min, max): number — random integer in [min, max)
fn random_int(args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error("crypto.randomInt requires 2 arguments".to_string());
    }

    let min = match args[0].as_i32() {
        Some(n) => n,
        None => return NativeCallResult::Error("Expected number for min".to_string()),
    };

    let max = match args[1].as_i32() {
        Some(n) => n,
        None => return NativeCallResult::Error("Expected number for max".to_string()),
    };

    if min >= max {
        return NativeCallResult::Error("crypto.randomInt: min must be less than max".to_string());
    }

    use rand::Rng;
    let n = rand::thread_rng().gen_range(min..max);
    NativeCallResult::i32(n)
}

/// crypto.randomUUID(): string — UUID v4
fn random_uuid(ctx: &dyn NativeContext, _args: &[NativeValue]) -> NativeCallResult {
    let id = uuid::Uuid::new_v4().to_string();
    NativeCallResult::Value(ctx.create_string(&id))
}

/// crypto.toHex(data: Buffer): string
fn to_hex(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("crypto.toHex requires 1 argument".to_string());
    }

    let data = match ctx.read_buffer(args[0]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid buffer: {}", e)),
    };

    let hex = hex::encode(data);
    NativeCallResult::Value(ctx.create_string(&hex))
}

/// crypto.fromHex(hex: string): Buffer
fn from_hex(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("crypto.fromHex requires 1 argument".to_string());
    }

    let hex_str = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid hex string: {}", e)),
    };

    match hex::decode(&hex_str) {
        Ok(bytes) => NativeCallResult::Value(ctx.create_buffer(&bytes)),
        Err(e) => NativeCallResult::Error(format!("crypto.fromHex: invalid hex: {}", e)),
    }
}

/// crypto.toBase64(data: Buffer): string
fn to_base64(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("crypto.toBase64 requires 1 argument".to_string());
    }

    let data = match ctx.read_buffer(args[0]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid buffer: {}", e)),
    };

    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
    NativeCallResult::Value(ctx.create_string(&b64))
}

/// crypto.fromBase64(b64: string): Buffer
fn from_base64(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("crypto.fromBase64 requires 1 argument".to_string());
    }

    let b64_str = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid base64 string: {}", e)),
    };

    use base64::Engine;
    match base64::engine::general_purpose::STANDARD.decode(&b64_str) {
        Ok(bytes) => NativeCallResult::Value(ctx.create_buffer(&bytes)),
        Err(e) => NativeCallResult::Error(format!("crypto.fromBase64: invalid base64: {}", e)),
    }
}

/// crypto.timingSafeEqual(a: Buffer, b: Buffer): boolean
fn timing_safe_equal(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error(
            "crypto.timingSafeEqual requires 2 arguments".to_string(),
        );
    }

    let a = match ctx.read_buffer(args[0]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid buffer a: {}", e)),
    };

    let b = match ctx.read_buffer(args[1]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid buffer b: {}", e)),
    };

    if a.len() != b.len() {
        return NativeCallResult::bool(false);
    }

    use subtle::ConstantTimeEq;
    let equal: bool = a.ct_eq(&b).into();
    NativeCallResult::bool(equal)
}

/// crypto.encrypt(key: Buffer, plaintext: Buffer): Buffer — AES-256-GCM
fn encrypt(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error("crypto.encrypt requires 2 arguments".to_string());
    }

    let key = match ctx.read_buffer(args[0]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid key: {}", e)),
    };

    let plaintext = match ctx.read_buffer(args[1]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid plaintext: {}", e)),
    };

    match encrypt_aes_gcm(&key, &plaintext) {
        Ok(result) => NativeCallResult::Value(ctx.create_buffer(&result)),
        Err(e) => NativeCallResult::Error(format!("crypto.encrypt: {}", e)),
    }
}

/// crypto.decrypt(key: Buffer, ciphertext: Buffer): Buffer — AES-256-GCM
fn decrypt(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 2 {
        return NativeCallResult::Error("crypto.decrypt requires 2 arguments".to_string());
    }

    let key = match ctx.read_buffer(args[0]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid key: {}", e)),
    };

    let data = match ctx.read_buffer(args[1]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid ciphertext: {}", e)),
    };

    match decrypt_aes_gcm(&key, &data) {
        Ok(result) => NativeCallResult::Value(ctx.create_buffer(&result)),
        Err(e) => NativeCallResult::Error(format!("crypto.decrypt: {}", e)),
    }
}

/// crypto.generateKey(bits: number): Buffer — random symmetric key
fn generate_key(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error("crypto.generateKey requires 1 argument".to_string());
    }

    let bits = match args[0].as_i32() {
        Some(n) => n,
        None => return NativeCallResult::Error("Expected number for bits".to_string()),
    };

    let byte_len = match bits {
        128 => 16,
        192 => 24,
        256 => 32,
        _ => {
            return NativeCallResult::Error(
                "crypto.generateKey: supported bit sizes are 128, 192, 256".to_string(),
            )
        }
    };

    let mut key = vec![0u8; byte_len];
    use aes_gcm::aead::rand_core::RngCore;
    aes_gcm::aead::OsRng.fill_bytes(&mut key);

    NativeCallResult::Value(ctx.create_buffer(&key))
}

/// crypto.sign(algorithm: string, privateKey: Buffer, data: Buffer): Buffer
fn sign(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 3 {
        return NativeCallResult::Error("crypto.sign requires 3 arguments".to_string());
    }

    let algorithm = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid algorithm: {}", e)),
    };

    let private_key = match ctx.read_buffer(args[1]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid private key: {}", e)),
    };

    let data = match ctx.read_buffer(args[2]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid data: {}", e)),
    };

    match algorithm.to_lowercase().as_str() {
        "ed25519" => match sign_ed25519(&private_key, &data) {
            Ok(sig) => NativeCallResult::Value(ctx.create_buffer(&sig)),
            Err(e) => NativeCallResult::Error(format!("crypto.sign: {}", e)),
        },
        _ => NativeCallResult::Error(format!(
            "Unsupported signing algorithm: {}. Supported: ed25519",
            algorithm
        )),
    }
}

/// crypto.verify(algorithm: string, publicKey: Buffer, data: Buffer, signature: Buffer): boolean
fn verify(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 4 {
        return NativeCallResult::Error("crypto.verify requires 4 arguments".to_string());
    }

    let algorithm = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid algorithm: {}", e)),
    };

    let public_key = match ctx.read_buffer(args[1]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid public key: {}", e)),
    };

    let data = match ctx.read_buffer(args[2]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid data: {}", e)),
    };

    let signature = match ctx.read_buffer(args[3]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid signature: {}", e)),
    };

    match algorithm.to_lowercase().as_str() {
        "ed25519" => match verify_ed25519(&public_key, &data, &signature) {
            Ok(valid) => NativeCallResult::bool(valid),
            Err(e) => NativeCallResult::Error(format!("crypto.verify: {}", e)),
        },
        _ => NativeCallResult::Error(format!(
            "Unsupported signing algorithm: {}. Supported: ed25519",
            algorithm
        )),
    }
}

/// crypto.generateKeyPair(algorithm: string): string[] — [publicPem, privatePem]
fn generate_key_pair(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.is_empty() {
        return NativeCallResult::Error(
            "crypto.generateKeyPair requires 1 argument".to_string(),
        );
    }

    let algorithm = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid algorithm: {}", e)),
    };

    match algorithm.to_lowercase().as_str() {
        "ed25519" => {
            let (public_pem, private_pem) = generate_ed25519_keypair();
            let pub_val = ctx.create_string(&public_pem);
            let priv_val = ctx.create_string(&private_pem);
            NativeCallResult::Value(ctx.create_array(&[pub_val, priv_val]))
        }
        _ => NativeCallResult::Error(format!(
            "Unsupported key pair algorithm: {}. Supported: ed25519",
            algorithm
        )),
    }
}

/// crypto.hkdf(hash, ikm, salt, info, length): Buffer — HKDF key derivation
fn hkdf(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 5 {
        return NativeCallResult::Error("crypto.hkdf requires 5 arguments".to_string());
    }

    let hash = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid hash: {}", e)),
    };

    let ikm = match ctx.read_buffer(args[1]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid ikm: {}", e)),
    };

    let salt = match ctx.read_buffer(args[2]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid salt: {}", e)),
    };

    let info = match ctx.read_buffer(args[3]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid info: {}", e)),
    };

    let length = match args[4].as_i32() {
        Some(n) => n as usize,
        None => return NativeCallResult::Error("Expected number for length".to_string()),
    };

    if length > 255 * 64 {
        return NativeCallResult::Error("crypto.hkdf: length too large".to_string());
    }

    match do_hkdf(&hash, &ikm, &salt, &info, length) {
        Ok(result) => NativeCallResult::Value(ctx.create_buffer(&result)),
        Err(e) => NativeCallResult::Error(format!("crypto.hkdf: {}", e)),
    }
}

/// crypto.pbkdf2(password, salt, iterations, length, hash): Buffer — PBKDF2 key derivation
fn pbkdf2(ctx: &dyn NativeContext, args: &[NativeValue]) -> NativeCallResult {
    if args.len() < 5 {
        return NativeCallResult::Error("crypto.pbkdf2 requires 5 arguments".to_string());
    }

    let password = match ctx.read_string(args[0]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid password: {}", e)),
    };

    let salt = match ctx.read_buffer(args[1]) {
        Ok(b) => b,
        Err(e) => return NativeCallResult::Error(format!("Invalid salt: {}", e)),
    };

    let iterations = match args[2].as_i32() {
        Some(n) => n as u32,
        None => return NativeCallResult::Error("Expected number for iterations".to_string()),
    };

    let length = match args[3].as_i32() {
        Some(n) => n as usize,
        None => return NativeCallResult::Error("Expected number for length".to_string()),
    };

    let hash = match ctx.read_string(args[4]) {
        Ok(s) => s,
        Err(e) => return NativeCallResult::Error(format!("Invalid hash: {}", e)),
    };

    if iterations == 0 {
        return NativeCallResult::Error(
            "crypto.pbkdf2: iterations must be greater than 0".to_string(),
        );
    }

    match do_pbkdf2(&password, &salt, iterations, length, &hash) {
        Ok(result) => NativeCallResult::Value(ctx.create_buffer(&result)),
        Err(e) => NativeCallResult::Error(format!("crypto.pbkdf2: {}", e)),
    }
}

// ============================================================================
// Internal Helpers
// ============================================================================

/// Hash data with the given algorithm, returning raw digest bytes
fn hash_bytes_internal(algorithm: &str, data: &[u8]) -> Result<Vec<u8>, String> {
    match algorithm {
        "sha256" => Ok(Sha256::digest(data).to_vec()),
        "sha384" => Ok(Sha384::digest(data).to_vec()),
        "sha512" => Ok(Sha512::digest(data).to_vec()),
        "sha1" => {
            use sha1::Sha1;
            Ok(Sha1::digest(data).to_vec())
        }
        "md5" => {
            Ok(md5::compute(data).to_vec())
        }
        _ => Err(format!(
            "Unsupported hash algorithm: {}. Supported: sha256, sha384, sha512, sha1, md5",
            algorithm
        )),
    }
}

/// Compute HMAC with the given algorithm
fn hmac_compute_internal(algorithm: &str, key: &[u8], data: &[u8]) -> Result<Vec<u8>, String> {
    use hmac::{Hmac, Mac};

    match algorithm {
        "sha256" => {
            type HmacSha256 = Hmac<Sha256>;
            let mut mac = HmacSha256::new_from_slice(key)
                .map_err(|e| format!("HMAC error: {}", e))?;
            mac.update(data);
            Ok(mac.finalize().into_bytes().to_vec())
        }
        "sha384" => {
            type HmacSha384 = Hmac<Sha384>;
            let mut mac = HmacSha384::new_from_slice(key)
                .map_err(|e| format!("HMAC error: {}", e))?;
            mac.update(data);
            Ok(mac.finalize().into_bytes().to_vec())
        }
        "sha512" => {
            type HmacSha512 = Hmac<Sha512>;
            let mut mac = HmacSha512::new_from_slice(key)
                .map_err(|e| format!("HMAC error: {}", e))?;
            mac.update(data);
            Ok(mac.finalize().into_bytes().to_vec())
        }
        _ => Err(format!(
            "Unsupported HMAC algorithm: {}. Supported: sha256, sha384, sha512",
            algorithm
        )),
    }
}

/// AES-256-GCM encrypt: returns nonce (12 bytes) + ciphertext + tag
fn encrypt_aes_gcm(key_bytes: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, String> {
    use aes_gcm::{aead::{Aead, KeyInit}, Aes256Gcm, Key, Nonce};
    use aes_gcm::aead::rand_core::RngCore;

    if key_bytes.len() != 32 {
        return Err("AES-256 requires a 32-byte key".to_string());
    }
    let key = Key::<Aes256Gcm>::from_slice(key_bytes);
    let cipher = Aes256Gcm::new(key);
    let mut nonce_bytes = [0u8; 12];
    aes_gcm::aead::OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher.encrypt(nonce, plaintext).map_err(|e| e.to_string())?;
    let mut result = Vec::with_capacity(12 + ciphertext.len());
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);
    Ok(result)
}

/// AES-256-GCM decrypt: expects nonce (12 bytes) + ciphertext + tag
fn decrypt_aes_gcm(key_bytes: &[u8], data: &[u8]) -> Result<Vec<u8>, String> {
    use aes_gcm::{aead::{Aead, KeyInit}, Aes256Gcm, Key, Nonce};

    if key_bytes.len() != 32 {
        return Err("AES-256 requires a 32-byte key".to_string());
    }
    if data.len() < 12 {
        return Err("Ciphertext too short (missing nonce)".to_string());
    }
    let key = Key::<Aes256Gcm>::from_slice(key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce = Nonce::from_slice(&data[..12]);
    cipher.decrypt(nonce, &data[12..]).map_err(|e| e.to_string())
}

/// Sign data with Ed25519
fn sign_ed25519(private_key_pem: &[u8], data: &[u8]) -> Result<Vec<u8>, String> {
    use ed25519_dalek::{pkcs8::DecodePrivateKey, Signer, SigningKey};

    let pem_str = std::str::from_utf8(private_key_pem).map_err(|e| e.to_string())?;
    let signing_key = SigningKey::from_pkcs8_pem(pem_str).map_err(|e| e.to_string())?;
    let signature = signing_key.sign(data);
    Ok(signature.to_bytes().to_vec())
}

/// Verify Ed25519 signature
fn verify_ed25519(public_key_pem: &[u8], data: &[u8], sig_bytes: &[u8]) -> Result<bool, String> {
    use ed25519_dalek::{pkcs8::DecodePublicKey, Signature, Verifier, VerifyingKey};

    let pem_str = std::str::from_utf8(public_key_pem).map_err(|e| e.to_string())?;
    let verifying_key =
        VerifyingKey::from_public_key_pem(pem_str).map_err(|e| e.to_string())?;
    let sig_array: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| "Invalid signature length (expected 64 bytes)".to_string())?;
    let signature = Signature::from_bytes(&sig_array);
    Ok(verifying_key.verify(data, &signature).is_ok())
}

/// Generate Ed25519 key pair, returns (public_pem, private_pem)
fn generate_ed25519_keypair() -> (String, String) {
    use ed25519_dalek::pkcs8::spki::der::pem::LineEnding;
    use ed25519_dalek::pkcs8::{EncodePrivateKey, EncodePublicKey};
    use ed25519_dalek::SigningKey;

    let mut rng = aes_gcm::aead::OsRng;
    let signing_key = SigningKey::generate(&mut rng);
    let private_pem = signing_key
        .to_pkcs8_pem(LineEnding::LF)
        .unwrap()
        .to_string();
    let public_pem = signing_key
        .verifying_key()
        .to_public_key_pem(LineEnding::LF)
        .unwrap();
    (public_pem, private_pem)
}

/// HKDF key derivation
fn do_hkdf(
    hash: &str,
    ikm: &[u8],
    salt: &[u8],
    info: &[u8],
    length: usize,
) -> Result<Vec<u8>, String> {
    use hkdf::Hkdf;

    let mut okm = vec![0u8; length];
    match hash.to_lowercase().as_str() {
        "sha256" | "sha-256" => {
            let hk = Hkdf::<Sha256>::new(Some(salt), ikm);
            hk.expand(info, &mut okm).map_err(|e| e.to_string())?;
        }
        "sha384" | "sha-384" => {
            let hk = Hkdf::<Sha384>::new(Some(salt), ikm);
            hk.expand(info, &mut okm).map_err(|e| e.to_string())?;
        }
        "sha512" | "sha-512" => {
            let hk = Hkdf::<Sha512>::new(Some(salt), ikm);
            hk.expand(info, &mut okm).map_err(|e| e.to_string())?;
        }
        _ => {
            return Err(format!(
                "Unsupported hash: {}. Supported: sha256, sha384, sha512",
                hash
            ))
        }
    }
    Ok(okm)
}

/// PBKDF2 key derivation
fn do_pbkdf2(
    password: &str,
    salt: &[u8],
    iterations: u32,
    length: usize,
    hash: &str,
) -> Result<Vec<u8>, String> {
    let mut output = vec![0u8; length];
    match hash.to_lowercase().as_str() {
        "sha256" | "sha-256" => {
            pbkdf2::pbkdf2_hmac::<Sha256>(password.as_bytes(), salt, iterations, &mut output);
        }
        "sha384" | "sha-384" => {
            pbkdf2::pbkdf2_hmac::<Sha384>(password.as_bytes(), salt, iterations, &mut output);
        }
        "sha512" | "sha-512" => {
            pbkdf2::pbkdf2_hmac::<Sha512>(password.as_bytes(), salt, iterations, &mut output);
        }
        _ => {
            return Err(format!(
                "Unsupported hash: {}. Supported: sha256, sha384, sha512",
                hash
            ))
        }
    }
    Ok(output)
}
