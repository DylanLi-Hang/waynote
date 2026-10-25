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
    // Force inactivity independently of compositor focus policy. This bypasses
    // Controller::hide(), which intentionally finishes editing.
    window.set_visible(false);
    pump();
    assert!(!window.is_active());
    window.notify("is-active");
    pump();
    assert!(
        this.borrow().cards[&id].editing(),
        "focus loss must retain Edit"
    );
    assert_eq!(window.keyboard_mode(), KeyboardMode::OnDemand);
    assert!(std::fs::read_to_string(&this.borrow().cards[&id].path)
        .unwrap()
        .contains("Saved native note"));
    window.set_visible(true);
    pump();
    assert!(this.borrow().cards[&id].editing());
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
    // Optional private-compositor pointer regression. Never use a desktop seat.
    if let Ok(probe) = std::env::var("WAYNOTE_TEST_POINTER") {
        use std::io::{BufRead, Write};
        use std::process::{Command, Stdio};
        assert!(std::env::var("XDG_RUNTIME_DIR")
            .unwrap()
            .starts_with("/tmp/waynote-focus-runtime-"));
        let mut child = Command::new(probe)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        let mut output = std::io::BufReader::new(child.stdout.take().unwrap());
        let mut send = |command: &str| {
            writeln!(input, "{command}").unwrap();
            input.flush().unwrap();
            let mut response = String::new();
            output.read_line(&mut response).unwrap();
            assert_eq!(response.trim(), "ok");
            pump();
        };
        window.set_visible(false);
        pump();
        window.set_visible(true);
        pump();
        let preview = this.borrow().cards[&id].preview.child().unwrap();
        let point = preview
            .compute_point(&window, &gtk::graphene::Point::new(12.0, 12.0))
            .unwrap();
        send(&format!("move {} {}", point.x() as i32, point.y() as i32));
        assert!(!window.is_active(), "Preview has no keyboard focus");
        // Deliver the real second-press handler while pointer focus is inside
        // but keyboard focus is still outside (Layer Shell None -> OnDemand).
        let controllers = preview.observe_controllers();
        let click = (0..controllers.n_items())
            .filter_map(|index| controllers.item(index))
            .filter_map(|item| item.downcast::<gtk::GestureClick>().ok())
            .find(|gesture| gesture.button() == 1)
            .unwrap();
        click.emit_by_name::<()>("pressed", &[&2i32, &12.0f64, &12.0f64]);
        pump();
        assert!(
            this.borrow().cards[&id].editing(),
            "double click enters Edit"
        );
        assert!(!window.is_active(), "keyboard focus has not arrived yet");
        this.borrow_mut().preview_after_outside_click();
        assert!(
            this.borrow().cards[&id].editing(),
            "inside release must retain Edit before keyboard focus arrives"
        );
        send("move 1200 850");
        assert!(
            this.borrow().cards[&id].editing(),
            "hover away retains Edit"
        );
        this.borrow_mut().preview_after_outside_click();
        assert!(
            !this.borrow().cards[&id].editing(),
            "outside release enters Preview"
        );
        // Check rendered allocation after EACH motion while the button remains
        // held. A drag_end-only implementation must fail on the first move.
        let panel = this.borrow().layer.as_ref().unwrap().widget.clone();
        let title = this.borrow().cards[&id].title.clone();
        let origin = panel.compute_bounds(&window).unwrap();
        let grab = title
            .compute_point(&window, &gtk::graphene::Point::new(24.0, 12.0))
            .unwrap();
        let (sx, sy) = (grab.x().round() as i32, grab.y().round() as i32);
        let max_x = window.width() as f32 - panel.width() as f32 / 3.0;
        let max_y = window.height() as f32 - panel.height() as f32 / 3.0;
        send(&format!("move {sx} {sy}"));
        send("press");
        for (x, y) in [
            (sx + 80, sy + 40),
            (sx + 160, sy + 80),
            (sx + 60, sy + 20), // Reverse without releasing.
            (1, 1),
            (window.width() - 2, window.height() - 2),
            (sx, sy), // Recover from clamping without accumulating drift.
        ] {
            send(&format!("move {x} {y}"));
            let actual = panel.compute_bounds(&window).unwrap();
            let expected = (
                (origin.x() + (x - sx) as f32).clamp(-panel.width() as f32 * 2.0 / 3.0, max_x),
                (origin.y() + (y - sy) as f32).clamp(0.0, max_y),
            );
            assert!(
                (actual.x() - expected.0).abs() <= 1.0 && (actual.y() - expected.1).abs() <= 1.0,
                "must follow before release at ({x}, {y}): expected {expected:?}, got {actual:?}"
            );
        }
        send("release");
        let released = panel.compute_bounds(&window).unwrap();
        send(&format!("move {} {}", sx + 100, sy + 60));
        let stopped = panel.compute_bounds(&window).unwrap();
        assert_eq!(released, stopped, "hover after release must not move panel");
        drop(input);
        assert!(child.wait().unwrap().success());
    }
    // A click notification while a popup is open must not leave Edit.
    Controller::edit(&this, &id, None);
    popup.popup();
    pump();
    this.borrow_mut().preview_after_outside_click();
    assert!(this.borrow().cards[&id].editing());
    popup.popdown();
    pump();
    // Focus loss/hover alone retains Edit (checked above). Only the explicit
    // compositor click notification takes the existing save/Preview path.
    window.set_visible(false);
    pump();
    this.borrow_mut().preview_after_outside_click();
    assert!(
        this.borrow().cards[&id].editing(),
        "Hidden notes ignore notifications"
    );
    window.set_visible(true);
    pump();
    assert!(
        !window.is_active(),
        "Run layer focus regressions in a private compositor"
    );
    this.borrow_mut().preview_after_outside_click();
    assert!(!this.borrow().cards[&id].editing());
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
    // Real pointer motion is checked above when WAYNOTE_TEST_POINTER is supplied.
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

#[cfg(feature = "layer-shell-prototype")]
#[test]
#[ignore = "requires private two-output Wayland compositor and virtual pointer"]
fn layer_transfer_gui_regressions() {
    use gtk4_layer_shell::{KeyboardMode, LayerShell};
    use std::io::{BufRead, Write};
    use std::process::{Command, Stdio};
    assert!(std::env::var("XDG_RUNTIME_DIR")
        .unwrap()
        .starts_with("/tmp/waynote-focus-runtime-"));
    std::env::set_var("WAYNOTE_LAYER_SHELL", "1");
    gtk::init().unwrap();
    let app = gtk::Application::builder()
        .application_id("dev.mryll.waynote.transfer.test")
        .build();
    app.register(None::<&gio::Cancellable>).unwrap();
    let display = gtk::gdk::Display::default().unwrap();
    let mut monitors: Vec<_> = display
        .monitors()
        .iter::<gtk::gdk::Monitor>()
        .map(Result::unwrap)
        .collect();
    monitors.sort_by_key(|m| (m.geometry().x(), m.geometry().y()));
    assert_eq!(monitors.len(), 2);
    let dir = tempfile::tempdir().unwrap();
    let paths = paths::Paths::from_env(|_| Some(dir.path().to_string_lossy().into_owned()));
    layout::save(
        &paths,
        &layout::BoardState {
            width: 394,
            height: 712,
            layer_x: 2367,
            layer_y: 688,
            layer_monitor: monitors[1].connector().map(|s| s.to_string()),
            ..Default::default()
        },
    )
    .unwrap();
    let this = Controller::with_paths(&app, paths.clone());
    Controller::show(&this);
    pump();
    let window = this.borrow().window.clone();
    let panel = this.borrow().layer.as_ref().unwrap().widget.clone();
    let restored_rect = monitors[1].geometry();
    assert_eq!(
        (window.width(), window.height()),
        (restored_rect.width(), restored_rect.height()),
        "saved big-screen offsets must not enlarge the smaller output canvas"
    );
    let restored_bounds = panel.compute_bounds(&window).unwrap();
    assert!(
        restored_bounds.x() + restored_bounds.width() / 3.0 <= restored_rect.width() as f32 + 1.0
            && restored_bounds.y() + restored_bounds.height() / 3.0
                <= restored_rect.height() as f32 + 1.0,
        "restored panel must be visible and reachable on the small screen"
    );
    let id = this.borrow().active.clone().unwrap();
    let title = this.borrow().cards[&id].title.clone();
    let mut child = Command::new(std::env::var("WAYNOTE_TEST_POINTER").unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = std::io::BufReader::new(child.stdout.take().unwrap());
    let mut send = |command: &str| {
        writeln!(input, "{command}").unwrap();
        input.flush().unwrap();
        let mut response = String::new();
        output.read_line(&mut response).unwrap();
        assert_eq!(response.trim(), "ok");
        pump();
    };
    for destination in [&monitors[0], &monitors[1], &monitors[0]] {
        Controller::edit(&this, &id, None);
        this.borrow().cards[&id]
            .editor
            .buffer()
            .set_text("# Cross-screen draft\n\nKeep this text.");
        pump();
        let source = window.monitor().unwrap();
        let origin = panel.compute_bounds(&window).unwrap();
        let point = title
            .compute_point(&window, &gtk::graphene::Point::new(20.0, 12.0))
            .unwrap();
        let sx = source.geometry().x() + point.x().round() as i32;
        let sy = source.geometry().y() + point.y().round() as i32;
        let grab = (point.x() - origin.x(), point.y() - origin.y());
        send(&format!("move {sx} {sy}"));
        send("press");
        send(&format!("move {} {}", sx + 60, sy + 30));
        let moved = panel.compute_bounds(&window).unwrap();
        assert!(
            (moved.x()
                - (origin.x() + 60.0).min(window.width() as f32 - panel.width() as f32 / 3.0))
            .abs()
                <= 1.0,
            "production title must follow before release"
        );
        let rect = destination.geometry();
        // Drop near the destination's far edge to expose stale clamps and
        // oversized GtkFixed minima when subsequently returning to a small output.
        let x = rect.x() + rect.width() - 80;
        let y = rect.y() + rect.height() - 80;
        send(&format!("move {x} {y}"));
        send("release");
        pump();
        assert_eq!(window.monitor().as_ref(), Some(destination));
        assert_eq!(
            (window.width(), window.height()),
            (rect.width(), rect.height())
        );
        let actual = panel.compute_bounds(&window).unwrap();
        let expected = (
            ((x - rect.x()) as f32 - grab.0).clamp(
                -panel.width() as f32 * 2.0 / 3.0,
                rect.width() as f32 - panel.width() as f32 / 3.0,
            ),
            ((y - rect.y()) as f32 - grab.1)
                .clamp(0.0, rect.height() as f32 - panel.height() as f32 / 3.0),
        );
        assert!(
            (actual.x() - expected.0).abs() <= 1.0 && (actual.y() - expected.1).abs() <= 1.0,
            "destination: expected {expected:?}, got {actual:?}"
        );
        assert!(
            this.borrow().cards[&id].editing(),
            "drag/remap must preserve Edit"
        );
        assert_eq!(window.keyboard_mode(), KeyboardMode::OnDemand);
        assert_eq!(
            this.borrow().cards[&id].raw(),
            "# Cross-screen draft\n\nKeep this text."
        );
        // Popovers, explicit Preview and saving still work after a transfer.
        let menu = this.borrow().cards[&id].menu.clone();
        menu.popup();
        pump();
        menu.popdown();
        pump();
        let point = title
            .compute_point(&window, &gtk::graphene::Point::new(20.0, 12.0))
            .unwrap();
        send(&format!(
            "move {} {}",
            rect.x() + point.x().round() as i32,
            rect.y() + point.y().round() as i32
        ));
        send("press");
        send("release");
        assert!(
            !this.borrow().cards[&id].editing(),
            "ordinary title click still enters Preview"
        );
        assert_eq!(window.keyboard_mode(), KeyboardMode::None);
        assert!(std::fs::read_to_string(&this.borrow().cards[&id].path)
            .unwrap()
            .contains("Keep this text."));
        assert!(this.borrow_mut().hide());
        pump();
        let saved = layout::load(&paths);
        assert_eq!(
            saved.layer_monitor,
            destination.connector().map(|s| s.to_string())
        );
        assert_eq!(
            (saved.layer_x, saved.layer_y),
            (actual.x().round() as i32, actual.y().round() as i32)
        );
        Controller::show(&this);
        pump();
        assert_eq!(window.monitor().as_ref(), Some(destination));
    }
    // Recover the clipped right/bottom panel using the visible title before resize.
    let rect = window.monitor().unwrap().geometry();
    let point = title
        .compute_point(&window, &gtk::graphene::Point::new(20.0, 12.0))
        .unwrap();
    send(&format!(
        "move {} {}",
        rect.x() + point.x().round() as i32,
        rect.y() + point.y().round() as i32
    ));
    send("press");
    send(&format!("move {} {}", rect.x() + 200, rect.y() + 150));
    send("release");
    let recovered = panel.compute_bounds(&window).unwrap();
    assert!(recovered.x() < 200.0 && recovered.y() < 150.0);
    // Resize still receives real pointer events after repeated output remaps.
    let grip = panel.last_child().unwrap();
    let point = grip
        .compute_point(
            &window,
            &gtk::graphene::Point::new((grip.width() / 2) as f32, (grip.height() / 2) as f32),
        )
        .unwrap();
    let rect = window.monitor().unwrap().geometry();
    let (sx, sy) = (
        rect.x() + point.x().round() as i32,
        rect.y() + point.y().round() as i32,
    );
    let size = (panel.width(), panel.height());
    send(&format!("move {sx} {sy}"));
    send("press");
    send(&format!("move {} {}", sx - 30, sy - 30));
    send("release");
    assert_eq!(
        (panel.width(), panel.height()),
        ((size.0 - 30).max(360), (size.1 - 30).max(300))
    );
    // Drag from the far end of the title to hide two thirds beyond the left edge.
    let point = title
        .compute_point(
            &window,
            &gtk::graphene::Point::new((title.width() - 8) as f32, 12.0),
        )
        .unwrap();
    send(&format!(
        "move {} {}",
        rect.x() + point.x().round() as i32,
        rect.y() + point.y().round() as i32
    ));
    send("press");
    send(&format!("move {} {}", rect.x() + 1, rect.y() + 150));
    send("release");
    let clipped = panel.compute_bounds(&window).unwrap();
    assert!(
        clipped.x() < -100.0,
        "left edge must allow negative coordinates: {clipped:?}"
    );
    assert!(clipped.x() >= -panel.width() as f32 * 2.0 / 3.0 - 1.0);
    assert_eq!(
        (window.width(), window.height()),
        (rect.width(), rect.height())
    );
    // A new controller must restore the output as well as negative coordinates.
    assert!(this.borrow_mut().hide());
    let saved = layout::load(&paths);
    let restored = Controller::with_paths(&app, paths);
    Controller::show(&restored);
    pump();
    let restored_window = restored.borrow().window.clone();
    assert_eq!(
        restored_window
            .monitor()
            .unwrap()
            .connector()
            .map(|s| s.to_string()),
        saved.layer_monitor
    );
    let bounds = restored
        .borrow()
        .layer
        .as_ref()
        .unwrap()
        .widget
        .compute_bounds(&restored_window)
        .unwrap();
    assert_eq!(
        (bounds.x().round() as i32, bounds.y().round() as i32),
        (saved.layer_x, saved.layer_y)
    );
    // Negative restored geometry must have a matching, usable input region.
    let restored_title = restored.borrow().cards[&id].title.clone();
    let point = restored_title
        .compute_point(
            &restored_window,
            &gtk::graphene::Point::new((restored_title.width() - 8) as f32, 12.0),
        )
        .unwrap();
    assert!(point.x() >= 0.0, "a visible title segment must remain");
    send(&format!(
        "move {} {}",
        rect.x() + point.x().round() as i32,
        rect.y() + point.y().round() as i32
    ));
    send("press");
    send(&format!("move {} {}", rect.x() + 600, rect.y() + 200));
    send("release");
    let recovered = restored
        .borrow()
        .layer
        .as_ref()
        .unwrap()
        .widget
        .compute_bounds(&restored_window)
        .unwrap();
    assert!(
        recovered.x() > 0.0,
        "restored clipped title must receive drag input"
    );
    restored.borrow_mut().hide();
    drop(input);
    assert!(child.wait().unwrap().success());
}
