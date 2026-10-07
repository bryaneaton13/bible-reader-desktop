mod appearance;
mod chapter_view;
mod fonts;
mod library;
mod palette;
mod reader;
mod selectable;
mod settings_window;
mod tabs;
mod translation_picker;
mod tray;

use bible_core::VerseId;
use gpui_kit::base::Root;
use gpui_kit::component::TitleBar;
use gpui_kit::*;

use crate::library::{Appearance, FontPairing, Library, Settings, data_dir, verse_of_the_day};
use crate::reader::ReaderView;

actions!(
    bible_reader,
    [
        Quit,
        CloseWindow,
        OpenReader,
        OpenVerseOfTheDay,
        CopyVerseOfTheDay,
        CopyVerse,
        CopyChapter,
        NextChapter,
        PrevChapter,
        GoBack,
        GoForward,
        OpenPalette,
        OpenCommands,
        ClosePalette,
        NewTab,
        CloseTab,
        ReopenClosedTab,
        NextTab,
        PrevTab,
        IncreaseFont,
        DecreaseFont,
        ResetFont,
        ToggleSidebar,
        ToggleInspector,
        ToggleRedLetter,
        ToggleVerseNumbers,
        ToggleDark,
        OpenSettings,
        FocusSidebar,
        FocusReader,
        OpenInNewTab,
        OpenInSplit,
        ToggleSplit,
        ClosePane,
        FocusLeftPane,
        FocusRightPane,
        SidebarLeft,
        SidebarRight,
        SidebarUp,
        SidebarDown,
        SidebarPrevBook,
        SidebarNextBook,
        SidebarOpen,
    ]
);

/// Switch to the translation at this position in the toolbar (⌘1, ⌘2, …).
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = bible_reader, no_json)]
pub struct SelectTranslation {
    pub index: usize,
}

/// Pick the reading theme.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = bible_reader, no_json)]
pub struct SetAppearance {
    pub appearance: Appearance,
}

/// Pick the title / body typeface pairing.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = bible_reader, no_json)]
pub struct SetFontPairing {
    pub pairing: FontPairing,
}

/// The main window, if open. Closing it leaves the app running in the menu bar.
struct MainWindow(Option<WindowHandle<Root>>, Option<Entity<ReaderView>>);

impl Global for MainWindow {}

fn main() {
    let library = match Library::load(&data_dir()) {
        Ok(library) => library,
        Err(err) => {
            eprintln!("error: {err:#}");
            std::process::exit(1);
        }
    };

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .with_quit_mode(QuitMode::Explicit)
        .run(move |cx| {
            gpui_kit::init(cx);
            fonts::register(cx);
            cx.set_global(library);
            cx.set_global(Settings::load());
            cx.set_global(MainWindow(None, None));
            cx.set_global(settings_window::SettingsWindow(None));

            bind_keys(cx);
            cx.on_action(|_: &Quit, cx| cx.quit());
            cx.on_action(|_: &OpenReader, cx| open_reader(None, cx));
            cx.on_action(|_: &OpenSettings, cx| settings_window::open(cx));
            cx.on_action(|_: &OpenVerseOfTheDay, cx| open_reader(Some(verse_of_the_day()), cx));
            cx.on_action(tray::copy_verse_of_the_day);
            cx.on_action(|action: &SetAppearance, cx| set_appearance(action.appearance, cx));
            cx.on_action(|action: &SetFontPairing, cx| {
                cx.update_global::<Settings, _>(|s, _| {
                    s.font_pairing = action.pairing;
                    s.save();
                });
            });
            cx.on_action(|_: &ToggleDark, cx| {
                let dark = gpui_kit::component::Theme::global(cx).mode.is_dark();
                set_appearance(
                    if dark {
                        Appearance::Light
                    } else {
                        Appearance::Dark
                    },
                    cx,
                );
            });
            cx.set_menus([
                Menu::new("Bible Reader").items([
                    MenuItem::action("Settings…", OpenSettings),
                    MenuItem::separator(),
                    MenuItem::action("Copy Verse of the Day", CopyVerseOfTheDay),
                    MenuItem::separator(),
                    MenuItem::action("Quit Bible Reader", Quit),
                ]),
                Menu::new("File").items([
                    MenuItem::action("New Tab", NewTab),
                    MenuItem::action("Reopen Closed Tab", ReopenClosedTab),
                    MenuItem::separator(),
                    MenuItem::action("Close Tab", CloseTab),
                    MenuItem::action("Close Window", CloseWindow),
                ]),
                Menu::new("Edit").items([
                    MenuItem::action("Copy", gpui_kit::base::input::Copy),
                    MenuItem::action("Copy Verse with Citation", CopyVerse),
                    MenuItem::action("Copy Chapter", CopyChapter),
                ]),
                Menu::new("Go").items([
                    MenuItem::action("Go to Passage or Search…", OpenPalette),
                    MenuItem::action("Commands…", OpenCommands),
                    MenuItem::separator(),
                    MenuItem::action("Next Chapter", NextChapter),
                    MenuItem::action("Previous Chapter", PrevChapter),
                    MenuItem::separator(),
                    MenuItem::action("Back", GoBack),
                    MenuItem::action("Forward", GoForward),
                ]),
                Menu::new("View").items([
                    MenuItem::action("Toggle Sidebar", ToggleSidebar),
                    MenuItem::action("Focus Sidebar", FocusSidebar),
                    MenuItem::separator(),
                    MenuItem::action("Split Right / Unsplit", ToggleSplit),
                    MenuItem::action("Close Pane", ClosePane),
                    MenuItem::action("Focus Left Pane", FocusLeftPane),
                    MenuItem::action("Focus Right Pane", FocusRightPane),
                    MenuItem::separator(),
                    MenuItem::action("Toggle Notes", ToggleInspector),
                    MenuItem::action("Toggle Red Letter", ToggleRedLetter),
                    MenuItem::action("Toggle Verse Numbers", ToggleVerseNumbers),
                    MenuItem::action("Toggle Dark Mode", ToggleDark),
                    MenuItem::submenu(
                        Menu::new("Theme").items(
                            Appearance::ALL.map(|a| {
                                MenuItem::action(a.label(), SetAppearance { appearance: a })
                            }),
                        ),
                    ),
                    MenuItem::separator(),
                    MenuItem::action("Larger Text", IncreaseFont),
                    MenuItem::action("Smaller Text", DecreaseFont),
                    MenuItem::action("Actual Size", ResetFont),
                ]),
                Menu::new("Window").items([
                    MenuItem::action("Show Next Tab", NextTab),
                    MenuItem::action("Show Previous Tab", PrevTab),
                ]),
            ]);

            if let Err(err) = tray::install(cx) {
                eprintln!("warning: menu bar item unavailable: {err}");
            }
            open_reader(None, cx);
            #[cfg(feature = "screenshot")]
            screenshot::run_if_requested(cx);
        });
}

/// Dev-only: drive the UI from env vars and save an offscreen render.
///
/// BIBLE_READER_SCREENSHOT=out.png   where to write the PNG (required)
/// BIBLE_READER_GOTO="ps 23"         passage to open first
/// BIBLE_READER_ACTIONS="palette:love one another,notes,dark"
/// BIBLE_READER_SCREENSHOT_WINDOW=settings   capture the Settings window instead
#[cfg(feature = "screenshot")]
mod screenshot {
    use std::time::Duration;

    use super::*;

    pub fn run_if_requested(cx: &mut App) {
        let Ok(out) = std::env::var("BIBLE_READER_SCREENSHOT") else {
            return;
        };
        let Some((window, view)) = cx
            .global::<MainWindow>()
            .0
            .zip(cx.global::<MainWindow>().1.clone())
        else {
            return;
        };
        if let Some(r) = std::env::var("BIBLE_READER_GOTO")
            .ok()
            .and_then(|s| bible_core::parse_reference(&s))
        {
            view.update(cx, |reader, cx| {
                reader.go_to(r.chapter_ref(), r.first_verse(), cx)
            });
        }
        let actions = std::env::var("BIBLE_READER_ACTIONS").unwrap_or_default();
        // Apply before the first frame: a window that isn't key may not
        // repaint again before the capture.
        let _ = window.update(cx, |_, window, cx| {
            for action in actions.split(',').filter(|a| !a.is_empty()) {
                view.update(cx, |reader, cx| reader.debug_action(action, window, cx));
            }
        });
        // BIBLE_READER_SCREENSHOT_WINDOW=settings captures the Settings window.
        let target = if std::env::var("BIBLE_READER_SCREENSHOT_WINDOW").as_deref() == Ok("settings")
        {
            settings_window::open(cx);
            cx.global::<settings_window::SettingsWindow>()
                .0
                .unwrap_or(window)
        } else {
            window
        };
        cx.spawn(async move |cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1500))
                .await;
            let result = target.update(cx, |_, window, _| window.render_to_image());
            match result {
                Ok(Ok(image)) => match image.save(&out) {
                    Ok(()) => eprintln!("saved {out}"),
                    Err(err) => eprintln!("error saving {out}: {err}"),
                },
                Ok(Err(err)) | Err(err) => eprintln!("error rendering: {err:#}"),
            }
            cx.update(|cx| cx.quit());
        })
        .detach();
    }
}

pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("cmd-shift-w", CloseWindow, None),
        KeyBinding::new("cmd-t", NewTab, Some("Reader")),
        KeyBinding::new("cmd-w", CloseTab, Some("Reader")),
        KeyBinding::new("cmd-shift-t", ReopenClosedTab, Some("Reader")),
        KeyBinding::new("ctrl-tab", NextTab, Some("Reader")),
        KeyBinding::new("ctrl-shift-tab", PrevTab, Some("Reader")),
        KeyBinding::new("cmd-}", NextTab, Some("Reader")),
        KeyBinding::new("cmd-{", PrevTab, Some("Reader")),
        KeyBinding::new("cmd-p", OpenPalette, Some("Reader")),
        KeyBinding::new("cmd-shift-p", OpenCommands, Some("Reader")),
        KeyBinding::new("cmd-k", OpenPalette, Some("Reader")),
        KeyBinding::new("cmd-l", OpenPalette, Some("Reader")),
        KeyBinding::new("cmd-f", OpenPalette, Some("Reader")),
        KeyBinding::new("escape", ClosePalette, Some("Palette")),
        KeyBinding::new("cmd-enter", OpenInNewTab, Some("Palette")),
        KeyBinding::new("alt-enter", OpenInSplit, Some("Palette")),
        KeyBinding::new("cmd-\\", ToggleSplit, Some("Reader")),
        KeyBinding::new("cmd-shift-\\", ClosePane, Some("Reader")),
        KeyBinding::new("cmd-alt-left", FocusLeftPane, Some("Reader")),
        KeyBinding::new("cmd-alt-right", FocusRightPane, Some("Reader")),
        KeyBinding::new("cmd-]", NextChapter, Some("Reader")),
        KeyBinding::new("cmd-[", PrevChapter, Some("Reader")),
        KeyBinding::new("alt-right", NextChapter, Some("Reader")),
        KeyBinding::new("alt-left", PrevChapter, Some("Reader")),
        KeyBinding::new("right", NextChapter, Some("Reader")),
        KeyBinding::new("left", PrevChapter, Some("Reader")),
        KeyBinding::new("cmd-b", ToggleSidebar, Some("Reader")),
        KeyBinding::new("cmd-alt-b", FocusSidebar, Some("Reader")),
        // Sidebar chapter picker: arrows move a cursor, Enter opens.
        KeyBinding::new("left", SidebarLeft, Some("Sidebar")),
        KeyBinding::new("right", SidebarRight, Some("Sidebar")),
        KeyBinding::new("up", SidebarUp, Some("Sidebar")),
        KeyBinding::new("down", SidebarDown, Some("Sidebar")),
        KeyBinding::new("alt-up", SidebarPrevBook, Some("Sidebar")),
        KeyBinding::new("alt-down", SidebarNextBook, Some("Sidebar")),
        KeyBinding::new("enter", SidebarOpen, Some("Sidebar")),
        KeyBinding::new("escape", FocusReader, Some("Sidebar")),
        KeyBinding::new("ctrl-cmd-left", GoBack, Some("Reader")),
        KeyBinding::new("ctrl-cmd-right", GoForward, Some("Reader")),
        KeyBinding::new("cmd-=", IncreaseFont, Some("Reader")),
        KeyBinding::new("cmd-+", IncreaseFont, Some("Reader")),
        KeyBinding::new("cmd--", DecreaseFont, Some("Reader")),
        KeyBinding::new("cmd-0", ResetFont, Some("Reader")),
        KeyBinding::new("cmd-ctrl-s", ToggleSidebar, Some("Reader")),
        KeyBinding::new("cmd-alt-n", ToggleInspector, Some("Reader")),
        KeyBinding::new("cmd-shift-l", ToggleDark, Some("Reader")),
        KeyBinding::new("cmd-shift-c", CopyVerse, Some("Reader")),
        KeyBinding::new("cmd-alt-c", CopyChapter, Some("Reader")),
        KeyBinding::new("cmd-1", SelectTranslation { index: 0 }, Some("Reader")),
        KeyBinding::new("cmd-2", SelectTranslation { index: 1 }, Some("Reader")),
        KeyBinding::new("cmd-3", SelectTranslation { index: 2 }, Some("Reader")),
        KeyBinding::new("cmd-4", SelectTranslation { index: 3 }, Some("Reader")),
    ]);
    cx.on_action(|_: &CloseWindow, cx| {
        if let Some(window) = cx.global::<MainWindow>().0 {
            let _ = window.update(cx, |_, window, _| window.remove_window());
        }
    });
}

/// Focus the existing window or open a new one, optionally jumping to a verse.
fn open_reader(verse: Option<VerseId>, cx: &mut App) {
    cx.activate(true);
    let existing = cx.global::<MainWindow>();
    if let (Some(window), Some(view)) = (existing.0, existing.1.clone())
        && window
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        if let Some(v) = verse {
            view.update(cx, |reader, cx| reader.go_to(v.chapter_ref(), Some(v), cx));
        }
        return;
    }

    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(1100.), px(820.)), cx)),
        titlebar: Some(TitlebarOptions {
            title: Some("Bible Reader".into()),
            // Vertically centered in the 44px toolbar, which doubles as the title bar.
            traffic_light_position: Some(point(px(16.), px(15.))),
            ..TitleBar::title_bar_options()
        }),
        window_min_size: Some(size(px(620.), px(400.))),
        ..TitleBar::window_options()
    };
    let opened = gpui_kit::open_window(options, cx, |window, cx| {
        appearance::reapply(window, cx);
        cx.new(|cx| ReaderView::new(window, cx))
    });
    match opened {
        Ok((handle, view)) => {
            if let Some(v) = verse {
                view.update(cx, |reader, cx| reader.go_to(v.chapter_ref(), Some(v), cx));
            }
            cx.set_global(MainWindow(handle.downcast::<Root>(), Some(view)));
        }
        Err(err) => eprintln!("error: failed to open window: {err:#}"),
    }
}

fn set_appearance(appearance: Appearance, cx: &mut App) {
    cx.update_global::<Settings, _>(|s, _| {
        s.appearance = appearance;
        s.save();
    });
    appearance::apply(appearance, None, cx);
}
