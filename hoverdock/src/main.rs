//! HoverDock — a cosmic-panel applet that gives the dock macOS-style hover
//! magnification, with the icons standing free rather than on a coloured plate.
//!
//! It is an applet rather than a dock of its own because the icons in COSMIC's
//! dock belong to `cosmic-app-list`, and no outside process can change how
//! another applet draws. So this one takes that slot: same position in the bar,
//! same pinned applications, different rendering.
//!
//! cosmic-panel hands each applet a Wayland socket to its own embedded
//! compositor, which speaks `wl_compositor`, `xdg_shell`, `wl_shm`, `wl_seat`
//! and `wl_output`. That makes an applet an ordinary xdg-shell client — no
//! libcosmic, no iced — so the row layout, the spring and the CPU rasteriser
//! are the same code that already worked in a layer-shell dock.
//!
//! # Why the surface never resizes
//!
//! The panel sizes its bar to the thickest applet in it. If the surface grew
//! while an icon was magnifying, the whole bar would resize every frame and the
//! panel would spend the animation re-laying out its clients. So the surface is
//! allocated once at its *largest* size — tall enough for a fully magnified
//! icon, wide enough for the widest the row can ever get — and everything
//! happens inside it. The extra area is transparent, and costs nothing.

mod config;
mod icons;
mod install;
mod launchers;
mod layout;
mod menu;
mod panel;
mod render;
mod text;
mod toplevels;

use anyhow::{Context, Result};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_compositor, delegate_output, delegate_pointer, delegate_registry, delegate_seat,
    delegate_shm, delegate_xdg_shell, delegate_xdg_window,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        xdg::{
            popup::{ConfigureKind, Popup, PopupConfigure, PopupHandler},
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
            XdgPositioner, XdgShell, XdgSurface,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use smithay_client_toolkit::delegate_xdg_popup;
use calloop::{
    EventLoop,
    timer::{TimeoutAction, Timer},
};
use calloop_wayland_source::WaylandSource;
use std::time::{Duration, Instant};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
};
use wayland_protocols::xdg::shell::client::xdg_positioner::{Anchor as XdgAnchor, ConstraintAdjustment, Gravity};

use config::{Config, Theme};
use icons::IconCache;
use launchers::Launcher;
use layout::{Metrics, Placed, Spring};
use panel::Host;
use text::Text;
use toplevels::Windows;

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;

/// How long the pointer has to rest on an icon before its name appears.
///
/// Long enough that sweeping along the row does not leave a trail of tooltips,
/// short enough that stopping on an icon feels like it answered you.
const TOOLTIP_DELAY: std::time::Duration = std::time::Duration::from_millis(600);

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    match std::env::args().nth(1).as_deref() {
        Some("--install") => return install_applet(),
        Some("--uninstall") => return uninstall_applet(),
        Some("--status") => return status(),
        Some("--help" | "-h") => {
            println!(
                "HoverDock — hover magnification for the COSMIC dock\n\n\
                 Usage: hoverdock [--install|--uninstall|--status]\n\n\
                 It is not started by hand: cosmic-panel launches it once it is\n\
                 listed in the dock's plugins. --install does that listing.\n\n\
                 Configuration: {}",
                Config::path().display()
            );
            return Ok(());
        }
        _ => {}
    }

    let started = Instant::now();
    let host = Host::from_env();
    if !Host::inside_panel() {
        log::warn!(
            "no COSMIC_PANEL_* in the environment — this is meant to be launched by \
             cosmic-panel, not by hand. Run --install, then restart the panel."
        );
    }
    let config = Config::load();
    let mut metrics = config.metrics(host.size.icon_size());
    let height = row_thickness(&metrics, &host, &config);
    fit_magnification(&mut metrics, height);

    let (pin_source, pinned, items) = pinned_items(&config);
    if items.is_empty() {
        log::warn!("nothing to show: no pinned applications were found");
    }

    let conn = Connection::connect_to_env()
        .context("no Wayland connection; cosmic-panel passes one in WAYLAND_SOCKET")?;
    let (globals, queue) = registry_queue_init(&conn)?;
    let qh = queue.handle();

    let compositor = CompositorState::bind(&globals, &qh).context("no wl_compositor")?;
    let xdg_shell = XdgShell::bind(&globals, &qh).context("no xdg_wm_base")?;
    let shm = Shm::bind(&globals, &qh).context("no wl_shm")?;

    let surface = compositor.create_surface(&qh);
    // The panel draws no decorations and would ignore a request for them.
    let window = xdg_shell.create_window(surface, WindowDecorations::None, &qh);
    window.set_title("HoverDock");
    window.set_app_id(install::APPLET_ID);

    let span = row_span(&metrics, items.len());
    let thickness = height;
    let vertical = !host.anchor.is_horizontal();
    let (width, height) = surface_size(span, thickness, vertical);
    let pool = SlotPool::new((width * height * 4).max(4096) as usize, &shm)?;
    let icon_pixels = (metrics.max_icon_size().ceil() as u32).max(16);

    let mut applet = Applet {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        pool,
        window,
        compositor,
        xdg_shell,
        host,
        vertical,
        span,
        thickness,
        theme: config.theme(),
        config,
        metrics,
        items,
        pinned,
        pin_source,
        icons: IconCache::new(icon_pixels),
        width,
        height,
        scale: 1,
        configured: false,
        cursor: None,
        pressed: None,
        drag: None,
        scales: Vec::new(),
        placed: Vec::new(),
        last_frame: Instant::now(),
        frame_pending: false,
        animating: true,
        exit: false,
        windows: None,
        hovered: None,
        hover_since: None,
        tooltip: None,
        max_span: None,
        menu: None,
        text: None,
        queue_handle: qh.clone(),
    };
    applet.scales = applet.items.iter().map(|_| Spring::new(1.0)).collect();

    // Declaring the geometry up front is what tells the panel how thick to make
    // its bar. Without it the panel measures the surface's bounding box, which
    // is not known until the first buffer is attached.
    applet
        .window
        .xdg_surface()
        .set_window_geometry(0, 0, width as i32, height as i32);
    applet.window.commit();

    log::info!(
        "started in {:.1} ms: {} icons, {}x{} surface, panel size {:?}",
        started.elapsed().as_secs_f32() * 1000.0,
        applet.items.len(),
        width,
        height,
        host.size
    );

    // Two connections now — the panel's embedded compositor and the real one —
    // so a blocking dispatch on either would starve the other. calloop watches
    // both file descriptors and dispatches whichever has something to say.
    let mut event_loop: EventLoop<Applet> = EventLoop::try_new()?;
    WaylandSource::new(conn.clone(), queue).insert(event_loop.handle())?;

    match Windows::connect() {
        Some((windows, host_queue)) => {
            let host_conn = windows.conn.clone();
            applet.windows = Some(windows);
            WaylandSource::new(host_conn, host_queue).insert(event_loop.handle())?;
        }
        None => log::info!("running without a view of open windows"),
    }

    // cosmic-app-library writes directly to this same favorites file when the
    // user chooses "Add to dock". A long-running applet has to notice that
    // external write; reading the list only once at startup leaves the new app
    // invisible until the whole panel is restarted.
    event_loop
        .handle()
        .insert_source(
            Timer::from_duration(Duration::from_millis(500)),
            |_, _, applet| {
                if applet.pin_source == PinSource::Cosmic {
                    let current = launchers::cosmic_favourites();
                    if current != applet.pinned {
                        log::info!("COSMIC favorites changed; rebuilding the dock");
                        applet.rebuild();
                    }
                }
                TimeoutAction::ToDuration(Duration::from_millis(500))
            },
        )
        .map_err(|err| anyhow::anyhow!("could not watch COSMIC favorites: {err}"))?;

    while !applet.exit {
        event_loop.dispatch(None, &mut applet)?;
    }
    Ok(())
}

fn install_applet() -> Result<()> {
    let path = install::write_desktop_entry()?;
    println!("desktop entry: {}", path.display());
    match install::enable()? {
        true => println!(
            "the dock now uses HoverDock in place of {}.",
            install::REPLACES
        ),
        false => println!("the dock was already using HoverDock."),
    }
    println!("Restart the panel to pick it up:  pkill -x cosmic-panel");
    Ok(())
}

fn uninstall_applet() -> Result<()> {
    match install::disable()? {
        true => println!("COSMIC's own app list is back in the dock."),
        false => println!("the dock was not using HoverDock."),
    }
    let path = install::desktop_file();
    if path.exists() {
        std::fs::remove_file(&path)?;
        println!("removed {}", path.display());
    }
    println!("Restart the panel to apply:  pkill -x cosmic-panel");
    Ok(())
}

fn status() -> Result<()> {
    let host = Host::from_env();
    let config = Config::load();
    let mut metrics = config.metrics(host.size.icon_size());
    let height = row_thickness(&metrics, &host, &config);
    fit_magnification(&mut metrics, height);
    let (_, _, items) = pinned_items(&config);
    println!("enabled in the dock: {}", install::is_enabled());
    println!("desktop entry:       {}", install::desktop_file().display());
    println!("config:              {}", Config::path().display());
    println!("panel size:          {:?}", host.size);
    println!(
        "icons:               {} at {:.0} px, up to {:.0} px magnified ({:.2}x)",
        items.len(),
        metrics.icon_size,
        metrics.max_icon_size(),
        metrics.magnification
    );
    let (w, h) = surface_size(row_span(&metrics, items.len()), height, !host.anchor.is_horizontal());
    println!(
        "surface:             {w}x{h} on the {:?} edge (panel ceiling {:.0} px)",
        host.anchor,
        host.size.max_thickness()
    );
    println!(
        "reserved space:      {:+.0} px against COSMIC's own app list",
        height as f32 - (metrics.icon_size + host.size.padding() * 2.0)
    );
    for item in &items {
        println!("  {} ({})", item.launcher.name, item.launcher.id);
    }
    Ok(())
}

/// Where the pinned list is kept, and therefore where a change to it goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PinSource {
    /// COSMIC's own favourites — the same file `cosmic-app-list` reads.
    Cosmic,
    /// An explicit `pinned = [...]` in this applet's config.
    Config,
}

/// One icon in the row, and where it came from in the pinned list.
///
/// The index matters: the pinned list can name applications that are not
/// installed, and those are skipped when drawing but must survive being
/// rewritten. Editing by index rather than by id is what keeps them.
struct Item {
    pinned_index: usize,
    launcher: Launcher,
}

/// Which applications to show, in order, and the list they came from.
fn pinned_items(config: &Config) -> (PinSource, Vec<String>, Vec<Item>) {
    let (source, pinned) = if config.pinned.is_empty() {
        (PinSource::Cosmic, launchers::cosmic_favourites())
    } else {
        (PinSource::Config, config.pinned.clone())
    };

    let installed = launchers::installed();
    let mut items = Vec::new();
    for (pinned_index, id) in pinned.iter().enumerate() {
        match launchers::find(&installed, id) {
            Some(launcher) => items.push(Item {
                pinned_index,
                launcher: launcher.clone(),
            }),
            None => log::warn!("pinned application {id:?} is not installed"),
        }
    }
    (source, pinned, items)
}

/// How tall the surface has to be.
///
/// Deliberately *not* "as tall as a magnified icon needs". The panel sizes its
/// bar to the thickest applet in it and reserves that much of the screen, so a
/// surface tall enough for a 1.8× icon pushes every window on the display down
/// by the difference — which is a worse thing to do to a desktop than a smaller
/// magnification is.
///
/// So the default is exactly what COSMIC's own app list occupies: one applet
/// unit, icon plus the panel's padding. `extra_height` buys more room for
/// anyone who would rather have the bigger effect, and `max_thickness` is the
/// hard ceiling — past it `constrain_dim` cuts the bar back and the icon with
/// it.
fn row_thickness(metrics: &Metrics, host: &Host, config: &Config) -> u32 {
    let unit = metrics.icon_size + host.size.padding() * 2.0;
    let wanted = unit + config.extra_height.max(0.0);
    // The panel's own padding and anchor gap sit outside our surface and it
    // does not tell us how big they are; its applet padding is a fair proxy.
    let ceiling = (host.size.max_thickness() - host.size.padding() * 2.0 - 8.0).max(unit);
    wanted.min(ceiling).ceil() as u32
}

/// The most an icon can grow before it runs out of surface.
///
/// The configured magnification is a wish, not a promise: whatever it says, an
/// icon drawn taller than the surface is an icon with its top and bottom
/// missing. Clamping here rather than clipping there is the difference between
/// a smaller effect and a broken-looking one.
fn fit_magnification(metrics: &mut Metrics, height: u32) {
    let room = (height as f32 - 2.0) / metrics.icon_size;
    metrics.magnification = metrics.magnification.min(room).max(1.0);
}

/// How wide the surface has to be: the widest the row can ever become.
///
/// Sampled rather than derived. The row's width at a given pointer position is
/// a sum over a falloff curve, and the maximum is not at an obvious place — it
/// depends on the number of icons, the reach and the shape of the curve. Fifty
/// samples of the real layout function cost microseconds once at startup and
/// cannot disagree with what is drawn later.
/// Shrink the row until it fits in `limit` pixels along the bar.
///
/// A vertical dock has a few hundred pixels to work with where a horizontal one
/// had thousands, so twenty icons at their natural size simply do not fit on the
/// screen — and a surface longer than the output does not get scrolled or
/// paged, it just runs off the end where nobody can click it.
///
/// Icon size and spacing come down together so the row keeps its proportions,
/// and never below a size you could still hit with a pointer. Returns true if
/// anything had to give.
fn fit_to_length(metrics: &mut Metrics, count: usize, limit: u32) -> bool {
    const MIN_ICON: f32 = 16.0;
    if count == 0 || limit == 0 || row_span(metrics, count) <= limit {
        return false;
    }

    let full_icon = metrics.icon_size;
    let full_spacing = metrics.spacing;
    // Twenty steps between "as asked for" and "as small as is usable" is finer
    // than a pixel at any realistic icon size.
    for step in 1..=20 {
        let scale = 1.0 - step as f32 / 20.0 * (1.0 - MIN_ICON / full_icon).max(0.0);
        metrics.icon_size = (full_icon * scale).max(MIN_ICON);
        metrics.spacing = full_spacing * scale;
        if row_span(metrics, count) <= limit {
            return true;
        }
    }
    true
}

/// Turn a length along the bar and a thickness across it into a surface size.
///
/// The one place the orientation becomes width and height. Everything upstream
/// of here is written along the bar, which is why a vertical dock is a change
/// of two lines rather than a second layout engine.
fn surface_size(span: u32, thickness: u32, vertical: bool) -> (u32, u32) {
    if vertical {
        (thickness, span)
    } else {
        (span, thickness)
    }
}

fn row_span(metrics: &Metrics, count: usize) -> u32 {
    if count == 0 {
        return 1;
    }
    let resting = metrics.resting_width(count);
    let mut widest = resting;
    let samples = 50;
    for step in 0..=samples {
        let cursor = resting * step as f32 / samples as f32;
        let scales = metrics.target_scales(count, Some(cursor), resting / 2.0);
        let grown: f32 = scales.iter().map(|s| s * metrics.icon_size).sum::<f32>()
            + (count - 1) as f32 * metrics.spacing;
        widest = widest.max(grown);
    }
    widest.ceil() as u32
}

struct Applet {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    window: Window,
    compositor: CompositorState,
    xdg_shell: XdgShell,

    host: Host,
    /// The bar runs down the side of the screen. Everything below is written
    /// along the bar and only becomes x or y where it has to.
    vertical: bool,
    /// Length of the surface along the bar, and its thickness across it.
    span: u32,
    thickness: u32,
    config: Config,
    theme: Theme,
    metrics: Metrics,
    items: Vec<Item>,
    /// The pinned list exactly as stored, including entries we cannot draw.
    pinned: Vec<String>,
    pin_source: PinSource,
    icons: IconCache,

    width: u32,
    height: u32,
    scale: i32,
    configured: bool,

    /// Pointer x in logical surface coordinates, when it is over us.
    cursor: Option<f32>,
    /// Which icon a press landed on, so a click is press *and* release on the
    /// same icon.
    pressed: Option<usize>,
    /// A press that has turned into a drag.
    drag: Option<Drag>,

    scales: Vec<Spring>,
    placed: Vec<Placed>,

    last_frame: Instant,
    frame_pending: bool,
    animating: bool,
    exit: bool,

    /// The windows that are actually open, over a second connection to the
    /// real compositor. `None` when there is no such connection to be had.
    windows: Option<Windows>,
    /// Which icon the pointer is resting on, and since when.
    hovered: Option<usize>,
    hover_since: Option<Instant>,
    /// The name shown after resting on an icon.
    tooltip: Option<TooltipState>,
    /// The longest the row may be, from the panel's own suggested bounds or,
    /// failing that, the output it is on. `None` until we are told.
    max_span: Option<u32>,
    /// The open right-click menu, if there is one.
    menu: Option<MenuState>,
    /// Built the first time a menu is opened, never at startup: building a font
    /// system scans every font on the machine and costs about 100 ms, which an
    /// applet that may never be right-clicked should not pay at login.
    text: Option<Text>,

    queue_handle: QueueHandle<Self>,
}

/// Exchange two icons in the row and in the pinned list at once.
///
/// The pinned list can name applications that are not installed, and those are
/// skipped when building the row. So the two entries to exchange are found by
/// the index each icon remembers, never by counting along the row — otherwise a
/// reorder here silently rearranges the entries nobody can see, and the list
/// comes back wrong the next time something *is* installed.
///
/// Returns false when there was nothing to do.
fn swap_pinned(pinned: &mut [String], items: &mut [Item], a: usize, b: usize) -> bool {
    if a == b || a >= items.len() || b >= items.len() {
        return false;
    }
    let (pa, pb) = (items[a].pinned_index, items[b].pinned_index);
    pinned.swap(pa, pb);
    items.swap(a, b);
    // The icons moved; the slots they occupy in the pinned list did not.
    items[a].pinned_index = pa;
    items[b].pinned_index = pb;
    true
}

/// An icon being dragged along the row.
///
/// Reordering happens live, one neighbour at a time, rather than all at once on
/// release: the row has to show where the icon will land while it is still in
/// the air, or dropping it is a guess.
struct Drag {
    /// Where it is now in the row — this moves as it passes its neighbours.
    item: usize,
    /// Pointer x when the button went down, to tell a drag from a click.
    start_x: f32,
    /// Pointer x now.
    x: f32,
    /// Set once the pointer has moved far enough to mean it.
    started: bool,
}

/// How far the pointer has to move before a click becomes a drag. Small enough
/// to feel responsive, large enough that a shaky click still launches the app.
const DRAG_THRESHOLD: f32 = 7.0;

/// The name of the icon the pointer is resting on.
struct TooltipState {
    popup: Popup,
    item: usize,
    width: u32,
    height: u32,
    configured: bool,
}

/// An open right-click menu.
struct MenuState {
    popup: Popup,
    rows: Vec<menu::Row>,
    /// Which icon it belongs to.
    item: usize,
    width: u32,
    height: u32,
    configured: bool,
    hovered: Option<usize>,
    /// The pointer has been inside at least once.
    ///
    /// Without this, a menu that opens underneath the pointer can be closed by
    /// the leave event its own appearance caused — which is exactly how an
    /// earlier attempt at this managed to be invisible.
    entered: bool,
}

impl Applet {
    /// Step the springs and re-place the row.
    fn animate(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32();
        self.last_frame = now;

        let row_center = self.span as f32 / 2.0;
        let targets = self
            .metrics
            .target_scales(self.items.len(), self.cursor, row_center);

        let mut moving = false;
        for (spring, target) in self.scales.iter_mut().zip(targets.iter()) {
            moving |= spring.step(*target, dt);
        }

        let current: Vec<f32> = self.scales.iter().map(|s| s.value).collect();
        self.placed = self.metrics.place(&current, self.cursor, row_center);
        self.keep_row_inside();

        // A dragged icon leaves the row and follows the pointer. Everything
        // else stays where the layout put it, so the gap it will drop into is
        // visible the whole time.
        if let Some(drag) = self.drag.as_ref().filter(|d| d.started) {
            if let Some(placed) = self.placed.get_mut(drag.item) {
                placed.center = drag.x;
            }
        }

        // Whose name would be shown, and has the pointer sat still long enough?
        let hovered = self.cursor.and_then(|cursor| self.hit(cursor));
        if hovered != self.hovered {
            self.hovered = hovered;
            self.hover_since = hovered.map(|_| now);
            self.hide_tooltip();
        }

        // Frames have to keep coming while the delay runs down, or the spring
        // settles, the animation stops, and the name never arrives — there is
        // no other clock in here.
        let waiting_to_name = match (self.hovered, self.hover_since, &self.tooltip) {
            (Some(_), Some(since), None) if self.menu.is_none() && self.drag.is_none() => {
                if now.duration_since(since) >= TOOLTIP_DELAY {
                    self.show_tooltip();
                    false
                } else {
                    true
                }
            }
            _ => false,
        };

        self.animating = moving || waiting_to_name;
    }

    /// Show the hovered icon's name.
    fn show_tooltip(&mut self) {
        let Some(index) = self.hovered else { return };
        let (Some(item), Some(placed)) = (self.items.get(index), self.placed.get(index).copied())
        else {
            return;
        };
        let label = item.launcher.name.clone();
        let text = self.text.get_or_insert_with(Text::new);
        let (width, height) = menu::tooltip_size(&label, text);

        let Some(positioner) = self.positioner_for(placed, width, height) else {
            return;
        };

        // A tooltip must not take pointer input. If it did, the pointer would
        // enter it, leave the dock, and the magnification would collapse the
        // moment the name appeared — the tooltip would fight the hover that
        // produced it. An empty input region also stops cosmic-panel taking a
        // grab for it, which is what keeps it from behaving like a menu.
        let surface = self.compositor.create_surface(&self.queue_handle);
        match Region::new(&self.compositor) {
            Ok(empty) => surface.set_input_region(Some(empty.wl_region())),
            Err(err) => log::warn!("could not make the tooltip click-through: {err}"),
        }

        let popup = match Popup::from_surface(
            Some(self.window.xdg_surface()),
            &positioner,
            &self.queue_handle,
            surface,
            &self.xdg_shell,
        ) {
            Ok(popup) => popup,
            Err(err) => {
                log::error!("could not show the name: {err}");
                return;
            }
        };

        self.tooltip = Some(TooltipState {
            popup,
            item: index,
            width,
            height,
            configured: false,
        });
    }

    fn hide_tooltip(&mut self) {
        self.tooltip = None;
    }

    fn draw_tooltip(&mut self) {
        let Some(state) = self.tooltip.as_ref() else {
            return;
        };
        if !state.configured {
            return;
        }
        let Some(item) = self.items.get(state.item) else {
            return;
        };
        let label = item.launcher.name.clone();
        let (logical_w, logical_h) = (state.width, state.height);
        let surface = state.popup.wl_surface().clone();

        let scale = self.scale.max(1);
        let width = logical_w * scale as u32;
        let height = logical_h * scale as u32;

        let Some(mut logical) = tiny_skia::Pixmap::new(logical_w, logical_h) else {
            return;
        };
        let theme = self.theme.clone();
        let text = self.text.get_or_insert_with(Text::new);
        menu::draw_tooltip(&mut logical, &theme, &label, text);

        let Ok((buffer, canvas)) = self.pool.create_buffer(
            width as i32,
            height as i32,
            width as i32 * 4,
            wl_shm::Format::Argb8888,
        ) else {
            return;
        };
        blit(canvas, &logical, scale as f32, width, height);
        surface.set_buffer_scale(scale);
        surface.damage_buffer(0, 0, width as i32, height as i32);
        if buffer.attach_to(&surface).is_ok() {
            surface.commit();
        }
    }

    /// A positioner that puts a `width`x`height` surface just outside the bar,
    /// lined up with one icon.
    fn positioner_for(&self, placed: Placed, width: u32, height: u32) -> Option<XdgPositioner> {
        let positioner = match XdgPositioner::new(&self.xdg_shell) {
            Ok(positioner) => positioner,
            Err(err) => {
                log::error!("no positioner: {err}");
                return None;
            }
        };
        positioner.set_size(width as i32, height as i32);

        let along = placed.left().max(0.0) as i32;
        let size_along = placed.size as i32;
        if self.vertical {
            positioner.set_anchor_rect(0, along, self.thickness as i32, size_along);
        } else {
            positioner.set_anchor_rect(along, 0, size_along, self.thickness as i32);
        }

        // Out of the bar, away from the screen edge it is on — down from a dock
        // at the top, out to the right from one on the left. Anywhere else and
        // it opens off the side of the screen.
        let (anchor, gravity, adjust) = match self.host.anchor {
            panel::Anchor::Top => (
                XdgAnchor::Bottom,
                Gravity::Bottom,
                ConstraintAdjustment::SlideX | ConstraintAdjustment::FlipY,
            ),
            panel::Anchor::Bottom => (
                XdgAnchor::Top,
                Gravity::Top,
                ConstraintAdjustment::SlideX | ConstraintAdjustment::FlipY,
            ),
            panel::Anchor::Left => (
                XdgAnchor::Right,
                Gravity::Right,
                ConstraintAdjustment::SlideY | ConstraintAdjustment::FlipX,
            ),
            panel::Anchor::Right => (
                XdgAnchor::Left,
                Gravity::Left,
                ConstraintAdjustment::SlideY | ConstraintAdjustment::FlipX,
            ),
        };
        positioner.set_anchor(anchor);
        positioner.set_gravity(gravity);
        positioner.set_constraint_adjustment(adjust);
        Some(positioner)
    }

    /// Swap two neighbouring icons, in the row and in the pinned list at once.
    ///
    /// The pinned list can hold applications that are not installed, so the two
    /// entries being exchanged are found by the index each icon remembers
    /// rather than by counting along the row. Those uninstalled entries keep
    /// their place, which is the only way a reorder here does not quietly
    /// rearrange somebody else's list.
    fn swap_items(&mut self, a: usize, b: usize) {
        if swap_pinned(&mut self.pinned, &mut self.items, a, b) {
            self.scales.swap(a, b);
        }
    }

    /// Move a dragged icon past whichever neighbour the pointer has reached.
    fn drag_to(&mut self, x: f32) {
        let Some(drag) = self.drag.as_mut() else {
            return;
        };
        if !drag.started && (x - drag.start_x).abs() >= DRAG_THRESHOLD {
            drag.started = true;
        }
        drag.x = x;
        if !drag.started {
            return;
        }

        // Only once the pointer is properly past a neighbour's centre. Swapping
        // as soon as the icons overlap makes the two trade places back and
        // forth while the pointer sits still between them.
        let item = drag.item;
        let left = item
            .checked_sub(1)
            .filter(|i| self.placed.get(*i).is_some_and(|p| x < p.center));
        let right = Some(item + 1)
            .filter(|i| *i < self.items.len())
            .filter(|i| self.placed.get(*i).is_some_and(|p| x > p.center));

        if let Some(target) = left.or(right) {
            self.swap_items(item, target);
            if let Some(drag) = self.drag.as_mut() {
                drag.item = target;
            }
        }
    }

    /// Let go. Writes the new order only if anything actually moved.
    fn end_drag(&mut self) -> bool {
        let Some(drag) = self.drag.take() else {
            return false;
        };
        if drag.started {
            self.save_pinned();
        }
        drag.started
    }

    /// Shift the row back inside the surface if the pointer-anchored layout has
    /// pushed it past an edge.
    ///
    /// The surface is wide enough for the widest row, but the row is anchored
    /// to the pointer, so at the extremes it can sit off-centre far enough to
    /// overhang. Anything outside the surface is not clipped by the panel — it
    /// simply is not there, and the end icon would lose a slice of itself.
    fn keep_row_inside(&mut self) {
        let (Some(first), Some(last)) = (self.placed.first(), self.placed.last()) else {
            return;
        };
        let left = first.left();
        let right = last.left() + last.size;
        let shift = if left < 0.0 {
            -left
        } else if right > self.span as f32 {
            self.span as f32 - right
        } else {
            return;
        };
        for placed in &mut self.placed {
            placed.center += shift;
        }
    }

    /// A pointer position reduced to one number: how far along the bar it is.
    ///
    /// Every hit test, the magnification curve and the drag all take this and
    /// nothing else, which is why a vertical dock needed no second copy of any
    /// of them.
    fn along(&self, position: (f64, f64)) -> f32 {
        if self.vertical {
            position.1 as f32
        } else {
            position.0 as f32
        }
    }

    /// Which icon is under a given point along the bar, if any.
    fn hit(&self, along: f32) -> Option<usize> {
        self.metrics.hit(&self.placed, along)
    }

    /// Which icons have at least one window open, in row order.
    fn running_flags(&self) -> Vec<bool> {
        let Some(windows) = self.windows.as_ref() else {
            return vec![false; self.items.len()];
        };
        self.items
            .iter()
            .map(|item| !windows.matching(&item.launcher).is_empty())
            .collect()
    }

    /// The titles of an icon's open windows.
    fn window_labels(&self, index: usize) -> Vec<String> {
        let (Some(windows), Some(item)) = (self.windows.as_ref(), self.items.get(index)) else {
            return Vec::new();
        };
        windows
            .matching(&item.launcher)
            .iter()
            .map(|window| window.label().to_string())
            .collect()
    }

    /// Focus one of an icon's windows. Returns false if it could not be done.
    fn focus_window(&mut self, item: usize, which: usize) -> bool {
        let (Some(windows), Some(item)) = (self.windows.as_ref(), self.items.get(item)) else {
            return false;
        };
        let matching = windows.matching(&item.launcher);
        let Some(window) = matching.get(which) else {
            return false;
        };
        let window = (*window).clone();
        windows.activate(&window)
    }

    /// A left click on an icon.
    ///
    /// Nothing open starts it. One window open raises that window rather than
    /// starting a second copy — which is what makes a dock a dock and not a
    /// row of shortcuts. Several, and the choice is yours, so the window list
    /// opens instead of one being picked for you.
    fn activate_item(&mut self, index: usize) {
        let count = self.window_labels(index).len();
        match count {
            0 => self.launch(index),
            1 => {
                if !self.focus_window(index, 0) {
                    // No way to raise it — starting it again beats a click that
                    // appears to do nothing at all.
                    self.launch(index);
                }
            }
            _ => self.open_menu(index),
        }
    }

    fn launch(&mut self, index: usize) {
        let Some(item) = self.items.get(index) else {
            return;
        };
        if let Err(err) = item.launcher.launch() {
            log::error!("could not start {}: {err:#}", item.launcher.name);
        }
    }

    /// The window list changed: redraw so the dots follow it.
    fn windows_changed(&mut self) {
        self.animating = true;
        self.draw();
    }

    /// Open the right-click menu under an icon.
    ///
    /// The popup is a real `xdg_popup` on our own surface. cosmic-panel turns
    /// it into a popup of the panel's layer surface on the host compositor and
    /// takes a grab there, which is what dismisses it when the pointer goes
    /// somewhere else — so there is no "the pointer left, close it" rule here.
    /// That rule is exactly what made an earlier dock's menu vanish the instant
    /// it opened, because it opened underneath the pointer.
    fn open_menu(&mut self, index: usize) {
        self.close_menu();
        self.hide_tooltip();

        // The window list is read first because it borrows self immutably and
        // the launcher borrow below outlives it.
        let windows = self.window_labels(index);
        let Some(item) = self.items.get(index) else {
            return;
        };
        let rows = menu::rows(&item.launcher, &windows, index, self.items.len());
        let text = self.text.get_or_insert_with(Text::new);
        let (width, height) = menu::size(&rows, text);

        let Some(placed) = self.placed.get(index).copied() else {
            return;
        };
        // Anchored to the icon, not to the pointer: the menu should line up
        // with the thing it belongs to however the pointer arrived.
        let Some(positioner) = self.positioner_for(placed, width, height) else {
            return;
        };

        let popup = match Popup::new(
            self.window.xdg_surface(),
            &positioner,
            &self.queue_handle,
            &self.compositor,
            &self.xdg_shell,
        ) {
            Ok(popup) => popup,
            Err(err) => {
                log::error!("could not open the menu: {err}");
                return;
            }
        };

        self.menu = Some(MenuState {
            popup,
            rows,
            item: index,
            width,
            height,
            configured: false,
            hovered: None,
            entered: false,
        });
    }

    fn close_menu(&mut self) {
        // Dropping the Popup destroys the protocol object, which is what tells
        // the compositor it is gone.
        self.menu = None;
    }

    fn draw_menu(&mut self) {
        let Some(state) = self.menu.as_mut() else {
            return;
        };
        if !state.configured {
            return;
        }

        let scale = self.scale.max(1);
        let width = state.width * scale as u32;
        let height = state.height * scale as u32;

        let Some(mut logical) = tiny_skia::Pixmap::new(state.width, state.height) else {
            return;
        };
        let text = self.text.get_or_insert_with(Text::new);
        menu::draw(&mut logical, &self.theme, &state.rows, state.hovered, text);

        let Ok((buffer, canvas)) = self.pool.create_buffer(
            width as i32,
            height as i32,
            width as i32 * 4,
            wl_shm::Format::Argb8888,
        ) else {
            log::error!("could not get a buffer for the menu");
            return;
        };
        blit(canvas, &logical, scale as f32, width, height);

        let surface = state.popup.wl_surface().clone();
        surface.set_buffer_scale(scale);
        surface.damage_buffer(0, 0, width as i32, height as i32);
        if let Err(err) = buffer.attach_to(&surface) {
            log::error!("could not attach the menu buffer: {err}");
            return;
        }
        surface.commit();
    }

    /// Carry out a menu row and close the menu.
    fn run_menu_action(&mut self, row: usize) {
        let Some(state) = self.menu.as_ref() else {
            return;
        };
        let Some(action) = state.rows.get(row).map(|r| r.action) else {
            return;
        };
        let item_index = state.item;
        self.close_menu();

        let Some(item) = self.items.get(item_index) else {
            return;
        };
        match action {
            menu::Action::Focus(which) => {
                if !self.focus_window(item_index, which) {
                    log::warn!("this compositor offers no way to raise a window");
                }
            }
            menu::Action::Launch => {
                if let Err(err) = item.launcher.launch() {
                    log::error!("could not start {}: {err:#}", item.launcher.name);
                }
            }
            menu::Action::RunAction(index) => {
                if let Err(err) = item.launcher.launch_action(index) {
                    log::error!("could not run that action: {err:#}");
                }
            }
            menu::Action::MoveLeft | menu::Action::MoveRight => {
                let other = if action == menu::Action::MoveLeft {
                    item_index.checked_sub(1)
                } else {
                    Some(item_index + 1).filter(|i| *i < self.items.len())
                };
                let Some(other) = other else { return };
                let (a, b) = (
                    self.items[item_index].pinned_index,
                    self.items[other].pinned_index,
                );
                self.pinned.swap(a, b);
                self.save_pinned();
            }
            menu::Action::Unpin => {
                let at = item.pinned_index;
                self.pinned.remove(at);
                self.save_pinned();
            }
        }
    }

    /// Write the pinned list back where it came from and rebuild the row.
    fn save_pinned(&mut self) {
        match self.pin_source {
            PinSource::Cosmic => {
                if let Err(err) = launchers::write_favourites(&self.pinned) {
                    log::error!("could not update COSMIC's favourites: {err}");
                    return;
                }
            }
            PinSource::Config => {
                self.config.pinned = self.pinned.clone();
                if let Err(err) = self.config.save() {
                    log::error!("could not save the config: {err:#}");
                    return;
                }
            }
        }
        self.rebuild();
    }

    /// Take a new limit on how long the row may be and rebuild to fit it.
    fn set_max_span(&mut self, limit: u32) {
        if self.max_span == Some(limit) {
            return;
        }
        self.max_span = Some(limit);

        // Start from the configured sizes every time rather than from whatever
        // the last limit left behind: a bar that gets *more* room back has to
        // grow again, not stay shrunk for the life of the process.
        let mut metrics = self.config.metrics(self.host.size.icon_size());
        fit_magnification(&mut metrics, self.thickness);
        let shrunk = fit_to_length(&mut metrics, self.items.len(), limit);
        if shrunk {
            log::info!(
                "row shrunk to {:.0} px icons to fit {limit} px of bar",
                metrics.icon_size
            );
        }

        if metrics.icon_size != self.metrics.icon_size {
            self.icons
                .resize((metrics.max_icon_size().ceil() as u32).max(16));
        }
        self.metrics = metrics;
        self.resize_surface();
    }

    /// Recompute the surface from the current metrics and item count.
    fn resize_surface(&mut self) {
        self.span = row_span(&self.metrics, self.items.len());
        if let Some(limit) = self.max_span {
            self.span = self.span.min(limit.max(1));
        }
        let (width, height) = surface_size(self.span, self.thickness, self.vertical);
        self.width = width;
        self.height = height;
        self.window
            .xdg_surface()
            .set_window_geometry(0, 0, width as i32, height as i32);
        self.animating = true;
        self.draw();
    }

    /// Re-read the pinned list and resize the surface to match.
    ///
    /// Only the row's *length* changes here, never its thickness, so the panel
    /// re-flows its bar sideways and no window on the screen moves.
    fn rebuild(&mut self) {
        let (source, pinned, items) = pinned_items(&self.config);
        self.pin_source = source;
        self.pinned = pinned;
        self.items = items;
        self.scales = self.items.iter().map(|_| Spring::new(1.0)).collect();
        self.placed.clear();
        // The item count changed, so the limit has to be reapplied against the
        // new count rather than the old one.
        if let Some(limit) = self.max_span {
            self.max_span = None;
            self.set_max_span(limit);
        } else {
            self.resize_surface();
        }
    }

    fn draw(&mut self) {
        if !self.configured || self.width == 0 {
            return;
        }

        self.animate();

        let qh = self.queue_handle.clone();
        let scale = self.scale.max(1) as f32;
        let width = (self.width as f32 * scale) as u32;
        let height = (self.height as f32 * scale) as u32;

        let Some(mut logical) = tiny_skia::Pixmap::new(self.width, self.height) else {
            return;
        };

        // Icons are centre-locked rather than resting on a baseline: inside a
        // panel the neighbouring applets are centred across the bar, and a row
        // that grew in one direction only would drift out of line with them the
        // moment anything was hovered.
        let center_line = self.thickness as f32 / 2.0;
        let panel = if self.config.plate {
            self.metrics.background(&self.placed)
        } else {
            None
        };

        {
            let scene = render::Scene {
                theme: &self.theme,
                panel,
                baseline: center_line + self.metrics.icon_size / 2.0,
                panel_height: self.metrics.icon_size + self.metrics.padding * 2.0,
                glass: self.config.glass,
                indicator: self.config.indicator,
                flipped: self.host.anchor == panel::Anchor::Top,
                center_line: Some(center_line),
                vertical: self.vertical,
                edge_at_zero: matches!(
                    self.host.anchor,
                    panel::Anchor::Top | panel::Anchor::Left
                ),
            };

            let running = self.running_flags();
            let mut items: Vec<render::Item> = self
                .items
                .iter()
                .zip(self.placed.iter())
                .zip(self.scales.iter())
                .enumerate()
                .map(|(index, ((item, placed), spring))| render::Item {
                    icon: item.launcher.icon.as_deref(),
                    placed: *placed,
                    emphasis: ((spring.value - 1.0)
                        / (self.metrics.magnification - 1.0).max(0.001))
                    .clamp(0.0, 1.0),
                    running: running.get(index).copied().unwrap_or(false),
                })
                .collect();

            render::draw(&mut logical, &scene, &mut items, &mut self.icons);
        }

        let surface = self.window.wl_surface().clone();
        let Ok((buffer, canvas)) = self.pool.create_buffer(
            width as i32,
            height as i32,
            width as i32 * 4,
            wl_shm::Format::Argb8888,
        ) else {
            log::error!("could not get a buffer from the pool");
            return;
        };
        blit(canvas, &logical, scale, width, height);

        surface.set_buffer_scale(self.scale.max(1));
        surface.damage_buffer(0, 0, width as i32, height as i32);

        // Another frame only while something is still moving. An idle dock must
        // cost nothing, or it is a background process that spins.
        if self.animating && !self.frame_pending {
            surface.frame(&qh, surface.clone());
            self.frame_pending = true;
        }

        if let Err(err) = buffer.attach_to(&surface) {
            log::error!("could not attach the buffer: {err}");
            return;
        }
        surface.commit();
    }

    fn request_frame(&mut self) {
        if self.frame_pending {
            return;
        }
        self.animating = true;
        self.draw();
    }
}

/// tiny-skia gives us premultiplied RGBA; `Argb8888` on a little-endian machine
/// wants the bytes the other way round.
fn blit(canvas: &mut [u8], logical: &tiny_skia::Pixmap, scale: f32, width: u32, height: u32) {
    let scaled;
    let source = if (scale - 1.0).abs() > f32::EPSILON {
        let Some(mut buffer) = tiny_skia::Pixmap::new(width, height) else {
            return;
        };
        buffer.draw_pixmap(
            0,
            0,
            logical.as_ref(),
            &tiny_skia::PixmapPaint {
                quality: tiny_skia::FilterQuality::Bicubic,
                ..Default::default()
            },
            tiny_skia::Transform::from_scale(scale, scale),
            None,
        );
        scaled = buffer;
        &scaled
    } else {
        logical
    };

    let _ = height;
    for (out, pixel) in canvas.chunks_exact_mut(4).zip(source.pixels()) {
        out[0] = pixel.blue();
        out[1] = pixel.green();
        out[2] = pixel.red();
        out[3] = pixel.alpha();
    }
}

impl CompositorHandler for Applet {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        factor: i32,
    ) {
        if factor != self.scale {
            self.scale = factor.max(1);
            self.draw();
        }
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.frame_pending = false;
        if self.animating {
            self.draw();
        }
    }

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        output: &wl_output::WlOutput,
    ) {
        // A backstop for when the panel never gets round to telling us. Better
        // a row that is slightly too small than icons nobody can reach.
        if self.max_span.is_some() {
            return;
        }
        let Some(info) = self.output_state.info(output) else {
            return;
        };
        let Some((w, h)) = info.logical_size else {
            return;
        };
        let length = if self.vertical { h } else { w };
        if length > 0 {
            // Leave room for the applets sharing the bar with us.
            let limit = (length as u32).saturating_sub(self.thickness * 4).max(64);
            self.set_max_span(limit);
        }
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl WindowHandler for Applet {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &Window,
        configure: WindowConfigure,
        _serial: u32,
    ) {
        // The panel says how much room it has for us by way of bounds — that is
        // its request that we shrink, and ignoring it is why a long row ran off
        // the end of a vertical dock.
        if let Some((w, h)) = configure.suggested_bounds {
            let limit = if self.vertical { h } else { w };
            if limit > 0 {
                self.set_max_span(limit);
            }
        }
        // The panel sends no size: applets choose their own, and ours is fixed
        // for the life of the process so a magnifying icon never resizes the
        // bar. The configure is still the signal that we may draw.
        //
        // The geometry is restated on every configure rather than only at
        // startup. It is what the panel measures to decide how thick its bar
        // has to be, and a geometry set before the surface had a buffer is the
        // kind of thing a compositor is within its rights to ignore.
        self.window
            .xdg_surface()
            .set_window_geometry(0, 0, self.width as i32, self.height as i32);
        self.configured = true;
        self.draw();
    }
}

impl SeatHandler for Applet {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer {
            if let Err(err) = self.seat_state.get_pointer(qh, &seat) {
                log::warn!("no pointer on this seat: {err}");
            }
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        _: Capability,
    ) {
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for Applet {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        let ours = self.window.wl_surface().clone();
        let menu_surface = self.menu.as_ref().map(|m| m.popup.wl_surface().clone());
        let mut changed = false;
        let mut menu_changed = false;

        for event in events {
            if Some(&event.surface) == menu_surface.as_ref() {
                match event.kind {
                    PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                        let hovered = self
                            .menu
                            .as_ref()
                            .and_then(|m| menu::hit(&m.rows, event.position.1 as f32));
                        if let Some(state) = self.menu.as_mut() {
                            state.entered = true;
                            if state.hovered != hovered {
                                state.hovered = hovered;
                                menu_changed = true;
                            }
                        }
                    }
                    PointerEventKind::Leave { .. } => {
                        // Moving off a menu that has been touched dismisses it.
                        // Dismissal must not depend on a click landing: if the
                        // click path is broken for any reason the menu becomes
                        // impossible to get rid of, which is far worse than a
                        // menu that closes a little eagerly.
                        if self.menu.as_ref().is_some_and(|state| state.entered) {
                            self.close_menu();
                            menu_changed = false;
                        }
                    }
                    PointerEventKind::Release { button, .. }
                        if button == BTN_LEFT || button == BTN_RIGHT =>
                    {
                        // Acting on release, not press, so the press that opened
                        // the menu cannot also pick the row it landed on.
                        let row = self
                            .menu
                            .as_ref()
                            .and_then(|m| menu::hit(&m.rows, event.position.1 as f32));
                        match row {
                            Some(row) => self.run_menu_action(row),
                            None => self.close_menu(),
                        }
                        menu_changed = false;
                    }
                    PointerEventKind::Press { .. } => {
                        // A press on the menu's own padding is a miss, and a
                        // miss should dismiss it rather than do nothing.
                        let row = self
                            .menu
                            .as_ref()
                            .and_then(|m| menu::hit(&m.rows, event.position.1 as f32));
                        if row.is_none() {
                            self.close_menu();
                            menu_changed = false;
                        }
                    }
                    _ => {}
                }
                continue;
            }

            if event.surface != ours {
                continue;
            }
            match event.kind {
                // Any press on the dock while a menu is open dismisses it and
                // does nothing else. Relying on the compositor's popup grab
                // alone left menus that could not be got rid of.
                PointerEventKind::Press { .. } if self.menu.is_some() => {
                    self.close_menu();
                    self.pressed = None;
                    self.drag = None;
                }
                // Coming back onto the dock closes it too. Between this and the
                // leave above, any movement of the pointer gets you out.
                PointerEventKind::Enter { .. } if self.menu.is_some() => {
                    self.close_menu();
                    self.cursor = Some(self.along(event.position));
                    changed = true;
                }
                PointerEventKind::Press { button, .. } if button == BTN_RIGHT => {
                    match self.hit(self.along(event.position)) {
                        Some(index) => self.open_menu(index),
                        None => self.close_menu(),
                    }
                }
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    let x = self.along(event.position);
                    self.cursor = Some(x);
                    if self.drag.is_some() {
                        self.drag_to(x);
                    }
                    changed = true;
                }
                PointerEventKind::Leave { .. } => {
                    self.cursor = None;
                    self.pressed = None;
                    // Letting go outside the dock keeps wherever it got to,
                    // rather than snapping it back to a place the pointer has
                    // already left.
                    self.end_drag();
                    changed = true;
                }
                PointerEventKind::Press { button, .. } if button == BTN_LEFT => {
                    let x = self.along(event.position);
                    self.pressed = self.hit(x);
                    self.drag = self.pressed.map(|item| Drag {
                        item,
                        start_x: x,
                        x,
                        started: false,
                    });
                }
                PointerEventKind::Release { button, .. } if button == BTN_LEFT => {
                    // A drag that moved something is not also a click, or every
                    // reorder would launch the application it moved.
                    let dragged = self.end_drag();
                    let released = self.hit(self.along(event.position));
                    if !dragged {
                        // A click is a press and a release on the same icon, so
                        // a press that slides off does not launch anything.
                        if let (Some(pressed), Some(released)) = (self.pressed, released) {
                            if pressed == released {
                                self.activate_item(pressed);
                            }
                        }
                    }
                    self.pressed = None;
                    changed = true;
                }
                _ => {}
            }
        }

        if menu_changed {
            self.draw_menu();
        }
        if changed {
            self.request_frame();
        }
    }
}

impl PopupHandler for Applet {
    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, popup: &Popup, config: PopupConfigure) {
        if let Some(tip) = self.tooltip.as_mut() {
            if &tip.popup == popup {
                if matches!(config.kind, ConfigureKind::Initial | ConfigureKind::Reactive) {
                    tip.width = (config.width as u32).max(1);
                    tip.height = (config.height as u32).max(1);
                }
                tip.configured = true;
                self.draw_tooltip();
                return;
            }
        }
        let Some(state) = self.menu.as_mut() else {
            return;
        };
        if &state.popup != popup {
            return;
        }
        // The compositor may have moved or resized the menu to keep it on the
        // screen. Its size is authoritative from here on, not the size we asked
        // for, or the buffer and the surface would disagree.
        if matches!(config.kind, ConfigureKind::Initial | ConfigureKind::Reactive) {
            state.width = (config.width as u32).max(1);
            state.height = (config.height as u32).max(1);
        }
        state.configured = true;
        self.draw_menu();
    }

    fn done(&mut self, _: &Connection, _: &QueueHandle<Self>, popup: &Popup) {
        // The compositor dismissed it — a click somewhere else, a key, a
        // workspace switch. It is already gone as far as the user is concerned.
        if self.menu.as_ref().is_some_and(|state| &state.popup == popup) {
            self.menu = None;
        }
        if self.tooltip.as_ref().is_some_and(|tip| &tip.popup == popup) {
            self.tooltip = None;
        }
    }
}

impl OutputHandler for Applet {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl ShmHandler for Applet {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for Applet {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(Applet);
delegate_output!(Applet);
delegate_seat!(Applet);
delegate_pointer!(Applet);
delegate_shm!(Applet);
delegate_xdg_shell!(Applet);
delegate_xdg_window!(Applet);
delegate_xdg_popup!(Applet);
delegate_registry!(Applet);

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics() -> Metrics {
        Metrics {
            icon_size: 48.0,
            spacing: 10.0,
            magnification: 1.8,
            reach: 1.3,
            padding: 8.0,
        }
    }

    #[test]
    fn the_surface_is_wide_enough_for_the_widest_the_row_can_get() {
        let metrics = metrics();
        let count = 8;
        let width = row_span(&metrics, count) as f32;

        // Every pointer position the row can be laid out for has to fit.
        let resting = metrics.resting_width(count);
        for step in 0..=200 {
            let cursor = resting * step as f32 / 200.0;
            let scales = metrics.target_scales(count, Some(cursor), width / 2.0);
            let placed = metrics.place(&scales, Some(cursor), width / 2.0);
            let span = placed.last().unwrap().left() + placed.last().unwrap().size
                - placed.first().unwrap().left();
            assert!(
                span <= width + 0.5,
                "row spans {span} in a {width} surface at cursor {cursor}"
            );
        }
    }

    fn item(pinned_index: usize, id: &str) -> Item {
        Item {
            pinned_index,
            launcher: Launcher {
                id: id.into(),
                name: id.into(),
                icon: None,
                exec: "true".into(),
                startup_class: None,
                terminal: false,
                path: std::path::PathBuf::from("/dev/null"),
                actions: Vec::new(),
            },
        }
    }

    #[test]
    fn reordering_moves_the_right_entries_in_the_pinned_list() {
        let mut pinned: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        let mut items = vec![item(0, "a"), item(1, "b"), item(2, "c")];

        assert!(swap_pinned(&mut pinned, &mut items, 0, 1));
        assert_eq!(pinned, ["b", "a", "c"]);
        assert_eq!(
            items.iter().map(|i| i.launcher.id.as_str()).collect::<Vec<_>>(),
            ["b", "a", "c"]
        );
        // Every icon still points at its own entry, or the next swap corrupts
        // the list.
        for (slot, item) in items.iter().enumerate() {
            assert_eq!(pinned[item.pinned_index], item.launcher.id, "slot {slot}");
        }
    }

    #[test]
    fn an_uninstalled_application_keeps_its_place_in_the_list() {
        // "ghost" is pinned but not installed, so it is not in the row at all.
        // Dragging around it must not move or lose it.
        let mut pinned: Vec<String> = ["a", "ghost", "b"].iter().map(|s| s.to_string()).collect();
        let mut items = vec![item(0, "a"), item(2, "b")];

        assert!(swap_pinned(&mut pinned, &mut items, 0, 1));
        assert_eq!(pinned, ["b", "ghost", "a"]);
        assert_eq!(pinned[1], "ghost");
        for item in &items {
            assert_eq!(pinned[item.pinned_index], item.launcher.id);
        }
    }

    #[test]
    fn swapping_an_icon_with_itself_or_off_the_end_does_nothing() {
        let mut pinned: Vec<String> = ["a", "b"].iter().map(|s| s.to_string()).collect();
        let mut items = vec![item(0, "a"), item(1, "b")];
        assert!(!swap_pinned(&mut pinned, &mut items, 1, 1));
        assert!(!swap_pinned(&mut pinned, &mut items, 0, 9));
        assert_eq!(pinned, ["a", "b"]);
    }

    #[test]
    fn repeated_swaps_stay_consistent() {
        // A drag is a run of single swaps; the invariant has to survive all of
        // them, not just the first.
        let mut pinned: Vec<String> =
            ["a", "b", "c", "d"].iter().map(|s| s.to_string()).collect();
        let mut items = vec![item(0, "a"), item(1, "b"), item(2, "c"), item(3, "d")];

        // Walk "a" all the way to the end.
        for slot in 0..3 {
            assert!(swap_pinned(&mut pinned, &mut items, slot, slot + 1));
        }
        assert_eq!(
            items.iter().map(|i| i.launcher.id.as_str()).collect::<Vec<_>>(),
            ["b", "c", "d", "a"]
        );
        assert_eq!(pinned, ["b", "c", "d", "a"]);
        for item in &items {
            assert_eq!(pinned[item.pinned_index], item.launcher.id);
        }
    }

    #[test]
    fn a_row_too_long_for_the_bar_is_shrunk_until_it_fits() {
        // The case that matters: twenty icons down the side of a laptop screen.
        let mut m = metrics();
        let count = 20;
        assert!(row_span(&m, count) > 900, "the test premise is wrong");

        assert!(fit_to_length(&mut m, count, 900));
        assert!(
            row_span(&m, count) <= 900,
            "still {} px in a 900 px bar",
            row_span(&m, count)
        );
        // Shrunk in proportion, not squashed into each other.
        assert!(m.icon_size < metrics().icon_size);
        assert!(m.spacing < metrics().spacing);
        assert!(m.icon_size >= 16.0, "shrunk past being clickable");
    }

    #[test]
    fn a_row_that_already_fits_is_left_alone() {
        let mut m = metrics();
        let before = (m.icon_size, m.spacing);
        assert!(!fit_to_length(&mut m, 5, 10_000));
        assert_eq!((m.icon_size, m.spacing), before);

        // And nonsense limits do not silently shrink it to nothing.
        assert!(!fit_to_length(&mut m, 5, 0));
        assert!(!fit_to_length(&mut m, 0, 100));
        assert_eq!((m.icon_size, m.spacing), before);
    }

    #[test]
    fn even_an_impossible_bar_leaves_usable_icons() {
        // Forty icons in 100 px cannot fit, and the answer is still icons you
        // could hit rather than a row of one-pixel slivers.
        let mut m = metrics();
        fit_to_length(&mut m, 40, 100);
        assert!(m.icon_size >= 16.0);
    }

    #[test]
    fn a_vertical_bar_swaps_the_surface_dimensions() {
        assert_eq!(surface_size(300, 64, false), (300, 64));
        assert_eq!(surface_size(300, 64, true), (64, 300));
    }

    #[test]
    fn an_empty_row_still_has_a_legal_surface() {
        // A zero-width surface is a protocol error, not an empty dock.
        assert!(row_span(&metrics(), 0) >= 1);
    }

    #[test]
    fn the_surface_never_exceeds_what_the_panel_will_accept() {
        for size in [
            panel::PanelSize::Xs,
            panel::PanelSize::S,
            panel::PanelSize::M,
            panel::PanelSize::L,
            panel::PanelSize::Xl,
            panel::PanelSize::Custom(40),
        ] {
            let host = Host {
                size,
                anchor: panel::Anchor::Top,
                spacing: 4.0,
            };
            // Even someone who asks for a great deal of extra room cannot push
            // the surface past what the panel will accept.
            let config = Config {
                extra_height: 500.0,
                ..Config::default()
            };
            let metrics = config.metrics(size.icon_size());
            let height = row_thickness(&metrics, &host, &config) as f32;
            assert!(
                height < size.max_thickness(),
                "{size:?}: {height} px surface against a {} px ceiling",
                size.max_thickness()
            );
        }
    }

    #[test]
    fn asking_for_no_extra_height_costs_no_screen_at_all() {
        // This is the one that matters on a real desktop: the panel reserves
        // its bar's thickness, so a taller applet pushes every window down.
        // Whatever the default is, zero has to mean *exactly* zero.
        for size in [
            panel::PanelSize::S,
            panel::PanelSize::M,
            panel::PanelSize::L,
            panel::PanelSize::Xl,
        ] {
            let host = Host {
                size,
                anchor: panel::Anchor::Top,
                spacing: 4.0,
            };
            let config = Config {
                extra_height: 0.0,
                ..Config::default()
            };
            let metrics = config.metrics(size.icon_size());
            let unit = metrics.icon_size + size.padding() * 2.0;
            assert_eq!(
                row_thickness(&metrics, &host, &config) as f32,
                unit,
                "{size:?} would move every window on the screen"
            );
        }
    }

    #[test]
    fn magnification_is_clamped_to_what_the_surface_can_show() {
        let host = Host {
            size: panel::PanelSize::L,
            anchor: panel::Anchor::Top,
            spacing: 4.0,
        };
        let config = Config::default();
        let mut metrics = config.metrics(host.size.icon_size());
        let height = row_thickness(&metrics, &host, &config);
        fit_magnification(&mut metrics, height);

        assert!(
            metrics.max_icon_size() <= height as f32,
            "a {:.0} px icon does not fit in {height} px",
            metrics.max_icon_size()
        );
        // And out of the box it is a magnification you can actually see. 1.3x
        // reads as the icon twitching; a dock effect starts around 1.5x.
        assert!(
            metrics.magnification >= 1.5,
            "only {:.2}x by default",
            metrics.magnification
        );
    }

    #[test]
    fn extra_height_buys_a_bigger_effect() {
        let host = Host {
            size: panel::PanelSize::L,
            anchor: panel::Anchor::Top,
            spacing: 4.0,
        };
        let generous = Config {
            extra_height: 40.0,
            ..Config::default()
        };
        let mut metrics = generous.metrics(host.size.icon_size());
        let height = row_thickness(&metrics, &host, &generous);
        fit_magnification(&mut metrics, height);
        // 30 px of room is enough for the configured 1.8x to survive intact.
        assert_eq!(metrics.magnification, generous.magnification);
    }
}
