//! Stationary Layer Shell canvas with a movable, click-through note panel.
use gtk::{glib, prelude::*};
use std::{cell::Cell, rc::Rc};

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

#[cfg(feature = "layer-shell-prototype")]
fn current_monitor(window: &gtk::ApplicationWindow) -> Option<gtk::gdk::Monitor> {
    use gtk4_layer_shell::LayerShell;
    window.monitor().or_else(|| {
        window
            .surface()
            .and_then(|surface| WidgetExt::display(window).monitor_at_surface(&surface))
    })
}

fn position(
    origin: (f64, f64),
    start: (f64, f64),
    pointer: (f64, f64),
    bounds: (f64, f64),
) -> (f64, f64) {
    (
        (origin.0 + pointer.0 - start.0).clamp(0.0, bounds.0.max(0.0)),
        (origin.1 + pointer.1 - start.1).clamp(0.0, bounds.1.max(0.0)),
    )
}

#[derive(Clone)]
pub struct Panel {
    window: glib::WeakRef<gtk::ApplicationWindow>,
    pending: Rc<Cell<Option<(f64, f64)>>>,
    pub widget: gtk::Box,
    canvas: gtk::Fixed,
    location: Rc<Cell<(f64, f64)>>,
}
impl Panel {
    pub fn attach(
        window: &gtk::ApplicationWindow,
        widget: &gtk::Box,
        state: &crate::platform::layout::BoardState,
    ) -> Option<Self> {
        if !enabled() {
            return None;
        }
        #[cfg(feature = "layer-shell-prototype")]
        {
            use gtk4_layer_shell::{Edge, Layer, LayerShell};
            window.init_layer_shell();
            let namespace = window.application().and_then(|app| app.application_id());
            window.set_namespace(namespace.as_deref());
            window.set_layer(Layer::Top);
            // Pointer coordinates and saved positions use the full monitor origin,
            // including desktops with bars reserving part of the work area.
            window.set_exclusive_zone(-1);
            if let Some(connector) = &state.layer_monitor {
                let display = WidgetExt::display(window);
                let monitor = display
                    .monitors()
                    .iter::<gtk::gdk::Monitor>()
                    .filter_map(Result::ok)
                    .find(|m| m.connector().as_deref() == Some(connector.as_str()));
                window.set_monitor(monitor.as_ref());
            }
            for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
                window.set_anchor(edge, true);
            }
        }
        keyboard(window, false);
        let css = gtk::CssProvider::new();
        css.load_from_data("window.waynote-canvas { background: transparent; box-shadow: none; }");
        gtk::style_context_add_provider_for_display(
            &WidgetExt::display(window),
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        window.add_css_class("waynote-canvas");
        let canvas = gtk::Fixed::new();
        window.set_child(None::<&gtk::Widget>);
        widget.set_halign(gtk::Align::Start);
        widget.set_valign(gtk::Align::Start);
        widget.set_size_request(state.width.max(360), state.height.max(300));
        let location = Rc::new(Cell::new((
            state.layer_x.max(0) as f64,
            state.layer_y.max(0) as f64,
        )));
        canvas.put(widget, location.get().0, location.get().1);
        window.set_child(Some(&canvas));
        let last_geometry = Rc::new(Cell::new(None));
        let mapped_geometry = last_geometry.clone();
        window.connect_map(move |w| {
            mapped_geometry.set(None);
            if let Some(surface) = w.surface() {
                surface.set_input_region(Some(&gtk::cairo::Region::create()));
            }
        });
        let panel_weak = widget.downgrade();
        let canvas_weak = canvas.downgrade();
        let loc = location.clone();
        let geometry_log = std::env::var_os("WAYNOTE_GEOMETRY_LOG");
        window.add_tick_callback(move |window, _| {
            let (Some(panel), Some(canvas)) = (panel_weak.upgrade(), canvas_weak.upgrade()) else {
                return glib::ControlFlow::Break;
            };
            if canvas.width() <= 0 || panel.width() <= 0 {
                return glib::ControlFlow::Continue;
            }
            // Recover when the output shrinks; GTK's natural minimum can still
            // exceed an unusually small output, which is not a supported layout.
            let requested = panel.size_request();
            let size = (
                requested.0.min(canvas.width()).max(360),
                requested.1.min(canvas.height()).max(300),
            );
            if size != requested {
                panel.set_size_request(size.0, size.1);
            }
            let next = position(
                loc.get(),
                (0.0, 0.0),
                (0.0, 0.0),
                (
                    (canvas.width() - panel.width()) as f64,
                    (canvas.height() - panel.height()) as f64,
                ),
            );
            if next != loc.get() {
                loc.set(next);
                canvas.move_(&panel, next.0, next.1);
            }
            // The input region must match the allocation already rendered, not
            // the position queued by a drag or output resize in this frame.
            if let (Some(surface), Some(rect)) = (window.surface(), panel.compute_bounds(window)) {
                let geometry = (rect.x(), rect.y(), rect.width(), rect.height());
                if last_geometry.get() != Some(geometry) {
                    let x = rect.x().floor() as i32;
                    let y = rect.y().floor() as i32;
                    let region =
                        gtk::cairo::Region::create_rectangle(&gtk::cairo::RectangleInt::new(
                            x,
                            y,
                            (rect.x() + rect.width()).ceil() as i32 - x,
                            (rect.y() + rect.height()).ceil() as i32 - y,
                        ));
                    surface.set_input_region(Some(&region));
                    // GTK may have painted the final resize before this tick.
                    // Queue a frame so Wayland commits the new input region
                    // even when no more pointer motion or redraw follows.
                    surface.queue_render();
                    if let Some(path) = &geometry_log {
                        use std::io::Write;
                        if let Ok(mut f) = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(path)
                        {
                            let _ = writeln!(
                                f,
                                "{} {} {} {}",
                                geometry.0, geometry.1, geometry.2, geometry.3
                            );
                        }
                    }
                    last_geometry.set(Some(geometry));
                }
            }
            glib::ControlFlow::Continue
        });
        Some(Self {
            window: window.downgrade(),
            pending: Rc::new(Cell::new(None)),
            widget: widget.clone(),
            canvas,
            location,
        })
    }
    pub fn save(&self, state: &mut crate::platform::layout::BoardState) {
        let (x, y) = self.pending.get().unwrap_or(self.location.get());
        #[cfg(feature = "layer-shell-prototype")]
        if let Some(window) = self.window.upgrade() {
            state.layer_monitor =
                current_monitor(&window).and_then(|m| m.connector().map(|s| s.to_string()));
        }
        state.layer_x = x.round() as i32;
        state.layer_y = y.round() as i32;
        if self.widget.width() > 0 && self.widget.height() > 0 {
            state.width = self.widget.width();
            state.height = self.widget.height();
        }
    }
    pub fn drag(&self, title: &gtk::Label) {
        let gesture = gtk::GestureDrag::new();
        gesture.set_button(1);
        let start = Rc::new(Cell::new(None));
        let begin = start.clone();
        let loc = self.location.clone();
        gesture.connect_drag_begin(move |g, _, _| {
            begin.set(
                g.current_event()
                    .and_then(|e| e.position())
                    .map(|p| (loc.get(), p)),
            );
        });
        let origin = start.clone();
        let loc = self.location.clone();
        let panel = self.widget.downgrade();
        let canvas = self.canvas.downgrade();
        gesture.connect_drag_update(move |g, _, _| {
            let Some((initial, pointer_start)) = origin.get() else {
                return;
            };
            let Some(pointer) = g.current_event().and_then(|e| e.position()) else {
                return;
            };
            let (Some(panel), Some(canvas)) = (panel.upgrade(), canvas.upgrade()) else {
                return;
            };
            let next = position(
                initial,
                pointer_start,
                pointer,
                (
                    (canvas.width() - panel.width()) as f64,
                    (canvas.height() - panel.height()) as f64,
                ),
            );
            // A real drag must not also activate the title's click-to-Preview.
            if (pointer.0 - pointer_start.0).abs() > 8.0
                || (pointer.1 - pointer_start.1).abs() > 8.0
            {
                g.set_state(gtk::EventSequenceState::Claimed);
            }
            loc.set(next);
            canvas.move_(&panel, next.0, next.1);
        });
        let widget = self.widget.downgrade();
        let canvas = self.canvas.downgrade();
        let window = self.window.clone();
        let location = self.location.clone();
        let pending = self.pending.clone();
        gesture.connect_drag_end(move |g, _, _| {
            let Some((initial, pointer_start)) = start.take() else {
                return;
            };
            let Some(pointer) = g.current_event().and_then(|e| e.position()) else {
                return;
            };
            let (Some(widget), Some(canvas)) = (widget.upgrade(), canvas.upgrade()) else {
                return;
            };
            let panel = Panel {
                widget,
                canvas,
                window: window.clone(),
                location: location.clone(),
                pending: pending.clone(),
            };
            panel.transfer(
                pointer,
                (pointer_start.0 - initial.0, pointer_start.1 - initial.1),
            );
        });
        title.set_cursor_from_name(Some("grab"));
        title.add_controller(gesture);
    }
    fn transfer(&self, pointer: (f64, f64), grab: (f64, f64)) {
        #[cfg(feature = "layer-shell-prototype")]
        {
            let Some(window) = self.window.upgrade() else {
                return;
            };
            let Some(source) = current_monitor(&window) else {
                return;
            };
            let rect = source.geometry();
            let global = (rect.x() as f64 + pointer.0, rect.y() as f64 + pointer.1);
            let destination = WidgetExt::display(&window)
                .monitors()
                .iter::<gtk::gdk::Monitor>()
                .filter_map(Result::ok)
                .find(|m| {
                    let r = m.geometry();
                    m.is_valid()
                        && global.0 >= r.x() as f64
                        && global.0 < (r.x() + r.width()) as f64
                        && global.1 >= r.y() as f64
                        && global.1 < (r.y() + r.height()) as f64
                });
            let Some(destination) = destination.filter(|m| m != &source) else {
                return;
            };
            let rect = destination.geometry();
            let target = (
                global.0 - rect.x() as f64 - grab.0,
                global.1 - rect.y() as f64 - grab.1,
            );
            let panel = self.clone();
            // Remapping during GTK's release dispatch invalidates native widgets.
            glib::idle_add_local_once(move || panel.relocate(&destination, target));
        }
        let _ = (pointer, grab);
    }
    #[cfg(feature = "layer-shell-prototype")]
    fn relocate(&self, monitor: &gtk::gdk::Monitor, target: (f64, f64)) {
        use gtk4_layer_shell::LayerShell;
        let Some(window) = self.window.upgrade() else {
            return;
        };
        if !monitor.is_valid() {
            return;
        }
        let rect = monitor.geometry();
        self.pending.set(Some(target));
        // A GtkFixed child's old offset/size can prevent the canvas shrinking.
        self.location.set((0.0, 0.0));
        self.canvas.move_(&self.widget, 0.0, 0.0);
        self.widget.set_size_request(
            self.widget.width().min(rect.width()).max(360),
            self.widget.height().min(rect.height()).max(300),
        );
        window.set_monitor(Some(monitor));
        let panel = self.clone();
        let monitor = monitor.clone();
        window.add_tick_callback(move |_, _| {
            if !monitor.is_valid() {
                panel.pending.set(None);
                return glib::ControlFlow::Break;
            }
            let rect = monitor.geometry();
            // The first frame after remap still has the source allocation.
            if panel.canvas.width() != rect.width() || panel.canvas.height() != rect.height() {
                return glib::ControlFlow::Continue;
            }
            let next = position(
                target,
                (0.0, 0.0),
                (0.0, 0.0),
                (
                    (panel.canvas.width() - panel.widget.width()) as f64,
                    (panel.canvas.height() - panel.widget.height()) as f64,
                ),
            );
            panel.location.set(next);
            panel.canvas.move_(&panel.widget, next.0, next.1);
            panel.pending.set(None);
            glib::ControlFlow::Break
        });
    }
    pub fn resize(&self, grip: &gtk::Label) {
        let gesture = gtk::GestureDrag::new();
        gesture.set_button(1);
        let start = Rc::new(Cell::new(None));
        let begin = start.clone();
        let panel = self.widget.downgrade();
        gesture.connect_drag_begin(move |g, _, _| {
            begin.set(panel.upgrade().and_then(|p| {
                g.current_event()
                    .and_then(|e| e.position())
                    .map(|pointer| ((p.width(), p.height()), pointer))
            }));
        });
        let origin = start.clone();
        let panel = self.widget.downgrade();
        let canvas = self.canvas.downgrade();
        let loc = self.location.clone();
        gesture.connect_drag_update(move |g, _, _| {
            let Some((size, pointer_start)) = origin.get() else {
                return;
            };
            let Some(pointer) = g.current_event().and_then(|e| e.position()) else {
                return;
            };
            let (Some(panel), Some(canvas)) = (panel.upgrade(), canvas.upgrade()) else {
                return;
            };
            let max = (
                (canvas.width() - loc.get().0 as i32).max(360),
                (canvas.height() - loc.get().1 as i32).max(300),
            );
            panel.set_size_request(
                (size.0 + (pointer.0 - pointer_start.0) as i32).clamp(360, max.0),
                (size.1 + (pointer.1 - pointer_start.1) as i32).clamp(300, max.1),
            );
        });
        gesture.connect_drag_end(move |_, _, _| start.set(None));
        grip.set_cursor_from_name(Some("se-resize"));
        grip.add_controller(gesture);
    }
}

#[cfg(test)]
mod tests {
    use super::position;
    #[test]
    fn grab_offset_stays_constant_through_reversals() {
        for pointer in [(600.25, 320.5), (450.0, 230.0), (425.25, 215.5)] {
            let p = position((400.0, 200.0), (425.25, 215.5), pointer, (1000.0, 800.0));
            assert_eq!((pointer.0 - p.0, pointer.1 - p.1), (25.25, 15.5));
        }
    }
    #[test]
    fn edges_clamp_and_recover_without_accumulating_error() {
        let origin = (400.0, 200.0);
        let start = (420.0, 220.0);
        assert_eq!(
            position(origin, start, (-100.0, -100.0), (600.0, 400.0)),
            (0.0, 0.0)
        );
        assert_eq!(
            position(origin, start, (2000.0, 2000.0), (600.0, 400.0)),
            (600.0, 400.0)
        );
        assert_eq!(position(origin, start, start, (600.0, 400.0)), origin);
        assert_eq!(position(origin, start, start, (-1.0, -1.0)), (0.0, 0.0));
    }
}
