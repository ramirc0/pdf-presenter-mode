//! Keeping the screen awake during a talk, through the Wayland idle inhibit protocol.

use wayland_client::backend::{Backend, ObjectId};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, delegate_noop};
use wayland_protocols::wp::idle_inhibit::zv1::client::zwp_idle_inhibit_manager_v1::ZwpIdleInhibitManagerV1;
use wayland_protocols::wp::idle_inhibit::zv1::client::zwp_idle_inhibitor_v1::ZwpIdleInhibitorV1;
use winit::raw_window_handle::{
    HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle,
};

/// The compositor's promise to keep the screen awake while a window shows.
/// It lasts until ppm exits, which ends it.
pub struct Awake {
    _connection: Connection,
    _queue: EventQueue<IdleQueue>,
    _inhibitor: ZwpIdleInhibitorV1,
}

/// The state behind ppm's own Wayland event queue. The idle inhibit objects send no events.
struct IdleQueue;

impl Awake {
    /// Asks the compositor to keep the screen awake while `window` is visible.
    ///
    /// # Errors
    ///
    /// Fails if `window` is not a Wayland window, or if the compositor lacks the protocol.
    pub fn keep(window: &(impl HasWindowHandle + HasDisplayHandle)) -> anyhow::Result<Self> {
        let (RawDisplayHandle::Wayland(display), RawWindowHandle::Wayland(surface)) = (
            window.display_handle()?.as_raw(),
            window.window_handle()?.as_raw(),
        ) else {
            anyhow::bail!("the window is not a Wayland window");
        };
        // SAFETY: the display is winit's connection, which lives until `main` returns, after the app.
        let backend = unsafe { Backend::from_foreign_display(display.display.as_ptr().cast()) };
        let connection = Connection::from_backend(backend);
        // SAFETY: the pointer is the live `wl_surface` of the window, made by winit on that connection.
        let surface =
            unsafe { ObjectId::from_ptr(WlSurface::interface(), surface.surface.as_ptr().cast()) }?;
        let surface = WlSurface::from_id(&connection, surface)?;
        let (globals, queue) = registry_queue_init::<IdleQueue>(&connection)?;
        let manager: ZwpIdleInhibitManagerV1 = globals.bind(&queue.handle(), 1..=1, ())?;
        let inhibitor = manager.create_inhibitor(&surface, &queue.handle(), ());
        connection.flush()?;
        Ok(Self {
            _connection: connection,
            _queue: queue,
            _inhibitor: inhibitor,
        })
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for IdleQueue {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

delegate_noop!(IdleQueue: ZwpIdleInhibitManagerV1);
delegate_noop!(IdleQueue: ZwpIdleInhibitorV1);
