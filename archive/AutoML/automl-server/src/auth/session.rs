use axum::http::{header, HeaderMap};

pub const SESSION_COOKIE: &str = "automl_session";
pub const STATE_COOKIE: &str = "automl_oauth_state";
pub const NEXT_COOKIE: &str = "automl_login_next";

pub fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').find_map(|cookie| {
                let (key, value) = cookie.trim().split_once('=')?;
                (key == name).then(|| value.to_string())
            })
        })
}

pub fn set_cookie(
    name: &str,
    value: &str,
    max_age_seconds: i64,
    http_only: bool,
    secure: bool,
    domain: Option<&str>,
) -> String {
    let mut cookie = format!("{name}={value}; Path=/; Max-Age={max_age_seconds}; SameSite=Lax");
    if let Some(domain) = domain {
        cookie.push_str(&format!("; Domain={domain}"));
    }
    if http_only {
        cookie.push_str("; HttpOnly");
    }
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

pub fn clear_cookie(name: &str, secure: bool, domain: Option<&str>) -> String {
    let mut cookie = format!("{name}=; Path=/; Max-Age=0; SameSite=Lax; HttpOnly");
    if let Some(domain) = domain {
        cookie.push_str(&format!("; Domain={domain}"));
    }
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

#[cfg(test)]
mod golden {
    //! Byte-exact `Set-Cookie` wire goldens for the W-ID session flow.
    //!
    //! These pin the EXACT cookie strings the `/auth/*` handlers emit, and are
    //! the reason this service keeps its hand-rolled SSO glue rather than adopt
    //! the fleet-shared `w-id-axum` `SsoApp`/`routes()`: w-id-axum's
    //! `build_cookie` hardcodes `Secure` + omits `Domain` + serializes via
    //! axum-extra `Cookie` (different attribute order, `Max-Age` from a
    //! `time::Duration`). automl instead drives a configurable `secure` flag
    //! and an `.auto.ml` apex `Domain` (for cross-subdomain SSO with
    //! scholar.auto.ml). Adopting `routes()` would change these bytes, so the
    //! migration is KEEP-LOCAL. Wire output unchanged.
    use super::*;

    #[test]
    fn set_cookie_session_prod_domain_byte_identical() {
        // /callback session cookie, production (secure on, apex domain).
        assert_eq!(
            set_cookie(SESSION_COOKIE, "tok", 3600, true, true, Some(".auto.ml")),
            "automl_session=tok; Path=/; Max-Age=3600; SameSite=Lax; Domain=.auto.ml; HttpOnly; Secure"
        );
    }

    #[test]
    fn set_cookie_state_dev_no_domain_no_secure_byte_identical() {
        // /login csrf-state cookie, local dev (secure off, no domain).
        assert_eq!(
            set_cookie(STATE_COOKIE, "csrf", 600, true, false, None),
            "automl_oauth_state=csrf; Path=/; Max-Age=600; SameSite=Lax; HttpOnly"
        );
    }

    #[test]
    fn clear_cookie_attribute_order_byte_identical() {
        // Note clear_cookie emits HttpOnly BEFORE Domain (distinct from set_cookie).
        assert_eq!(
            clear_cookie(SESSION_COOKIE, true, Some(".auto.ml")),
            "automl_session=; Path=/; Max-Age=0; SameSite=Lax; HttpOnly; Domain=.auto.ml; Secure"
        );
        assert_eq!(
            clear_cookie(NEXT_COOKIE, false, None),
            "automl_login_next=; Path=/; Max-Age=0; SameSite=Lax; HttpOnly"
        );
    }

    #[test]
    fn cookie_names_are_the_automl_specific_set() {
        // Three cookies (session + csrf-state + dedicated login-next). w-id-axum
        // names the carried-next cookie `{state}_next`; automl uses a distinct
        // constant — another reason routes() is not byte-compatible.
        assert_eq!(SESSION_COOKIE, "automl_session");
        assert_eq!(STATE_COOKIE, "automl_oauth_state");
        assert_eq!(NEXT_COOKIE, "automl_login_next");
    }
}
