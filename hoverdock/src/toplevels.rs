//! Seeing the windows that are actually open.
//!
//! An applet talks to cosmic-panel's *own* embedded compositor, which knows
//! about the panel and nothing else. It has no idea a browser is running. To
//! find that out the applet needs a second Wayland connection, to the real
//! compositor, which cosmic-panel hands over in `X_PRIVILEGED_WAYLAND_SOCKET`
//! — but only to applets whose desktop entry asks for it with
//! `X-HostWaylandDisplay=true`.
//!
//! Over that connection:
//!
//! - `ext_foreign_toplevel_list_v1` enumerates every window, with its app id
//!   and title. That is enough for the running dots and for a list of windows
//!   per application.
//! - `zcosmic_toplevel_info_v1` turns each of those into a COSMIC handle and
//!   reports state, which is how a window knows it is the focused one.
//! - `zcosmic_toplevel_manager_v1` is the only one of the three that can
//!   *activate* a window. Without it the dock could show you your windows but
//!   not switch to them.
//!
//! Everything here degrades rather than fails: a missing global means fewer
//! features, never a dock that will not start.

use cosmic_protocols::toplevel_info::v1::client::{
    zcosmic_toplevel_handle_v1::{self, ZcosmicToplevelHandleV1},
    zcosmic_toplevel_info_v1::{self, ZcosmicToplevelInfoV1},
};
use cosmic_protocols::toplevel_management::v1::client::zcosmic_toplevel_manager_v1::{
    self, ZcosmicToplevelManagerV1,
};
use wayland_client::globals::{registry_queue_init, GlobalList};
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::{
    ext_foreign_toplevel_handle_v1::{self, ExtForeignToplevelHandleV1},
    ext_foreign_toplevel_list_v1::{self, ExtForeignToplevelListV1},
};

use crate::Applet;

/// One open window.
#[derive(Debug, Clone)]
pub struct Window {
    pub handle: ExtForeignToplevelHandleV1,
    /// The COSMIC twin of the handle, which is what can be activated.
    pub cosmic: Option<ZcosmicToplevelHandleV1>,
    pub app_id: String,
    pub title: String,
    pub activated: bool,
    /// Set between `app_id`/`title` arriving and the `done` that commits them.
    pending_app_id: Option<String>,
    pending_title: Option<String>,
}

impl Window {
    /// What to call it in a menu.
    pub fn label(&self) -> &str {
        if self.title.trim().is_empty() {
            &self.app_id
        } else {
            &self.title
        }
    }
}

/// Everything reached over the host connection.
pub struct Windows {
    pub conn: Connection,
    info: Option<ZcosmicToplevelInfoV1>,
    manager: Option<ZcosmicToplevelManagerV1>,
    seat: Option<WlSeat>,
    pub windows: Vec<Window>,
}

impl Windows {
    /// Connect to the real compositor and start listening.
    ///
    /// Returns `None` when there is no host connection to be had — running
    /// outside a panel, or an applet whose desktop entry has not asked for one.
    /// That is a normal state, not an error: the dock still launches things.
    pub fn connect() -> Option<(Self, EventQueue<Applet>)> {
        let conn = host_connection()?;
        let (globals, queue) = match registry_queue_init::<Applet>(&conn) {
            Ok(pair) => pair,
            Err(err) => {
                log::warn!("host connection has no registry: {err}");
                return None;
            }
        };
        let qh = queue.handle();

        // The list is the one global worth refusing to start without: with no
        // window list there is nothing here to do at all.
        if globals
            .bind::<ExtForeignToplevelListV1, _, _>(&qh, 1..=1, ())
            .is_err()
        {
            log::warn!("no ext_foreign_toplevel_list_v1; the dock cannot see open windows");
            return None;
        }

        let info = bind_optional::<ZcosmicToplevelInfoV1>(&globals, &qh, 2..=3, "toplevel info");
        let manager =
            bind_optional::<ZcosmicToplevelManagerV1>(&globals, &qh, 1..=4, "toplevel manager");
        if manager.is_none() {
            log::warn!("no zcosmic_toplevel_manager_v1; windows can be listed but not focused");
        }
        let seat = bind_optional::<WlSeat>(&globals, &qh, 1..=9, "seat");

        Some((
            Self {
                conn,
                info,
                manager,
                seat,
                windows: Vec::new(),
            },
            queue,
        ))
    }

    /// Every open window belonging to an application, in the order the
    /// compositor reported them.
    pub fn matching<'a>(&'a self, launcher: &'a crate::Launcher) -> Vec<&'a Window> {
        self.windows
            .iter()
            .filter(|window| launcher.matches_app_id(&window.app_id))
            .collect()
    }

    /// Bring a window to the front.
    ///
    /// Returns false when the compositor did not offer a way to — the caller
    /// then has to decide whether launching a new instance is better than
    /// doing nothing at all.
    pub fn activate(&self, window: &Window) -> bool {
        let (Some(manager), Some(cosmic), Some(seat)) =
            (self.manager.as_ref(), window.cosmic.as_ref(), self.seat.as_ref())
        else {
            return false;
        };
        manager.activate(cosmic, seat);
        // The request is only queued until something flushes the connection,
        // and this connection has no draw loop of its own to do it.
        let _ = self.conn.flush();
        true
    }

    fn index_of(&self, handle: &ExtForeignToplevelHandleV1) -> Option<usize> {
        self.windows.iter().position(|w| &w.handle == handle)
    }

    fn by_cosmic(&mut self, handle: &ZcosmicToplevelHandleV1) -> Option<&mut Window> {
        self.windows
            .iter_mut()
            .find(|w| w.cosmic.as_ref() == Some(handle))
    }
}

fn bind_optional<I>(
    globals: &GlobalList,
    qh: &QueueHandle<Applet>,
    version: std::ops::RangeInclusive<u32>,
    what: &str,
) -> Option<I>
where
    I: Proxy + 'static,
    Applet: Dispatch<I, ()>,
{
    match globals.bind::<I, _, _>(qh, version, ()) {
        Ok(global) => Some(global),
        Err(err) => {
            log::info!("no {what}: {err}");
            None
        }
    }
}

/// The socket to the real compositor, if there is one.
fn host_connection() -> Option<Connection> {
    use std::os::fd::FromRawFd;
    use std::os::unix::net::UnixStream;

    if let Some(raw) = std::env::var_os("X_PRIVILEGED_WAYLAND_SOCKET")
        .and_then(|v| v.to_str().and_then(|v| v.trim().parse::<i32>().ok()))
    {
        // SAFETY: cosmic-panel passes this fd to us and to nobody else, and it
        // is read here exactly once, at startup, before anything is spawned.
        let stream = unsafe { UnixStream::from_raw_fd(raw) };
        match Connection::from_socket(stream) {
            Ok(conn) => return Some(conn),
            Err(err) => log::warn!("privileged socket unusable: {err}"),
        }
    }

    // Falling back to WAYLAND_DISPLAY covers running the binary by hand. But
    // only once WAYLAND_SOCKET is gone: that fd is the panel's own compositor,
    // and connecting to it a second time would produce a window list that is
    // permanently empty and no error to explain why.
    if std::env::var_os("WAYLAND_SOCKET").is_some() {
        log::info!("no privileged socket; not falling back to the panel's own compositor");
        return None;
    }
    match Connection::connect_to_env() {
        Ok(conn) => Some(conn),
        Err(err) => {
            log::info!("no host compositor connection: {err}");
            None
        }
    }
}

// --- the list ---------------------------------------------------------------

impl Dispatch<ExtForeignToplevelListV1, ()> for Applet {
    fn event(
        state: &mut Self,
        _: &ExtForeignToplevelListV1,
        event: ext_foreign_toplevel_list_v1::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let ext_foreign_toplevel_list_v1::Event::Toplevel { toplevel } = event else {
            return;
        };
        let Some(windows) = state.windows.as_mut() else {
            return;
        };
        // The COSMIC handle is asked for straight away rather than on first
        // use: it is what carries the focused state, and a window that never
        // gets one can still be listed, just not activated.
        let cosmic = windows
            .info
            .as_ref()
            .map(|info| info.get_cosmic_toplevel(&toplevel, qh, ()));
        windows.windows.push(Window {
            handle: toplevel,
            cosmic,
            app_id: String::new(),
            title: String::new(),
            activated: false,
            pending_app_id: None,
            pending_title: None,
        });
    }

    wayland_client::event_created_child!(Applet, ExtForeignToplevelListV1, [
        ext_foreign_toplevel_list_v1::EVT_TOPLEVEL_OPCODE => (ExtForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ExtForeignToplevelHandleV1, ()> for Applet {
    fn event(
        state: &mut Self,
        handle: &ExtForeignToplevelHandleV1,
        event: ext_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(windows) = state.windows.as_mut() else {
            return;
        };
        let Some(index) = windows.index_of(handle) else {
            return;
        };

        match event {
            // Title and app id are double buffered: they are not true until
            // `done` says so, and a half-applied window would flicker in and
            // out of the dock as it started up.
            ext_foreign_toplevel_handle_v1::Event::AppId { app_id } => {
                windows.windows[index].pending_app_id = Some(app_id);
                return;
            }
            ext_foreign_toplevel_handle_v1::Event::Title { title } => {
                windows.windows[index].pending_title = Some(title);
                return;
            }
            ext_foreign_toplevel_handle_v1::Event::Done => {
                let window = &mut windows.windows[index];
                if let Some(app_id) = window.pending_app_id.take() {
                    window.app_id = app_id;
                }
                if let Some(title) = window.pending_title.take() {
                    window.title = title;
                }
            }
            ext_foreign_toplevel_handle_v1::Event::Closed => {
                let window = windows.windows.remove(index);
                if let Some(cosmic) = window.cosmic {
                    cosmic.destroy();
                }
                window.handle.destroy();
            }
            _ => return,
        }
        state.windows_changed();
    }
}

// --- COSMIC's view of the same windows ---------------------------------------

impl Dispatch<ZcosmicToplevelInfoV1, ()> for Applet {
    fn event(
        _: &mut Self,
        _: &ZcosmicToplevelInfoV1,
        _: zcosmic_toplevel_info_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // Everything useful arrives on the handles, not on the manager. The
        // deprecated `toplevel` event is not subscribed to: version 2 and up
        // pair handles to the ext list instead, which is what we do.
    }

    wayland_client::event_created_child!(Applet, ZcosmicToplevelInfoV1, [
        zcosmic_toplevel_info_v1::EVT_TOPLEVEL_OPCODE => (ZcosmicToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ZcosmicToplevelHandleV1, ()> for Applet {
    fn event(
        state: &mut Self,
        handle: &ZcosmicToplevelHandleV1,
        event: zcosmic_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let zcosmic_toplevel_handle_v1::Event::State { state: raw } = event else {
            return;
        };
        let Some(windows) = state.windows.as_mut() else {
            return;
        };
        // The state array is a flat run of little-endian u32s.
        let activated = raw.chunks_exact(4).any(|chunk| {
            u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
                == zcosmic_toplevel_handle_v1::State::Activated as u32
        });
        if let Some(window) = windows.by_cosmic(handle) {
            if window.activated != activated {
                window.activated = activated;
            }
        }
    }
}

impl Dispatch<ZcosmicToplevelManagerV1, ()> for Applet {
    fn event(
        _: &mut Self,
        _: &ZcosmicToplevelManagerV1,
        _: zcosmic_toplevel_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // Only `capabilities`, which we do not gate anything on: activate is
        // either accepted or quietly ignored by the compositor.
    }
}

wayland_client::delegate_noop!(Applet: ignore WlSeat);
