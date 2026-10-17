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
        let label = button.label().unwrap();
        if label.lines().next().unwrap().trim_start_matches("✓ ") == title {
            return button;
        }
        row = widget.next_sibling();
    }
    panic!("Missing selector entry: {title}");
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
    assert!(
        this.borrow().cards.is_empty(),
        "Run against isolated empty XDG paths"
    );
    Controller::show(&this);
    pump();
    assert_eq!(app.windows().len(), 1);
    assert_eq!(this.borrow().cards.len(), 1);
    let first = this.borrow().cards.keys().next().unwrap().clone();
    assert!(this.borrow().cards[&first].editing());
    let editor = this.borrow().cards[&first].editor.clone();
    editor.buffer().set_text("# Draft\n\n- [ ] Task\n");
    // Hide must flush the buffer even before its deferred changed event runs.
    Controller::toggle(&this);
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
    assert!(!this.borrow().cards[&first].editing());
    assert_eq!(
        this.borrow().cards.len(),
        1,
        "Summon must not create duplicates"
    );
    assert_eq!(app.windows().len(), 1);
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
    click.emit_by_name::<()>("released", &[&1i32, &12.0f64, &12.0f64]);
    pump();
    assert!(this.borrow().cards[&first].editing());
    // FlowBox activates its child after a TextView click. That must not take
    // focus from the editor and turn a caret move or text selection into Preview.
    let child = this.borrow().cards[&first]
        .root
        .parent()
        .unwrap()
        .downcast::<gtk::FlowBoxChild>()
        .unwrap();
    let buffer = editor.buffer();
    for offset in [4, 12] {
        buffer.place_cursor(&buffer.iter_at_offset(offset));
        child.emit_by_name::<()>("activate", &[]);
        pump();
        assert!(
            this.borrow().cards[&first].editing(),
            "Editor click left Edit"
        );
        assert!(editor.has_focus(), "Card container stole editor focus");
        assert_eq!(buffer.cursor_position(), offset);
    }
    buffer.select_range(&buffer.iter_at_offset(2), &buffer.iter_at_offset(10));
    child.emit_by_name::<()>("activate", &[]);
    pump();
    assert!(this.borrow().cards[&first].editing());
    assert!(editor.has_focus());
    assert!(buffer.has_selection());
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
    assert!(selector_button(&this.borrow(), "- [ ] Locked task")
        .label()
        .unwrap()
        .starts_with("✓ "));

    // One column makes the distant selector target deterministic on any display.
    let (source, destination) = {
        let mut board = this.borrow_mut();
        board.flow.set_max_children_per_line(1);
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
    notes.popdown();
    Controller::edit(&this, &source, None);
    pump();
    assert!(!this.borrow().cards[&first]
        .root
        .has_css_class("waynote-active"));
    let adjustment = this.borrow().scroller.vadjustment();
    adjustment.set_value(0.0);
    let select_distant = selector_button(&this.borrow(), "Distant 11");
    // Finishing the source editor queues a render tick before revealing the target.
    select_distant.emit_clicked();
    let revealed = adjustment.value();
    assert!(revealed > 0.0, "Fixture must overflow the board");
    pump();
    assert!(
        (adjustment.value() - revealed).abs() < 1.0,
        "Selector reveal moved from {revealed} to {} after rendering (upper={}, page={})",
        adjustment.value(),
        adjustment.upper(),
        adjustment.page_size()
    );
    assert_eq!(this.borrow().active.as_deref(), Some(destination.as_str()));
    assert!(!this.borrow().cards[&destination].editing());
    assert_eq!(
        gtk::prelude::GtkWindowExt::focus(&this.borrow().window),
        this.borrow().cards[&destination].preview.child(),
        "Locked selection keeps keyboard focus on its selectable preview"
    );
    assert!(this.borrow().cards[&destination]
        .root
        .has_css_class("waynote-active"));
    assert!(!this.borrow().cards[&source]
        .root
        .has_css_class("waynote-active"));
    assert!(selector_button(&this.borrow(), "Distant 11")
        .label()
        .unwrap()
        .starts_with("✓ "));
    {
        let mut board = this.borrow_mut();
        let card = board.cards.get_mut(&first).unwrap();
        card.note.locked = false;
        card.editor.buffer().set_text("Local unsaved draft");
        std::fs::write(&card.path, "External content").unwrap();
        assert!(!board.save_card(&first));
        assert!(
            !board.hide(),
            "Save conflict must not discard a hidden draft"
        );
        assert!(board.cards[&first].blocked);
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
        }
        assert!(board.cards.is_empty());
        assert!(board.empty.is_visible());
        assert!(board.hide());
    }
    Controller::show(&this);
    pump();
    assert!(
        this.borrow().cards.is_empty(),
        "Deleting the final note leaves an empty board"
    );
    assert_eq!(app.windows().len(), 1);
    this.borrow().window.destroy();
}
