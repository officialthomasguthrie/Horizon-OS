//! Headless multi-monitor test: place several outputs in one shared logical space
//! and prove each one renders its own region of the scene rather than mirroring
//! the whole thing. A real in-process Wayland client maps a window; the compositor
//! reads each output back through the software renderer and we assert on the
//! pixels. No display: the per-output region rendering the DRM backend scans out
//! is exercised through `output_render_elements`, the same headless split the
//! single-output render test uses.
//!
//! Only built with the `render` feature (the offscreen renderer and the id-based
//! output API).

#![cfg(all(target_os = "linux", feature = "render"))]

use std::io::Write;
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use compositor::Compositor;
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::wl_buffer::{self, WlBuffer};
use wayland_client::protocol::wl_compositor::{self, WlCompositor};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_shm::{self, WlShm};
use wayland_client::protocol::wl_shm_pool::{self, WlShmPool};
use wayland_client::protocol::wl_surface::{self, WlSurface};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::xdg::shell::client::xdg_surface::{self, XdgSurface};
use wayland_protocols::xdg::shell::client::xdg_toplevel::{self, XdgToplevel};
use wayland_protocols::xdg::shell::client::xdg_wm_base::{self, XdgWmBase};

const WIN: i32 = 64;
// Each test monitor; small so the layout math is easy to read in the assertions.
const OW: i32 = 300;
const OH: i32 = 200;
// Opaque magenta as a 0xAARRGGBB word: A=FF, R=FF, G=00, B=FF.
const MAGENTA: u32 = 0xFFFF_00FF;
// The clear colour behind everything: opaque black.
const BLACK: u32 = 0xFF00_0000;

fn runtime_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let d = std::env::temp_dir().join(format!("horizon-multimon-test.{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).ok();
        std::env::set_var("XDG_RUNTIME_DIR", &d);
        d
    })
    .as_path()
}

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

// Spawn a client that maps a WIN x WIN magenta toplevel and holds it open until
// told to quit. Returns the channels: ready (buffer committed) and a quit sender.
fn spawn_window(path: PathBuf) -> (Receiver<()>, mpsc::Sender<()>, thread::JoinHandle<()>) {
    let (ready_tx, ready_rx) = mpsc::channel::<()>();
    let (quit_tx, quit_rx) = mpsc::channel::<()>();
    let handle = thread::spawn(move || {
        let conn = Connection::from_socket(UnixStream::connect(&path).unwrap()).unwrap();
        let (globals, mut queue) = registry_queue_init::<App>(&conn).unwrap();
        let qh = queue.handle();
        let mut app = App;

        let wl_compositor: WlCompositor = globals.bind(&qh, 1..=1, ()).unwrap();
        let wm_base: XdgWmBase = globals.bind(&qh, 1..=1, ()).unwrap();
        let shm: WlShm = globals.bind(&qh, 1..=1, ()).unwrap();

        let surface = wl_compositor.create_surface(&qh, ());
        let xdg_surface = wm_base.get_xdg_surface(&surface, &qh, ());
        let toplevel = xdg_surface.get_toplevel(&qh, ());
        toplevel.set_title("horizon-multimon".to_string());
        surface.commit();
        queue.roundtrip(&mut app).unwrap();

        let stride = WIN * 4;
        let len = (stride * WIN) as usize;
        let mut file = tempfile::tempfile().unwrap();
        let mut bytes = Vec::with_capacity(len);
        for _ in 0..(WIN * WIN) {
            bytes.extend_from_slice(&MAGENTA.to_le_bytes());
        }
        file.write_all(&bytes).unwrap();
        file.flush().unwrap();

        let pool = shm.create_pool(file.as_fd(), len as i32, &qh, ());
        let buffer = pool.create_buffer(0, WIN, WIN, stride, wl_shm::Format::Argb8888, &qh, ());
        surface.attach(Some(&buffer), 0, 0);
        surface.damage(0, 0, WIN, WIN);
        surface.commit();
        queue.roundtrip(&mut app).unwrap();

        ready_tx.send(()).unwrap();
        // Keep the buffer and its backing file alive while the server renders.
        quit_rx.recv().unwrap();
        toplevel.destroy();
        xdg_surface.destroy();
        surface.destroy();
        conn.flush().unwrap();
    });
    (ready_rx, quit_tx, handle)
}

// Two outputs side by side; the window maps at the scene origin, so it falls on
// the left output. The left reads back the window, the right reads back nothing:
// each output paints its own region, the right is not a mirror of the left.
#[test]
fn window_shows_only_on_the_output_that_covers_it() {
    let _ = runtime_dir();
    let mut comp = Compositor::new().expect("start compositor");
    let path = runtime_dir().join(comp.socket_name());

    // Left at the origin, right immediately to its logical right.
    let left = comp.add_output("LEFT", OW, OH, 0, 0);
    let right = comp.add_output("RIGHT", OW, OH, OW, 0);

    let (ready_rx, quit_tx, client) = spawn_window(path);
    pump_until(&mut comp, &ready_rx, "buffer commit");
    for _ in 0..3 {
        comp.dispatch(Some(Duration::from_millis(20))).unwrap();
    }

    // The left output covers logical (0,0), where the window maps.
    let lf = comp.render_output(left).expect("render left");
    assert_eq!(lf.width as i32, OW);
    assert_eq!(lf.height as i32, OH);
    assert_eq!(
        lf.argb(WIN as u32 / 2, WIN as u32 / 2),
        MAGENTA,
        "the window should appear on the left output"
    );
    assert_eq!(
        lf.argb(OW as u32 - 1, OH as u32 - 1),
        BLACK,
        "empty space on the left output should be the clear colour"
    );

    // The right output's region starts at logical x=OW, past the window, so it
    // shows nothing: it is not mirroring the left.
    let rf = comp.render_output(right).expect("render right");
    for (x, y) in [
        (WIN as u32 / 2, WIN as u32 / 2),
        (10, 10),
        (OW as u32 / 2, OH as u32 / 2),
    ] {
        assert_eq!(
            rf.argb(x, y),
            BLACK,
            "the right output must not mirror the window at {x},{y}"
        );
    }

    quit_tx.send(()).unwrap();
    client.join().unwrap();
}

// One output, one window at the scene origin. Moving the output across the shared
// space changes which region it shows, and the window lands at the matching local
// offset: this proves an output renders its own logical region, not the whole
// scene from a fixed origin.
#[test]
fn moving_an_output_shifts_the_region_it_renders() {
    let _ = runtime_dir();
    let mut comp = Compositor::new().expect("start compositor");
    let path = runtime_dir().join(comp.socket_name());

    let out = comp.add_output("SOLO", OW, OH, 0, 0);

    let (ready_rx, quit_tx, client) = spawn_window(path);
    pump_until(&mut comp, &ready_rx, "buffer commit");
    for _ in 0..3 {
        comp.dispatch(Some(Duration::from_millis(20))).unwrap();
    }

    // At the origin the window sits at the output's top-left: local (0,0)..(WIN,WIN).
    let f0 = comp.render_output(out).expect("render at origin");
    assert_eq!(f0.argb(10, 10), MAGENTA, "window at the output's top-left");
    assert_eq!(
        f0.argb(WIN as u32 + 10, 10),
        BLACK,
        "just right of the window is clear"
    );

    // Shift the output left in logical space (its origin goes negative), so the
    // window, still at logical (0,0), appears SHIFT pixels in from the left edge.
    const SHIFT: i32 = 40;
    comp.move_output(out, -SHIFT, 0);
    comp.dispatch(Some(Duration::from_millis(20))).unwrap();

    let f1 = comp.render_output(out).expect("render after move");
    assert_eq!(
        f1.argb(10, 10),
        BLACK,
        "the window's old top-left is now clear: the region moved"
    );
    assert_eq!(
        f1.argb(SHIFT as u32 + 10, 10),
        MAGENTA,
        "the window now renders at its logical position offset by the output origin"
    );
    assert_eq!(
        f1.argb((SHIFT + WIN) as u32 + 10, 10),
        BLACK,
        "past the shifted window is clear again"
    );

    quit_tx.send(()).unwrap();
    client.join().unwrap();
}

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

impl Dispatch<WlShm, ()> for App {
    fn event(
        _: &mut Self,
        _: &WlShm,
        _: wl_shm::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlShmPool, ()> for App {
    fn event(
        _: &mut Self,
        _: &WlShmPool,
        _: wl_shm_pool::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlBuffer, ()> for App {
    fn event(
        _: &mut Self,
        _: &WlBuffer,
        _: wl_buffer::Event,
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
