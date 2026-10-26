//! Rows for the toolbar's searchable translation dropdown (gpui-kit `Select`).
//! The trigger shows just the abbreviation; the list shows the full name too,
//! and search matches either. Bibles from YouVersion carry a small tag.
//! The final row opens Bible sources settings, even when no Bible matches.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::select::SelectItem;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::library::SourceEntry;

#[derive(Clone, PartialEq)]
pub enum TranslationChoice {
    Bible(String),
    AddMore,
}

#[derive(Clone)]
pub struct TranslationItem {
    /// What tabs store: the abbreviation, or `yv:<id>` for YouVersion Bibles.
    choice: TranslationChoice,
    label: String,
    name: SharedString,
    remote: bool,
}

impl TranslationItem {
    pub fn from_entry(entry: &SourceEntry) -> Self {
        Self {
            choice: TranslationChoice::Bible(entry.key.clone()),
            label: entry.label.clone(),
            name: entry.name.clone().into(),
            remote: entry.remote,
        }
    }

    pub fn items(entries: &[SourceEntry]) -> Vec<Self> {
        entries
            .iter()
            .map(Self::from_entry)
            .chain(std::iter::once(Self {
                choice: TranslationChoice::AddMore,
                label: "Add more".into(),
                name: "".into(),
                remote: false,
            }))
            .collect()
    }
}

impl SelectItem for TranslationItem {
    type Value = TranslationChoice;

    fn title(&self) -> SharedString {
        if self.choice == TranslationChoice::AddMore {
            return self.label.clone().into();
        }
        format!("{} ({})", self.label, self.name).into()
    }

    fn display_title(&self) -> Option<AnyElement> {
        Some(self.label.clone().into_any_element())
    }

    fn render(&self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let tag_bg = theme.muted;
        if self.choice == TranslationChoice::AddMore {
            return div().text_color(theme.link).child(self.label.clone());
        }
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

    fn value(&self) -> &TranslationChoice {
        &self.choice
    }

    fn matches(&self, query: &str) -> bool {
        let query = query.to_lowercase();
        self.choice == TranslationChoice::AddMore
            || self.label.to_lowercase().contains(&query)
            || self.name.to_lowercase().contains(&query)
            || (self.remote && "youversion".contains(&query))
    }
}
