/// Local captcha hint. WinRT OCR is optional; failure must not block login.
pub fn recognize(png: &[u8]) -> Option<String> {
    if png.len() < 32 {
        return None;
    }
    None
}
