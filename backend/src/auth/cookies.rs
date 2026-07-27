use axum::http::{header, HeaderValue};
use axum::response::Response;

pub const AUTH_INIT_COOKIE: &str = "gp_auth_init";
pub const SESSION_COOKIE: &str = "gp_session";

fn append_set_cookie(res: &mut Response, value: &str) {
    if let Ok(hv) = HeaderValue::from_str(value) {
        res.headers_mut().append(header::SET_COOKIE, hv);
    }
}

pub fn set_auth_init_cookie(res: &mut Response, secret: &str, max_age_secs: u64, secure: bool) {
    let secure_flag = if secure { "; Secure" } else { "" };
    let cookie = format!(
        "{AUTH_INIT_COOKIE}={secret}; HttpOnly{secure_flag}; SameSite=Lax; Path=/v1/auth; Max-Age={max_age_secs}"
    );
    append_set_cookie(res, &cookie);
}

pub fn clear_auth_init_cookie(res: &mut Response, secure: bool) {
    let secure_flag = if secure { "; Secure" } else { "" };
    let cookie = format!(
        "{AUTH_INIT_COOKIE}=; HttpOnly{secure_flag}; SameSite=Lax; Path=/v1/auth; Max-Age=0"
    );
    append_set_cookie(res, &cookie);
}

pub fn set_session_cookie(res: &mut Response, token: &str, max_age_secs: u64, secure: bool) {
    let secure_flag = if secure { "; Secure" } else { "" };
    let cookie = format!(
        "{SESSION_COOKIE}={token}; HttpOnly{secure_flag}; SameSite=Lax; Path=/; Max-Age={max_age_secs}"
    );
    append_set_cookie(res, &cookie);
}

pub fn clear_session_cookie(res: &mut Response, secure: bool) {
    let secure_flag = if secure { "; Secure" } else { "" };
    let cookie = format!(
        "{SESSION_COOKIE}=; HttpOnly{secure_flag}; SameSite=Lax; Path=/; Max-Age=0"
    );
    append_set_cookie(res, &cookie);
}

pub fn parse_cookie<'a>(header: Option<&'a str>, name: &str) -> Option<&'a str> {
    let header = header?;
    for part in header.split(';') {
        let part = part.trim();
        let Some(rest) = part.strip_prefix(name) else { continue };
        let Some(value) = rest.strip_prefix('=') else { continue };
        return Some(value);
    }
    None
}
