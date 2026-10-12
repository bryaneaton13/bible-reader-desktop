//! "Sign in with YouVersion": OAuth authorization-code flow with PKCE.
//!
//! Mirrors YouVersion's own Swift SDK. The app opens [`SignInRequest::url`]
//! in the browser; YouVersion redirects to a custom-scheme URL carrying a
//! `state`; we hand that callback to [`complete`], which finalizes it with
//! `/auth/callback` (a 302 whose `Location` has the code) and exchanges the
//! code at `/auth/token`.

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;

use crate::cache::now;
use crate::client::{Client, Error};

/// Where YouVersion sends the browser back to. The bundled app registers
/// this scheme in its Info.plist.
pub const REDIRECT_URI: &str = "youversionauth://callback";

/// Permissions that travel in the OAuth `scope`; anything else goes in
/// `requested_permissions`.
const SCOPES: &[&str] = &["openid", "profile", "email"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
    pub state: String,
    pub nonce: String,
}

#[derive(Debug, Clone)]
pub struct SignInRequest {
    pub url: String,
    pub pkce: Pkce,
}

fn random_token(bytes: usize) -> Result<String, Error> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| Error::SignIn(e.to_string()))?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

pub fn challenge_for(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

impl Pkce {
    pub fn generate() -> Result<Self, Error> {
        let verifier = random_token(32)?;
        Ok(Self {
            challenge: challenge_for(&verifier),
            verifier,
            state: random_token(24)?,
            nonce: random_token(24)?,
        })
    }
}

/// Build the browser URL for a sign-in attempt.
pub fn begin(client: &Client, permissions: &[&str]) -> Result<SignInRequest, Error> {
    let pkce = Pkce::generate()?;
    let mut scopes: Vec<&str> = permissions
        .iter()
        .copied()
        .filter(|p| SCOPES.contains(p))
        .collect();
    if !scopes.contains(&"openid") {
        scopes.push("openid");
    }
    scopes.sort_unstable();
    let extra: Vec<&str> = permissions
        .iter()
        .copied()
        .filter(|p| !SCOPES.contains(p))
        .collect();
    let mut query = vec![
        ("response_type", "code".to_string()),
        ("client_id", client.app_key().to_string()),
        ("redirect_uri", REDIRECT_URI.to_string()),
        ("nonce", pkce.nonce.clone()),
        ("state", pkce.state.clone()),
        ("code_challenge", pkce.challenge.clone()),
        ("code_challenge_method", "S256".to_string()),
        ("scope", scopes.join(" ")),
    ];
    if !extra.is_empty() {
        query.push(("requested_permissions", extra.join(",")));
    }
    let url = client.url("/auth/authorize", &query)?;
    Ok(SignInRequest {
        url: url.to_string(),
        pkce,
    })
}

/// A signed-in user. Stored in the macOS Keychain, never in settings.json.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    pub id_token: String,
    /// Seconds since the Unix epoch.
    pub expires_at: u64,
    pub permissions: Vec<String>,
    pub user_id: Option<String>,
    pub name: Option<String>,
    pub email: Option<String>,
    pub picture: Option<String>,
}

impl Session {
    /// The access token has (nearly) run out; refresh before using it.
    pub fn needs_refresh(&self, now: u64) -> bool {
        self.expires_at <= now + 60
    }

    /// Name to greet the user with.
    pub fn display_name(&self) -> String {
        self.name
            .clone()
            .or_else(|| self.email.clone())
            .unwrap_or_else(|| "YouVersion user".to_string())
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: String,
    #[serde(default)]
    id_token: String,
    #[serde(default)]
    scope: String,
    #[serde(default)]
    expires_in: serde_json::Value,
}

fn expires_in_secs(v: &serde_json::Value) -> u64 {
    match v {
        serde_json::Value::Number(n) => n.as_u64().unwrap_or(0),
        serde_json::Value::String(s) => s.trim().parse().unwrap_or(0),
        _ => 0,
    }
}

fn decode_jwt(token: &str) -> serde_json::Value {
    let mut parts = token.split('.');
    let (_, Some(payload), Some(_)) = (parts.next(), parts.next(), parts.next()) else {
        return serde_json::Value::Null;
    };
    let payload = payload.trim_end_matches('=');
    URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| STANDARD.decode(payload))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or(serde_json::Value::Null)
}

fn claim(claims: &serde_json::Value, name: &str) -> Option<String> {
    claims.get(name)?.as_str().map(str::to_string)
}

fn split_permissions(scope: &str) -> Vec<String> {
    scope
        .split([' ', ','])
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect()
}

fn session_from(
    tokens: TokenResponse,
    id_token_fallback: &str,
    nonce: Option<&str>,
) -> Result<Session, Error> {
    let id_token = if tokens.id_token.is_empty() {
        id_token_fallback.to_string()
    } else {
        tokens.id_token
    };
    let claims = decode_jwt(&id_token);
    if let Some(expected) = nonce
        && claim(&claims, "nonce").as_deref() != Some(expected)
    {
        return Err(Error::SignIn(
            "the response didn't match this sign-in".into(),
        ));
    }
    Ok(Session {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        expires_at: now() + expires_in_secs(&tokens.expires_in),
        permissions: split_permissions(&tokens.scope),
        user_id: claim(&claims, "sub"),
        name: claim(&claims, "name"),
        email: claim(&claims, "email"),
        picture: claim(&claims, "profile_picture"),
        id_token,
    })
}

/// The `code` in a redirect URL's query.
fn code_from(location: &str) -> Option<String> {
    Url::parse(location)
        .ok()?
        .query_pairs()
        .find(|(k, _)| k == "code")
        .map(|(_, v)| v.into_owned())
}

/// Finish a sign-in from the URL the browser was redirected to.
pub fn complete(client: &Client, callback_url: &str, pkce: &Pkce) -> Result<Session, Error> {
    let callback = Url::parse(callback_url)
        .map_err(|_| Error::SignIn("unreadable callback address".into()))?;
    let params: Vec<(String, String)> = callback
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if let Some((_, why)) = params.iter().find(|(k, _)| k == "error") {
        return Err(Error::SignIn(why.replace('_', " ")));
    }
    if params
        .iter()
        .find(|(k, _)| k == "state")
        .map(|(_, v)| v.as_str())
        != Some(pkce.state.as_str())
    {
        return Err(Error::SignIn(
            "the response didn't match this sign-in".into(),
        ));
    }
    let query: Vec<(&str, String)> = params
        .iter()
        .map(|(k, v)| (k.as_str(), v.clone()))
        .collect();
    let url = client.url("/auth/callback", &query)?;
    let location = client
        .get_location(&url)?
        .ok_or_else(|| Error::SignIn("YouVersion didn't return an authorization code".into()))?;
    let code = code_from(&location)
        .ok_or_else(|| Error::SignIn("YouVersion didn't return an authorization code".into()))?;
    let body = client.post_form(
        "/auth/token",
        &[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", REDIRECT_URI),
            ("client_id", client.app_key()),
            ("code_verifier", &pkce.verifier),
        ],
    )?;
    let tokens: TokenResponse =
        serde_json::from_str(&body).map_err(|e| Error::Invalid(e.to_string()))?;
    session_from(tokens, "", Some(&pkce.nonce))
}

/// Trade the refresh token for a new access token.
pub fn refresh(client: &Client, session: &Session) -> Result<Session, Error> {
    let body = client.post_form(
        "/auth/token",
        &[
            ("grant_type", "refresh_token"),
            ("client_id", client.app_key()),
            ("refresh_token", &session.refresh_token),
        ],
    )?;
    let mut tokens: TokenResponse =
        serde_json::from_str(&body).map_err(|e| Error::Invalid(e.to_string()))?;
    if tokens.refresh_token.is_empty() {
        tokens.refresh_token = session.refresh_token.clone();
    }
    let mut fresh = session_from(tokens, &session.id_token, None)?;
    // A refresh response carries no profile; keep what we knew.
    fresh.user_id = fresh.user_id.or_else(|| session.user_id.clone());
    fresh.name = fresh.name.or_else(|| session.name.clone());
    fresh.email = fresh.email.or_else(|| session.email.clone());
    fresh.picture = fresh.picture.or_else(|| session.picture.clone());
    Ok(fresh)
}

/// Keychain storage for the session.
pub mod store {
    use super::Session;

    #[cfg(target_os = "macos")]
    mod imp {
        use super::Session;
        use security_framework::passwords::{
            delete_generic_password, get_generic_password, set_generic_password,
        };

        const SERVICE: &str = "com.biblereader.BibleReader.youversion";
        const ACCOUNT: &str = "session";

        pub fn save(session: &Session) {
            if let Ok(json) = serde_json::to_vec(session) {
                let _ = set_generic_password(SERVICE, ACCOUNT, &json);
            }
        }

        pub fn load() -> Option<Session> {
            serde_json::from_slice(&get_generic_password(SERVICE, ACCOUNT).ok()?).ok()
        }

        pub fn clear() {
            let _ = delete_generic_password(SERVICE, ACCOUNT);
        }
    }

    #[cfg(not(target_os = "macos"))]
    mod imp {
        use super::Session;

        pub fn save(_: &Session) {}
        pub fn load() -> Option<Session> {
            None
        }
        pub fn clear() {}
    }

    pub use imp::{clear, load, save};
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn pkce_challenge_matches_rfc7636_example() {
        // RFC 7636 appendix B.
        assert_eq!(
            challenge_for("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn pkce_values_are_fresh_and_url_safe() {
        let a = Pkce::generate().unwrap();
        let b = Pkce::generate().unwrap();
        assert_ne!(a.verifier, b.verifier);
        assert_ne!(a.state, b.state);
        assert_eq!(a.challenge, challenge_for(&a.verifier));
        assert!(a.verifier.len() >= 43);
        assert!(!a.state.contains(['+', '/', '=']));
    }

    #[test]
    fn authorize_url_carries_pkce_and_scopes() {
        let client = Client::new("my-app-key");
        let req = begin(&client, &["profile", "highlights"]).unwrap();
        let url = Url::parse(&req.url).unwrap();
        assert_eq!(url.path(), "/auth/authorize");
        let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(q["client_id"], "my-app-key");
        assert_eq!(q["response_type"], "code");
        assert_eq!(q["redirect_uri"], REDIRECT_URI);
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["code_challenge"], req.pkce.challenge);
        assert_eq!(q["state"], req.pkce.state);
        assert_eq!(q["scope"], "openid profile");
        assert_eq!(q["requested_permissions"], "highlights");
    }

    fn jwt(claims: &str) -> String {
        format!("h.{}.s", URL_SAFE_NO_PAD.encode(claims))
    }

    #[test]
    fn session_reads_profile_and_checks_nonce() {
        let tokens = || TokenResponse {
            access_token: "at".into(),
            refresh_token: "rt".into(),
            id_token: jwt(r#"{"nonce":"n1","sub":"u9","name":"Ada","email":"a@x.org"}"#),
            scope: "openid profile".into(),
            expires_in: serde_json::json!("3600"),
        };
        let s = session_from(tokens(), "", Some("n1")).unwrap();
        assert_eq!(s.user_id.as_deref(), Some("u9"));
        assert_eq!(s.display_name(), "Ada");
        assert_eq!(s.permissions, ["openid", "profile"]);
        assert!(!s.needs_refresh(now()));
        assert!(s.needs_refresh(now() + 3600));
        assert!(matches!(
            session_from(tokens(), "", Some("other")),
            Err(Error::SignIn(_))
        ));
    }

    #[test]
    fn finds_the_code_in_a_redirect_location() {
        assert_eq!(
            code_from("youversionauth://callback?code=abc123&state=s").as_deref(),
            Some("abc123")
        );
        assert_eq!(code_from("youversionauth://callback?state=s"), None);
    }

    #[test]
    fn complete_rejects_wrong_state_and_provider_errors_without_network() {
        let client = Client::new("k");
        let pkce = Pkce::generate().unwrap();
        let wrong = format!("{REDIRECT_URI}?state=nope");
        assert!(matches!(
            complete(&client, &wrong, &pkce),
            Err(Error::SignIn(_))
        ));
        let denied = format!("{REDIRECT_URI}?error=access_denied&state={}", pkce.state);
        let Err(Error::SignIn(why)) = complete(&client, &denied, &pkce) else {
            panic!("expected sign-in error");
        };
        assert_eq!(why, "access denied");
        assert_eq!(client.requests_made(), 0);
    }
}
