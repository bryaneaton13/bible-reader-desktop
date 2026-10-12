//! YouVersion Platform client: licensed Bibles over the network, cached on
//! disk for 30 days, plus the optional "Sign in with YouVersion" flow.
//!
//! No UI and no GPUI. Everything here blocks, so call it from a background
//! thread.

pub mod auth;
pub mod cache;
pub mod client;
pub mod html;
pub mod source;

pub use auth::{Pkce, REDIRECT_URI, Session, SignInRequest};
pub use cache::{CACHE_TTL, Cache};
pub use client::{Client, DEFAULT_HOST, Error};
pub use html::{Chapter, parse_chapter, verse_texts};
pub use source::{
    BibleInfo, Fetched, SearchPage, Source, parse_usfm_verse, usfm_chapter, usfm_verse,
};
