//! One resident board. Note files are the model; GTK buffers hold unsaved drafts.
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
    flow: gtk::FlowBox,
    scroller: gtk::ScrolledWindow,
    empty: gtk::Label,
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
) {
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
}
fn scroller(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .min_content_height(220)
        .max_content_height(460)
        .propagate_natural_height(true)
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
        let flow = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .homogeneous(true)
            .column_spacing(18)
            .row_spacing(18)
            .min_children_per_line(1)
            .max_children_per_line(4)
            .valign(gtk::Align::Start)
            .margin_start(18)
            .margin_end(18)
            .margin_top(18)
            .margin_bottom(18)
            .build();
        let scroller = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .child(&flow)
            .build();
        let empty = gtk::Label::new(Some("No notes yet. Create a note to start writing."));
        empty.set_margin_top(30);
        empty.set_margin_bottom(30);
        let message = gtk::Label::new(None);
        message.set_wrap(true);
        message.set_visible(false);
        let new = gtk::Button::with_label("+ New note");
        let notes = gtk::MenuButton::builder().label("Notes ▾").build();
        let search = gtk::SearchEntry::builder()
            .placeholder_text("Search notes")
            .build();
        let selector = gtk::ListBox::new();
        selector.set_selection_mode(gtk::SelectionMode::None);
        let choices = gtk::Box::new(gtk::Orientation::Vertical, 8);
        choices.set_margin_top(10);
        choices.set_margin_bottom(10);
        choices.set_margin_start(10);
        choices.set_margin_end(10);
        choices.append(&search);
        choices.append(
            &gtk::ScrolledWindow::builder()
                .min_content_width(280)
                .max_content_height(380)
                .propagate_natural_height(true)
                .child(&selector)
                .build(),
        );
        let selector_popover = gtk::Popover::builder().child(&choices).build();
        notes.set_popover(Some(&selector_popover));
        let toolbar = gtk::HeaderBar::new();
        toolbar.pack_start(&notes);
        toolbar.pack_end(&new);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&message);
        content.append(&empty);
        content.append(&scroller);
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title(if std::env::var("WAYNOTE_DEV").as_deref() == Ok("1") {
                "Waynote — Development"
            } else {
                "Waynote"
            })
            .default_width(state.width.max(360))
            .default_height(state.height.max(300))
            .child(&content)
            .build();
        window.set_titlebar(Some(&toolbar));
        let active = state.active.clone();
        let this = Rc::new(RefCell::new(Self {
            window,
            flow,
            scroller,
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
        let weak = Rc::downgrade(&this);
        this.borrow()
            .window
            .connect_is_active_notify(move |window| {
                if !window.is_active() {
                    defer(&weak, |this| {
                        if !this.borrow().window.is_active() {
                            this.borrow_mut().finish_edit();
                        }
                    });
                }
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
        Self::start_watcher(&this);
        this
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
                    card.resolve.set_visible(true);
                    card.status
                        .set_text("Changed externally. Keep draft as a new note or reload.");
                    continue;
                }
                self.remove_card(&id);
            }
            self.add_card(loaded);
        }
        if self
            .active
            .as_ref()
            .is_some_and(|id| !self.cards.contains_key(id))
        {
            self.active = None;
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
            if let Some(child) = card.root.parent() {
                self.flow.remove(&child);
            }
        }
    }

    fn add_card(&mut self, loaded: store::LoadedNote) {
        let id = loaded.note.id.clone();
        let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
        root.add_css_class("waynote-card");
        root.add_css_class(&loaded.note.color);
        root.set_size_request(300, -1);
        root.set_hexpand(true);
        root.set_valign(gtk::Align::Start);
        root.set_widget_name(&format!("note-{id}"));
        let title = gtk::Label::new(Some(&loaded.note.title()));
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title.set_hexpand(true);
        title.set_xalign(0.0);
        title.add_css_class("waynote-header-title");
        let lock = gtk::Label::new(if loaded.note.locked {
            Some("🔒")
        } else {
            None
        });
        lock.set_tooltip_text(Some("Content is read-only"));
        let menu_button = gtk::MenuButton::builder()
            .label("⋯")
            .tooltip_text("Note actions")
            .build();
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
        let colors = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let color_popover = gtk::Popover::builder().child(&colors).build();
        for color in theme::NOTE_COLORS {
            button(color, &colors, &self.weak, &id, move |this, id| {
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
        header.append(&title);
        header.append(&lock);
        header.append(&menu_button);
        root.append(&header);
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
        let preview = gtk::ScrolledWindow::new();
        preview.set_min_content_height(220);
        preview.set_max_content_height(460);
        preview.set_propagate_natural_height(true);
        preview.set_hscrollbar_policy(gtk::PolicyType::Automatic);
        stack.add_named(&preview, Some("preview"));
        stack.add_named(&scroller(&editor), Some("edit"));
        stack.set_visible_child_name("preview");
        root.append(&stack);
        let status = gtk::Label::new(Some("Saved"));
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
        self.flow.insert(&root, -1);
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
        let focus = gtk::EventControllerFocus::new();
        let weak = self.weak.clone();
        let focus_id = id.clone();
        focus.connect_leave(move |_| {
            let id = focus_id.clone();
            defer(&weak, move |this| {
                let mut board = this.borrow_mut();
                let Some(card) = board.cards.get(&id) else {
                    return;
                };
                if card.menu.is_visible() {
                    return;
                }
                if board.active.as_deref() == Some(id.as_str()) && !card.editor.has_focus() {
                    board.finish_edit();
                }
            });
        });
        editor.add_controller(focus);
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
                content_width: 280,
            },
            toggle,
        );
        preview.set_widget_name(&format!("preview-{id}"));
        disable_tasks(&preview.clone().upcast(), card.note.locked);
        if !card.note.locked {
            let gesture = gtk::GestureClick::new();
            gesture.set_button(1);
            let view = preview.clone();
            let weak = self.weak.clone();
            let id = id.to_string();
            gesture.connect_released(move |g, _, x, y| {
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
        card.status.set_text(if card.blocked {
            "External change needs resolution"
        } else {
            "Saving…"
        });
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
                card.status.set_text("Saved");
                self.sync_known();
                true
            }
            Ok(store::SaveOutcome::Conflict(path)) => {
                card.blocked = true;
                card.resolve.set_visible(true);
                card.status.set_text(&format!(
                    "External change: draft copy saved at {}",
                    path.display()
                ));
                false
            }
            Err(e) => {
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
    fn finish_edit(&mut self) -> bool {
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
        true
    }
    fn edit(this: &Rc<RefCell<Self>>, id: &str, line: Option<i32>) {
        let editor = {
            let mut board = this.borrow_mut();
            if !board.window.is_visible() {
                return;
            }
            if board.active.as_deref() != Some(id) && !board.finish_edit() {
                return;
            }
            let Some(card) = board.cards.get(id) else {
                return;
            };
            if card.note.locked {
                return;
            }
            card.stack.set_visible_child_name("edit");
            card.root.add_css_class("waynote-active");
            let editor = card.editor.clone();
            board.active = Some(id.to_string());
            board.refresh_selector();
            editor
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
    fn refresh_selector(&self) {
        while let Some(child) = self.selector.first_child() {
            self.selector.remove(&child);
        }
        let query = self.search.text().to_lowercase();
        for (id, card) in &self.cards {
            if !card.raw().to_lowercase().contains(&query) {
                continue;
            }
            let title = card.note.title();
            let snippet: String = card
                .raw()
                .lines()
                .find(|line| !line.trim().is_empty() && !line.starts_with('#'))
                .unwrap_or("Empty note")
                .chars()
                .take(60)
                .collect();
            let selected = if self.active.as_deref() == Some(id) {
                "✓ "
            } else {
                ""
            };
            let item = gtk::Button::with_label(&format!("{selected}{title}\n{snippet}"));
            item.set_tooltip_text(Some(&card.raw()));
            let weak = self.weak.clone();
            let id = id.clone();
            item.connect_clicked(move |_| {
                if let Some(this) = weak.upgrade() {
                    let (scroller, flow, root, locked) = {
                        let mut board = this.borrow_mut();
                        if !board.finish_edit() {
                            return;
                        }
                        board.selector_popover.popdown();
                        let Some(card) = board.cards.get(&id) else {
                            return;
                        };
                        let values = (
                            board.scroller.clone(),
                            board.flow.clone(),
                            card.root.clone(),
                            card.note.locked,
                        );
                        card.root.add_css_class("waynote-active");
                        board.active = Some(id.clone());
                        board.refresh_selector();
                        values
                    };
                    if let Some(bounds) = root.compute_bounds(&flow) {
                        scroller.vadjustment().set_value(bounds.y() as f64);
                    }
                    if !locked {
                        Self::edit(&this, &id, None);
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
            board.insert_note(note)
        };
        if let Some(id) = result {
            let window = this.borrow().window.clone();
            window.present();
            let weak = Rc::downgrade(this);
            defer(&weak, move |this| Self::edit(this, &id, None));
        }
    }
    pub fn show(this: &Rc<RefCell<Self>>) {
        let (window, first, empty, scroll) = {
            let mut board = this.borrow_mut();
            let first = board.initial;
            board.initial = false;
            (
                board.window.clone(),
                first,
                board.cards.is_empty(),
                board.state.scroll,
            )
        };
        window.present();
        if first && empty {
            Self::create(this);
        } else {
            let weak = Rc::downgrade(this);
            defer(&weak, move |this| {
                this.borrow().scroller.vadjustment().set_value(scroll)
            });
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
        if self.window.width() > 0 && self.window.height() > 0 {
            self.state.width = self.window.width().max(360);
            self.state.height = self.window.height().max(300);
        }
        self.state.scroll = self.scroller.vadjustment().value();
        self.state.active = self.active.clone();
        if let Err(e) = layout::save(&self.paths, &self.state) {
            self.report(&format!("Cannot save board position: {e}"));
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
// links, selections, checkboxes and images handle their own pointer gestures.
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
    if target.buffer().has_selection() {
        return None;
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
