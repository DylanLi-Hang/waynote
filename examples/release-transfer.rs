use gtk::{gdk, prelude::*};
use gtk4_layer_shell::LayerShell;
use std::rc::Rc;
mod platform {
    pub mod layout {
        pub struct BoardState {
            pub width: i32,
            pub height: i32,
            pub layer_x: i32,
            pub layer_y: i32,
            pub layer_monitor: Option<String>,
        }
    }
}
#[allow(dead_code)]
#[path = "../src/app/layer_window.rs"]
mod layer_window;
fn main() {
    let app = gtk::Application::builder()
        .application_id("dev.waynote.release.demo")
        .build();
    app.connect_activate(activate);
    app.run();
}

fn activate(app: &gtk::Application) {
    let display = gdk::Display::default().unwrap();
    let mut monitors: Vec<_> = display
        .monitors()
        .iter::<gdk::Monitor>()
        .map(Result::unwrap)
        .collect();
    monitors.sort_by_key(|m| (m.geometry().x(), m.geometry().y()));
    if monitors.is_empty() {
        app.quit();
        return;
    }
    if std::env::var_os("DEMO_TEST_BARS").is_some() {
        use gtk4_layer_shell::{Edge, Layer};
        for m in &monitors {
            let bar = gtk::ApplicationWindow::builder()
                .application(app)
                .default_height(40)
                .build();
            bar.init_layer_shell();
            bar.set_monitor(Some(m));
            bar.set_layer(Layer::Top);
            for e in [Edge::Top, Edge::Left, Edge::Right] {
                bar.set_anchor(e, true);
            }
            bar.set_exclusive_zone(40);
            bar.set_child(Some(&gtk::Label::new(Some("test reserved top bar"))));
            bar.set_visible(true);
        }
    }
    if std::env::var_os("DEMO_BARS_ONLY").is_some() {
        println!("READY bars");
        return;
    }
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Waynote cross-screen demo")
        .build();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let css = gtk::CssProvider::new();
    css.load_from_data(
        ".release-panel {background:#fff1b8;color:#202020;} .demo-label {padding:18px;}",
    );
    gtk::style_context_add_provider_for_display(
        &display,
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    content.add_css_class("release-panel");
    let label=gtk::Label::new(Some("CROSS-SCREEN DEMO\n\nDrag: follows within this screen.\nCross the edge: release to transfer.\nThe grab offset stays fixed."));
    label.add_css_class("demo-label");
    label.set_vexpand(true);
    content.append(&label);
    let close = gtk::Button::with_label("Close demo");
    let a = app.clone();
    close.connect_clicked(move |_| a.quit());
    content.append(&close);
    let state = platform::layout::BoardState {
        width: 360,
        height: 300,
        layer_x: 100,
        layer_y: 100,
        layer_monitor: None,
    };
    let panel = Rc::new(
        layer_window::Panel::attach(&window, &content, &state)
            .expect("WAYNOTE_LAYER_SHELL=1 required"),
    );
    // Coordinate math uses monitor origins. Cover the full monitor even
    // when a shell bar reserves space; the panel alone receives input.
    window.set_exclusive_zone(-1);
    window.set_monitor(Some(&monitors[0]));
    panel.drag(&label);
    window.set_visible(true);
    println!("READY: production continuous drag, release to transfer");
}
