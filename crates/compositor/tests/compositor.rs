//! Headless end-to-end tests: a real, in-process Wayland client connects to the
//! compositor over its socket and exercises the protocol, with no display. The
//! server runs on the test thread (pumped a batch at a time) and the client on
//! another; they talk over the Unix socket exactly as a separate process would.
//!
//! The client's blocking calls (`registry_queue_init`, `roundtrip`) only return
//! once the server replies, and the server only replies while we pump it, so the
//! test thread pumps continuously while the client is mid-call.

#![cfg(target_os = "linux")]

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use compositor::Compositor;
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::wl_compositor::{self, WlCompositor};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_surface::{self, WlSurface};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::xdg::shell::client::xdg_surface::{self, XdgSurface};
use wayland_protocols::xdg::shell::client::xdg_toplevel::{self, XdgToplevel};
use wayland_protocols::xdg::shell::client::xdg_wm_base::{self, XdgWmBase};

// One process-wide XDG_RUNTIME_DIR. The socket source binds under it, and the
// client connects to a path inside it. Set once (OnceLock), before any
// compositor is built, so parallel tests share the directory while each binds
// its own auto-named socket (wayland-1, wayland-2, ...).
fn runtime_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let d = std::env::temp_dir().join(format!("horizon-comp-test.{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).ok();
        std::env::set_var("XDG_RUNTIME_DIR", &d);
        d
    })
    .as_path()
}

// Drive the server until the client thread sends its result, or time out.
fn pump_until<T>(comp: &mut Compositor, rx: &Receiver<T>, what: &str) -> T {
    let start = Instant::now();
    loop {
        comp.dispatch(Some(Duration::from_millis(20)))
            .expect("dispatch");
        match rx.try_recv() {
            Ok(v) => return v,
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => panic!("client thread died before {what}"),
        }
        if start.elapsed() > Duration::from_secs(10) {
            panic!("timed out waiting for {what}");
        }
    }
}

// A client connects, lists the registry, and the compositor must advertise the
// core protocol set every app relies on.
#[test]
fn advertises_core_globals() {
    let _ = runtime_dir();
    let mut comp = Compositor::new().expect("start compositor");
    let path = runtime_dir().join(comp.socket_name());

    let (tx, rx) = mpsc::channel();
    let client = thread::spawn(move || {
        let conn = Connection::from_socket(UnixStream::connect(&path).unwrap()).unwrap();
        let (globals, _queue) = registry_queue_init::<App>(&conn).unwrap();
        let names: Vec<String> = globals
            .contents()
            .clone_list()
            .into_iter()
            .map(|g| g.interface)
            .collect();
        tx.send(names).unwrap();
    });

    let names = pump_until(&mut comp, &rx, "registry");
    client.join().unwrap();

    for iface in [
        "wl_compositor",
        "wl_shm",
        "xdg_wm_base",
        "wl_seat",
        "wl_output",
    ] {
        assert!(
            names.iter().any(|n| n == iface),
            "compositor did not advertise {iface}; got {names:?}"
        );
    }
}

// The real lifecycle: a client opens an xdg toplevel and the compositor maps it
// into the scene with its title, then drops it from the scene when the client
// destroys it.
#[test]
fn toplevel_maps_then_unmaps() {
    let _ = runtime_dir();
    let mut comp = Compositor::new().expect("start compositor");
    let path = runtime_dir().join(comp.socket_name());

    assert_eq!(comp.window_count(), 0, "scene starts empty");

    let (up_tx, up_rx) = mpsc::channel::<()>();
    let (down_tx, down_rx) = mpsc::channel::<()>();
    let client = thread::spawn(move || {
        let conn = Connection::from_socket(UnixStream::connect(&path).unwrap()).unwrap();
        let (globals, mut queue) = registry_queue_init::<App>(&conn).unwrap();
        let qh = queue.handle();
        let mut app = App;

        let wl_compositor: WlCompositor = globals.bind(&qh, 1..=1, ()).unwrap();
        let wm_base: XdgWmBase = globals.bind(&qh, 1..=1, ()).unwrap();
        let surface = wl_compositor.create_surface(&qh, ());
        let xdg_surface = wm_base.get_xdg_surface(&surface, &qh, ());
        let toplevel = xdg_surface.get_toplevel(&qh, ());
        toplevel.set_title("horizon-test".to_string());
        // Initial commit with no buffer; the server replies with a configure.
        surface.commit();
        queue.roundtrip(&mut app).unwrap();

        up_tx.send(()).unwrap();
        // Hold the window mapped while the test asserts, then tear it down.
        down_rx.recv().unwrap();
        toplevel.destroy();
        xdg_surface.destroy();
        surface.destroy();
        conn.flush().unwrap();
    });

    pump_until(&mut comp, &up_rx, "toplevel");
    // A couple more batches so the mapping has fully settled.
    for _ in 0..3 {
        comp.dispatch(Some(Duration::from_millis(20))).unwrap();
    }
    assert_eq!(comp.window_count(), 1, "toplevel should be mapped");
    assert_eq!(comp.window_titles(), vec!["horizon-test".to_string()]);

    down_tx.send(()).unwrap();
    let start = Instant::now();
    loop {
        comp.dispatch(Some(Duration::from_millis(20))).unwrap();
        if comp.window_count() == 0 {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "toplevel never unmapped"
        );
    }
    client.join().unwrap();
}

// Minimal client-side protocol state: ack what must be acked (xdg ping, xdg
// surface configure), ignore the rest.
struct App;

impl Dispatch<WlRegistry, GlobalListContents> for App {
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

impl Dispatch<WlCompositor, ()> for App {
    fn event(
        _: &mut Self,
        _: &WlCompositor,
        _: wl_compositor::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSurface, ()> for App {
    fn event(
        _: &mut Self,
        _: &WlSurface,
        _: wl_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<XdgWmBase, ()> for App {
    fn event(
        _: &mut Self,
        wm_base: &XdgWmBase,
        event: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            wm_base.pong(serial);
        }
    }
}

impl Dispatch<XdgSurface, ()> for App {
    fn event(
        _: &mut Self,
        xdg_surface: &XdgSurface,
        event: xdg_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            xdg_surface.ack_configure(serial);
        }
    }
}

impl Dispatch<XdgToplevel, ()> for App {
    fn event(
        _: &mut Self,
        _: &XdgToplevel,
        _: xdg_toplevel::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
