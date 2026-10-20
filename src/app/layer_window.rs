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

pub struct Panel {
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
            window.set_exclusive_zone(0);
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
            widget: widget.clone(),
            canvas,
            location,
        })
    }
    pub fn save(&self, state: &mut crate::platform::layout::BoardState) {
        let (x, y) = self.location.get();
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
            loc.set(next);
            canvas.move_(&panel, next.0, next.1);
        });
        gesture.connect_drag_end(move |_, _, _| start.set(None));
        title.set_cursor_from_name(Some("grab"));
        title.add_controller(gesture);
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
