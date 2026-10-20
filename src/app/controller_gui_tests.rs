//! Run inside an isolated XDG directory and D-Bus session with a GTK display.
use super::*;

fn pump() {
    let context = glib::MainContext::default();
    let deadline = std::time::Instant::now() + Duration::from_millis(120);
    while std::time::Instant::now() < deadline {
        for _ in 0..100 {
            if !context.pending() {
                break;
            }
            context.iteration(false);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn escape(this: &Rc<RefCell<Controller>>) {
    let controllers = this.borrow().window.observe_controllers();
    let keys = (0..controllers.n_items())
        .filter_map(|index| controllers.item(index))
        .find_map(|item| item.downcast::<gtk::EventControllerKey>().ok())
        .unwrap();
    assert!(keys.emit_by_name::<bool>(
        "key-pressed",
        &[&gdk::Key::Escape, &0u32, &gdk::ModifierType::empty()],
    ));
    pump();
}

fn checkbox(widget: &gtk::Widget) -> Option<gtk::CheckButton> {
    if let Ok(check) = widget.clone().downcast::<gtk::CheckButton>() {
        return Some(check);
    }
    let mut child = widget.first_child();
    while let Some(widget) = child {
        if let Some(check) = checkbox(&widget) {
            return Some(check);
        }
        child = widget.next_sibling();
    }
    None
}
fn selector_button(board: &Controller, title: &str) -> gtk::Button {
    let mut row = board.selector.first_child();
    while let Some(widget) = row {
        let button = widget
            .clone()
            .downcast::<gtk::ListBoxRow>()
            .unwrap()
            .child()
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap();
        let content = button.child().unwrap().downcast::<gtk::Box>().unwrap();
        let text = content
            .first_child()
            .unwrap()
            .downcast::<gtk::Box>()
            .unwrap();
        let label = text
            .first_child()
            .unwrap()
            .downcast::<gtk::Label>()
            .unwrap();
        if label.text() == title {
            assert_eq!(label.xalign(), 0.0);
            let snippet = label
                .next_sibling()
                .unwrap()
                .downcast::<gtk::Label>()
                .unwrap();
            assert_eq!(snippet.xalign(), 0.0);
            assert_ne!(
                snippet.text(),
                label.text(),
                "Summary must not repeat the title"
            );
            return button;
        }
        row = widget.next_sibling();
    }
    panic!("Missing selector entry: {title}");
}

fn assert_selected(board: &Controller, id: &str) {
    assert_eq!(board.active.as_deref(), Some(id));
    assert_eq!(
        board.pages.visible_child(),
        Some(board.cards[id].root.clone().upcast())
    );
    assert_eq!(
        board
            .cards
            .values()
            .filter(|card| card.root.is_child_visible())
            .count(),
        1
    );
}

#[test]
#[ignore = "requires an isolated GTK display, XDG directories and D-Bus session"]
fn board_gui_regressions() {
    gtk::init().expect("GTK display required");
    let app = gtk::Application::builder()
        .application_id("dev.mryll.waynote.test")
        .build();
    app.register(None::<&gio::Cancellable>).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let paths = paths::Paths::from_env(|key| match key {
        "XDG_DATA_HOME" | "XDG_CONFIG_HOME" | "XDG_STATE_HOME" => {
            Some(directory.path().join(key).to_string_lossy().into_owned())
        }
        _ => None,
    });
    let this = Controller::with_paths(&app, paths);
    crate::app::actions::register(&app, &this);
    assert!(
        this.borrow().cards.is_empty(),
        "Run against isolated empty XDG paths"
    );
    Controller::show(&this);
    pump();
    assert_eq!(app.windows().len(), 1);
    assert!(!this.borrow().window.is_decorated());
    assert!(this.borrow().window.titlebar().is_none());
    assert_eq!(this.borrow().pages.margin_top(), 0);
    assert_eq!(this.borrow().cards.len(), 1);
    let first = this.borrow().cards.keys().next().unwrap().clone();
    assert!(this.borrow().cards[&first].editing());
    assert_selected(&this.borrow(), &first);
    assert!(!this.borrow().cards[&first].status.is_visible());
    let editor = this.borrow().cards[&first].editor.clone();
    editor.buffer().set_text("# Draft\n\n- [ ] Task\n");
    // Change the allocation independently of GTK's initial default size.
    this.borrow().window.set_size_request(630, 570);
    pump();
    let resized = (this.borrow().window.width(), this.borrow().window.height());
    // Hide must flush the buffer even before its deferred changed event runs.
    Controller::toggle(&this);
    assert_eq!(
        this.borrow().window.default_size(),
        resized,
        "Hide must retain the resized dimensions for the next map"
    );
    this.borrow().window.set_size_request(-1, -1);
    pump();
    assert!(!this.borrow().window.is_visible());
    assert_eq!(
        crate::core::frontmatter::parse(
            &std::fs::read_to_string(&this.borrow().cards[&first].path).unwrap()
        )
        .body,
        "# Draft\n\n- [ ] Task\n"
    );
    Controller::toggle(&this);
    pump();
    assert_eq!(
        (this.borrow().window.width(), this.borrow().window.height()),
        resized,
        "Show must preserve the last window size"
    );
    assert!(!this.borrow().cards[&first].editing());
    assert_eq!(
        this.borrow().cards.len(),
        1,
        "Summon must not create duplicates"
    );
    assert_eq!(app.windows().len(), 1);
    Controller::edit(&this, &first, None);
    editor
        .buffer()
        .set_text("# Draft\n\n- [ ] Task\n\nSaved with Escape\n");
    escape(&this);
    assert!(
        this.borrow().window.is_visible(),
        "Edit Escape keeps the note visible"
    );
    assert!(!this.borrow().cards[&first].editing());
    assert!(std::fs::read_to_string(&this.borrow().cards[&first].path)
        .unwrap()
        .contains("Saved with Escape"));
    escape(&this);
    assert!(
        !this.borrow().window.is_visible(),
        "Preview Escape hides the note"
    );
    Controller::show(&this);
    pump();
    Controller::create(&this);
    let queued = this
        .borrow()
        .cards
        .keys()
        .find(|id| *id != &first)
        .unwrap()
        .clone();
    Controller::toggle(&this); // hide before the deferred create->edit callback
    pump();
    Controller::toggle(&this);
    pump();
    assert!(this.borrow().cards.values().all(|card| !card.editing()));
    this.borrow_mut().delete(&queued);
    let check = checkbox(&this.borrow().cards[&first].preview.clone().upcast()).unwrap();
    check.set_active(true);
    pump();
    assert!(this.borrow().cards[&first].note.body.contains("- [x] Task"));
    assert!(
        !this.borrow().cards[&first].editing(),
        "Checkbox does not enter edit mode"
    );
    // Exercise the actual Preview click handler, including GTK's internal text child.
    let preview = this.borrow().cards[&first]
        .preview
        .child()
        .unwrap()
        .downcast::<gtk::TextView>()
        .unwrap();
    assert!(preview_edit_line(&preview, 12.0, 12.0).is_some());
    let controllers = preview.observe_controllers();
    let click = (0..controllers.n_items())
        .filter_map(|index| controllers.item(index))
        .filter_map(|item| item.downcast::<gtk::GestureClick>().ok())
        .find(|gesture| gesture.button() == 1)
        .unwrap();
    click.emit_by_name::<()>("pressed", &[&1i32, &12.0f64, &12.0f64]);
    click.emit_by_name::<()>("released", &[&1i32, &12.0f64, &12.0f64]);
    pump();
    assert!(
        !this.borrow().cards[&first].editing(),
        "Single click stays in Preview"
    );
    // A double click explicitly enters Edit even if Preview text is selected.
    let preview_buffer = preview.buffer();
    preview_buffer.select_range(
        &preview_buffer.start_iter(),
        &preview_buffer.iter_at_offset(2),
    );
    click.emit_by_name::<()>("pressed", &[&2i32, &12.0f64, &12.0f64]);
    pump();
    assert!(this.borrow().cards[&first].editing());
    let buffer = editor.buffer();
    buffer.place_cursor(&buffer.start_iter());
    editor.emit_by_name::<()>(
        "move-cursor",
        &[&gtk::MovementStep::LogicalPositions, &2i32, &false],
    );
    pump();
    assert!(this.borrow().cards[&first].editing());
    assert!(
        editor.is_focus(),
        "Caret stays in the editor even when the compositor deactivates the window"
    );
    assert_eq!(buffer.cursor_position(), 2);
    editor.emit_by_name::<()>(
        "move-cursor",
        &[&gtk::MovementStep::LogicalPositions, &3i32, &true],
    );
    pump();
    assert!(buffer.has_selection());
    buffer.insert(&mut buffer.end_iter(), "\nSaved from outside click\n");
    // Focus-follows-mouse may deactivate the window without a click.
    let other = gtk::Window::new();
    other.present();
    pump();
    assert!(!this.borrow().window.is_active());
    assert!(
        this.borrow().cards[&first].editing(),
        "Window deactivation alone must not finish editing"
    );
    app.activate_action("preview", None);
    pump();
    assert!(
        !this.borrow().cards[&first].editing(),
        "Outside-click action enters Preview"
    );
    assert!(
        this.borrow().window.is_visible(),
        "Outside-click action keeps the note visible"
    );
    assert!(
        !this.borrow().window.is_active(),
        "Preview must not steal focus back"
    );
    assert!(std::fs::read_to_string(&this.borrow().cards[&first].path)
        .unwrap()
        .contains("Saved from outside click"));
    Controller::edit(&this, &first, None);
    other.destroy();
    this.borrow().window.present();
    pump();
    // The paper title is the explicit way back to Preview.
    let title = this.borrow().cards[&first].title.clone();
    let controllers = title.observe_controllers();
    let click = (0..controllers.n_items())
        .filter_map(|index| controllers.item(index))
        .find_map(|item| item.downcast::<gtk::GestureClick>().ok())
        .unwrap();
    click.emit_by_name::<()>("released", &[&1i32, &12.0f64, &12.0f64]);
    pump();
    assert!(!this.borrow().cards[&first].editing());
    Controller::edit(&this, &first, None);
    pump();
    editor.buffer().set_text("Uncommitted newest content");
    assert_eq!(
        this.borrow().cards[&first].raw(),
        "Uncommitted newest content",
        "Copy source must include newest draft"
    );
    Controller::create(&this);
    pump();
    assert!(!this.borrow().cards[&first].editing());
    assert_eq!(
        this.borrow().cards[&first].saved_body,
        "Uncommitted newest content"
    );
    assert_eq!(
        app.windows().len(),
        1,
        "Every note belongs to the same board"
    );
    let second = this.borrow().active.clone().unwrap();
    assert_selected(&this.borrow(), &second);
    let select_first = selector_button(&this.borrow(), "Uncommitted newest content");
    let summary = select_first
        .child()
        .unwrap()
        .first_child()
        .unwrap()
        .last_child()
        .unwrap()
        .downcast::<gtk::Label>()
        .unwrap();
    assert_eq!(summary.text(), "No additional text");
    select_first.emit_clicked();
    pump();
    assert_selected(&this.borrow(), &first);
    assert!(
        !this.borrow().cards[&first].editing(),
        "Switching opens Preview"
    );
    {
        let mut board = this.borrow_mut();
        board.finish_edit();
        let card = board.cards.get_mut(&first).unwrap();
        card.note.locked = true;
        card.dirty = true;
        card.editor.buffer().set_text("- [ ] Locked task");
        assert!(board.save_card(&first));
        board.rebuild_card(&first);
    }
    let check = checkbox(&this.borrow().cards[&first].preview.clone().upcast()).unwrap();
    assert!(!check.is_sensitive());
    Controller::edit(&this, &first, None);
    pump();
    assert!(!this.borrow().cards[&first].editing());
    let select_locked = selector_button(&this.borrow(), "- [ ] Locked task");
    select_locked.emit_clicked();
    pump();
    assert_eq!(this.borrow().active.as_deref(), Some(first.as_str()));
    assert!(this.borrow().cards[&first]
        .root
        .has_css_class("waynote-active"));
    assert!(selector_button(&this.borrow(), "- [ ] Locked task").has_css_class("waynote-selected"));

    // Many notes exercise the selector without displaying a card grid.
    let (source, destination) = {
        let mut board = this.borrow_mut();
        let source = board.cards.keys().find(|id| *id != &first).unwrap().clone();
        let mut destination = String::new();
        for index in 0..12 {
            destination = board
                .insert_note(Note {
                    id: ulid::Ulid::new().to_string(),
                    color: "yellow".into(),
                    locked: true,
                    tags: vec![],
                    extra: BTreeMap::new(),
                    body: format!("# Distant {index}\nRead-only target"),
                })
                .unwrap();
        }
        (source, destination)
    };
    pump();
    Controller::edit(&this, &source, None);
    pump();
    assert!(this.borrow().cards[&source].editing());
    let menu = this.borrow().cards[&source].menu.clone();
    menu.popup();
    pump();
    let long_title = "很长的中文标题与 English title ".repeat(8);
    {
        let mut board = this.borrow_mut();
        board
            .insert_note(Note {
                id: ulid::Ulid::new().to_string(),
                color: "blue".into(),
                locked: false,
                tags: vec![],
                extra: BTreeMap::new(),
                body: format!("# {long_title}\n{}", "正文摘要与 long content ".repeat(12)),
            })
            .unwrap();
        board.select(&source);
    }
    let (notes, search) = {
        let board = this.borrow();
        (board.selector_popover.clone(), board.search.clone())
    };
    menu.popdown();
    notes.popup();
    search.grab_focus();
    pump();
    assert!(
        gtk::prelude::GtkWindowExt::focus(&this.borrow().window)
            .is_some_and(|focus| focus.is_ancestor(&search)),
        "Closing a card menu must not steal Notes search focus"
    );
    let long_option = selector_button(&this.borrow(), long_title.trim());
    assert!(
        notes.width() <= 360,
        "Long text must not widen the panel beyond the note"
    );
    assert!(
        long_option.height() < 80,
        "Long text must stay in a compact two-line row"
    );
    notes.popdown();
    Controller::edit(&this, &source, None);
    pump();
    assert!(!this.borrow().cards[&first]
        .root
        .has_css_class("waynote-active"));
    search.set_text("Distant 11");
    this.borrow().refresh_selector();
    assert!(this
        .borrow()
        .selector
        .first_child()
        .unwrap()
        .next_sibling()
        .is_none());
    let select_distant = selector_button(&this.borrow(), "Distant 11");
    select_distant.emit_clicked();
    pump();
    assert_selected(&this.borrow(), &destination);
    assert!(!this.borrow().cards[&source].editing());
    this.borrow_mut().save_state();
    assert_eq!(
        layout::load(&this.borrow().paths).active.as_deref(),
        Some(destination.as_str())
    );
    search.set_text("");
    this.borrow().refresh_selector();
    assert_eq!(this.borrow().active.as_deref(), Some(destination.as_str()));
    assert!(!this.borrow().cards[&destination].editing());
    assert_eq!(
        gtk::prelude::GtkWindowExt::focus(&this.borrow().window),
        this.borrow().cards[&destination].preview.child(),
        "Selection keeps keyboard focus on its selectable preview"
    );
    assert!(this.borrow().cards[&destination]
        .root
        .has_css_class("waynote-active"));
    assert!(!this.borrow().cards[&source]
        .root
        .has_css_class("waynote-active"));
    assert!(selector_button(&this.borrow(), "Distant 11").has_css_class("waynote-selected"));
    this.borrow_mut().cards.get_mut(&first).unwrap().note.locked = false;
    Controller::edit(&this, &first, None);
    pump();
    {
        let mut board = this.borrow_mut();
        let card = board.cards.get_mut(&first).unwrap();
        card.editor.buffer().set_text("Local unsaved draft");
        std::fs::write(&card.path, "External content").unwrap();
        assert!(!board.save_card(&first));
        drop(board);
        app.activate_action("preview", None);
        pump();
        assert!(
            this.borrow().cards[&first].editing(),
            "Failed outside-click save retains Edit"
        );
        escape(&this);
        let mut board = this.borrow_mut();
        assert!(board.window.is_visible());
        assert!(
            board.cards[&first].editing(),
            "Failed-save Escape keeps the draft in Edit"
        );
        assert!(
            !board.select(&destination),
            "Failed saves prevent switching"
        );
        assert_selected(&board, &first);
        assert!(
            !board.hide(),
            "Save conflict must not discard a hidden draft"
        );
        assert!(board.cards[&first].blocked);
        assert!(
            board.cards[&first].status.is_visible(),
            "Save errors stay visible"
        );
        assert_eq!(
            std::fs::read_to_string(&board.cards[&first].path).unwrap(),
            "External content"
        );
        assert_eq!(board.cards[&first].raw(), "Local unsaved draft");
        // Explicit discard is tested only after verifying both copies survived.
        board.remove_card(&first);
        board.reload();
        let remaining: Vec<_> = board.cards.keys().cloned().collect();
        for id in remaining {
            board.delete(&id);
            if let Some(id) = &board.active {
                assert_selected(&board, id);
            }
        }
        assert!(board.cards.is_empty());
        assert!(board.empty.is_visible());
        assert!(board.hide());
    }
    Controller::show(&this);
    pump();
    assert!(
        this.borrow().cards.is_empty(),
        "Deleting the final note leaves an empty paper"
    );
    let new = this
        .borrow()
        .empty
        .last_child()
        .unwrap()
        .downcast::<gtk::Button>()
        .unwrap();
    new.emit_clicked();
    pump();
    assert_eq!(this.borrow().cards.len(), 1);
    assert!(!this.borrow().empty.is_visible());
    assert!(this.borrow().cards.values().next().unwrap().editing());
    assert_eq!(app.windows().len(), 1);
    this.borrow().window.destroy();
}

#[cfg(feature = "layer-shell-prototype")]
#[test]
#[ignore = "requires Wayland Layer Shell v4"]
fn layer_gui_regressions() {
    use gtk4_layer_shell::{Edge, KeyboardMode, LayerShell};
    std::env::set_var("WAYNOTE_LAYER_SHELL", "1");
    gtk::init().unwrap();
    assert!(gtk4_layer_shell::is_supported());
    let app = gtk::Application::builder()
        .application_id("dev.mryll.waynote.layer.test")
        .build();
    app.register(None::<&gio::Cancellable>).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let paths = paths::Paths::from_env(|_| Some(dir.path().to_string_lossy().into_owned()));
    let this = Controller::with_paths(&app, paths);
    Controller::show(&this);
    pump();
    let window = this.borrow().window.clone();
    assert!(window.is_layer_window());
    assert!(
        window.is_anchor(Edge::Bottom),
        "drag canvas must be stationary across the output"
    );
    let id = this.borrow().active.clone().unwrap();
    Controller::edit(&this, &id, None);
    this.borrow().cards[&id]
        .editor
        .buffer()
        .set_text("# Layer test\n\nSaved native note");
    assert_eq!(window.keyboard_mode(), KeyboardMode::OnDemand);
    assert!(this.borrow_mut().finish_edit());
    pump();
    assert_eq!(window.keyboard_mode(), KeyboardMode::None);
    assert!(std::fs::read_to_string(&this.borrow().cards[&id].path)
        .unwrap()
        .contains("Saved native note"));
    let popup = this.borrow().selector_popover.clone();
    popup.popup();
    pump();
    assert_eq!(window.keyboard_mode(), KeyboardMode::OnDemand);
    this.borrow().search.set_text("Layer test");
    pump();
    assert!(this.borrow().selector.first_child().is_some());
    popup.popdown();
    pump();
    assert_eq!(window.keyboard_mode(), KeyboardMode::None);
    let menu = this.borrow().cards[&id].menu.clone();
    menu.popup();
    pump();
    assert_eq!(window.keyboard_mode(), KeyboardMode::OnDemand);
    menu.popdown();
    pump();
    assert_eq!(window.keyboard_mode(), KeyboardMode::None);
    // A failed save must retain the edit buffer and prevent hiding the canvas.
    Controller::edit(&this, &id, None);
    let original_path = this.borrow().cards[&id].path.clone();
    let blocked_parent = dir.path().join("not-a-directory");
    std::fs::write(&blocked_parent, "block").unwrap();
    {
        let mut board = this.borrow_mut();
        let card = board.cards.get_mut(&id).unwrap();
        card.path = blocked_parent.join("note.md");
        card.editor.buffer().set_text("Draft survives failed save");
        assert!(!board.hide());
        assert!(board.window.is_visible());
        assert!(board.cards[&id].editing());
        assert_eq!(board.cards[&id].raw(), "Draft survives failed save");
        assert_eq!(window.keyboard_mode(), KeyboardMode::OnDemand);
        board.cards.get_mut(&id).unwrap().path = original_path;
        assert!(board.finish_edit());
    }
    pump();
    // Real pointer motion is covered by the private-compositor integration test.
    let panel = this.borrow().layer.as_ref().unwrap().widget.clone();
    assert!(window.width() > panel.width());
    assert!(window.height() > panel.height());
    panel.set_size_request(460, 480);
    pump();
    let bounds = panel.compute_bounds(&window).unwrap();
    assert!(this.borrow_mut().hide());
    pump();
    let saved = layout::load(&this.borrow().paths);
    assert_eq!((saved.width, saved.height), (panel.width(), panel.height()));
    assert_eq!(
        (saved.layer_x, saved.layer_y),
        (bounds.x() as i32, bounds.y() as i32)
    );
    Controller::show(&this);
    pump();
    assert!(window.is_mapped());
    assert_eq!(window.keyboard_mode(), KeyboardMode::None);
    assert!(!window.is_active());
    this.borrow_mut().hide();
}
