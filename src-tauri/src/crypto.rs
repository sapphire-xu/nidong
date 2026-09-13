use base64::{engine::general_purpose::STANDARD, Engine};
use rand::rngs::OsRng;
use rsa::pkcs8::DecodePublicKey;
use rsa::{Pkcs1v15Encrypt, RsaPublicKey};

/// Fallback SPKI from login_neu.js ?v=20260302 (docs 6.2).
pub const DEFAULT_PUB_B64: &str = "MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAnjA28DLKXZzxbKmo9/1WkVLf1mr+wtLXLXt6sC4WiBCtsbzF5ewm7ARZeAdS3iZtqlYPn6IcUoOw42H8nAK/tfFcIb6dZ1K0atn0U39oWCGPzYuKtLJeMuNZiDXVuAXtojrckOjLW9B3gUnaNGLuIx0fYe66l0o9WjU2cGLNZQfiIxs2h00z1EA9IdSnVxiVQWSD+lsP3JZXh2TT287la4Y4603SQNKTK/QvXfcmccwTEd1IW6HwGxD6QrkInBiHisKWxmveN7UDSaQRZ/J97G0YC32pD38WT53izXeK0p/kU/X37VP555um1wVWFvPIuc9I7gMP1+hq5a+X6c++tQIDAQAB";

pub fn sanitize_pub_b64(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '+' || *c == '/' || *c == '=')
        .collect()
}

fn wrap_pem64(b64: &str) -> String {
    b64.as_bytes()
        .chunks(64)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn parse_pub_key(pub_b64: &str) -> Option<RsaPublicKey> {
    let b64 = sanitize_pub_b64(pub_b64);
    if b64.len() < 200 {
        return None;
    }
    if let Ok(der) = STANDARD.decode(b64.as_bytes()) {
        if let Ok(k) = RsaPublicKey::from_public_key_der(&der) {
            return Some(k);
        }
    }
    let pem = format!(
        "-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----",
        wrap_pem64(&b64)
    );
    RsaPublicKey::from_public_key_pem(&pem).ok()
}

pub fn encrypt_rsa(username: &str, password: &str, pub_b64: &str) -> crate::error::Result<String> {
    let key = parse_pub_key(pub_b64)
        .or_else(|| parse_pub_key(DEFAULT_PUB_B64))
        .ok_or("RSA 公钥无效")?;
    let plain = format!("{username}{password}");
    let mut rng = OsRng;
    let enc = key
        .encrypt(&mut rng, Pkcs1v15Encrypt, plain.as_bytes())
        .map_err(|e| format!("RSA 加密失败: {e}"))?;
    Ok(STANDARD.encode(enc))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_key_parses() {
        assert!(parse_pub_key(DEFAULT_PUB_B64).is_some());
        let c = encrypt_rsa("testuser", "testpass", DEFAULT_PUB_B64).expect("encrypt");
        assert!(c.len() > 80);
    }
}
