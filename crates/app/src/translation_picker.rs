//! Rows for the toolbar's searchable translation dropdown (gpui-kit `Select`).
//! The trigger shows just the abbreviation; the list shows the full name too,
//! and search matches either. Bibles from YouVersion carry a small tag.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::select::SelectItem;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::library::SourceEntry;

#[derive(Clone)]
pub struct TranslationItem {
    /// What tabs store: the abbreviation, or `yv:<id>` for YouVersion Bibles.
    key: String,
    label: String,
    name: SharedString,
    remote: bool,
}

impl TranslationItem {
    pub fn from_entry(entry: &SourceEntry) -> Self {
        Self {
            key: entry.key.clone(),
            label: entry.label.clone(),
            name: entry.name.clone().into(),
            remote: entry.remote,
        }
    }
}

impl SelectItem for TranslationItem {
    type Value = String;

    fn title(&self) -> SharedString {
        format!("{} ({})", self.label, self.name).into()
    }

    fn display_title(&self) -> Option<AnyElement> {
        Some(self.label.clone().into_any_element())
    }

    fn render(&self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let tag_bg = theme.muted;
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .w(px(48.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.label.clone()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(muted)
                    .child(self.name.clone()),
            )
            .when(self.remote, |row| {
                row.child(
                    div()
                        .px_1p5()
                        .rounded_sm()
                        .bg(tag_bg)
                        .text_xs()
                        .text_color(muted)
                        .child("YouVersion"),
                )
            })
    }

    fn value(&self) -> &String {
        &self.key
    }

    fn matches(&self, query: &str) -> bool {
        let query = query.to_lowercase();
        self.label.to_lowercase().contains(&query)
            || self.name.to_lowercase().contains(&query)
            || (self.remote && "youversion".contains(&query))
    }
}
