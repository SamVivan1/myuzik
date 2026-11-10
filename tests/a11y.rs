//! Accessibility assertions for the shell.
//!
//! What can be verified headlessly, and what cannot:
//!
//! `Window::debug_a11y_tree_json()` serialises the accesskit tree that the last
//! frame produced, but gpui only builds that tree when an assistive technology
//! is connected (`Window::is_a11y_active()`), and in the headless test platform
//! `PlatformWindow::a11y_init` is the trait's default no-op, so the activation
//! callback can never fire. `a11y_tree_is_not_observable_in_the_headless_harness`
//! pins that down explicitly, and the remaining tests assert the parts that are
//! observable through the real `Shell`:
//!
//! * the keyboard focus order (Tab / Shift+Tab drive gpui's real tab-stop map,
//!   which only contains elements that are focusable, tracked and tab stops);
//! * that every interactive control is in that order, in the documented groups
//!   (sidebar → content → player bar → window chrome);
//! * that typing in the search field is not hijacked by the global transport
//!   shortcuts and that Tab leaves the field;
//! * that every icon-only control carries an accesskit role and an aria label in
//!   the element chain that renders it (a source-level check, because the tree
//!   itself is not observable here).

use gpui::{AppContext, TestAppContext, VisualTestContext, WindowOptions};
use httpmock::prelude::*;
use myuzik::app::Route;
use myuzik::config::{AppConfig, Paths};
use myuzik::player::engine::PlayerHandle;
use myuzik::player::state::PlaybackStatus;
use myuzik::ui::Shell;

// --- harness ---------------------------------------------------------------

fn dummy_player() -> PlayerHandle {
    let (tx, cmd_rx) = flume::unbounded();
    let (evt_tx, rx) = flume::unbounded();
    std::mem::forget(cmd_rx);
    std::mem::forget(evt_tx);
    PlayerHandle { tx, rx }
}

fn temp_paths(tag: &str) -> Paths {
    let base = std::env::temp_dir().join(format!("myuzik-a11y-{}-{}", std::process::id(), tag));
    Paths {
        config: base.join("config.toml"),
        cache: base.clone(),
        covers: base.join("covers"),
        lyrics: base.join("lyrics"),
        logs: base.join("logs"),
    }
}

fn ok(body: &str) -> String {
    format!(r#"{{"subsonic-response":{{"status":"ok","version":"1.16.1",{body}}}}}"#)
}

/// A shell backed by a mocked, logged-in Navidrome with two albums.
fn mock_shell(
    cx: &mut TestAppContext,
    tag: &str,
) -> (MockServer, gpui::Entity<Shell>, VisualTestContext) {
    let server = MockServer::start();
    let albums = ok(r#""albumList2":{"album":[
            {"id":"a1","name":"Alpha","artist":"Artist A","songCount":2,"duration":300,"year":2026},
            {"id":"a2","name":"Beta","artist":"Artist B","songCount":1,"duration":200,"year":2025}
        ]}"#);
    server.mock(|when, then| {
        when.method(GET).path("/rest/getAlbumList2.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(&albums);
    });
    server.mock(|when, then| {
        when.method(GET).path("/rest/getArtists.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(ok(r#""artists":{"index":[{"artist":[{"id":"ar1","name":"Artist A","albumCount":1}]}]}"#));
    });
    server.mock(|when, then| {
        when.method(GET).path("/rest/getPlaylists.view");
        then.status(200)
            .header("content-type", "application/json")
            .body(ok(r#""playlists":{}"#));
    });

    let url = server.base_url();
    let paths = temp_paths(tag);
    std::fs::create_dir_all(&paths.cache).unwrap();
    std::fs::write(paths.config.parent().unwrap().join("credentials"), "secret").unwrap();
    let config = AppConfig {
        server_url: url,
        username: "tester".to_string(),
        ..Default::default()
    };
    let paths_for_window = paths.clone();
    let window = cx.update(|cx| {
        cx.open_window(WindowOptions::default(), move |_, cx| {
            cx.new(|cx| Shell::new(cx, dummy_player(), config, paths_for_window, None))
        })
        .unwrap()
    });
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    let shell = window.root(&mut vcx).unwrap();
    // Wait until the mocked library has been loaded so content controls exist.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        vcx.run_until_parked();
        if shell.read_with(&vcx, |s, cx| s.loaded_album_count(cx)) >= 2 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    // Paint a few frames so every content control has registered its focus
    // handle (the library arrives asynchronously).
    for _ in 0..3 {
        vcx.update(|window, _| window.refresh());
        vcx.run_until_parked();
    }
    (server, shell, vcx)
}

/// The logical name of the control that currently has keyboard focus.
fn focused_control(cx: &mut VisualTestContext, shell: &gpui::Entity<Shell>) -> Option<String> {
    cx.update(|window, app| shell.read_with(app, |shell, _| shell.focused_control(window)))
}

fn press_tab(cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
}

fn press_shift_tab(cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("shift-tab");
    cx.run_until_parked();
}

/// Walk the focus order from the top, recording each control once.
fn walk_focus_order(
    cx: &mut VisualTestContext,
    shell: &gpui::Entity<Shell>,
    steps: usize,
) -> Vec<String> {
    let mut order = Vec::new();
    for _ in 0..steps {
        press_tab(cx);
        match focused_control(cx, shell) {
            Some(name) => order.push(name),
            None => break,
        }
    }
    order
}

const NAV_ORDER: [&str; 8] = [
    "nav-home",
    "nav-nowplaying",
    "nav-search",
    "nav-albums",
    "nav-artists",
    "nav-playlists",
    "nav-favorites",
    "nav-settings",
];

const PLAYER_ORDER: [&str; 9] = [
    "player-now-playing",
    "player-seek-back",
    "player-prev",
    "player-play",
    "player-next",
    "player-seek-forward",
    "player-volume-down",
    "player-volume-up",
    "player-pin-lyrics",
];

const CHROME_ORDER: [&str; 3] = ["window-close", "window-minimize", "window-maximize"];

// --- tests -----------------------------------------------------------------

/// Pins down why the automated assertions below are not tree-based: the
/// accesskit tree is never built in the headless harness.
///
/// gpui builds the tree only while `is_a11y_active()` (an assistive technology
/// is connected). The test platform window does not implement `a11y_init`, so
/// the activation callback never runs and `debug_a11y_tree_json()` stays `None`.
/// If a future gpui revision starts exposing the tree here, this test fails and
/// the role/label assertions below should be rewritten against the tree.
#[gpui::test]
fn a11y_tree_is_not_observable_in_the_headless_harness(cx: &mut TestAppContext) {
    let (_server, _shell, mut vcx) = mock_shell(cx, "tree");
    let (enabled, active, json) = vcx.update(|window, _| {
        (
            window.is_a11y_enabled(),
            window.is_a11y_active(),
            window.debug_a11y_tree_json(),
        )
    });
    assert!(
        enabled,
        "accessibility support is force-disabled, so roles and labels would never be exposed"
    );
    assert!(
        !active,
        "a11y became active in the headless harness: rewrite the role/label \
         assertions in this file against the accesskit tree"
    );
    assert!(
        json.is_none(),
        "debug_a11y_tree_json() now returns a tree in the headless harness: \
         assert roles and labels against it instead of the source scan below"
    );
}

/// Tab walks the whole shell: sidebar navigation, then page content, then the
/// player bar, then the window chrome, and every stop is reachable exactly once.
#[gpui::test]
fn tab_order_is_nav_then_content_then_player_bar_then_chrome(cx: &mut TestAppContext) {
    let (_server, shell, mut vcx) = mock_shell(cx, "order");
    assert_eq!(shell.read_with(&vcx, |s, _| s.route()), Route::Home);

    let registered: Vec<String> = shell.read_with(&vcx, |s, _| s.focus_controls());
    assert!(
        registered.len() >= 20,
        "only {} controls are registered for keyboard focus: {registered:?}",
        registered.len()
    );

    // Walk once through the whole ring: every registered control must appear.
    let order = walk_focus_order(&mut vcx, &shell, registered.len());
    assert_eq!(
        order.len(),
        registered.len(),
        "focus stopped after {} of {} stops: {order:?}",
        order.len(),
        registered.len()
    );

    let mut seen = order.clone();
    seen.sort();
    let mut expected = registered.clone();
    expected.sort();
    assert_eq!(
        seen, expected,
        "the tab order does not cover exactly the registered controls"
    );

    // Sidebar order is the visual order of the navigation list.
    assert_eq!(&order[..8], &NAV_ORDER, "sidebar tab order: {order:?}");
    // The two content controls on Home, then the album cards.
    assert_eq!(order[9], "content-search-shortcut");
    assert_eq!(order[10], "content-see-all-albums");
    assert_eq!(order[11], "album:a1");
    assert_eq!(order[12], "album:a2");
    // The player bar follows the content.
    assert_eq!(
        &order[13..22],
        &PLAYER_ORDER,
        "player bar tab order: {order:?}"
    );
    // Window chrome comes last.
    assert_eq!(&order[22..25], &CHROME_ORDER, "chrome tab order: {order:?}");

    // The sidebar "pin lyrics" button belongs to the navigation group.
    assert_eq!(order[8], "sidebar-pin-lyrics");
}

/// Shift+Tab visits the same ring backwards, so focus order is reversible.
#[gpui::test]
fn shift_tab_reverses_the_focus_order(cx: &mut TestAppContext) {
    let (_server, shell, mut vcx) = mock_shell(cx, "reverse");

    // Three Tabs from the root: nav-home, nav-nowplaying, nav-search.
    let forward = walk_focus_order(&mut vcx, &shell, 3);
    assert_eq!(forward, NAV_ORDER[..3].to_vec());

    // Two Shift+Tabs must retrace: nav-nowplaying, nav-home.
    let mut back = Vec::new();
    for _ in 0..2 {
        press_shift_tab(&mut vcx);
        back.push(focused_control(&mut vcx, &shell).expect("focus lost"));
    }
    assert_eq!(
        back,
        vec!["nav-nowplaying".to_string(), "nav-home".to_string()]
    );
}

/// Typing in the search field must not be hijacked by the global transport
/// shortcuts, and Tab must move focus out of the field.
#[gpui::test]
fn typing_in_search_is_not_hijacked_and_tab_leaves_the_field(cx: &mut TestAppContext) {
    let (_server, shell, mut vcx) = mock_shell(cx, "search");

    // The nav item focuses the search field, exactly as a user would.
    let nav_search = vcx.debug_bounds("nav-search").expect("nav-search bounds");
    vcx.simulate_click(nav_search.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert_eq!(shell.read_with(&vcx, |s, _| s.route()), Route::Search);

    let status_before = shell.read_with(&vcx, |s, cx| s.playback_status(cx));

    // "hello world" contains a space: the space must be typed, not turned into
    // the global play/pause shortcut.
    vcx.simulate_keystrokes("hello space world");
    vcx.run_until_parked();
    assert_eq!(
        shell.read_with(&vcx, |s, _| s.search_query().to_string()),
        "hello world",
        "typing into the search field was hijacked"
    );
    let status_after = shell.read_with(&vcx, |s, cx| s.playback_status(cx));
    assert_eq!(
        status_before, status_after,
        "a space typed in the search field toggled playback"
    );
    assert_ne!(status_after, PlaybackStatus::Playing);

    // Tab leaves the field for a control that is in the shell's focus order.
    press_tab(&mut vcx);
    let after_tab = focused_control(&mut vcx, &shell);
    assert!(
        after_tab.is_some(),
        "Tab did not move focus out of the search field"
    );
    assert!(
        registered_controls(&shell, &vcx).contains(&after_tab.clone().unwrap()),
        "Tab focused an unregistered control: {after_tab:?}"
    );
}

fn registered_controls(shell: &gpui::Entity<Shell>, cx: &VisualTestContext) -> Vec<String> {
    shell.read_with(cx, |s, _| s.focus_controls())
}

/// Every route renders the persistent chrome (sidebar + player bar) as tab
/// stops, so keyboard navigation survives route changes.
#[gpui::test]
fn persistent_chrome_stays_tabbable_on_every_route(cx: &mut TestAppContext) {
    let (_server, shell, mut vcx) = mock_shell(cx, "routes");
    for route in [
        Route::Home,
        Route::NowPlaying,
        Route::Search,
        Route::Settings,
        Route::Albums,
        Route::Artists,
        Route::Playlists,
        Route::Favorites,
    ] {
        shell.update(&mut vcx, |s, _| s.go(route.clone()));
        vcx.run_until_parked();
        let controls = registered_controls(&shell, &vcx);
        for expected in NAV_ORDER.iter().chain(PLAYER_ORDER.iter()) {
            assert!(
                controls.iter().any(|c| c == expected),
                "route {route:?} lost the tab stop {expected:?}: {controls:?}"
            );
        }
        assert_eq!(
            shell.read_with(&vcx, |s, _| s.route()),
            route,
            "route did not change"
        );
    }
}

// --- source-level role/label coverage -------------------------------------
//
// The accesskit tree is not observable headlessly (see the first test), so the
// roles and labels each control renders are checked against the rendering code.
// A control that loses its `.role(...)` or `.aria_label(...)` fails here.

fn chain_after(source: &str, anchor: &str) -> String {
    let start = source
        .find(anchor)
        .unwrap_or_else(|| panic!("rendering code for {anchor:?} not found"));
    // The element chain for one control is well under this many bytes.
    source[start..(start + 1400).min(source.len())].to_string()
}

#[test]
fn icon_only_controls_expose_a_role_and_a_label() {
    let shell = include_str!("../src/ui/shell.rs");
    let now_playing = include_str!("../src/ui/now_playing.rs");
    let overlay = include_str!("../src/overlay/mod.rs");

    // (source, anchor, expected label fragment or None for a dynamic label)
    let cases: [(&str, &str, &str, Option<&str>); 14] = [
        (shell, ".id(\"play-btn\")", "shell", Some("if is_playing")),
        (
            shell,
            ".id(\"prev-btn\")",
            "shell",
            Some("\"Previous track\""),
        ),
        (shell, ".id(\"next-btn\")", "shell", Some("\"Next track\"")),
        (
            shell,
            ".id(\"seek-back\")",
            "shell",
            Some("\"Seek back 10 seconds\""),
        ),
        (
            shell,
            ".id(\"seek-fwd\")",
            "shell",
            Some("\"Seek forward 10 seconds\""),
        ),
        (shell, ".id(\"vol-down\")", "shell", Some("\"Volume down\"")),
        (shell, ".id(\"vol-up\")", "shell", Some("\"Volume up\"")),
        (
            shell,
            ".id(\"win-close\")",
            "shell",
            Some("\"Close window\""),
        ),
        (
            shell,
            ".id(\"home-search\")",
            "shell",
            Some("\"Search your library (Ctrl K)\""),
        ),
        (
            now_playing,
            ".id(\"np-prev\")",
            "now_playing",
            Some("\"Previous track\""),
        ),
        (
            now_playing,
            ".id(\"np-offset-down\")",
            "now_playing",
            Some("\"Lyrics earlier by 500 ms\""),
        ),
        (
            overlay,
            ".id(\"overlay-close\")",
            "overlay",
            Some("\"Close lyrics overlay\""),
        ),
        // A grid of clickable album cards.
        (
            shell,
            ".id(format!(\"album-{}\", aid))",
            "shell",
            Some("format!(\"Open album"),
        ),
        // A list of clickable track rows.
        (
            shell,
            ".id(format!(\"track-{}\", i))",
            "shell",
            Some("format!(\"Play"),
        ),
    ];

    for (source, anchor, file, label) in cases {
        let chain = chain_after(source, anchor);
        assert!(
            chain.contains(".role(gpui::Role::"),
            "{anchor} in {file} exposes no accesskit role"
        );
        assert!(
            chain.contains(".track_focus(") || chain.contains(".tab_index("),
            "{anchor} in {file} is not keyboard focusable"
        );
        // A visible-text-only control does not need a label; an icon-only one
        // does. Every control listed here is icon-only or a bare glyph.
        assert!(
            chain.contains(".aria_label("),
            "{anchor} in {file} has no accessible label"
        );
        if let Some(label) = label {
            assert!(
                chain.contains(&format!(".aria_label({label}")),
                "{anchor} in {file} has no accessible label {label:?}"
            );
        }
    }
}

#[test]
fn keyboard_focus_wiring_is_present_for_every_group() {
    let shell = include_str!("../src/ui/shell.rs");

    // Four tab groups give the documented order: nav → content → player → chrome.
    for group in ["TAB_NAV", "TAB_CONTENT", "TAB_PLAYER", "TAB_CHROME"] {
        let uses = shell.matches(&format!(".tab_index(Self::{group})")).count();
        assert!(uses > 0, "no control is placed in the {group} tab group");
    }
    // The focus ring is the 2px OS accent, applied over a reserved border so
    // nothing shifts when focus arrives.
    assert!(
        shell.matches(".border_2()").count() >= 20,
        "focus rings are missing from most controls"
    );
    assert!(
        shell.contains("t.os_accent"),
        "the focus ring does not use the OS accent token"
    );
    assert!(
        shell.contains("window.focus_next(cx)") && shell.contains("window.focus_prev(cx)"),
        "Tab / Shift+Tab are not wired to gpui's tab stops"
    );
    // Lists and grids carry container roles so a screen reader can announce them.
    for role in [
        "gpui::Role::Grid",
        "gpui::Role::List",
        "gpui::Role::ListItem",
        "gpui::Role::Link",
        "gpui::Role::Button",
        "gpui::Role::TextInput",
    ] {
        assert!(
            shell.contains(role) || include_str!("../src/ui/now_playing.rs").contains(role),
            "no element uses {role}"
        );
    }
}
