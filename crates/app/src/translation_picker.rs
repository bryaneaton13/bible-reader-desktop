//! Rows for the toolbar's searchable translation dropdown (gpui-kit `Select`).
//! The trigger shows just the abbreviation; the list shows the full name too,
//! and search matches either.

use bible_store::Translation;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::select::SelectItem;
use gpui_kit::*;

#[derive(Clone)]
pub struct TranslationItem {
    abbrev: String,
    name: SharedString,
}

impl TranslationItem {
    pub fn from_translation(t: &Translation) -> Self {
        Self {
            abbrev: t.info.abbrev.clone(),
            name: t.info.name.clone().into(),
        }
    }
}

impl SelectItem for TranslationItem {
    type Value = String;

    fn title(&self) -> SharedString {
        format!("{} ({})", self.abbrev, self.name).into()
    }

    fn display_title(&self) -> Option<AnyElement> {
        Some(self.abbrev.clone().into_any_element())
    }

    fn render(&self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .w(px(40.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.abbrev.clone()),
            )
            .child(div().text_color(muted).child(self.name.clone()))
    }

    fn value(&self) -> &String {
        &self.abbrev
    }

    fn matches(&self, query: &str) -> bool {
        let query = query.to_lowercase();
        self.abbrev.to_lowercase().contains(&query) || self.name.to_lowercase().contains(&query)
    }
}
