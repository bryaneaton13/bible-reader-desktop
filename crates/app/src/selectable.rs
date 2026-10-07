//! Drag-select and ⌘C for chapter text.
//!
//! gpui-base's `SelectableText` takes plain text, but chapter blocks are
//! `InteractiveText` over styled runs. This wraps any such element and
//! registers its text layout with the window-level text selection that
//! `Root` already owns (its `TextSelectionLayer` and ⌘C binding).

use std::ops::Range;

use gpui_kit::base::{
    TextSelectionHandle, TextSelectionRegistration, TextSelectionRun, Theme as BaseTheme,
};
use gpui_kit::{
    AnyElement, App, BorderStyle, Bounds, Corners, Edges, Element, ElementId, GlobalElementId,
    Hitbox, HitboxBehavior, InspectorElementId, IntoElement, LayoutId, PaintQuad, Pixels, Point,
    SharedString, TextLayout, Window, transparent_black,
};

pub struct SelectableBlock {
    id: ElementId,
    text: SharedString,
    layout: TextLayout,
    order: u64,
    inner: AnyElement,
}

impl SelectableBlock {
    /// `layout` must come from the `StyledText` inside `inner`
    /// (`StyledText::layout()` shares state with the element it was cloned from).
    pub fn new(
        id: impl Into<ElementId>,
        order: u64,
        text: SharedString,
        layout: TextLayout,
        inner: impl IntoElement,
    ) -> Self {
        Self {
            id: id.into(),
            text,
            layout,
            order,
            inner: inner.into_any_element(),
        }
    }
}

impl IntoElement for SelectableBlock {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for SelectableBlock {
    type RequestLayoutState = TextSelectionHandle;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let handle = window.with_element_state(
            global_id.expect("SelectableBlock needs a stable element id"),
            |retained: Option<TextSelectionHandle>, _| {
                let handle =
                    retained.unwrap_or_else(|| TextSelectionHandle::new(self.text.clone(), cx));
                (handle.clone(), handle)
            },
        );
        handle.set_fallback_copy_text(self.text.to_string(), cx);
        (self.inner.request_layout(window, cx), handle)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        handle: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        // Inserted before the inner element's hitboxes so footnote clicks and
        // tooltips (later, so on top) keep working.
        let hitbox: Hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        self.inner.prepaint(window, cx);
        let registration = TextSelectionRegistration::new(hitbox, bounds)
            .with_document_order(self.order)
            .with_text_bounds(vec![bounds])
            .with_rendered_element(handle, window, cx);
        handle.register(registration, window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        handle: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let before = gpui_kit::base::TextSelection::selected_text(window, cx);
        let projection = handle.update_runs(
            &[
                TextSelectionRun::new(self.text.clone(), self.layout.clone(), bounds)
                    .with_document_order(self.order),
            ],
            cx,
        );
        if before != gpui_kit::base::TextSelection::selected_text(window, cx) {
            window.refresh();
        }
        let color = BaseTheme::global(cx).tokens.colors.selection;
        for range in projection.ranges().iter().flatten().cloned() {
            paint_selection(&self.layout, range, color, window);
        }
        self.inner.paint(window, cx);
    }
}

fn paint_selection(
    layout: &TextLayout,
    range: Range<usize>,
    color: gpui_kit::Hsla,
    window: &mut Window,
) {
    let (Some(start), Some(end)) = (
        layout.position_for_index(range.start),
        layout.position_for_index(range.end),
    ) else {
        return;
    };
    for bounds in quads(start, end, layout.bounds(), layout.line_height()) {
        window.paint_quad(PaintQuad {
            bounds,
            background: color.into(),
            corner_radii: Corners::default(),
            border_widths: Edges::default(),
            border_color: transparent_black(),
            border_style: BorderStyle::default(),
        });
    }
}

fn quads(
    start: Point<Pixels>,
    end: Point<Pixels>,
    bounds: Bounds<Pixels>,
    line_height: Pixels,
) -> Vec<Bounds<Pixels>> {
    if start.y == end.y {
        return vec![Bounds::from_corners(
            start,
            Point::new(end.x, end.y + line_height),
        )];
    }
    let mut out = vec![Bounds::from_corners(
        start,
        Point::new(bounds.right(), start.y + line_height),
    )];
    if end.y > start.y + line_height {
        out.push(Bounds::from_corners(
            Point::new(bounds.left(), start.y + line_height),
            Point::new(bounds.right(), end.y),
        ));
    }
    out.push(Bounds::from_corners(
        Point::new(bounds.left(), end.y),
        Point::new(end.x, end.y + line_height),
    ));
    out
}
