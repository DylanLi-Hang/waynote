//! Test pointer: accepts commands only inside the private headless test runtime.
use std::{
    env,
    error::Error,
    io::{self, BufRead},
};
use wayland_client::{
    Connection, Dispatch, QueueHandle,
    globals::{GlobalListContents, registry_queue_init},
    protocol::wl_registry,
};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};

#[derive(Default)]
struct State;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrVirtualPointerManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwlrVirtualPointerManagerV1,
        _: <ZwlrVirtualPointerManagerV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrVirtualPointerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwlrVirtualPointerV1,
        _: <ZwlrVirtualPointerV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    assert!(env::var("XDG_RUNTIME_DIR")?.starts_with("/tmp/waynote-focus-runtime-"));
    let width = env::var("WAYNOTE_TEST_WIDTH")
        .unwrap_or("1280".into())
        .parse()?;
    let height = env::var("WAYNOTE_TEST_HEIGHT")
        .unwrap_or("900".into())
        .parse()?;
    let connection = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<State>(&connection)?;
    let qh = queue.handle();
    let manager: ZwlrVirtualPointerManagerV1 = globals.bind(&qh, 1..=2, ())?;
    let pointer = manager.create_virtual_pointer(None, &qh, ());
    queue.roundtrip(&mut State)?;
    for line in io::stdin().lock().lines() {
        let line = line?;
        let v: Vec<_> = line.split_whitespace().collect();
        match v[0] {
            "move" => {
                pointer.motion_absolute(timestamp(), v[1].parse()?, v[2].parse()?, width, height)
            }
            "press" => pointer.button(
                timestamp(),
                272,
                wayland_client::protocol::wl_pointer::ButtonState::Pressed,
            ),
            "release" => pointer.button(
                timestamp(),
                272,
                wayland_client::protocol::wl_pointer::ButtonState::Released,
            ),
            _ => panic!("invalid probe command"),
        }
        pointer.frame();
        connection.flush()?;
        queue.roundtrip(&mut State)?;
        println!("ok");
    }
    Ok(())
}

fn timestamp() -> u32 {
    #[repr(C)]
    struct Time {
        sec: i64,
        nsec: i64,
    }
    unsafe extern "C" {
        fn clock_gettime(clock: i32, time: *mut Time) -> i32;
    }
    let mut t = Time { sec: 0, nsec: 0 };
    assert_eq!(unsafe { clock_gettime(1, &mut t) }, 0);
    (t.sec * 1000 + t.nsec / 1_000_000) as u32
}
