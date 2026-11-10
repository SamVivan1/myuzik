//! Tray icon support: a `org.kde.StatusNotifierItem` exported on the session
//! bus, together with a `com.canonical.dbusmenu` menu.
//!
//! A shell finds tray icons through `org.kde.StatusNotifierWatcher`: the item
//! owns a bus name such as `org.kde.StatusNotifierItem-<pid>-1`, exports the item
//! interface at `/StatusNotifierItem` and the menu at `/MenuBar`, and then calls
//! `RegisterStatusNotifierItem` on the watcher. GNOME's AppIndicator extension
//! draws the icon from `IconPixmap` and reads the menu through DBusMenu.
//!
//! [`Tray::start`] runs a worker thread with its own D-Bus connection, so the UI
//! thread never talks to the bus. Anything that fails there is either returned by
//! `start` as [`TrayError`] or logged with `tracing`; a missing session bus only
//! means the app runs without a tray.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use flume::{Receiver, Sender};
use serde::Serialize;
use zbus::blocking::Connection;
use zbus::zvariant::{DynamicType, ObjectPath, OwnedValue, Value};

/// The object path a status notifier watcher looks the item up at.
const ITEM_PATH: &str = "/StatusNotifierItem";
/// The object path the DBusMenu is exported at.
const MENU_PATH: &str = "/MenuBar";
const SNI_INTERFACE: &str = "org.kde.StatusNotifierItem";
const MENU_INTERFACE: &str = "com.canonical.dbusmenu";
const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";
const WATCHER_SERVICE: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const WATCHER_INTERFACE: &str = "org.kde.StatusNotifierWatcher";
/// The item id and title prefix show up in the shell, so they carry the app name.
const APP_ID: &str = "Myuzik";
/// Root id of a DBusMenu tree.
const ROOT_ID: i32 = 0;
/// DBusMenu `Version` property.
const DBUSMENU_VERSION: u32 = 3;
/// How long `start` waits for the worker to come up before giving up.
const START_TIMEOUT: Duration = Duration::from_secs(10);
/// `(width, height, ARGB32 pixels)` as StatusNotifierItem wants it.
type IconPixmaps = Vec<(i32, i32, Vec<u8>)>;
/// `(icon name, icon pixmaps, title, subtitle)`.
type ToolTip = (String, IconPixmaps, String, String);
/// DBusMenu item properties.
type MenuProperties = HashMap<String, Value<'static>>;
/// One DBusMenu item: `(id, properties, children)`.
type MenuItem = (i32, MenuProperties, Vec<Value<'static>>);

/// What the user asked for from the tray.
///
/// The player owns the meaning of each variant; the tray only turns tray
/// interactions into them and never acts on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    /// Show and focus the app (left click on the icon, `Activate`).
    Show,
    /// Hide the app window.
    Hide,
    /// Toggle playback.
    PlayPause,
    /// Skip to the next track.
    Next,
    /// Go back to the previous track.
    Previous,
    /// Toggle the pinned lyrics overlay.
    PinLyrics,
    /// Quit the app.
    Quit,
}

/// Why the tray could not be started.
///
/// Every failure is reported through this type instead of a panic, so the
/// caller can log it and keep running without a tray.
#[derive(Debug)]
pub struct TrayError {
    message: String,
}

impl TrayError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    fn dbus(context: &str, error: &zbus::Error) -> Self {
        Self::new(format!("{context}: {error}"))
    }
}

impl fmt::Display for TrayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for TrayError {}

/// A tray icon that is live on the session bus.
///
/// Dropping the handle shuts the worker down and releases the bus name, so the
/// icon disappears from the shell; call [`Tray::shutdown`] to wait for that to
/// happen instead of leaving it to a background thread.
pub struct Tray {
    control: Sender<ControlMessage>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl Tray {
    /// Register on the session bus.
    ///
    /// `icon_rgba` is `size` by `size` RGBA8 pixels; it is converted to the
    /// big-endian ARGB32 pixmaps StatusNotifierItem expects. The returned
    /// receiver yields one [`TrayCommand`] per user action.
    ///
    /// A failure (no session bus, a name that is already taken, ...) is returned
    /// as [`TrayError`] and leaves nothing registered.
    pub fn start(
        icon_rgba: Vec<u8>,
        size: u32,
    ) -> Result<(Tray, Receiver<TrayCommand>), TrayError> {
        let (command_tx, command_rx) = flume::unbounded::<TrayCommand>();
        let (control_tx, control_rx) = flume::unbounded::<ControlMessage>();
        let (ready_tx, ready_rx) = flume::bounded::<Result<(), TrayError>>(1);

        if size == 0 {
            tracing::warn!("tray: icon size is zero, the shell will have no icon to draw");
        }
        let icon = Arc::new(icon_pixmaps(&icon_rgba, size));
        let state = Arc::new(Mutex::new(TrayState::default()));

        let worker_state = Arc::clone(&state);
        let worker_commands = command_tx.clone();
        let worker = std::thread::Builder::new()
            .name("myuzik-tray".to_owned())
            .spawn(move || {
                run_worker(worker_state, icon, worker_commands, control_rx, ready_tx);
            })
            .map_err(|error| TrayError::new(format!("cannot spawn the tray thread: {error}")))?;

        match ready_rx.recv_timeout(START_TIMEOUT) {
            Ok(Ok(())) => Ok((
                Tray {
                    control: control_tx,
                    worker: Mutex::new(Some(worker)),
                },
                command_rx,
            )),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(error)
            }
            Err(error) => {
                tracing::warn!("tray: worker did not start in time: {error}");
                let _ = control_tx.send(ControlMessage::Shutdown);
                Err(TrayError::new(
                    "the tray worker did not finish registering on the session bus",
                ))
            }
        }
    }

    /// Set the track the tooltip and the `Title` property describe.
    pub fn set_track(&self, title: &str, artist: &str) {
        self.send(ControlMessage::Track {
            title: title.to_owned(),
            artist: artist.to_owned(),
        });
    }

    /// Tell the tray whether playback is running, so the menu says Pause or Play.
    pub fn set_playing(&self, playing: bool) {
        self.send(ControlMessage::Playing(playing));
    }

    /// Tell the tray whether the lyrics overlay is pinned, so the menu label fits.
    pub fn set_pinned(&self, pinned: bool) {
        self.send(ControlMessage::Pinned(pinned));
    }

    /// Release the bus name and wait for the worker thread to finish.
    ///
    /// Safe to call more than once and from any thread; the second call is a
    /// no-op. Dropping the handle does the same without waiting.
    pub fn shutdown(&self) {
        self.send(ControlMessage::Shutdown);
        let worker = self
            .worker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if let Some(worker) = worker
            && worker.join().is_err()
        {
            tracing::warn!("tray: worker thread panicked");
        }
    }

    fn send(&self, message: ControlMessage) {
        if self.control.send(message).is_err() {
            tracing::debug!("tray: worker is not running, update dropped");
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// What the UI asks the worker to do.
#[derive(Debug)]
enum ControlMessage {
    Track { title: String, artist: String },
    Playing(bool),
    Pinned(bool),
    Shutdown,
}

/// The tray state the D-Bus properties are read from.
#[derive(Debug, Clone, Default)]
struct TrayState {
    title: String,
    artist: String,
    playing: bool,
    pinned: bool,
    /// Bumped whenever the menu layout changes, for DBusMenu `LayoutUpdated`.
    revision: u32,
}

impl TrayState {
    fn display_title(&self) -> String {
        match (self.title.is_empty(), self.artist.is_empty()) {
            (false, false) => format!("{} \u{2014} {}", self.title, self.artist),
            (false, true) => self.title.clone(),
            (true, false) => self.artist.clone(),
            (true, true) => APP_ID.to_owned(),
        }
    }

    fn bump_revision(&mut self) -> u32 {
        self.revision = self.revision.wrapping_add(1);
        self.revision
    }
}

/// Stable DBusMenu item ids and everything derived from them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuEntry {
    Show,
    Hide,
    PlayPause,
    Previous,
    Next,
    PinLyrics,
    Separator,
    Quit,
}

impl MenuEntry {
    /// Menu order, top to bottom.
    const ALL: [MenuEntry; 8] = [
        MenuEntry::Show,
        MenuEntry::Hide,
        MenuEntry::PlayPause,
        MenuEntry::Previous,
        MenuEntry::Next,
        MenuEntry::PinLyrics,
        MenuEntry::Separator,
        MenuEntry::Quit,
    ];

    /// The id sent to the shell; these must never change between releases.
    const fn id(self) -> i32 {
        match self {
            MenuEntry::Show => 1,
            MenuEntry::Hide => 2,
            MenuEntry::PlayPause => 3,
            MenuEntry::Previous => 4,
            MenuEntry::Next => 5,
            MenuEntry::PinLyrics => 6,
            MenuEntry::Separator => 7,
            MenuEntry::Quit => 8,
        }
    }

    fn from_id(id: i32) -> Option<Self> {
        Self::ALL.into_iter().find(|entry| entry.id() == id)
    }

    fn is_separator(self) -> bool {
        matches!(self, MenuEntry::Separator)
    }

    fn command(self) -> Option<TrayCommand> {
        match self {
            MenuEntry::Show => Some(TrayCommand::Show),
            MenuEntry::Hide => Some(TrayCommand::Hide),
            MenuEntry::PlayPause => Some(TrayCommand::PlayPause),
            MenuEntry::Previous => Some(TrayCommand::Previous),
            MenuEntry::Next => Some(TrayCommand::Next),
            MenuEntry::PinLyrics => Some(TrayCommand::PinLyrics),
            MenuEntry::Quit => Some(TrayCommand::Quit),
            MenuEntry::Separator => None,
        }
    }

    fn label(self, playing: bool, pinned: bool) -> String {
        match self {
            MenuEntry::Show => "Show".to_owned(),
            MenuEntry::Hide => "Hide".to_owned(),
            MenuEntry::PlayPause if playing => "Pause".to_owned(),
            MenuEntry::PlayPause => "Play".to_owned(),
            MenuEntry::Previous => "Previous".to_owned(),
            MenuEntry::Next => "Next".to_owned(),
            MenuEntry::PinLyrics if pinned => "Unpin lyrics".to_owned(),
            MenuEntry::PinLyrics => "Pin lyrics".to_owned(),
            MenuEntry::Separator => String::new(),
            MenuEntry::Quit => "Quit".to_owned(),
        }
    }
}

/// Turn a DBusMenu `Event` into the command it stands for.
///
/// Only `clicked` is a user action; `opened`, `closed` and friends are noise
/// from hosts that track menu state.
fn command_for_event(id: i32, event_id: &str) -> Option<TrayCommand> {
    if event_id != "clicked" {
        return None;
    }
    MenuEntry::from_id(id).and_then(MenuEntry::command)
}

/// Convert RGBA8 pixels to the big-endian ARGB32 pixmaps SNI wants.
///
/// A short buffer is zero-filled rather than refused: a broken icon should cost
/// the icon, not the tray.
fn icon_pixmaps(rgba: &[u8], size: u32) -> IconPixmaps {
    if size == 0 {
        return Vec::new();
    }
    let side = size as usize;
    let expected = side * side * 4;
    if rgba.len() < expected {
        tracing::warn!(
            "tray: icon buffer is {} bytes, expected {expected}; padding with zeros",
            rgba.len()
        );
    }
    let mut argb = Vec::with_capacity(expected);
    for pixel in 0..side * side {
        let offset = pixel * 4;
        let channel = |index: usize| rgba.get(offset + index).copied().unwrap_or(0);
        let (red, green, blue, alpha) = (channel(0), channel(1), channel(2), channel(3));
        argb.extend_from_slice(&[alpha, red, green, blue]);
    }
    vec![(size as i32, size as i32, argb)]
}

fn tooltip_of(state: &TrayState, icon: &IconPixmaps) -> ToolTip {
    (
        String::new(),
        icon.clone(),
        state.display_title(),
        APP_ID.to_owned(),
    )
}

fn menu_properties(entry: MenuEntry, state: &TrayState) -> MenuProperties {
    let mut properties = MenuProperties::new();
    properties.insert(
        "label".to_owned(),
        Value::from(entry.label(state.playing, state.pinned)),
    );
    properties.insert("enabled".to_owned(), Value::from(true));
    properties.insert("visible".to_owned(), Value::from(true));
    let kind = if entry.is_separator() {
        "separator"
    } else {
        "standard"
    };
    properties.insert("type".to_owned(), Value::from(kind));
    properties
}

fn root_properties() -> MenuProperties {
    let mut properties = MenuProperties::new();
    properties.insert("label".to_owned(), Value::from(""));
    properties.insert("visible".to_owned(), Value::from(true));
    properties.insert("children-display".to_owned(), Value::from("submenu"));
    properties
}

/// Keep only the properties the caller asked for; an empty list means "all".
fn filter_properties(properties: MenuProperties, property_names: &[String]) -> MenuProperties {
    if property_names.is_empty() {
        return properties;
    }
    properties
        .into_iter()
        .filter(|(name, _)| property_names.iter().any(|wanted| wanted == name))
        .collect()
}

fn item_view(entry: MenuEntry, state: &TrayState, property_names: &[String]) -> MenuItem {
    (
        entry.id(),
        filter_properties(menu_properties(entry, state), property_names),
        Vec::new(),
    )
}

fn menu_layout(state: &TrayState, recursion_depth: i32, property_names: &[String]) -> MenuItem {
    let children = if recursion_depth == 0 {
        Vec::new()
    } else {
        MenuEntry::ALL
            .iter()
            .map(|entry| Value::from(item_view(*entry, state, property_names)))
            .collect()
    };
    (
        ROOT_ID,
        filter_properties(root_properties(), property_names),
        children,
    )
}

fn lock_state(state: &Arc<Mutex<TrayState>>) -> MutexGuard<'_, TrayState> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// `org.kde.StatusNotifierItem`: the icon, its tooltip and the click methods.
struct StatusNotifierItem {
    state: Arc<Mutex<TrayState>>,
    icon: Arc<IconPixmaps>,
    commands: Sender<TrayCommand>,
    menu_path: ObjectPath<'static>,
}

impl StatusNotifierItem {
    fn send(&self, command: TrayCommand) {
        if self.commands.send(command).is_err() {
            tracing::debug!("tray: command channel is closed, {command:?} dropped");
        }
    }
}

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl StatusNotifierItem {
    // Left click, and middle click on hosts that report it that way.
    fn activate(&self, x: i32, y: i32) {
        tracing::debug!(x, y, "tray: activate");
        self.send(TrayCommand::Show);
    }

    fn secondary_activate(&self, x: i32, y: i32) {
        tracing::debug!(x, y, "tray: secondary activate");
        self.send(TrayCommand::Show);
    }

    fn context_menu(&self, x: i32, y: i32) {
        // The menu itself belongs to the shell: it reads it from DBusMenu and
        // opens it where it wants. Only hosts without DBusMenu support call
        // this, and for them surfacing the app is the closest equivalent.
        tracing::debug!(x, y, "tray: context menu requested");
        self.send(TrayCommand::Show);
    }

    fn scroll(&self, delta: i32, orientation: &str) {
        tracing::debug!(delta, orientation, "tray: scroll");
    }

    fn provide_xdg_activation_token(&self, token: &str) {
        tracing::debug!(token, "tray: xdg activation token");
    }

    fn x_ayatana_secondary_activate(&self, timestamp: u32) {
        tracing::debug!(timestamp, "tray: ayatana secondary activate");
        self.send(TrayCommand::Show);
    }

    #[zbus(property)]
    fn category(&self) -> String {
        "ApplicationStatus".to_owned()
    }

    #[zbus(property)]
    fn id(&self) -> String {
        APP_ID.to_owned()
    }

    #[zbus(property)]
    fn title(&self) -> String {
        lock_state(&self.state).display_title()
    }

    #[zbus(property)]
    fn status(&self) -> String {
        // Passive would make the shell hide us.
        "Active".to_owned()
    }

    #[zbus(property)]
    fn window_id(&self) -> i32 {
        0
    }

    #[zbus(property)]
    fn icon_theme_path(&self) -> String {
        String::new()
    }

    #[zbus(property)]
    fn menu(&self) -> ObjectPath<'static> {
        self.menu_path.clone()
    }

    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn icon_name(&self) -> String {
        String::new()
    }

    #[zbus(property)]
    fn icon_pixmap(&self) -> IconPixmaps {
        (*self.icon).clone()
    }

    #[zbus(property)]
    fn overlay_icon_name(&self) -> String {
        String::new()
    }

    #[zbus(property)]
    fn overlay_icon_pixmap(&self) -> IconPixmaps {
        Vec::new()
    }

    #[zbus(property)]
    fn attention_icon_name(&self) -> String {
        String::new()
    }

    #[zbus(property)]
    fn attention_icon_pixmap(&self) -> IconPixmaps {
        Vec::new()
    }

    #[zbus(property)]
    fn attention_movie_name(&self) -> String {
        String::new()
    }

    #[zbus(property)]
    fn tool_tip(&self) -> ToolTip {
        tooltip_of(&lock_state(&self.state), &self.icon)
    }
}

/// `com.canonical.dbusmenu`: the menu the shell draws for us.
struct StatusNotifierMenu {
    state: Arc<Mutex<TrayState>>,
    commands: Sender<TrayCommand>,
}

impl StatusNotifierMenu {
    fn snapshot(&self) -> TrayState {
        lock_state(&self.state).clone()
    }

    fn send(&self, command: TrayCommand) {
        if self.commands.send(command).is_err() {
            tracing::debug!("tray: command channel is closed, {command:?} dropped");
        }
    }
}

#[zbus::interface(name = "com.canonical.dbusmenu")]
impl StatusNotifierMenu {
    #[zbus(property)]
    fn version(&self) -> u32 {
        DBUSMENU_VERSION
    }

    #[zbus(property)]
    fn text_direction(&self) -> String {
        "ltr".to_owned()
    }

    #[zbus(property)]
    fn status(&self) -> String {
        "normal".to_owned()
    }

    #[zbus(property)]
    fn icon_theme_path(&self) -> Vec<String> {
        Vec::new()
    }

    fn get_layout(
        &self,
        parent_id: i32,
        recursion_depth: i32,
        property_names: Vec<String>,
    ) -> (u32, MenuItem) {
        let state = self.snapshot();
        let item = if parent_id == ROOT_ID {
            menu_layout(&state, recursion_depth, &property_names)
        } else if let Some(entry) = MenuEntry::from_id(parent_id) {
            item_view(entry, &state, &property_names)
        } else {
            (parent_id, MenuProperties::new(), Vec::new())
        };
        (state.revision, item)
    }

    fn get_group_properties(
        &self,
        ids: Vec<i32>,
        property_names: Vec<String>,
    ) -> Vec<(i32, MenuProperties)> {
        let state = self.snapshot();
        let ids = if ids.is_empty() {
            // An empty request means every item.
            let mut all = vec![ROOT_ID];
            all.extend(MenuEntry::ALL.iter().map(|entry| entry.id()));
            all
        } else {
            ids
        };
        ids.into_iter()
            .filter_map(|id| {
                if id == ROOT_ID {
                    Some((
                        ROOT_ID,
                        filter_properties(root_properties(), &property_names),
                    ))
                } else {
                    MenuEntry::from_id(id).map(|entry| {
                        (
                            id,
                            filter_properties(menu_properties(entry, &state), &property_names),
                        )
                    })
                }
            })
            .collect()
    }

    fn get_property(&self, id: i32, name: String) -> Value<'static> {
        let state = self.snapshot();
        let properties = if id == ROOT_ID {
            root_properties()
        } else {
            match MenuEntry::from_id(id) {
                Some(entry) => menu_properties(entry, &state),
                None => return Value::from(String::new()),
            }
        };
        properties
            .get(&name)
            .cloned()
            .unwrap_or_else(|| Value::from(String::new()))
    }

    fn event(&self, id: i32, event_id: String, data: OwnedValue, timestamp: u32) {
        tracing::debug!(id, event = %event_id, timestamp, data = ?data, "tray: menu event");
        if let Some(command) = command_for_event(id, &event_id) {
            self.send(command);
        }
    }

    fn about_to_show(&self, id: i32) -> bool {
        tracing::debug!(id, "tray: about to show");
        true
    }

    fn about_to_show_group(&self, ids: Vec<i32>) -> (Vec<i32>, Vec<i32>) {
        // Nothing needs an update: the layout is always current. The second
        // list would carry ids that could not be resolved.
        tracing::debug!(?ids, "tray: about to show group");
        (Vec::new(), Vec::new())
    }
}

/// The live bus side: registrations plus the worker loop that pushes updates.
struct Service {
    connection: Connection,
    own_name: String,
    state: Arc<Mutex<TrayState>>,
    icon: Arc<IconPixmaps>,
}

impl Service {
    fn start(
        state: Arc<Mutex<TrayState>>,
        icon: Arc<IconPixmaps>,
        commands: Sender<TrayCommand>,
    ) -> Result<Self, TrayError> {
        let connection = Connection::session()
            .map_err(|error| TrayError::dbus("cannot connect to the session bus", &error))?;
        let menu_path = ObjectPath::try_from(MENU_PATH)
            .map_err(|error| TrayError::new(format!("invalid menu path {MENU_PATH}: {error}")))?;

        connection
            .object_server()
            .at(
                ITEM_PATH,
                StatusNotifierItem {
                    state: Arc::clone(&state),
                    icon: Arc::clone(&icon),
                    commands: commands.clone(),
                    menu_path,
                },
            )
            .map_err(|error| TrayError::dbus("cannot export the tray item", &error))?;
        connection
            .object_server()
            .at(
                MENU_PATH,
                StatusNotifierMenu {
                    state: Arc::clone(&state),
                    commands,
                },
            )
            .map_err(|error| TrayError::dbus("cannot export the tray menu", &error))?;

        let own_name = format!("{SNI_INTERFACE}-{}-1", std::process::id());
        connection
            .request_name(own_name.as_str())
            .map_err(|error| TrayError::dbus("cannot own the tray bus name", &error))?;

        if let Err(error) = register_with_watcher(&connection, &own_name) {
            // Without a watcher nothing can pick the item up, but the item itself
            // is exported and a host that finds it another way still works.
            tracing::warn!("tray: StatusNotifierWatcher refused the item: {error}");
        }
        tracing::debug!(name = %own_name, "tray registered");
        Ok(Self {
            connection,
            own_name,
            state,
            icon,
        })
    }

    fn serve(self, control: Receiver<ControlMessage>) {
        while let Ok(message) = control.recv() {
            match message {
                ControlMessage::Track { title, artist } => self.publish_track(title, artist),
                ControlMessage::Playing(playing) => self.publish_playing(playing),
                ControlMessage::Pinned(pinned) => self.publish_pinned(pinned),
                ControlMessage::Shutdown => break,
            }
        }
        self.stop();
    }

    fn stop(self) {
        // Closing the socket releases the well-known name, which is what makes
        // the icon disappear from the shell.
        if let Err(error) = self.connection.close() {
            tracing::debug!("tray: closing the bus connection failed: {error}");
        }
        tracing::debug!(name = %self.own_name, "tray stopped");
    }

    fn emit<B>(&self, path: &str, interface: &str, signal: &str, body: &B)
    where
        B: Serialize + DynamicType,
    {
        if let Err(error) = self
            .connection
            .emit_signal(None::<&str>, path, interface, signal, body)
        {
            tracing::warn!(signal, %error, "tray: cannot emit signal");
        }
    }

    fn publish_track(&self, title: String, artist: String) {
        let (display_title, tooltip) = {
            let mut state = lock_state(&self.state);
            state.title = title;
            state.artist = artist;
            (state.display_title(), tooltip_of(&state, &self.icon))
        };

        self.emit(ITEM_PATH, SNI_INTERFACE, "NewTitle", &());
        self.emit(ITEM_PATH, SNI_INTERFACE, "NewToolTip", &());

        let mut changed = MenuProperties::new();
        changed.insert("Title".to_owned(), Value::from(display_title));
        changed.insert("ToolTip".to_owned(), Value::from(tooltip));
        let invalidated = Vec::<String>::new();
        self.emit(
            ITEM_PATH,
            PROPERTIES_INTERFACE,
            "PropertiesChanged",
            &(SNI_INTERFACE, changed, invalidated),
        );
    }

    fn publish_playing(&self, playing: bool) {
        let revision = {
            let mut state = lock_state(&self.state);
            if state.playing == playing {
                return;
            }
            state.playing = playing;
            state.bump_revision()
        };
        self.emit(MENU_PATH, MENU_INTERFACE, "LayoutUpdated", &(revision, 0));
    }

    fn publish_pinned(&self, pinned: bool) {
        let revision = {
            let mut state = lock_state(&self.state);
            if state.pinned == pinned {
                return;
            }
            state.pinned = pinned;
            state.bump_revision()
        };
        self.emit(MENU_PATH, MENU_INTERFACE, "LayoutUpdated", &(revision, 0));
    }
}

fn register_with_watcher(connection: &Connection, own_name: &str) -> zbus::Result<()> {
    connection.call_method(
        Some(WATCHER_SERVICE),
        WATCHER_PATH,
        Some(WATCHER_INTERFACE),
        "RegisterStatusNotifierItem",
        &(own_name,),
    )?;
    Ok(())
}

fn run_worker(
    state: Arc<Mutex<TrayState>>,
    icon: Arc<IconPixmaps>,
    commands: Sender<TrayCommand>,
    control: Receiver<ControlMessage>,
    ready: Sender<Result<(), TrayError>>,
) {
    let service = match Service::start(state, icon, commands) {
        Ok(service) => service,
        Err(error) => {
            tracing::warn!("tray: not available: {error}");
            let _ = ready.send(Err(error));
            return;
        }
    };
    let _ = ready.send(Ok(()));
    service.serve(control);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_ids_are_unique_and_stable() {
        let ids: Vec<i32> = MenuEntry::ALL.iter().map(|entry| entry.id()).collect();
        assert_eq!(ids, vec![1, 2, 3, 4, 5, 6, 7, 8]);
        let mut unique = ids.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), ids.len());
    }

    #[test]
    fn menu_ids_round_trip() {
        for entry in MenuEntry::ALL {
            assert_eq!(MenuEntry::from_id(entry.id()), Some(entry));
        }
        assert_eq!(MenuEntry::from_id(0), None);
        assert_eq!(MenuEntry::from_id(9), None);
        assert_eq!(MenuEntry::from_id(-1), None);
    }

    #[test]
    fn menu_entries_map_to_commands() {
        assert_eq!(MenuEntry::Show.command(), Some(TrayCommand::Show));
        assert_eq!(MenuEntry::Hide.command(), Some(TrayCommand::Hide));
        assert_eq!(MenuEntry::PlayPause.command(), Some(TrayCommand::PlayPause));
        assert_eq!(MenuEntry::Previous.command(), Some(TrayCommand::Previous));
        assert_eq!(MenuEntry::Next.command(), Some(TrayCommand::Next));
        assert_eq!(MenuEntry::PinLyrics.command(), Some(TrayCommand::PinLyrics));
        assert_eq!(MenuEntry::Quit.command(), Some(TrayCommand::Quit));
        assert_eq!(MenuEntry::Separator.command(), None);
    }

    #[test]
    fn play_pause_label_follows_playback() {
        assert_eq!(MenuEntry::PlayPause.label(false, false), "Play");
        assert_eq!(MenuEntry::PlayPause.label(true, false), "Pause");
        // The pin state does not leak into other labels.
        assert_eq!(MenuEntry::Show.label(true, true), "Show");
        assert_eq!(MenuEntry::Previous.label(true, true), "Previous");
        assert_eq!(MenuEntry::Next.label(true, true), "Next");
        assert_eq!(MenuEntry::Hide.label(false, false), "Hide");
        assert_eq!(MenuEntry::Quit.label(false, false), "Quit");
    }

    #[test]
    fn pin_lyrics_label_follows_pin_state() {
        assert_eq!(MenuEntry::PinLyrics.label(false, false), "Pin lyrics");
        assert_eq!(MenuEntry::PinLyrics.label(false, true), "Unpin lyrics");
    }

    #[test]
    fn separator_has_no_label_and_no_command() {
        let state = TrayState::default();
        let properties = menu_properties(MenuEntry::Separator, &state);
        assert_eq!(properties.get("label"), Some(&Value::from("")));
        assert_eq!(properties.get("type"), Some(&Value::from("separator")));
        assert!(MenuEntry::Separator.command().is_none());
    }

    #[test]
    fn clicked_events_map_to_commands() {
        assert_eq!(
            command_for_event(MenuEntry::Show.id(), "clicked"),
            Some(TrayCommand::Show)
        );
        assert_eq!(
            command_for_event(MenuEntry::PlayPause.id(), "clicked"),
            Some(TrayCommand::PlayPause)
        );
        assert_eq!(
            command_for_event(MenuEntry::Quit.id(), "clicked"),
            Some(TrayCommand::Quit)
        );
        assert_eq!(command_for_event(MenuEntry::Show.id(), "opened"), None);
        assert_eq!(command_for_event(MenuEntry::Show.id(), "closed"), None);
        assert_eq!(
            command_for_event(MenuEntry::Separator.id(), "clicked"),
            None
        );
        assert_eq!(command_for_event(999, "clicked"), None);
    }

    #[test]
    fn standard_items_carry_a_label_and_a_type() {
        let state = TrayState::default();
        let properties = menu_properties(MenuEntry::Show, &state);
        assert_eq!(properties.get("label"), Some(&Value::from("Show")));
        assert_eq!(properties.get("type"), Some(&Value::from("standard")));
        assert_eq!(properties.get("enabled"), Some(&Value::from(true)));
        assert_eq!(properties.get("visible"), Some(&Value::from(true)));
    }

    #[test]
    fn requested_property_names_filter_the_map() {
        let state = TrayState::default();
        let properties = menu_properties(MenuEntry::Show, &state);
        let wanted = vec!["type".to_owned(), "children-display".to_owned()];
        let filtered = filter_properties(properties, &wanted);
        assert_eq!(filtered.len(), 1);
        assert!(filtered.contains_key("type"));
    }

    #[test]
    fn layout_contains_every_item_in_menu_order() {
        let state = TrayState::default();
        let (root_id, properties, children) = menu_layout(&state, -1, &[]);
        assert_eq!(root_id, ROOT_ID);
        assert_eq!(
            properties.get("children-display"),
            Some(&Value::from("submenu"))
        );
        assert_eq!(children.len(), MenuEntry::ALL.len());
    }

    #[test]
    fn layout_respects_the_recursion_depth() {
        let state = TrayState::default();
        let (_, _, children) = menu_layout(&state, 0, &[]);
        assert!(children.is_empty());
    }

    #[test]
    fn display_title_joins_title_and_artist() {
        assert_eq!(TrayState::default().display_title(), "Myuzik");
        let title_only = TrayState {
            title: "Song".to_owned(),
            ..TrayState::default()
        };
        assert_eq!(title_only.display_title(), "Song");
        let both = TrayState {
            title: "Song".to_owned(),
            artist: "Band".to_owned(),
            ..TrayState::default()
        };
        assert_eq!(both.display_title(), "Song \u{2014} Band");
        let artist_only = TrayState {
            artist: "Band".to_owned(),
            ..TrayState::default()
        };
        assert_eq!(artist_only.display_title(), "Band");
    }

    #[test]
    fn tooltip_follows_the_track() {
        let state = TrayState {
            title: "Song".to_owned(),
            artist: "Band".to_owned(),
            ..TrayState::default()
        };
        let icon = vec![(1, 1, vec![0u8; 4])];
        let (name, pixmaps, title, subtitle) = tooltip_of(&state, &icon);
        assert!(name.is_empty());
        assert_eq!(pixmaps.len(), 1);
        assert_eq!(title, "Song \u{2014} Band");
        assert_eq!(subtitle, "Myuzik");
    }

    #[test]
    fn icon_pixmap_is_big_endian_argb32() {
        // One RGBA pixel: r=10, g=20, b=30, a=40.
        let pixmaps = icon_pixmaps(&[10, 20, 30, 40], 1);
        assert_eq!(pixmaps.len(), 1);
        let (width, height, pixels) = &pixmaps[0];
        assert_eq!((*width, *height), (1, 1));
        assert_eq!(pixels, &vec![40, 10, 20, 30]);
    }

    #[test]
    fn icon_pixmap_converts_every_pixel() {
        let mut rgba = Vec::new();
        for value in 0..4u8 {
            rgba.extend_from_slice(&[value, value + 1, value + 2, value + 3]);
        }
        let pixmaps = icon_pixmaps(&rgba, 2);
        let (width, height, pixels) = &pixmaps[0];
        assert_eq!((*width, *height), (2, 2));
        assert_eq!(pixels.len(), 16);
        assert_eq!(&pixels[0..4], &[3, 0, 1, 2]);
        assert_eq!(&pixels[4..8], &[4, 1, 2, 3]);
        assert_eq!(&pixels[8..12], &[5, 2, 3, 4]);
        assert_eq!(&pixels[12..16], &[6, 3, 4, 5]);
    }

    #[test]
    fn short_icon_buffer_is_padded_instead_of_panicking() {
        let pixmaps = icon_pixmaps(&[1, 2], 1);
        let (width, height, pixels) = &pixmaps[0];
        assert_eq!((*width, *height), (1, 1));
        assert_eq!(pixels, &vec![0, 1, 2, 0]);
    }

    #[test]
    fn zero_sized_icon_yields_no_pixmaps() {
        assert!(icon_pixmaps(&[], 0).is_empty());
    }

    #[test]
    fn revisions_advance() {
        let mut state = TrayState::default();
        assert_eq!(state.revision, 0);
        assert_eq!(state.bump_revision(), 1);
        assert_eq!(state.bump_revision(), 2);
    }

    #[test]
    fn tray_handle_can_move_between_threads() {
        // The UI owns the handle and the player thread only sends commands, so
        // both sides have to cross threads.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Tray>();
        assert_send_sync::<TrayCommand>();
        assert_send_sync::<TrayError>();
    }

    fn busctl_list() -> String {
        match std::process::Command::new("busctl")
            .args(["--user", "list"])
            .output()
        {
            Ok(output) => String::from_utf8_lossy(&output.stdout).into_owned(),
            Err(error) => format!("busctl failed: {error}"),
        }
    }

    fn name_has_owner(connection: &Connection, name: &str) -> bool {
        let reply = connection.call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "NameHasOwner",
            &(name,),
        );
        match reply {
            Ok(reply) => {
                let body = reply.body();
                body.deserialize::<bool>().unwrap_or(false)
            }
            Err(error) => {
                println!("NameHasOwner({name}) failed: {error}");
                false
            }
        }
    }

    fn property(
        connection: &Connection,
        name: &str,
        path: &str,
        interface: &str,
        field: &str,
    ) -> Option<String> {
        let reply = connection
            .call_method(
                Some(name),
                path,
                Some(PROPERTIES_INTERFACE),
                "Get",
                &(interface, field),
            )
            .ok()?;
        let body = reply.body();
        let value: OwnedValue = body.deserialize().ok()?;
        String::try_from(value).ok()
    }

    /// Read one DBusMenu item property through `com.canonical.dbusmenu`.
    fn menu_property(connection: &Connection, name: &str, id: i32, field: &str) -> Option<String> {
        let reply = connection
            .call_method(
                Some(name),
                MENU_PATH,
                Some(MENU_INTERFACE),
                "GetProperty",
                &(id, field),
            )
            .ok()?;
        let body = reply.body();
        let value: OwnedValue = body.deserialize().ok()?;
        String::try_from(value).ok()
    }

    /// The watcher's own list of registered items, straight from `busctl`.
    fn watcher_items() -> String {
        match std::process::Command::new("busctl")
            .args([
                "--user",
                "get-property",
                WATCHER_SERVICE,
                WATCHER_PATH,
                WATCHER_INTERFACE,
                "RegisteredStatusNotifierItems",
            ])
            .output()
        {
            Ok(output) => String::from_utf8_lossy(&output.stdout).into_owned(),
            Err(error) => format!("busctl failed: {error}"),
        }
    }

    /// `busctl introspect`, so the test can check the wire names the shell sees.
    fn introspect(name: &str, path: &str) -> String {
        match std::process::Command::new("busctl")
            .args(["--user", "introspect", name, path])
            .output()
        {
            Ok(output) => String::from_utf8_lossy(&output.stdout).into_owned(),
            Err(error) => format!("busctl failed: {error}"),
        }
    }

    /// Run `action` while `dbus-monitor` records session-bus signals.
    fn monitor_signals(action: impl FnOnce()) -> String {
        let mut monitor = match std::process::Command::new("dbus-monitor")
            .args(["--session", "type='signal'"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(monitor) => monitor,
            Err(error) => {
                println!("cannot run dbus-monitor: {error}");
                return String::new();
            }
        };
        // Give dbus-monitor time to install its match rule.
        std::thread::sleep(Duration::from_millis(400));
        action();
        std::thread::sleep(Duration::from_millis(400));
        let _ = monitor.kill();
        match monitor.wait_with_output() {
            Ok(output) => String::from_utf8_lossy(&output.stdout).into_owned(),
            Err(error) => {
                println!("cannot read dbus-monitor output: {error}");
                String::new()
            }
        }
    }

    /// Real round trips against the running session bus: name ownership, the
    /// watcher registration, the menu and the commands the item emits.
    ///
    /// Not part of `cargo test` because it needs a session bus with a
    /// StatusNotifierWatcher:
    /// `cargo test --lib platform::tray -- --ignored --test-threads=1`.
    #[test]
    #[ignore = "needs a live session bus and a StatusNotifierWatcher"]
    fn smoke_registers_answers_and_unregisters() {
        // Start the tray first: with no session bus this is the code path that
        // has to report an error instead of panicking.
        let (tray, commands) = match Tray::start(vec![0x40; 22 * 22 * 4], 22) {
            Ok(started) => started,
            Err(error) => {
                println!("tray did not start, nothing to smoke test: {error}");
                return;
            }
        };

        let side = match Connection::session() {
            Ok(connection) => connection,
            Err(error) => {
                println!("no session bus, nothing to smoke test: {error}");
                tray.shutdown();
                return;
            }
        };
        let name = format!("{SNI_INTERFACE}-{}-1", std::process::id());

        assert!(name_has_owner(&side, &name), "{name} is not owned");
        let listing = busctl_list();
        let matched: Vec<&str> = listing
            .lines()
            .filter(|line| line.contains(&name))
            .collect();
        println!(
            "busctl --user list (matching rows):\n{}",
            matched.join("\n")
        );
        assert!(!matched.is_empty(), "busctl does not list {name}");

        // The watcher is what tells the shell about us.
        let registered = watcher_items();
        println!(
            "busctl --user get-property {WATCHER_SERVICE} {WATCHER_PATH} \
             {WATCHER_INTERFACE} RegisteredStatusNotifierItems:\n{registered}"
        );
        assert!(
            registered.contains(&name),
            "the watcher does not know {name}"
        );

        // Track, menu labels and the item properties. Everything is done inside
        // `monitor_signals` so the signals the updates emit are captured too.
        let signals = monitor_signals(|| {
            tray.set_track("Song", "Band");
            tray.set_playing(true);
            for _ in 0..20 {
                if menu_property(&side, &name, MenuEntry::PlayPause.id(), "label").as_deref()
                    == Some("Pause")
                {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        println!("signals emitted while the track changed:\n{signals}");
        for expected in [
            "path=/StatusNotifierItem; interface=org.kde.StatusNotifierItem; member=NewTitle",
            "path=/StatusNotifierItem; interface=org.kde.StatusNotifierItem; member=NewToolTip",
            "path=/StatusNotifierItem; interface=org.freedesktop.DBus.Properties; \
             member=PropertiesChanged",
            "path=/MenuBar; interface=com.canonical.dbusmenu; member=LayoutUpdated",
        ] {
            assert!(
                signals.contains(expected),
                "missing signal {expected}\n{signals}"
            );
        }
        let label = menu_property(&side, &name, MenuEntry::PlayPause.id(), "label");
        assert_eq!(label.as_deref(), Some("Pause"), "play/pause label flip");
        let separator = menu_property(&side, &name, MenuEntry::Separator.id(), "type");
        assert_eq!(
            separator.as_deref(),
            Some("separator"),
            "separator item type"
        );
        let title = property(&side, &name, ITEM_PATH, SNI_INTERFACE, "Title");
        assert_eq!(
            title.as_deref(),
            Some("Song \u{2014} Band"),
            "title property"
        );

        // The wire names a shell reads: these are the spec names, not the Rust
        // method names.
        let item_introspection = introspect(&name, ITEM_PATH);
        println!("busctl --user introspect {name} {ITEM_PATH}:\n{item_introspection}");
        for member in [
            "Activate",
            "SecondaryActivate",
            "ContextMenu",
            "IconPixmap",
            "ToolTip",
            "Title",
            "Menu",
            "Status",
            "Id",
        ] {
            assert!(
                item_introspection.contains(member),
                "the item does not expose {member}:\n{item_introspection}"
            );
        }
        let menu_introspection = introspect(&name, MENU_PATH);
        println!("busctl --user introspect {name} {MENU_PATH}:\n{menu_introspection}");
        for member in ["GetLayout", "GetGroupProperties", "Event", "AboutToShow"] {
            assert!(
                menu_introspection.contains(member),
                "the menu does not expose {member}:\n{menu_introspection}"
            );
        }

        // A click on the menu item becomes one command.
        side.call_method(
            Some(name.as_str()),
            MENU_PATH,
            Some(MENU_INTERFACE),
            "Event",
            &(
                MenuEntry::PlayPause.id(),
                "clicked",
                Value::from(0i32),
                0u32,
            ),
        )
        .expect("Event call");
        assert_eq!(
            commands.recv_timeout(Duration::from_secs(2)),
            Ok(TrayCommand::PlayPause)
        );

        // A left click on the icon is Show.
        side.call_method(
            Some(name.as_str()),
            ITEM_PATH,
            Some(SNI_INTERFACE),
            "Activate",
            &(0i32, 0i32),
        )
        .expect("Activate call");
        assert_eq!(
            commands.recv_timeout(Duration::from_secs(2)),
            Ok(TrayCommand::Show)
        );

        tray.shutdown();
        for _ in 0..20 {
            if !name_has_owner(&side, &name) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(!name_has_owner(&side, &name), "{name} survived shutdown");
        let after = busctl_list();
        let still_there: Vec<&str> = after.lines().filter(|line| line.contains(&name)).collect();
        println!("after shutdown, matching rows: {still_there:?}");
        assert!(
            still_there.is_empty(),
            "{name} is still listed after shutdown"
        );
    }
}
