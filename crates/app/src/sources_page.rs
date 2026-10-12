//! Settings ▸ Bible sources: the optional YouVersion connection. An app key,
//! an optional "Sign in with YouVersion", the list of Bibles to add, and the
//! 30-day cache. Every row reads the global `Remote` / `Settings` and writes
//! them back, so the window repaints from the globals it observes.

use bible_youversion::BibleInfo;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::link::Link;
use gpui_kit::component::setting::{SettingField, SettingGroup, SettingItem, SettingPage};
use gpui_kit::component::skeleton::Skeleton;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Disableable as _, Sizable as _};
use gpui_kit::component::{Icon, IconName};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::library::{SavedBible, Settings};
use crate::remote::{self, Catalog, KeySource, Remote, SignIn};

pub const PAGE_TITLE: &str = "Bible sources";
const PORTAL_URL: &str = "https://platform.youversion.com";
/// Rows shown at once, so a catalog of hundreds stays quick to draw.
const ROWS_SHOWN: usize = 50;

/// Languages offered for the catalog: (range, label). `*` is every language.
const LANGUAGES: &[(&str, &str)] = &[
    ("en", "English"),
    ("es", "Spanish"),
    ("fr", "French"),
    ("de", "German"),
    ("pt", "Portuguese"),
    ("it", "Italian"),
    ("ru", "Russian"),
    ("zh", "Chinese"),
    ("ko", "Korean"),
    ("ja", "Japanese"),
    ("ar", "Arabic"),
    ("hi", "Hindi"),
    ("sw", "Swahili"),
    ("*", "All languages"),
];

/// Text typed into the Bibles filter. Not saved.
#[derive(Default)]
pub struct Filter(pub String);

impl Global for Filter {}

fn filter(cx: &App) -> String {
    cx.try_global::<Filter>()
        .map(|f| f.0.trim().to_lowercase())
        .unwrap_or_default()
}

fn update_settings(cx: &mut App, f: impl FnOnce(&mut Settings)) {
    cx.update_global::<Settings, _>(|s, _| {
        f(s);
        s.save();
    });
}

fn muted_text(cx: &App, text: impl Into<SharedString>) -> Div {
    div()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

/// A small pill, e.g. "Needs license".
fn pill(cx: &App, text: &'static str) -> Div {
    let theme = cx.theme();
    div()
        .px_1p5()
        .py_0p5()
        .rounded_sm()
        .bg(theme.muted)
        .text_xs()
        .text_color(theme.muted_foreground)
        .child(text)
}

pub fn page(cx: &App) -> SettingPage {
    SettingPage::new(PAGE_TITLE)
        .description(
            "Add licensed Bibles from YouVersion. Optional: the bundled translations always work offline.",
        )
        .group(connection_group())
        .group(account_group())
        .group(bibles_group(cx))
        .group(cache_group())
}

// MARK: Connection

fn connection_group() -> SettingGroup {
    SettingGroup::new()
        .title("Connection")
        .item(
            SettingItem::new(
                "App key",
                SettingField::input(
                    |cx: &App| cx.global::<Settings>().youversion.app_key.clone().into(),
                    |value: SharedString, cx: &mut App| {
                        update_settings(cx, |s| s.youversion.app_key = value.trim().to_string());
                    },
                ),
            )
            .description("Paste the key from your app in the YouVersion Platform portal, then press Connect.")
            .keywords(["key", "api", "youversion", "token"]),
        )
        .item(SettingItem::render(|_, _, cx| {
            let remote = cx.try_global::<Remote>();
            let (has_key, key_source, requests) = remote.map_or((false, KeySource::None, 0), |r| {
                (r.has_key(), r.key_source, r.requests_made())
            });
            let theme = cx.theme();
            let (dot, status) = if has_key {
                let from = match key_source {
                    KeySource::Settings => "your pasted key",
                    KeySource::Environment => "YVP_APP_KEY from the environment",
                    KeySource::Bundled => "the key built into this app",
                    KeySource::None => "",
                };
                (
                    theme.success,
                    format!("Connected with {from} · {requests} API requests this session"),
                )
            } else {
                (
                    theme.muted_foreground,
                    "No app key yet. YouVersion Bibles stay off until you add one.".to_string(),
                )
            };
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().size_2().rounded_full().bg(dot))
                        .child(div().text_sm().child(status)),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            Button::new("yv-connect")
                                .label("Connect")
                                .small()
                                .on_click(|_, _, cx| remote::reload(cx)),
                        )
                        .child(
                            Link::new("yv-portal")
                                .href(PORTAL_URL)
                                .text_sm()
                                .child("Open the YouVersion Platform portal"),
                        ),
                )
        }))
}

// MARK: Account

fn account_group() -> SettingGroup {
    SettingGroup::new()
        .title("Account")
        .item(SettingItem::render(|_, _, cx| account_card(cx)))
}

fn initials(name: &str) -> String {
    let letters: String = name
        .split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(2)
        .flat_map(char::to_uppercase)
        .collect();
    if letters.is_empty() {
        "Y".into()
    } else {
        letters
    }
}

fn account_card(cx: &App) -> Div {
    let theme = cx.theme();
    let card = div()
        .flex()
        .flex_col()
        .gap_3()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(theme.border);
    let Some(remote) = cx.try_global::<Remote>().filter(|r| r.has_key()) else {
        return card.child(muted_text(
            cx,
            "Add an app key above to enable Sign in with YouVersion.",
        ));
    };
    if let Some(session) = &remote.session {
        let name = session.display_name();
        let gained = match &remote.catalog {
            Catalog::Ready(data) => data.gained_by_sign_in,
            _ => None,
        };
        return card
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .size_10()
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(theme.primary)
                            .text_color(theme.primary_foreground)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(initials(&name)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(name.clone()))
                            .when_some(session.email.clone(), |d, email| {
                                d.child(muted_text(cx, email))
                            }),
                    )
                    .child(
                        Button::new("yv-sign-out")
                            .label("Sign out")
                            .small()
                            .on_click(|_, _, cx| remote::sign_out(cx)),
                    ),
            )
            .child(muted_text(
                cx,
                match gained {
                    Some(1) => "Signing in added 1 Bible to your list.".to_string(),
                    Some(n) => format!("Signing in added {n} Bibles to your list."),
                    None => "Signed in. Your Bibles list below reflects your account.".to_string(),
                },
            ));
    }
    let intro = muted_text(
        cx,
        "Signing in is optional. When you do, YouVersion may offer more Bibles for your account, and the list below refreshes to show them. Reading the bundled translations never needs it.",
    );
    match &remote.sign_in {
        SignIn::Waiting | SignIn::Finishing => {
            let waiting = remote.sign_in == SignIn::Waiting;
            card.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_sm()
                    .child(Spinner::new().small())
                    .child(if waiting {
                        "Finish signing in in your browser…"
                    } else {
                        "Finishing sign in…"
                    }),
            )
            .when(waiting, |c| {
                c.child(muted_text(
                    cx,
                    "When YouVersion sends you back, Bible Reader comes forward on its own. Sign in works from the installed app (scripts/bundle.sh --install).",
                ))
                .child(
                    div().flex().child(
                        Button::new("yv-cancel-sign-in")
                            .label("Cancel")
                            .ghost()
                            .small()
                            .on_click(|_, _, cx| remote::cancel_sign_in(cx)),
                    ),
                )
            })
        }
        other => card
            .child(intro)
            .when_some(
                match other {
                    SignIn::Failed(message) => Some(message.clone()),
                    _ => None,
                },
                |c, message| {
                    c.child(
                        div()
                            .flex()
                            .items_start()
                            .gap_2()
                            .text_sm()
                            .text_color(cx.theme().danger)
                            .child(Icon::new(IconName::CircleAlert))
                            .child(message),
                    )
                },
            )
            .child(
                div().flex().child(
                    Button::new("yv-sign-in")
                        .icon(IconName::CircleUser)
                        .label("Sign in with YouVersion")
                        .primary()
                        .on_click(|_, _, cx| remote::begin_sign_in(cx)),
                ),
            ),
    }
}

// MARK: Bibles

fn add_bible(cx: &mut App, bible: &BibleInfo) {
    let saved = SavedBible {
        id: bible.id,
        abbrev: bible.label(),
        name: bible.name().to_string(),
        copyright: bible
            .copyright
            .clone()
            .unwrap_or_default()
            .trim()
            .to_string(),
        books: bible.books.clone(),
        publisher: String::new(),
        publisher_url: bible.publisher_url.clone().unwrap_or_default(),
    };
    update_settings(cx, |s| {
        if !s.youversion.bibles.iter().any(|b| b.id == saved.id) {
            s.youversion.bibles.push(saved);
        }
    });
    // The list has no copyright notice: fetch it (and the publisher) now.
    remote::refresh_saved(cx);
}

fn remove_bible(cx: &mut App, id: u32) {
    update_settings(cx, |s| s.youversion.bibles.retain(|b| b.id != id));
}

fn bible_switch(id: u32, saved: Option<SavedBible>, info: Option<BibleInfo>) -> SettingField<bool> {
    SettingField::switch(
        move |cx: &App| {
            cx.global::<Settings>()
                .youversion
                .bibles
                .iter()
                .any(|b| b.id == id)
        },
        move |on: bool, cx: &mut App| {
            if !on {
                remove_bible(cx, id);
            } else if let Some(info) = &info {
                add_bible(cx, info);
            } else if let Some(saved) = &saved {
                update_settings(cx, |s| s.youversion.bibles.push(saved.clone()));
            }
        },
    )
}

fn bible_row(info: &BibleInfo) -> SettingItem {
    let mut description = String::new();
    if !info.language_tag.is_empty() {
        description.push_str(&info.language_tag);
    }
    if let Some(copyright) = info.copyright.as_deref().filter(|c| !c.trim().is_empty()) {
        if !description.is_empty() {
            description.push_str(" · ");
        }
        description.push_str(copyright.trim());
    }
    SettingItem::new(
        format!("{} · {}", info.label(), info.name()),
        bible_switch(info.id, None, Some(info.clone())),
    )
    .description(description)
}

fn saved_row(bible: &SavedBible) -> SettingItem {
    SettingItem::new(
        format!("{} · {}", bible.abbrev, bible.name),
        bible_switch(bible.id, Some(bible.clone()), None),
    )
    .description(bible.copyright.clone())
}

fn matches_filter(info: &BibleInfo, needle: &str) -> bool {
    needle.is_empty()
        || info.label().to_lowercase().contains(needle)
        || info.name().to_lowercase().contains(needle)
        || info.title.to_lowercase().contains(needle)
}

fn bibles_group(cx: &App) -> SettingGroup {
    let languages = LANGUAGES
        .iter()
        .map(|(range, label)| (SharedString::from(*range), SharedString::from(*label)))
        .collect();
    let mut group = SettingGroup::new()
        .title("Bibles")
        .item(
            SettingItem::new(
                "Language",
                SettingField::dropdown(
                    languages,
                    |cx: &App| {
                        cx.global::<Settings>()
                            .youversion
                            .languages
                            .first()
                            .cloned()
                            .unwrap_or_else(|| "en".into())
                            .into()
                    },
                    |value: SharedString, cx: &mut App| {
                        update_settings(cx, |s| s.youversion.languages = vec![value.to_string()]);
                        remote::refresh_catalog(cx);
                    },
                )
                .default_value(SharedString::from("en")),
            )
            .description("Which language's Bibles to list."),
        )
        .item(
            SettingItem::new(
                "Find a Bible",
                SettingField::input(
                    |cx: &App| {
                        cx.try_global::<Filter>()
                            .map(|f| f.0.clone())
                            .unwrap_or_default()
                            .into()
                    },
                    |value: SharedString, cx: &mut App| cx.set_global(Filter(value.to_string())),
                ),
            )
            .description("Filter by name or abbreviation, like “NIV” or “standard”."),
        )
        .item(
            SettingItem::new(
                "Show Bibles that need a license",
                SettingField::switch(
                    |cx: &App| cx.global::<Settings>().youversion.show_unlicensed,
                    |on: bool, cx: &mut App| {
                        update_settings(cx, |s| s.youversion.show_unlicensed = on);
                        remote::refresh_catalog(cx);
                    },
                )
                .default_value(false),
            )
            .description(
                "Listed for reference; reading them needs the publisher's license on your app.",
            ),
        );
    group = group.item(SettingItem::render(|_, _, cx| catalog_status(cx)));
    group.items(catalog_rows(cx))
}

/// Loading, error or hint line above the Bible rows.
fn catalog_status(cx: &App) -> Div {
    let state = cx
        .try_global::<Remote>()
        .map(|r| (r.has_key(), r.catalog.clone()));
    let line = div().flex().flex_col().gap_2();
    match state {
        None | Some((false, _)) => line.child(muted_text(
            cx,
            "Add an app key to load the list of Bibles. Bibles you added earlier stay available.",
        )),
        Some((true, Catalog::Idle)) => line.child(
            div().flex().child(
                Button::new("yv-load-catalog")
                    .label("Load Bibles")
                    .small()
                    .on_click(|_, _, cx| remote::refresh_catalog(cx)),
            ),
        ),
        Some((true, Catalog::Loading)) => line
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_sm()
                    .child(Spinner::new().small())
                    .child("Loading Bibles from YouVersion…"),
            )
            .children((0..4).map(|ix| {
                Skeleton::new()
                    .h_8()
                    .w(relative([0.92, 0.8, 0.88, 0.7][ix]))
                    .rounded_md()
            })),
        Some((true, Catalog::Failed(message))) => line
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap_2()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(Icon::new(IconName::CircleAlert))
                    .child(message),
            )
            .child(
                div().flex().child(
                    Button::new("yv-retry-catalog")
                        .icon(IconName::RefreshCw)
                        .label("Try again")
                        .small()
                        .on_click(|_, _, cx| remote::refresh_catalog(cx)),
                ),
            ),
        Some((true, Catalog::Ready(data))) => {
            let shown = data.licensed.len();
            line.child(muted_text(
                cx,
                match (shown, data.from_cache) {
                    (0, _) => "No licensed Bibles for this language yet. Accept a publisher's license in the portal, then reload.".to_string(),
                    (n, true) => format!("{n} Bibles available · from cache"),
                    (n, false) => format!("{n} Bibles available"),
                },
            ))
        }
    }
}

/// The Bible rows: the catalog when it has loaded, otherwise just what the
/// user already added (so they can still remove things offline).
fn catalog_rows(cx: &App) -> Vec<SettingItem> {
    let filter = filter(cx);
    match cx.try_global::<Remote>().map(|r| &r.catalog) {
        Some(Catalog::Ready(data)) => {
            let matching: Vec<&BibleInfo> = data
                .licensed
                .iter()
                .filter(|b| matches_filter(b, &filter))
                .collect();
            let mut rows: Vec<SettingItem> = matching
                .iter()
                .take(ROWS_SHOWN)
                .map(|b| bible_row(b))
                .collect();
            if matching.len() > ROWS_SHOWN {
                let more = matching.len() - ROWS_SHOWN;
                rows.push(SettingItem::render(move |_, _, cx| {
                    muted_text(
                        cx,
                        format!(
                            "{more} more. Use “Find a Bible” or pick a language to narrow the list."
                        ),
                    )
                }));
            }
            for bible in data
                .unlicensed
                .iter()
                .filter(|b| matches_filter(b, &filter))
                .take(ROWS_SHOWN)
            {
                let title = format!("{} · {}", bible.label(), bible.name());
                rows.push(SettingItem::render(move |_, _, cx| {
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_color(cx.theme().muted_foreground)
                        .child(div().flex_1().min_w_0().child(title.clone()))
                        .child(pill(cx, "Needs license"))
                }));
            }
            rows
        }
        _ => cx
            .global::<Settings>()
            .youversion
            .bibles
            .iter()
            .filter(|b| {
                filter.is_empty()
                    || b.abbrev.to_lowercase().contains(&filter)
                    || b.name.to_lowercase().contains(&filter)
            })
            .map(saved_row)
            .collect(),
    }
}

// MARK: Cache

fn cache_group() -> SettingGroup {
    SettingGroup::new()
        .title("Storage")
        .item(SettingItem::render(|_, _, cx| {
            let stats = cx
                .try_global::<Remote>()
                .map_or((0, 0), |r| r.cache_stats());
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .child(div().text_sm().child(remote::describe_cache(stats)))
                        .child(
                            Button::new("yv-clear-cache")
                                .label("Clear cache")
                                .small()
                                .disabled(stats.0 == 0)
                                .on_click(|_, _, cx| {
                                    remote::clear_cache(cx);
                                }),
                        ),
                )
                .child(muted_text(
                    cx,
                    "YouVersion chapters and search results are kept on this Mac for 30 days, then deleted. They are never saved in the app's files or its source code. Open chapters read offline while cached.",
                ))
        }))
}
