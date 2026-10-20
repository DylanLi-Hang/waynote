//! Optional native Layer Shell window behavior; no compositor IPC.
use gtk::prelude::*;

pub fn enabled() -> bool {
    cfg!(feature = "layer-shell-prototype")
        && std::env::var("WAYNOTE_LAYER_SHELL").as_deref() == Ok("1")
}
pub fn show(window: &gtk::ApplicationWindow) {
    if enabled() {
        window.set_visible(true);
    } else {
        window.present();
    }
}
pub fn keyboard(window: &gtk::ApplicationWindow, active: bool) {
    #[cfg(feature = "layer-shell-prototype")]
    if enabled() {
        use gtk4_layer_shell::{KeyboardMode, LayerShell};
        window.set_keyboard_mode(if active {
            KeyboardMode::OnDemand
        } else {
            KeyboardMode::None
        });
    }
    let _ = (window, active);
}
pub fn setup(window: &gtk::ApplicationWindow, state: &crate::platform::layout::BoardState) {
    #[cfg(feature = "layer-shell-prototype")]
    if enabled() {
        use gtk4_layer_shell::{Edge, Layer, LayerShell};
        window.init_layer_shell();
        window.set_namespace(Some("dev.mryll.waynote.layershell.development"));
        window.set_layer(Layer::Top);
        window.set_exclusive_zone(0);
        window.set_anchor(Edge::Top, true);
        window.set_anchor(Edge::Left, true);
        window.set_margin(Edge::Left, state.layer_x.max(0));
        window.set_margin(Edge::Top, state.layer_y.max(0));
        keyboard(window, false);
    }
    let _ = (window, state);
}
pub fn save(window: &gtk::ApplicationWindow, state: &mut crate::platform::layout::BoardState) {
    #[cfg(feature = "layer-shell-prototype")]
    if enabled() {
        use gtk4_layer_shell::{Edge, LayerShell};
        state.layer_x = window.margin(Edge::Left);
        state.layer_y = window.margin(Edge::Top);
    }
    let _ = (window, state);
}
/// Commit movement on release: local gesture coordinates stay stable throughout the drag.
pub fn drag(window: &gtk::ApplicationWindow, title: &gtk::Label) {
    #[cfg(feature = "layer-shell-prototype")]
    if enabled() {
        use gtk4_layer_shell::{Edge, LayerShell};
        let gesture = gtk::GestureDrag::new();
        gesture.set_button(1);
        let weak = window.downgrade();
        gesture.connect_drag_end(move |_, dx, dy| {
            if let Some(window) = weak.upgrade() {
                let monitor = window
                    .surface()
                    .and_then(|s| gtk::prelude::WidgetExt::display(&window).monitor_at_surface(&s));
                let bounds = monitor.map(|m| m.geometry());
                let max_x = bounds
                    .as_ref()
                    .map_or(i32::MAX, |r| (r.width() - window.width()).max(0));
                let max_y = bounds
                    .as_ref()
                    .map_or(i32::MAX, |r| (r.height() - window.height()).max(0));
                window.set_margin(
                    Edge::Left,
                    (window.margin(Edge::Left) + dx as i32).clamp(0, max_x),
                );
                window.set_margin(
                    Edge::Top,
                    (window.margin(Edge::Top) + dy as i32).clamp(0, max_y),
                );
            }
        });
        title.set_cursor_from_name(Some("move"));
        title.add_controller(gesture);
    }
    let _ = (window, title);
}
pub fn resize(window: &gtk::ApplicationWindow, grip: &gtk::Label) {
    let gesture = gtk::GestureDrag::new();
    gesture.set_button(1);
    let start = std::rc::Rc::new(std::cell::Cell::new((400, 440)));
    let weak = window.downgrade();
    let origin = start.clone();
    gesture.connect_drag_begin(move |_, _, _| {
        if let Some(w) = weak.upgrade() {
            origin.set((w.width(), w.height()));
        }
    });
    let weak = window.downgrade();
    gesture.connect_drag_update(move |_, dx, dy| {
        if let Some(w) = weak.upgrade() {
            let (width, height) = start.get();
            w.set_default_size((width + dx as i32).max(360), (height + dy as i32).max(300));
        }
    });
    grip.set_cursor_from_name(Some("se-resize"));
    grip.add_controller(gesture);
}
