use gtk::{gdk, prelude::*};
use gtk4_layer_shell::LayerShell;
use std::{cell::Cell, rc::Rc};
mod platform {
    pub mod layout {
        pub struct BoardState {
            pub width: i32,
            pub height: i32,
            pub layer_x: i32,
            pub layer_y: i32,
        }
    }
}
// Compile the actual production panel, including its drag handlers/input region.
#[allow(dead_code)]
mod layer_window {
    include!("../src/app/layer_window.rs");
    impl Panel {
        pub fn relocate_demo(
            &self,
            w: &gtk::ApplicationWindow,
            m: &gtk::gdk::Monitor,
            x: i32,
            y: i32,
        ) {
            use gtk4_layer_shell::LayerShell;
            // Remove the old offset before shrinking the canvas: GtkFixed includes
            // positioned children in its minimum size, so an offscreen child can
            // prevent the smaller destination allocation from ever arriving.
            self.location.set((0., 0.));
            self.canvas.move_(&self.widget, 0., 0.);
            w.set_monitor(Some(m));
            // set_monitor remaps asynchronously. The first tick still has the old
            // allocation, whose clamp would permanently discard the destination.
            let canvas = self.canvas.clone();
            let panel = self.widget.clone();
            let location = self.location.clone();
            let monitor = m.clone();
            w.add_tick_callback(move |_, _| {
                if !monitor.is_valid() {
                    return gtk::glib::ControlFlow::Break;
                }
                let target = monitor.geometry();
                if canvas.width() != target.width() || canvas.height() != target.height() {
                    return gtk::glib::ControlFlow::Continue;
                }
                let x = (x as f64).clamp(0., (canvas.width() - panel.width()).max(0) as f64);
                let y = (y as f64).clamp(0., (canvas.height() - panel.height()).max(0) as f64);
                location.set((x, y));
                canvas.move_(&panel, x, y);
                println!("SETTLED {x} {y}");
                gtk::glib::ControlFlow::Break
            });
        }
    }
}
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
    // Extend the production drag only at release. The canvas stays stationary
    // during motion; moving/remapping the surface during a grab breaks tracking.
    let controllers = label.observe_controllers();
    let gesture = (0..controllers.n_items())
        .filter_map(|i| controllers.item(i))
        .find_map(|o| o.downcast::<gtk::GestureDrag>().ok())
        .unwrap();
    let start = Rc::new(Cell::new(None));
    let begin = start.clone();
    let p = panel.clone();
    gesture.connect_drag_begin(move |g, _, _| {
        let mut s = platform::layout::BoardState {
            width: 0,
            height: 0,
            layer_x: 0,
            layer_y: 0,
        };
        p.save(&mut s);
        begin.set(
            g.current_event()
                .and_then(|e| e.position())
                .map(|v| (v.0 - s.layer_x as f64, v.1 - s.layer_y as f64)),
        );
    });
    let current = Rc::new(Cell::new(0usize));
    let n = window.clone();
    gesture.connect_drag_end(move |g, _, _| {
        let Some(grab) = start.take() else {
            return;
        };
        let Some(pointer) = g.current_event().and_then(|e| e.position()) else {
            return;
        };
        let old = monitors[current.get()].geometry();
        let global = (old.x() as f64 + pointer.0, old.y() as f64 + pointer.1);
        if let Some((i, m)) = monitors.iter().enumerate().find(|(_, m)| {
            let r = m.geometry();
            m.is_valid()
                && global.0 >= r.x() as f64
                && global.0 < (r.x() + r.width()) as f64
                && global.1 >= r.y() as f64
                && global.1 < (r.y() + r.height()) as f64
        }) {
            if i == current.get() {
                return;
            }
            let r = m.geometry();
            let mut state = platform::layout::BoardState {
                width: 0,
                height: 0,
                layer_x: 0,
                layer_y: 0,
            };
            panel.save(&mut state);
            state.layer_x = (global.0 - r.x() as f64 - grab.0)
                .round()
                .clamp(0., (r.width() - state.width).max(0) as f64)
                as i32;
            state.layer_y = (global.1 - r.y() as f64 - grab.1)
                .round()
                .clamp(0., (r.height() - state.height).max(0) as f64)
                as i32;
            // Preserve panel instance and its drag handlers: transfer its canvas
            // allocation, then let subsequent production drags read the new origin.
            let panel = panel.clone();
            let n = n.clone();
            let m = m.clone();
            let current = current.clone();
            // Finish GTK's release dispatch before unmapping its native surface.
            gtk::glib::idle_add_local_once(move || {
                relocate(&panel, &n, &m, state.layer_x, state.layer_y);
                current.set(i);
                println!("PLACED {i}");
            });
        }
    });
    window.set_visible(true);
    println!("READY: production continuous drag, release to transfer");
}
fn relocate(
    panel: &layer_window::Panel,
    window: &gtk::ApplicationWindow,
    monitor: &gdk::Monitor,
    x: i32,
    y: i32,
) {
    panel.relocate_demo(window, monitor, x, y);
}
