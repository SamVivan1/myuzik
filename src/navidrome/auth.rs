use md5::Digest;

pub fn random_salt() -> String {
    let mut buf = [0u8; 16];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        let _ = f.read_exact(&mut buf);
    }
    hex::encode(buf)
}

pub fn subsonic_token(password: &str, salt: &str) -> String {
    hex::encode(md5::Md5::digest(format!("{}{}", password, salt)))
}

pub fn auth_params<'a>(
    username: &'a str,
    password: &'a str,
    salt: &'a str,
) -> Vec<(&'static str, String)> {
    vec![
        ("u", username.to_string()),
        ("t", subsonic_token(password, salt)),
        ("s", salt.to_string()),
        ("v", "1.16.1".to_string()),
        ("c", "myuzik".to_string()),
        ("f", "json".to_string()),
    ]
}
