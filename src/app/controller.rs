//! One resident window showing one selected note. GTK buffers hold unsaved drafts.
use super::layer_window;
use crate::{
    core::{id_policy::UlidGen, markdown, note::Note, reconcile::Known, theme},
    platform::{config, layout, paths, render, store, watcher},
};
use gtk::{gdk, gio, glib, prelude::*};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    path::PathBuf,
    rc::{Rc, Weak},
    sync::{Arc, Mutex},
    time::Duration,
};

struct Card {
    note: Note,
    path: PathBuf,
    hash: String,
    saved_body: String,
    dirty: bool,
    blocked: bool,
    root: gtk::Box,
    title: gtk::Label,
    notes: gtk::MenuButton,
    stack: gtk::Stack,
    preview: gtk::ScrolledWindow,
    editor: gtk::TextView,
    menu: gtk::Popover,
    status: gtk::Label,
    resolve: gtk::Box,
    autosave: Option<glib::SourceId>,
}
impl Card {
    fn raw(&self) -> String {
        let buffer = self.editor.buffer();
        buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .to_string()
    }
    fn editing(&self) -> bool {
        self.stack.visible_child_name().as_deref() == Some("edit")
    }
}

pub struct Controller {
    window: gtk::ApplicationWindow,
    layer: Option<layer_window::Panel>,
    pages: gtk::Stack,
    empty: gtk::Box,
    message: gtk::Label,
    selector: gtk::ListBox,
    search: gtk::SearchEntry,
    selector_popover: gtk::Popover,
    cards: BTreeMap<String, Card>,
    active: Option<String>,
    initial: bool,
    paths: paths::Paths,
    config: config::Config,
    state: layout::BoardState,
    watch_state: Arc<Mutex<watcher::WatchState>>,
    _watch: Option<watcher::WatchGuard>,
    _hold: gio::ApplicationHoldGuard,
    weak: Weak<RefCell<Self>>,
}

fn defer(weak: &Weak<RefCell<Controller>>, task: impl FnOnce(&Rc<RefCell<Controller>>) + 'static) {
    let weak = weak.clone();
    glib::idle_add_local_once(move || {
        if let Some(board) = weak.upgrade() {
            task(&board);
        }
    });
}
fn button(
    label: &str,
    parent: &gtk::Box,
    weak: &Weak<RefCell<Controller>>,
    id: &str,
    action: impl Fn(&Rc<RefCell<Controller>>, &str) + 'static,
) -> gtk::Button {
    let b = gtk::Button::with_label(label);
    let weak = weak.clone();
    let id = id.to_string();
    b.connect_clicked(move |_| {
        let id = id.clone();
        let action = &action;
        // Button signals run outside controller borrows; each handler snapshots
        // GTK handles before writing a buffer or changing focus.
        if let Some(board) = weak.upgrade() {
            action(&board, &id);
        }
    });
    parent.append(&b);
    b
}
fn scroller(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .child(child)
        .build()
}

impl Controller {
    pub fn new(app: &gtk::Application) -> Rc<RefCell<Self>> {
        Self::with_paths(app, paths::system())
    }

    fn with_paths(app: &gtk::Application, paths: paths::Paths) -> Rc<RefCell<Self>> {
        render::install_card_css();
        let state = layout::load(&paths);
        let config = config::load(&paths);
        let pages = gtk::Stack::builder()
            .vexpand(true)
            .hhomogeneous(false)
            .vhomogeneous(false)
            .build();
        let empty = gtk::Box::new(gtk::Orientation::Vertical, 12);
        empty.add_css_class("waynote-card");
        empty.add_css_class(&config.default_color);
        empty.set_vexpand(true);
        let prompt = gtk::Label::new(Some("No notes yet. Create a note to start writing."));
        prompt.set_wrap(true);
        empty.append(&prompt);
        let message = gtk::Label::new(None);
        message.set_wrap(true);
        message.set_visible(false);
        let new = gtk::Button::with_label("+ New note");
        let search = gtk::SearchEntry::builder()
            .placeholder_text("Search notes")
            .build();
        let selector = gtk::ListBox::new();
        selector.set_selection_mode(gtk::SelectionMode::None);
        selector.add_css_class("waynote-note-list");
        let choices = gtk::Box::new(gtk::Orientation::Vertical, 8);
        choices.set_width_request(284);
        choices.set_margin_top(10);
        choices.set_margin_bottom(10);
        choices.set_margin_start(10);
        choices.set_margin_end(10);
        choices.append(&search);
        choices.append(
            &gtk::ScrolledWindow::builder()
                .hscrollbar_policy(gtk::PolicyType::Never)
                .max_content_height(320)
                .propagate_natural_height(true)
                .child(&selector)
                .build(),
        );
        let selector_popover = gtk::Popover::builder()
            .child(&choices)
            .halign(gtk::Align::Start)
            .build();
        selector_popover.add_css_class("waynote-selector");
        new.add_css_class("waynote-control");
        empty.append(&new);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&message);
        content.append(&empty);
        content.append(&pages);
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title(if std::env::var("WAYNOTE_DEV").as_deref() == Ok("1") {
                "Waynote — Development"
            } else {
                "Waynote"
            })
            .default_width(state.width.max(360))
            .default_height(state.height.max(300))
            .decorated(false)
            .child(&content)
            .build();
        let layer = layer_window::Panel::attach(&window, &content, &state);
        if let Some(layer) = &layer {
            let grip = gtk::Label::new(Some("◢"));
            grip.set_halign(gtk::Align::End);
            grip.set_tooltip_text(Some("Drag to resize"));
            layer.resize(&grip);
            content.append(&grip);
        }
        window.add_css_class("waynote-window");
        let active = state.active.clone();
        let this = Rc::new(RefCell::new(Self {
            window,
            layer,
            pages,
            empty,
            message,
            selector,
            search,
            selector_popover,
            cards: BTreeMap::new(),
            active,
            initial: true,
            paths,
            config,
            state,
            watch_state: Arc::new(Mutex::new(watcher::WatchState::new(vec![]))),
            _watch: None,
            _hold: app.hold(),
            weak: Weak::new(),
        }));
        this.borrow_mut().weak = Rc::downgrade(&this);
        this.borrow_mut().reload();
        let weak = Rc::downgrade(&this);
        new.connect_clicked(move |_| {
            if let Some(this) = weak.upgrade() {
                Self::create(&this);
            }
        });
        let weak = Rc::downgrade(&this);
        this.borrow().search.connect_search_changed(move |_| {
            defer(&weak, |this| this.borrow_mut().refresh_selector());
        });
        let weak = Rc::downgrade(&this);
        this.borrow().window.connect_close_request(move |_| {
            if let Some(this) = weak.upgrade() {
                this.borrow_mut().hide();
            }
            glib::Propagation::Stop
        });
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(&this);
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            if key == gdk::Key::Escape {
                if let Some(this) = weak.upgrade() {
                    let mut board = this.borrow_mut();
                    if board.selector_popover.is_visible() {
                        board.selector_popover.popdown();
                    } else if let Some(card) = board.cards.values().find(|c| c.menu.is_visible()) {
                        card.menu.popdown();
                    } else if board
                        .active
                        .as_ref()
                        .and_then(|id| board.cards.get(id))
                        .is_some_and(Card::editing)
                    {
                        board.finish_edit();
                    } else {
                        board.hide();
                    }
                }
                return glib::Propagation::Stop;
            }
            if modifiers.contains(gdk::ModifierType::CONTROL_MASK) && key == gdk::Key::s {
                if let Some(this) = weak.upgrade() {
                    this.borrow_mut().flush();
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        this.borrow().window.add_controller(keys);
        if layer_window::enabled() {
            Self::watch_popup(
                &this.borrow().selector_popover,
                &this.borrow().window,
                &Rc::downgrade(&this),
            );
            let weak = Rc::downgrade(&this);
            this.borrow().window.connect_is_active_notify(move |_| {
                defer(&weak, |this| {
                    let mut board = this.borrow_mut();
                    if !board.window.is_active() && !board.popup_visible() {
                        // Focus can follow the pointer. Save without treating
                        // focus loss as an explicit request to leave Edit.
                        board.flush();
                    }
                });
            });
        }
        Self::start_watcher(&this);
        this
    }

    fn popup_visible(&self) -> bool {
        self.selector_popover.is_visible() || self.cards.values().any(|c| c.menu.is_visible())
    }
    fn sync_keyboard(&self) {
        let editing = self
            .active
            .as_ref()
            .and_then(|id| self.cards.get(id))
            .is_some_and(Card::editing);
        layer_window::keyboard(&self.window, editing || self.popup_visible());
    }
    fn watch_popup(
        popup: &gtk::Popover,
        window: &gtk::ApplicationWindow,
        weak: &Weak<RefCell<Self>>,
    ) {
        let window = window.clone();
        popup.connect_show(move |_| layer_window::keyboard(&window, true));
        let weak = weak.clone();
        popup.connect_closed(move |_| defer(&weak, |this| this.borrow().sync_keyboard()));
    }

    fn report(&self, text: &str) {
        self.message.set_text(text);
        self.message.set_visible(!text.is_empty());
        if !text.is_empty() {
            eprintln!("waynote: {text}");
        }
    }
    fn sync_known(&self) {
        self.watch_state.lock().unwrap().known = self
            .cards
            .iter()
            .map(|(id, c)| Known {
                id: id.clone(),
                path: c.path.to_string_lossy().into_owned(),
                hash: c.hash.clone(),
            })
            .collect();
    }
    fn start_watcher(this: &Rc<RefCell<Self>>) {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut board = this.borrow_mut();
        match watcher::watch(
            &board.paths,
            Duration::from_millis(board.config.watch_debounce_ms),
            board.watch_state.clone(),
            move |_| {
                let _ = tx.send(());
            },
        ) {
            Ok(watch) => board._watch = Some(watch),
            Err(e) => board.report(&format!("File watching unavailable: {e}")),
        }
        let weak = board.weak.clone();
        glib::timeout_add_local(Duration::from_millis(150), move || {
            let Some(this) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if rx.try_iter().count() > 0 {
                this.borrow_mut().reload();
            }
            glib::ControlFlow::Continue
        });
    }

    fn reload(&mut self) {
        let loaded = match store::load_all(&self.paths, &mut UlidGen) {
            Ok(notes) => notes,
            Err(e) => {
                self.report(&format!("Cannot read notes: {e}"));
                return;
            }
        };
        let ids: Vec<_> = loaded.iter().map(|n| n.note.id.clone()).collect();
        let removed: Vec<_> = self
            .cards
            .keys()
            .filter(|id| !ids.contains(id))
            .cloned()
            .collect();
        for id in removed {
            let card = &self.cards[&id];
            if card.raw() != card.saved_body || card.dirty {
                let card = self.cards.get_mut(&id).unwrap();
                card.blocked = true;
                card.status.set_visible(true);
                card.resolve.set_visible(true);
                card.status
                    .set_text("File removed externally. Keep draft as a new note or reload.");
            } else {
                self.remove_card(&id);
            }
        }
        for loaded in loaded {
            let id = loaded.note.id.clone();
            if let Some(card) = self.cards.get_mut(&id) {
                if loaded.disk_hash == card.hash {
                    card.path = loaded.path;
                    continue;
                }
                if card.raw() != card.saved_body || card.dirty {
                    card.blocked = true;
                    card.status.set_visible(true);
                    card.resolve.set_visible(true);
                    card.status
                        .set_text("Changed externally. Keep draft as a new note or reload.");
                    continue;
                }
                self.remove_card(&id);
            }
            self.add_card(loaded);
        }
        if !self
            .active
            .as_ref()
            .is_some_and(|id| self.cards.contains_key(id))
        {
            self.active = self.cards.keys().next().cloned();
        }
        self.empty.set_visible(self.cards.is_empty());
        self.refresh_selector();
        self.sync_known();
    }
    fn remove_card(&mut self, id: &str) {
        if let Some(mut card) = self.cards.remove(id) {
            if let Some(timer) = card.autosave.take() {
                timer.remove();
            }
            card.notes.set_popover(None::<&gtk::Popover>);
            self.pages.remove(&card.root);
        }
    }

    fn add_card(&mut self, loaded: store::LoadedNote) {
        let id = loaded.note.id.clone();
        let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
        root.add_css_class("waynote-card");
        root.add_css_class(&loaded.note.color);
        root.set_size_request(300, -1);
        root.set_hexpand(true);
        root.set_vexpand(true);
        root.set_widget_name(&format!("note-{id}"));
        let title = gtk::Label::new(Some(&loaded.note.title()));
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title.set_hexpand(true);
        title.set_xalign(0.0);
        title.add_css_class("waynote-header-title");
        let lock = gtk::Label::new(if loaded.note.locked {
            Some("Locked")
        } else {
            None
        });
        lock.add_css_class("waynote-header-title");
        lock.set_tooltip_text(Some("Content is read-only"));
        let menu_button = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .tooltip_text("Note actions")
            .build();
        menu_button.add_css_class("waynote-control");
        let actions = gtk::Box::new(gtk::Orientation::Vertical, 5);
        let menu = gtk::Popover::builder().child(&actions).build();
        menu_button.set_popover(Some(&menu));
        button("Copy Markdown", &actions, &self.weak, &id, |this, id| {
            let board = this.borrow();
            let Some(card) = board.cards.get(id) else {
                return;
            };
            board.window.clipboard().set_text(&card.raw());
            card.menu.popdown();
        });
        let color_menu = gtk::MenuButton::builder().label("Color").build();
        let colors = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        colors.add_css_class("waynote-color-palette");
        let color_popover = gtk::Popover::builder().child(&colors).build();
        for color in theme::NOTE_COLORS {
            let swatch = button("", &colors, &self.weak, &id, move |this, id| {
                let mut board = this.borrow_mut();
                if let Some(card) = board.cards.get_mut(id) {
                    card.root.remove_css_class(&card.note.color);
                    card.note.color = color.into();
                    card.root.add_css_class(color);
                    card.dirty = true;
                    card.menu.popdown();
                }
                board.save_card(id);
                board.render_card(id);
            });
            swatch.set_tooltip_text(Some(color));
            swatch.add_css_class("waynote-swatch");
            swatch.add_css_class(color);
        }
        color_menu.set_popover(Some(&color_popover));
        actions.append(&color_menu);
        button(
            if loaded.note.locked {
                "Unlock content"
            } else {
                "Lock content"
            },
            &actions,
            &self.weak,
            &id,
            |this, id| {
                let mut board = this.borrow_mut();
                if !board.finish_edit() {
                    return;
                }
                if let Some(card) = board.cards.get_mut(id) {
                    card.note.locked = !card.note.locked;
                    card.dirty = true;
                }
                if board.save_card(id) {
                    board.rebuild_card(id);
                }
            },
        );
        actions.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        button("Delete to Trash", &actions, &self.weak, &id, |this, id| {
            this.borrow_mut().delete(id);
        });
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        header.add_css_class("waynote-header");
        let notes = gtk::MenuButton::builder()
            .icon_name("view-list-symbolic")
            .tooltip_text("Select a note")
            .build();
        notes.add_css_class("waynote-control");
        let new = gtk::Button::builder()
            .icon_name("list-add-symbolic")
            .tooltip_text("New note")
            .build();
        new.add_css_class("waynote-control");
        let weak = self.weak.clone();
        new.connect_clicked(move |_| {
            if let Some(this) = weak.upgrade() {
                Self::create(&this);
            }
        });
        header.append(&notes);
        header.append(&title);
        header.append(&lock);
        header.append(&new);
        header.append(&menu_button);
        if layer_window::enabled() {
            let hide = gtk::Button::with_label("Hide");
            let weak = self.weak.clone();
            hide.connect_clicked(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.borrow_mut().hide();
                }
            });
            header.append(&hide);
            root.append(&header);
            if let Some(layer) = &self.layer {
                layer.drag(&title);
            }
            Self::watch_popup(&menu, &self.window, &self.weak);
        } else {
            let handle = gtk::WindowHandle::builder().child(&header).build();
            root.append(&handle);
        }
        title.set_tooltip_text(Some("Click to preview; drag to move"));
        let preview_click = gtk::GestureClick::new();
        preview_click.set_button(1);
        let weak = self.weak.clone();
        preview_click.connect_released(move |_, _, _, _| {
            if let Some(this) = weak.upgrade() {
                this.borrow_mut().finish_edit();
            }
        });
        title.add_controller(preview_click);
        let editor = gtk::TextView::builder()
            .editable(true)
            .cursor_visible(true)
            .wrap_mode(gtk::WrapMode::WordChar)
            .accepts_tab(false)
            .build();
        editor.add_css_class("waynote-edit");
        editor.buffer().set_text(&loaded.note.body);
        editor.set_widget_name(&format!("editor-{id}"));
        let stack = gtk::Stack::new();
        stack.set_vhomogeneous(false);
        stack.set_hhomogeneous(false);
        stack.set_vexpand(true);
        let preview = gtk::ScrolledWindow::new();
        preview.set_vexpand(true);
        preview.set_hscrollbar_policy(gtk::PolicyType::Automatic);
        stack.add_named(&preview, Some("preview"));
        stack.add_named(&scroller(&editor), Some("edit"));
        stack.set_visible_child_name("preview");
        root.append(&stack);
        let status = gtk::Label::new(None);
        status.add_css_class("waynote-save-error");
        status.set_visible(false);
        status.set_xalign(0.0);
        status.set_wrap(true);
        root.append(&status);
        let resolve = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        button(
            "Keep draft as new note",
            &resolve,
            &self.weak,
            &id,
            |this, id| {
                let note = {
                    let board = this.borrow();
                    let Some(card) = board.cards.get(id) else {
                        return;
                    };
                    let mut note = card.note.clone();
                    note.body = card.raw();
                    note.id = ulid::Ulid::new().to_string();
                    note
                };
                let mut board = this.borrow_mut();
                if board.insert_note(note).is_some() {
                    board.remove_card(id);
                    board.reload();
                }
            },
        );
        button("Reload", &resolve, &self.weak, &id, |this, id| {
            // Conflict-aware saves already preserve a recovery copy on disk.
            // An external-change warning without such a copy must preserve the draft first.
            let mut board = this.borrow_mut();
            let card = &board.cards[id];
            let recovery =
                board
                    .paths
                    .trash_dir()
                    .join(format!("{}-draft-{}.md", id, ulid::Ulid::new()));
            let mut note = card.note.clone();
            note.body = card.raw();
            if let Err(e) = std::fs::create_dir_all(board.paths.trash_dir())
                .and_then(|_| store::save_to(&recovery, &note))
            {
                board.report(&format!("Cannot preserve draft: {e}"));
                return;
            }
            board.remove_card(id);
            board.reload();
            board.report(&format!(
                "Previous draft preserved at {}",
                recovery.display()
            ));
        });
        resolve.set_visible(false);
        root.append(&resolve);
        self.pages.add_named(&root, Some(&id));
        let saved_body = loaded.note.body.clone();
        self.cards.insert(
            id.clone(),
            Card {
                note: loaded.note,
                path: loaded.path,
                hash: loaded.disk_hash,
                saved_body,
                dirty: false,
                blocked: false,
                root,
                title,
                notes,
                stack,
                preview,
                editor: editor.clone(),
                menu,
                status,
                resolve,
                autosave: None,
            },
        );
        let weak = self.weak.clone();
        let edit_id = id.clone();
        editor.buffer().connect_changed(move |_| {
            let id = edit_id.clone();
            defer(&weak, move |this| Self::changed(this, &id));
        });
        let weak = self.weak.clone();
        let menu_id = id.clone();
        self.cards[&id].menu.connect_closed(move |_| {
            let id = menu_id.clone();
            defer(&weak, move |this| {
                let editor = {
                    let board = this.borrow();
                    board
                        .cards
                        .get(&id)
                        .filter(|c| {
                            c.editing()
                                && board.window.is_active()
                                && !board.selector_popover.is_visible()
                                && board.active.as_deref() == Some(id.as_str())
                                && gtk::prelude::GtkWindowExt::focus(&board.window)
                                    .is_none_or(|focus| focus.is_ancestor(&c.root))
                        })
                        .map(|c| c.editor.clone())
                };
                if let Some(editor) = editor {
                    editor.grab_focus();
                }
            });
        });
        self.wire_editor_keys(&editor, &id);
        self.render_card(&id);
    }
    fn wire_editor_keys(&self, editor: &gtk::TextView, id: &str) {
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = self.weak.clone();
        let id = id.to_string();
        keys.connect_key_pressed(move |_, key, _, mods| {
            if mods.contains(gdk::ModifierType::CONTROL_MASK) && key == gdk::Key::v {
                if let Some(this) = weak.upgrade() {
                    let clipboard = this.borrow().window.clipboard();
                    if clipboard
                        .formats()
                        .contains_type(gdk::Texture::static_type())
                    {
                        Self::paste_image(&this, &id);
                        return glib::Propagation::Stop;
                    }
                }
            }
            glib::Propagation::Proceed
        });
        editor.add_controller(keys);
    }
    fn paste_image(this: &Rc<RefCell<Self>>, id: &str) {
        let clipboard = this.borrow().window.clipboard();
        let weak = Rc::downgrade(this);
        let id = id.to_string();
        clipboard.read_texture_async(None::<&gio::Cancellable>, move |result| {
            let Some(this) = weak.upgrade() else { return };
            let Ok(Some(texture)) = result else { return };
            let board = this.borrow();
            let Some(card) = board.cards.get(&id) else {
                return;
            };
            if card.note.locked || !card.editing() {
                return;
            }
            let caps = crate::core::image_paste::ImageCaps {
                max_pixels: board.config.image_max_pixels,
                max_bytes: board.config.image_max_bytes,
                downsample: board.config.image_downsample,
            };
            let raw = crate::platform::clipboard_image::texture_to_raw(&texture);
            let ts = ulid::Ulid::new().to_string();
            match crate::platform::clipboard_image::store_pasted_image(
                &raw,
                caps,
                &id,
                &ts,
                &board.paths.attachments_dir(&id),
            ) {
                Ok(Some(snippet)) => card.editor.buffer().insert_at_cursor(&snippet),
                Ok(None) => board.report("Clipboard image exceeds configured limits."),
                Err(e) => board.report(&format!("Cannot paste image: {e}")),
            }
        });
    }
    fn render_card(&self, id: &str) {
        let Some(card) = self.cards.get(id) else {
            return;
        };
        let weak = self.weak.clone();
        let task_id = id.to_string();
        let toggle = Rc::new(move |index| {
            let id = task_id.clone();
            defer(&weak, move |this| {
                let mut board = this.borrow_mut();
                let Some(card) = board.cards.get_mut(&id) else {
                    return;
                };
                if card.note.locked {
                    return;
                }
                let raw = markdown::toggle_task(&card.raw(), index);
                card.editor.buffer().set_text(&raw);
                card.dirty = true;
                board.save_card(&id);
                board.render_card(&id);
            });
        });
        let preview = render::render_view(
            &markdown::parse(&card.raw()),
            &render::RenderOpts {
                base_dir: self.paths.notes_dir(),
                color: card.note.color.clone(),
                content_width: render::DEFAULT_CONTENT_WIDTH,
            },
            toggle,
        );
        preview.set_widget_name(&format!("preview-{id}"));
        disable_tasks(&preview.clone().upcast(), card.note.locked);
        if !card.note.locked {
            let gesture = gtk::GestureClick::new();
            gesture.set_button(1);
            // Handle the second press before TextView claims it for word selection.
            gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
            let view = preview.clone();
            let weak = self.weak.clone();
            let id = id.to_string();
            gesture.connect_pressed(move |g, presses, x, y| {
                if presses != 2 {
                    return;
                }
                let Some(line) = preview_edit_line(&view, x, y) else {
                    return;
                };
                g.set_state(gtk::EventSequenceState::Claimed);
                let id = id.clone();
                defer(&weak, move |this| Self::edit(this, &id, line));
            });
            preview.add_controller(gesture);
        }
        let old_scroll = scroll_adjustments(&card.preview.clone().upcast());
        let positions: Vec<_> = old_scroll.iter().map(gtk::Adjustment::value).collect();
        card.preview.set_child(Some(&preview));
        let restore: Vec<_> = scroll_adjustments(&card.preview.clone().upcast())
            .into_iter()
            .zip(positions)
            .collect();
        let first_frame = Cell::new(true);
        card.preview.add_tick_callback(move |_, _| {
            if first_frame.replace(false) {
                return glib::ControlFlow::Continue;
            }
            for (adjustment, value) in &restore {
                adjustment.set_value(*value);
            }
            glib::ControlFlow::Break
        });
    }
    fn rebuild_card(&mut self, id: &str) {
        let Some(card) = self.cards.get(id) else {
            return;
        };
        let loaded = store::LoadedNote {
            path: card.path.clone(),
            note: card.note.clone(),
            disk_hash: card.hash.clone(),
        };
        self.remove_card(id);
        self.add_card(loaded);
        self.refresh_selector();
    }
    fn changed(this: &Rc<RefCell<Self>>, id: &str) {
        let mut board = this.borrow_mut();
        let weak = board.weak.clone();
        let Some(card) = board.cards.get_mut(id) else {
            return;
        };
        let changed = card.raw() != card.saved_body;
        if !changed {
            return;
        }
        card.dirty = true;
        if let Some(timer) = card.autosave.take() {
            timer.remove();
        }
        let id = id.to_string();
        card.autosave = Some(glib::timeout_add_local_once(
            Duration::from_millis(450),
            move || {
                if let Some(this) = weak.upgrade() {
                    let mut board = this.borrow_mut();
                    if let Some(card) = board.cards.get_mut(&id) {
                        card.autosave = None;
                    }
                    board.save_card(&id);
                    board.refresh_selector();
                }
            },
        ));
    }
    fn save_card(&mut self, id: &str) -> bool {
        let Some(card) = self.cards.get_mut(id) else {
            return true;
        };
        if let Some(timer) = card.autosave.take() {
            timer.remove();
        }
        let raw = card.raw();
        if !card.dirty && raw == card.saved_body {
            return true;
        }
        if card.blocked {
            return false;
        }
        card.note.body = raw;
        match store::save_checked_to(
            &card.path,
            &card.note,
            &card.hash,
            &ulid::Ulid::new().to_string(),
        ) {
            Ok(store::SaveOutcome::Saved(hash)) => {
                card.hash = hash.clone();
                card.saved_body = card.note.body.clone();
                card.dirty = false;
                card.title.set_text(&card.note.title());
                card.status.set_visible(false);
                self.sync_known();
                true
            }
            Ok(store::SaveOutcome::Conflict(path)) => {
                card.blocked = true;
                card.status.set_visible(true);
                card.resolve.set_visible(true);
                card.status.set_text(&format!(
                    "External change: draft copy saved at {}",
                    path.display()
                ));
                false
            }
            Err(e) => {
                card.status.set_visible(true);
                card.status
                    .set_text(&format!("Save failed: {e}. Draft kept here."));
                false
            }
        }
    }
    fn flush(&mut self) -> bool {
        let ids: Vec<_> = self.cards.keys().cloned().collect();
        let mut success = true;
        for id in ids {
            success = self.save_card(&id) && success;
        }
        success
    }
    /// Called only by the compositor's non-consuming mouse-release binding.
    pub(super) fn preview_after_outside_click(&mut self) {
        if self.window.is_visible() && !self.window.is_active() && !self.popup_visible() {
            self.finish_edit();
        }
    }
    pub(super) fn finish_edit(&mut self) -> bool {
        let Some(id) = self.active.clone() else {
            return true;
        };
        if !self.cards.get(&id).is_some_and(Card::editing) {
            if let Some(card) = self.cards.get(&id) {
                card.root.remove_css_class("waynote-active");
            }
            return true;
        }
        if !self.save_card(&id) {
            return false;
        }
        if let Some(card) = self.cards.get(&id) {
            card.stack.set_visible_child_name("preview");
            card.root.remove_css_class("waynote-active");
        }
        self.render_card(&id);
        self.sync_keyboard();
        true
    }
    fn edit(this: &Rc<RefCell<Self>>, id: &str, line: Option<i32>) {
        let editor = {
            let mut board = this.borrow_mut();
            if !board.window.is_visible() {
                return;
            }
            if !board.select(id) {
                return;
            }
            let Some(card) = board.cards.get(id) else {
                return;
            };
            if card.note.locked {
                return;
            }
            layer_window::keyboard(&board.window, true);
            card.stack.set_visible_child_name("edit");
            card.root.add_css_class("waynote-active");
            card.editor.clone()
        };
        editor.grab_focus();
        if let Some(line) = line {
            let buffer = editor.buffer();
            let iter = buffer
                .iter_at_line(line)
                .unwrap_or_else(|| buffer.end_iter());
            buffer.place_cursor(&iter);
        }
    }
    fn select(&mut self, id: &str) -> bool {
        if !self.cards.contains_key(id)
            || (self.active.as_deref() != Some(id) && !self.finish_edit())
        {
            return false;
        }
        self.active = Some(id.to_string());
        self.refresh_selector();
        true
    }
    fn refresh_selector(&self) {
        self.pages.set_visible(!self.cards.is_empty());
        // One shared selector is attached only to the visible note's control.
        for (id, card) in &self.cards {
            if self.active.as_deref() != Some(id) && card.notes.popover().is_some() {
                card.notes.set_popover(None::<&gtk::Popover>);
            }
        }
        if let Some(id) = &self.active {
            if let Some(card) = self.cards.get(id) {
                if card.notes.popover().is_none() {
                    card.notes.set_popover(Some(&self.selector_popover));
                }
            }
            self.pages.set_visible_child_name(id);
        }
        while let Some(child) = self.selector.first_child() {
            self.selector.remove(&child);
        }
        let query = self.search.text().to_lowercase();
        for (id, card) in &self.cards {
            if !card.raw().to_lowercase().contains(&query) {
                continue;
            }
            let title = card.note.title();
            let raw = card.raw();
            let snippet = raw
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty() && !line.starts_with('#') && *line != title)
                .unwrap_or("No additional text");
            let text = gtk::Box::new(gtk::Orientation::Vertical, 4);
            text.set_hexpand(true);
            for (value, class) in [
                (title.as_str(), "waynote-note-title"),
                (snippet, "waynote-note-snippet"),
            ] {
                let label = gtk::Label::builder()
                    .label(value)
                    .xalign(0.0)
                    .ellipsize(gtk::pango::EllipsizeMode::End)
                    .max_width_chars(28)
                    .build();
                label.add_css_class(class);
                text.append(&label);
            }
            let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            content.append(&text);
            let selected = self.active.as_deref() == Some(id);
            let check = gtk::Image::from_icon_name("object-select-symbolic");
            check.set_opacity(if selected { 1.0 } else { 0.0 });
            content.append(&check);
            let item = gtk::Button::builder().child(&content).build();
            item.add_css_class("waynote-note-option");
            if selected {
                item.add_css_class("waynote-selected");
            }
            item.set_tooltip_text(Some(&raw));
            let weak = self.weak.clone();
            let id = id.clone();
            item.connect_clicked(move |_| {
                if let Some(this) = weak.upgrade() {
                    let preview = {
                        let mut board = this.borrow_mut();
                        if !board.finish_edit() || !board.select(&id) {
                            return;
                        }
                        board.selector_popover.popdown();
                        let card = &board.cards[&id];
                        card.root.add_css_class("waynote-active");
                        card.preview.child()
                    };
                    if let Some(preview) = preview {
                        preview.grab_focus();
                    }
                }
            });
            self.selector.append(&item);
        }
    }
    fn insert_note(&mut self, note: Note) -> Option<String> {
        let id = note.id.clone();
        let path = self
            .paths
            .notes_dir()
            .join(format!("{}-{}.md", id, note.slug()));
        match store::save_checked_to(&path, &note, "", &ulid::Ulid::new().to_string()) {
            Ok(store::SaveOutcome::Saved(hash)) => {
                self.add_card(store::LoadedNote {
                    path,
                    note,
                    disk_hash: hash,
                });
                self.sync_known();
                self.refresh_selector();
                self.empty.set_visible(false);
                Some(id)
            }
            Ok(store::SaveOutcome::Conflict(_)) => {
                self.report("New note path already exists; try again.");
                None
            }
            Err(e) => {
                self.report(&format!("Cannot create note: {e}"));
                None
            }
        }
    }
    pub fn create(this: &Rc<RefCell<Self>>) {
        let result = {
            let mut board = this.borrow_mut();
            if !board.finish_edit() {
                return;
            }
            let note = Note {
                id: ulid::Ulid::new().to_string(),
                color: board.config.default_color.clone(),
                locked: false,
                tags: vec![],
                extra: BTreeMap::new(),
                body: String::new(),
            };
            let id = board.insert_note(note);
            if let Some(id) = &id {
                board.select(id);
            }
            id
        };
        if let Some(id) = result {
            let window = this.borrow().window.clone();
            layer_window::show(&window);
            let weak = Rc::downgrade(this);
            defer(&weak, move |this| Self::edit(this, &id, None));
        }
    }
    pub fn show(this: &Rc<RefCell<Self>>) {
        let (window, first, empty) = {
            let mut board = this.borrow_mut();
            let first = board.initial;
            board.initial = false;
            (board.window.clone(), first, board.cards.is_empty())
        };
        layer_window::show(&window);
        if first && empty {
            Self::create(this);
        }
    }
    pub fn toggle(this: &Rc<RefCell<Self>>) {
        let visible = this.borrow().window.is_visible();
        if visible {
            this.borrow_mut().hide();
        } else {
            Self::show(this);
        }
    }
    fn save_state(&mut self) {
        if let Some(layer) = &self.layer {
            layer.save(&mut self.state);
        } else if self.window.width() > 0 && self.window.height() > 0 {
            self.state.width = self.window.width().max(360);
            self.state.height = self.window.height().max(300);
            self.window
                .set_default_size(self.state.width, self.state.height);
        }
        self.state.active = self.active.clone();
        if let Err(e) = layout::save(&self.paths, &self.state) {
            self.report(&format!("Cannot save window state: {e}"));
        }
    }
    pub fn hide(&mut self) -> bool {
        if !self.flush() {
            self.report(
                "Resolve the highlighted save error before hiding. Your draft is still here.",
            );
            return false;
        }
        self.finish_edit();
        self.save_state();
        self.selector_popover.popdown();
        for card in self.cards.values() {
            card.menu.popdown();
        }
        self.window.set_visible(false);
        true
    }
    pub fn quit(&mut self) -> bool {
        if !self.hide() {
            return false;
        }
        if let Some(app) = self.window.application() {
            app.quit();
        }
        true
    }
    fn delete(&mut self, id: &str) {
        if !self.save_card(id) {
            return;
        }
        let Some(card) = self.cards.get(id) else {
            return;
        };
        let destination = self.paths.trash_dir().join(card.path.file_name().unwrap());
        let result = std::fs::create_dir_all(self.paths.trash_dir()).and_then(|_| {
            if destination.exists() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "Trash already contains this note",
                ));
            }
            std::fs::rename(&card.path, &destination)
        });
        if let Err(e) = result {
            self.report(&format!("Cannot move note to Trash: {e}"));
            return;
        }
        self.remove_card(id);
        if self.active.as_deref() == Some(id) {
            self.active = self.cards.keys().next().cloned();
        }
        self.sync_known();
        self.refresh_selector();
        self.empty.set_visible(self.cards.is_empty());
        self.report(&format!(
            "Note moved to {}. Its Markdown file can be restored to the notes folder.",
            destination.display()
        ));
    }
}
fn disable_tasks(widget: &gtk::Widget, locked: bool) {
    if widget.is::<gtk::CheckButton>() {
        widget.set_sensitive(!locked);
    }
    let mut child = widget.first_child();
    while let Some(widget) = child {
        disable_tasks(&widget, locked);
        child = widget.next_sibling();
    }
}

#[cfg(test)]
#[path = "controller_gui_tests.rs"]
mod gui_tests;

// GtkText is an implementation child; table cells are nested TextViews. Let
// links, checkboxes and images handle their own pointer gestures.
fn preview_edit_line(view: &gtk::TextView, x: f64, y: f64) -> Option<Option<i32>> {
    let mut picked = view.pick(x, y, gtk::PickFlags::DEFAULT);
    let mut target = view.clone();
    let mut point = gtk::graphene::Point::new(x as f32, y as f32);
    while let Some(widget) = picked {
        if widget == view.clone().upcast::<gtk::Widget>() {
            break;
        }
        if widget.is::<gtk::CheckButton>() || widget.is::<gtk::Picture>() {
            return None;
        }
        if let Some(cell) = widget.downcast_ref::<gtk::TextView>() {
            point = view.compute_point(cell, &point)?;
            target = cell.clone();
            break;
        }
        picked = widget.parent();
    }
    let (bx, by) = target.window_to_buffer_coords(
        gtk::TextWindowType::Widget,
        point.x() as i32,
        point.y() as i32,
    );
    let line = if let Some(iter) = target.iter_at_location(bx, by) {
        if iter
            .tags()
            .iter()
            .any(|tag| tag.name().is_some_and(|name| name.starts_with("link|")))
        {
            return None;
        }
        iter.line()
    } else {
        0
    };
    Some(if target == *view { Some(line) } else { None })
}

fn scroll_adjustments(widget: &gtk::Widget) -> Vec<gtk::Adjustment> {
    let mut result = Vec::new();
    if let Some(scroller) = widget.downcast_ref::<gtk::ScrolledWindow>() {
        result.extend([scroller.vadjustment(), scroller.hadjustment()]);
    }
    let mut child = widget.first_child();
    while let Some(widget) = child {
        result.extend(scroll_adjustments(&widget));
        child = widget.next_sibling();
    }
    result
}
