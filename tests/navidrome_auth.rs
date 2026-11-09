use myuzik::navidrome::auth::{random_salt, subsonic_token, auth_params};

#[test]
fn token_is_md5_of_password_plus_salt() {
    let token = subsonic_token("secret", "abc123");
    assert_eq!(token, "f22ae4f5eacb35c993e910606a413410");
}

#[test]
fn auth_params_include_required_fields() {
    let params = auth_params("user", "secret", "abc123");
    let map: std::collections::HashMap<_, _> = params.into_iter().collect();
    assert_eq!(map.get("u"), Some(&"user".to_string()));
    assert_eq!(map.get("t"), Some(&"f22ae4f5eacb35c993e910606a413410".to_string()));
    assert_eq!(map.get("s"), Some(&"abc123".to_string()));
    assert_eq!(map.get("v"), Some(&"1.16.1".to_string()));
    assert_eq!(map.get("c"), Some(&"myuzik".to_string()));
    assert_eq!(map.get("f"), Some(&"json".to_string()));
}

#[test]
fn random_salt_is_16_bytes_hex() {
    let s = random_salt();
    assert_eq!(s.len(), 32);
    assert!(s.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn random_salts_differ() {
    assert_ne!(random_salt(), random_salt());
}
