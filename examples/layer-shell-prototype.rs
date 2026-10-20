//! Isolated window/focus probe. Never opens the installed app's notes.
use gtk::{gio, glib, prelude::*};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use std::{cell::RefCell, path::PathBuf, rc::Rc, time::Duration};

const ID: &str = "dev.mryll.waynote.layershell.prototype";

struct Probe {
    window: gtk::ApplicationWindow,
    editor: gtk::TextView,
    preview: gtk::Label,
    stack: gtk::Stack,
    status: gtk::Label,
    search: gtk::Popover,
    file: gtk::Button,
    chooser: RefCell<Option<gio::Cancellable>>,
    root: PathBuf,
    _hold: gio::ApplicationHoldGuard,
}
impl Probe {
    fn new(app: &gtk::Application, root: PathBuf) -> Rc<Self> {
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("Waynote — Layer Shell prototype")
            .default_width(400)
            .default_height(360)
            .decorated(false)
            .build();
        window.init_layer_shell();
        window.set_namespace(app.application_id().as_deref());
        window.set_layer(Layer::Top);
        window.set_anchor(Edge::Top, true);
        window.set_anchor(Edge::Right, true);
        window.set_margin(Edge::Top, 80);
        window.set_margin(Edge::Right, 30);
        window.set_exclusive_zone(0);
        window.set_keyboard_mode(KeyboardMode::None);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            match edge {
                Edge::Top => content.set_margin_top(16),
                Edge::Bottom => content.set_margin_bottom(16),
                Edge::Left => content.set_margin_start(16),
                _ => content.set_margin_end(16),
            }
        }
        content.append(&gtk::Label::new(Some(
            "[PROTOTYPE] Waynote · isolated scratch note",
        )));
        let status = gtk::Label::new(None);
        status.set_wrap(true);
        content.append(&status);
        let editor = gtk::TextView::builder()
            .wrap_mode(gtk::WrapMode::WordChar)
            .vexpand(true)
            .build();
        let text = std::fs::read_to_string(root.join("data/draft.txt")).unwrap_or_else(|_| {
            "Double-click to edit this isolated scratch note.\nYour real notes are not used.".into()
        });
        editor.buffer().set_text(&text);
        let preview = gtk::Label::builder()
            .label(&text)
            .wrap(true)
            .xalign(0.)
            .yalign(0.)
            .vexpand(true)
            .build();
        let stack = gtk::Stack::new();
        stack.add_named(&preview, Some("preview"));
        stack.add_named(
            &gtk::ScrolledWindow::builder()
                .child(&editor)
                .vexpand(true)
                .build(),
            Some("edit"),
        );
        content.append(&stack);
        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let edit = gtk::Button::with_label("Edit");
        let done = gtk::Button::with_label("Preview");
        let menu = gtk::MenuButton::builder().label("Search").build();
        let search = gtk::Popover::new();
        let entry = gtk::SearchEntry::builder()
            .placeholder_text("Focus test only")
            .build();
        search.set_child(Some(&entry));
        menu.set_popover(Some(&search));
        let file = gtk::Button::with_label("File dialog");
        for b in [&edit, &done] {
            controls.append(b);
        }
        controls.append(&menu);
        content.append(&controls);
        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let hide = gtk::Button::with_label("Hide");
        let quit = gtk::Button::with_label("Quit prototype");
        footer.append(&hide);
        footer.append(&quit);
        content.append(&footer);
        window.set_child(Some(&content));
        let probe = Rc::new(Self {
            window,
            editor,
            preview,
            stack,
            status,
            search,
            file: file.clone(),
            chooser: RefCell::new(None),
            root,
            _hold: app.hold(),
        });
        probe.status.set_text("Preview · keyboard focus disabled");
        let p = Rc::downgrade(&probe);
        edit.connect_clicked(move |_| {
            if let Some(p) = p.upgrade() {
                p.edit();
            }
        });
        let p = Rc::downgrade(&probe);
        done.connect_clicked(move |_| {
            if let Some(p) = p.upgrade() {
                p.finish();
            }
        });
        let p = Rc::downgrade(&probe);
        hide.connect_clicked(move |_| {
            if let Some(p) = p.upgrade() {
                p.hide();
            }
        });
        let p = Rc::downgrade(&probe);
        let a = app.clone();
        quit.connect_clicked(move |_| {
            if let Some(p) = p.upgrade() {
                if p.finish() {
                    a.quit();
                }
            }
        });
        let p = Rc::downgrade(&probe);
        probe.window.connect_close_request(move |_| {
            if let Some(p) = p.upgrade() {
                p.hide();
            }
            glib::Propagation::Stop
        });
        let p = Rc::downgrade(&probe);
        let click = gtk::GestureClick::new();
        click.connect_pressed(move |_, count, _, _| {
            if count == 2 {
                if let Some(p) = p.upgrade() {
                    p.edit();
                }
            }
        });
        probe.preview.add_controller(click);
        let p = Rc::downgrade(&probe);
        probe.search.connect_show(move |_| {
            if let Some(p) = p.upgrade() {
                p.window.set_keyboard_mode(KeyboardMode::OnDemand);
                p.status.set_text("Search · keyboard focus on demand");
                entry.grab_focus();
            }
        });
        let p = Rc::downgrade(&probe);
        probe.search.connect_closed(move |_| {
            if let Some(p) = p.upgrade() {
                if p.stack.visible_child_name().as_deref() == Some("preview") {
                    p.window.set_keyboard_mode(KeyboardMode::None);
                    p.status.set_text("Preview · keyboard focus disabled");
                }
            }
        });
        let p = Rc::downgrade(&probe);
        file.connect_clicked(move |_| {
            if let Some(p) = p.upgrade() {
                if p.chooser.borrow().is_some() {
                    return;
                }
                let cancel = gio::Cancellable::new();
                *p.chooser.borrow_mut() = Some(cancel.clone());
                p.file.set_sensitive(false);
                let dialog = gtk::FileDialog::builder()
                    .title("Prototype file-dialog test (does not import)")
                    .build();
                let weak = Rc::downgrade(&p);
                // Layer surfaces cannot be exported as xdg_toplevel parents.
                // Let the portal own a standalone chooser instead.
                dialog.open(None::<&gtk::Window>, Some(&cancel), move |result| {
                    if let Some(p) = weak.upgrade() {
                        p.chooser.borrow_mut().take();
                        p.file.set_sensitive(true);
                        p.status.set_text(if result.is_ok() {
                            "File selected; no data imported"
                        } else {
                            "File dialog closed"
                        });
                    }
                });
            }
        });
        let p = Rc::downgrade(&probe);
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                if let Some(p) = p.upgrade() {
                    if p.search.is_visible() {
                        p.search.popdown();
                    } else if p.stack.visible_child_name().as_deref() == Some("edit") {
                        p.finish();
                    } else {
                        p.hide();
                    }
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        probe.window.add_controller(keys);
        let p = Rc::downgrade(&probe);
        probe.window.connect_is_active_notify(move |window| {
            if !window.is_active() {
                if let Some(p) = p.upgrade() {
                    if p.stack.visible_child_name().as_deref() == Some("edit") {
                        p.finish();
                    }
                }
            }
        });
        probe
    }
    fn edit(&self) {
        self.window.set_keyboard_mode(KeyboardMode::OnDemand);
        self.stack.set_visible_child_name("edit");
        self.editor.grab_focus();
        self.status
            .set_text("Edit · keyboard focus on demand (click editor if needed)");
    }
    fn finish(&self) -> bool {
        let b = self.editor.buffer();
        let text = b.text(&b.start_iter(), &b.end_iter(), false);
        let data = self.root.join("data");
        let saved = std::fs::create_dir_all(&data).and_then(|_| {
            std::fs::write(data.join("draft.txt.tmp"), text.as_bytes())?;
            std::fs::rename(data.join("draft.txt.tmp"), data.join("draft.txt"))
        });
        if let Err(error) = saved {
            self.status
                .set_text(&format!("Save failed; draft retained: {error}"));
            return false;
        }
        self.preview.set_text(&text);
        self.stack.set_visible_child_name("preview");
        self.window.set_keyboard_mode(KeyboardMode::None);
        self.status.set_text("Preview · keyboard focus disabled");
        true
    }
    fn hide(&self) {
        if self.finish() {
            self.search.popdown();
            self.window.set_visible(false);
        }
    }
    fn show(&self) {
        self.window.set_visible(true);
    }
    fn smoke(self: &Rc<Self>, app: &gtk::Application) {
        let p = self.clone();
        let app = app.clone();
        self.show();
        glib::timeout_add_local_once(Duration::from_millis(400), move || {
            assert!(p.window.is_mapped());
            assert!(p.window.is_layer_window());
            assert_eq!(p.window.layer(), Layer::Top);
            assert_eq!(p.window.keyboard_mode(), KeyboardMode::None);
            assert!(
                !p.window.is_active(),
                "Preview must not acquire keyboard focus"
            );
            p.edit();
            assert_eq!(p.window.keyboard_mode(), KeyboardMode::OnDemand);
            p.editor
                .buffer()
                .set_text("Isolated Layer Shell smoke test");
            assert!(p.finish());
            assert_eq!(
                std::fs::read_to_string(p.root.join("data/draft.txt")).unwrap(),
                "Isolated Layer Shell smoke test"
            );
            assert_eq!(p.window.keyboard_mode(), KeyboardMode::None);
            p.hide();
            assert!(!p.window.is_visible());
            p.show();
            glib::timeout_add_local_once(Duration::from_millis(400), move || {
                assert!(p.window.is_mapped());
                assert!(!p.window.is_active());
                println!("PASS: real Wayland map, Top layer, non-focus Preview, mode changes, save, hide/remap");
                app.quit();
            });
        });
    }
}
fn main() -> glib::ExitCode {
    let command = std::env::args().nth(1).unwrap_or_else(|| "toggle".into());
    if ![
        "toggle",
        "show",
        "hide",
        "preview",
        "quit",
        "smoke",
        "dialog-smoke",
    ]
    .contains(&command.as_str())
    {
        eprintln!(
            "Usage: layer-shell-prototype [toggle|show|hide|preview|quit|smoke|dialog-smoke]"
        );
        return glib::ExitCode::FAILURE;
    }
    let id = if command.ends_with("smoke") {
        format!("{ID}.smoke")
    } else {
        ID.to_owned()
    };
    let app = gtk::Application::builder()
        .application_id(&id)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let slot = Rc::new(RefCell::new(None::<Rc<Probe>>));
    app.connect_command_line(move |app, invocation| {
        let args = invocation.arguments();
        let command = args.get(1).and_then(|s| s.to_str()).unwrap_or("toggle");
        if slot.borrow().is_none() {
            if matches!(command, "quit" | "hide" | "preview") {
                return glib::ExitCode::SUCCESS;
            }
            if !gtk4_layer_shell::is_supported() || gtk4_layer_shell::protocol_version() < 4 {
                eprintln!(
                    "This prototype requires Wayland Layer Shell v4 (on-demand keyboard focus)."
                );
                return glib::ExitCode::FAILURE;
            }
            let root = std::env::var_os("WAYNOTE_PROTOTYPE_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    std::env::current_dir()
                        .unwrap()
                        .join("target/layer-prototype")
                });
            let root = if command.ends_with("smoke") {
                root.join("smoke")
            } else {
                root
            };
            *slot.borrow_mut() = Some(Probe::new(app, root));
        }
        let p = slot.borrow().as_ref().unwrap().clone();
        match command {
            "quit" => {
                if p.finish() {
                    app.quit();
                }
            }
            "hide" => p.hide(),
            "preview" => {
                p.finish();
            }
            "toggle" if p.window.is_visible() => p.hide(),
            "smoke" => p.smoke(app),
            "dialog-smoke" => {
                p.show();
                let app = app.clone();
                glib::timeout_add_local_once(Duration::from_millis(400), move || {
                    p.file.emit_clicked();
                    assert!(!p.file.is_sensitive());
                    glib::timeout_add_local_once(Duration::from_secs(2), move || {
                        assert!(p.window.is_mapped());
                        let cancel = p.chooser.borrow().as_ref().expect("chooser remains open").clone();
                        cancel.cancel();
                        glib::timeout_add_local_once(Duration::from_secs(1), move || {
                            assert!(p.chooser.borrow().is_none());
                            assert!(p.file.is_sensitive());
                            assert!(p.window.is_mapped());
                            assert_eq!(p.window.keyboard_mode(), KeyboardMode::None);
                            assert!(!p.window.is_active());
                            println!("PASS: file chooser open/cancel, callback, layer survives, Preview remains unfocused");
                            app.quit();
                        });
                    });
                });
            },
            _ => p.show(),
        }
        glib::ExitCode::SUCCESS
    });
    app.run_with_args(&["layer-shell-prototype", &command])
}
