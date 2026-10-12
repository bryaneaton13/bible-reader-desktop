//! The optional YouVersion source: app-wide state (key, sign-in, Bible
//! catalog) and the glue between the blocking `bible-youversion` crate and
//! GPUI. Network work runs on the background executor; state changes go
//! through `update_global::<Remote>` so every window that observes it repaints.

use bible_youversion::{
    BibleInfo, Cache, Client, Error, Pkce, REDIRECT_URI, Session, Source, auth,
};
use gpui_kit::*;

use crate::library::{Settings, support_dir};

/// Where the app key came from, for the Settings page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySource {
    None,
    Settings,
    Environment,
    Bundled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignIn {
    Idle,
    /// The browser is open; waiting for YouVersion to send us back.
    Waiting,
    /// Back from the browser; trading the code for tokens.
    Finishing,
    Failed(String),
}

/// Bibles YouVersion lists for the chosen languages.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CatalogData {
    /// Readable with this app key.
    pub licensed: Vec<BibleInfo>,
    /// Listed but not licensed to this app (only when the user asks to see them).
    pub unlicensed: Vec<BibleInfo>,
    pub from_cache: bool,
    /// Licensed Bibles that only appeared after signing in, when known.
    pub gained_by_sign_in: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Catalog {
    #[default]
    Idle,
    Loading,
    Ready(CatalogData),
    Failed(String),
}

pub struct Remote {
    source: Option<Source>,
    pub key_source: KeySource,
    pub session: Option<Session>,
    pub sign_in: SignIn,
    pending: Option<Pkce>,
    pub catalog: Catalog,
    /// How many licensed Bibles the catalog had the last time we looked
    /// signed out, to tell the user what signing in added.
    signed_out_count: Option<usize>,
}

impl Global for Remote {}

/// Result of a background request, plus a refreshed session if the access
/// token had to be renewed first.
pub struct Outcome<T> {
    pub result: Result<T, Error>,
    pub session: Option<Session>,
}

fn cache_dir() -> std::path::PathBuf {
    support_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("cache/youversion")
}

fn resolve_key(settings: &Settings) -> (String, KeySource) {
    let pasted = settings.youversion.app_key.trim();
    if !pasted.is_empty() {
        return (pasted.to_string(), KeySource::Settings);
    }
    if let Ok(key) = std::env::var("YVP_APP_KEY")
        && !key.trim().is_empty()
    {
        return (key.trim().to_string(), KeySource::Environment);
    }
    if let Some(key) = option_env!("YVP_APP_KEY").filter(|k| !k.trim().is_empty()) {
        return (key.trim().to_string(), KeySource::Bundled);
    }
    (String::new(), KeySource::None)
}

impl Remote {
    fn build(settings: &Settings, session: Option<Session>) -> Self {
        let (key, key_source) = resolve_key(settings);
        let source = (key_source != KeySource::None).then(|| {
            let client = Client::new(key);
            client.set_token(session.as_ref().map(|s| s.access_token.clone()));
            Source::new(client, Cache::new(cache_dir()))
        });
        Self {
            source,
            key_source,
            session,
            sign_in: SignIn::Idle,
            pending: None,
            catalog: Catalog::Idle,
            signed_out_count: None,
        }
    }

    /// A state with a ready-made source, for tests and demos.
    #[cfg(any(test, feature = "screenshot"))]
    pub fn with_source(source: Source) -> Self {
        Self {
            source: Some(source),
            key_source: KeySource::Settings,
            session: None,
            sign_in: SignIn::Idle,
            pending: None,
            catalog: Catalog::Idle,
            signed_out_count: None,
        }
    }

    pub fn has_key(&self) -> bool {
        self.source.is_some()
    }

    pub fn requests_made(&self) -> u64 {
        self.source
            .as_ref()
            .map_or(0, |s| s.client().requests_made())
    }

    /// Cached responses on disk: (entries, bytes).
    pub fn cache_stats(&self) -> (usize, u64) {
        self.source
            .as_ref()
            .map_or_else(|| Cache::new(cache_dir()).stats(), |s| s.cache().stats())
    }
}

/// Set up the global at launch. Reads the Keychain only for people who have
/// signed in before, so everyone else never sees a Keychain prompt.
pub fn init(cx: &mut App) {
    let settings = cx.global::<Settings>().clone();
    let session = settings
        .youversion
        .signed_in
        .then(auth::store::load)
        .flatten();
    cx.set_global(Remote::build(&settings, session));
    // Drop anything past the 30-day window.
    let cache = Cache::new(cache_dir());
    cx.background_executor()
        .spawn(async move {
            cache.purge_expired();
        })
        .detach();
}

pub fn source(cx: &App) -> Option<Source> {
    cx.try_global::<Remote>()?.source.clone()
}

/// Rebuild the client after the app key changed. Keeps the session.
pub fn reload(cx: &mut App) {
    let settings = cx.global::<Settings>().clone();
    let session = cx.try_global::<Remote>().and_then(|r| r.session.clone());
    cx.set_global(Remote::build(&settings, session));
    refresh_catalog(cx);
}

/// Run `f` on the background executor with the source, renewing the access
/// token first if it has run out. `None` when no app key is configured.
pub fn request<T: Send + 'static>(
    cx: &App,
    f: impl FnOnce(&Source) -> Result<T, Error> + Send + 'static,
) -> Option<Task<Outcome<T>>> {
    let remote = cx.try_global::<Remote>()?;
    let source = remote.source.clone()?;
    let session = remote.session.clone();
    Some(cx.background_executor().spawn(async move {
        let mut renewed = None;
        if let Some(old) = session.filter(|s| s.needs_refresh(bible_youversion::cache::now()))
            && let Ok(fresh) = auth::refresh(source.client(), &old)
        {
            source.client().set_token(Some(fresh.access_token.clone()));
            auth::store::save(&fresh);
            renewed = Some(fresh);
        }
        Outcome {
            result: f(&source),
            session: renewed,
        }
    }))
}

/// Store a session the background renewed.
pub fn apply_renewed(session: Option<Session>, cx: &mut App) {
    if let Some(session) = session
        && cx.try_global::<Remote>().is_some()
    {
        cx.update_global::<Remote, _>(|r, _| r.session = Some(session));
    }
}

fn update(cx: &mut App, f: impl FnOnce(&mut Remote)) {
    if cx.try_global::<Remote>().is_some() {
        cx.update_global::<Remote, _>(|r, _| f(r));
    }
}

// MARK: Catalog

/// (Re)load the list of Bibles for the chosen languages.
pub fn refresh_catalog(cx: &mut App) {
    let settings = cx.global::<Settings>().youversion.clone();
    let languages = settings.languages.clone();
    let show_unlicensed = settings.show_unlicensed;
    let previous = cx.try_global::<Remote>().and_then(|r| match &r.catalog {
        Catalog::Ready(data) => Some(data.licensed.len()),
        _ => None,
    });
    let signed_in = cx
        .try_global::<Remote>()
        .is_some_and(|r| r.session.is_some());
    // Signing in can change the list, so a signed-out list must not be served
    // from cache afterwards.
    let audience = if signed_in { "user" } else { "anon" };
    let Some(task) = request(cx, move |source| {
        let licensed = source.bibles(&languages, false, audience)?;
        let mut unlicensed = Vec::new();
        let mut from_cache = licensed.from_cache;
        if show_unlicensed {
            let all = source.bibles(&languages, true, audience)?;
            from_cache &= all.from_cache;
            unlicensed = all
                .value
                .into_iter()
                .filter(|b| !licensed.value.iter().any(|l| l.id == b.id))
                .collect();
        }
        Ok(CatalogData {
            licensed: licensed.value,
            unlicensed,
            from_cache,
            gained_by_sign_in: None,
        })
    }) else {
        update(cx, |r| r.catalog = Catalog::Idle);
        return;
    };
    update(cx, |r| {
        r.catalog = Catalog::Loading;
        if !signed_in {
            r.signed_out_count = previous;
        }
    });
    cx.spawn(async move |cx| {
        let outcome = task.await;
        cx.update(|cx| {
            apply_renewed(outcome.session, cx);
            update(cx, |r| {
                r.catalog = match outcome.result {
                    Ok(mut data) => {
                        if r.session.is_some() {
                            data.gained_by_sign_in = r
                                .signed_out_count
                                .map(|before| data.licensed.len().saturating_sub(before))
                                .filter(|n| *n > 0);
                        }
                        Catalog::Ready(data)
                    }
                    Err(err) => Catalog::Failed(err.to_string()),
                };
            });
        });
    })
    .detach();
}

// MARK: Sign in

/// Open YouVersion's sign-in page in the browser.
pub fn begin_sign_in(cx: &mut App) {
    let Some(source) = source(cx) else {
        update(cx, |r| {
            r.sign_in = SignIn::Failed(Error::MissingKey.to_string())
        });
        return;
    };
    match auth::begin(source.client(), &["profile"]) {
        Ok(request) => {
            update(cx, |r| {
                r.pending = Some(request.pkce);
                r.sign_in = SignIn::Waiting;
            });
            cx.open_url(&request.url);
        }
        Err(err) => update(cx, |r| r.sign_in = SignIn::Failed(err.to_string())),
    }
}

pub fn cancel_sign_in(cx: &mut App) {
    update(cx, |r| {
        r.pending = None;
        r.sign_in = SignIn::Idle;
    });
}

/// The platform handed us URLs (a `youversionauth://` redirect).
pub fn handle_urls(urls: Vec<String>, cx: &mut App) {
    let Some(url) = urls
        .into_iter()
        .find(|u| u.starts_with(REDIRECT_URI.split("://").next().unwrap_or("youversionauth")))
    else {
        return;
    };
    finish_sign_in(url, cx);
}

fn finish_sign_in(url: String, cx: &mut App) {
    let Some(pkce) = cx.try_global::<Remote>().and_then(|r| r.pending.clone()) else {
        return; // not a sign-in we started
    };
    let Some(source) = source(cx) else { return };
    update(cx, |r| r.sign_in = SignIn::Finishing);
    cx.activate(true);
    let task = cx.background_executor().spawn(async move {
        let session = auth::complete(source.client(), &url, &pkce)?;
        auth::store::save(&session);
        source
            .client()
            .set_token(Some(session.access_token.clone()));
        Ok::<_, Error>(session)
    });
    cx.spawn(async move |cx| {
        let result = task.await;
        cx.update(|cx| match result {
            Ok(session) => {
                update(cx, |r| {
                    r.session = Some(session);
                    r.pending = None;
                    r.sign_in = SignIn::Idle;
                });
                cx.update_global::<Settings, _>(|s, _| {
                    s.youversion.signed_in = true;
                    s.save();
                });
                refresh_catalog(cx);
            }
            Err(err) => update(cx, |r| {
                r.pending = None;
                r.sign_in = SignIn::Failed(err.to_string());
            }),
        });
    })
    .detach();
}

pub fn sign_out(cx: &mut App) {
    auth::store::clear();
    if let Some(source) = source(cx) {
        source.client().set_token(None);
    }
    update(cx, |r| {
        r.session = None;
        r.pending = None;
        r.sign_in = SignIn::Idle;
    });
    cx.update_global::<Settings, _>(|s, _| {
        s.youversion.signed_in = false;
        s.save();
    });
    refresh_catalog(cx);
}

// MARK: Cache

pub fn clear_cache(cx: &mut App) -> usize {
    let removed = source(cx).map_or_else(|| Cache::new(cache_dir()).clear(), |s| s.cache().clear());
    // Repaint the page that shows the cache size.
    update(cx, |_| {});
    removed
}

/// "12 chapters · 1.4 MB" style summary.
pub fn describe_cache((entries, bytes): (usize, u64)) -> String {
    let size = match bytes {
        0 => "empty".to_string(),
        b if b < 1024 * 1024 => format!("{} KB", (b as f64 / 1024.).ceil() as u64),
        b => format!("{:.1} MB", b as f64 / (1024. * 1024.)),
    };
    match entries {
        0 => "Nothing cached".to_string(),
        1 => format!("1 response · {size}"),
        n => format!("{n} responses · {size}"),
    }
}

/// Dev-only fixtures for `scripts/shot.sh`: a fake YouVersion source whose
/// cache is pre-filled with synthetic, public-domain text, so every state of
/// the UI can be photographed without a key or network. Nothing here is
/// real YouVersion content, and it only runs under the throwaway HOME.
#[cfg(feature = "screenshot")]
pub mod demo {
    use super::*;
    use crate::library::{SavedBible, remote_key};

    pub const ID: u32 = 9999;

    const JOHN_3: &str = r#"{"id":"JHN.3","reference":"John 3","content":"<div><div class=\"s1\">The New Birth</div><div class=\"p\"><span class=\"yv-v\" v=\"1\"></span><span class=\"yv-vlbl\">1</span>There was a man of the Pharisees, named Nicodemus, a ruler of the Jews: <span class=\"yv-v\" v=\"2\"></span><span class=\"yv-vlbl\">2</span>The same came to Jesus by night, and said unto him, Rabbi, we know that thou art a teacher come from God.<span class=\"yv-n f\"><span class=\"fr\">3:2 </span><span class=\"ft\">Demo footnote text.</span></span></div><div class=\"p\"><span class=\"yv-v\" v=\"3\"></span><span class=\"yv-vlbl\">3</span><span class=\"wj\">Jesus answered and said unto him, Verily, verily, I say unto thee, Except a man be born again, he cannot see the kingdom of God.</span></div><div class=\"s1\">For God So Loved</div><div class=\"q1\"><span class=\"yv-v\" v=\"16\"></span><span class=\"yv-vlbl\">16</span><span class=\"wj\">For God so loved the world,</span></div><div class=\"q2\"><span class=\"wj\">that he gave his only begotten Son,</span></div><div class=\"q2\"><span class=\"wj\">that whosoever believeth in him should not perish,</span></div><div class=\"q2\"><span class=\"wj\">but have everlasting life.</span></div></div>"}"#;

    const CATALOG: &str = r#"{"data":[{"id":9999,"abbreviation":"DEMO","title":"Demo Bible","localized_title":"Demo Bible (public-domain KJV wording)","language_tag":"en","copyright":"Public domain demo text, not from YouVersion."},{"id":9101,"abbreviation":"SAMPLE","title":"Sample Standard Version","language_tag":"en","copyright":"© Example Publisher"},{"id":9102,"abbreviation":"TEST","title":"Test Study Bible","language_tag":"en","copyright":"© Example Publisher"}],"total_size":3}"#;

    const UNLICENSED: &str = r#"{"data":[{"id":9999,"abbreviation":"DEMO","title":"Demo Bible","language_tag":"en"},{"id":9101,"abbreviation":"SAMPLE","title":"Sample Standard Version","language_tag":"en"},{"id":9102,"abbreviation":"TEST","title":"Test Study Bible","language_tag":"en"},{"id":9200,"abbreviation":"LOCKED","title":"Locked Example Bible","language_tag":"en"}],"total_size":4}"#;

    pub fn seed(cx: &mut App) {
        let dir = std::env::temp_dir().join(format!("br-yv-demo-{}", std::process::id()));
        let cache = Cache::new(&dir);
        let _ = cache.put("passage:9999:JHN.3:html", JOHN_3);
        let _ = cache.put("bibles:en:false:", CATALOG);
        let _ = cache.put("bibles:en:true:", UNLICENSED);
        cx.set_global(Remote::with_source(Source::new(
            Client::new("demo-key"),
            cache,
        )));
        cx.update_global::<Settings, _>(|s, _| {
            if !s.youversion.bibles.iter().any(|b| b.id == ID) {
                s.youversion.bibles.push(SavedBible {
                    id: ID,
                    abbrev: "DEMO".into(),
                    name: "Demo Bible (public-domain KJV wording)".into(),
                    copyright: "Public domain demo text, not from YouVersion.".into(),
                });
            }
        });
    }

    pub fn key() -> String {
        remote_key(ID)
    }

    /// Fake sign-in states: `signedin`, `waiting`, `finishing`, `failed`.
    pub fn sign_in(state: &str, cx: &mut App) {
        update(cx, |r| match state {
            "signedin" => {
                r.session = Some(Session {
                    access_token: "x".into(),
                    refresh_token: "x".into(),
                    id_token: "x".into(),
                    expires_at: u64::MAX,
                    permissions: vec!["profile".into()],
                    user_id: Some("demo".into()),
                    name: Some("Ada Lovelace".into()),
                    email: Some("ada@example.org".into()),
                    picture: None,
                });
                if let Catalog::Ready(data) = &mut r.catalog {
                    data.gained_by_sign_in = Some(2);
                }
            }
            "waiting" => r.sign_in = SignIn::Waiting,
            "finishing" => r.sign_in = SignIn::Finishing,
            "failed" => {
                r.sign_in =
                    SignIn::Failed("Sign in failed: the response didn't match this sign-in".into())
            }
            "loadingcatalog" => r.catalog = Catalog::Loading,
            "catalogerror" => {
                r.catalog = Catalog::Failed(Error::Offline(String::new()).to_string())
            }
            _ => {}
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn pasted_key_wins_and_blank_means_none() {
        let mut settings = Settings::default();
        settings.youversion.app_key = "  abc ".into();
        assert_eq!(
            resolve_key(&settings),
            ("abc".to_string(), KeySource::Settings)
        );
        settings.youversion.app_key = "   ".into();
        let (key, source) = resolve_key(&settings);
        // The environment may carry a key on a developer machine.
        assert_eq!(key.is_empty(), source == KeySource::None);
    }

    #[test]
    fn describes_cache_sizes() {
        assert_eq!(describe_cache((0, 0)), "Nothing cached");
        assert_eq!(describe_cache((1, 300)), "1 response · 1 KB");
        assert_eq!(
            describe_cache((12, 3 * 1024 * 1024 / 2)),
            "12 responses · 1.5 MB"
        );
    }
}
