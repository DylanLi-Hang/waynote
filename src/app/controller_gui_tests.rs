//! Run inside an isolated XDG directory and D-Bus session with a GTK display.
use super::*;

fn pump() {
    let context=glib::MainContext::default();
    for _ in 0..40 { while context.pending() { context.iteration(false); } }
}
fn checkbox(widget:&gtk::Widget)->Option<gtk::CheckButton> {
    if let Ok(check)=widget.clone().downcast::<gtk::CheckButton>() { return Some(check) }
    let mut child=widget.first_child();
    while let Some(widget)=child {
        if let Some(check)=checkbox(&widget) { return Some(check) }
        child=widget.next_sibling();
    }
    None
}
#[test]
#[ignore = "requires an isolated GTK display, XDG directories and D-Bus session"]
fn board_gui_regressions() {
    gtk::init().expect("GTK display required");
    let app=gtk::Application::builder().application_id("dev.mryll.waynote.test").build();
    app.register(None::<&gio::Cancellable>).unwrap();
    let this=Controller::new(&app);
    assert!(this.borrow().cards.is_empty(),"Run against isolated empty XDG paths");
    Controller::show(&this); pump();
    assert_eq!(app.windows().len(),1);
    assert_eq!(this.borrow().cards.len(),1);
    let first=this.borrow().cards.keys().next().unwrap().clone();
    assert!(this.borrow().cards[&first].editing());
    let editor=this.borrow().cards[&first].editor.clone();
    editor.buffer().set_text("# Draft\n\n- [ ] Task\n");
    // Hide must flush the buffer even before its deferred changed event runs.
    Controller::toggle(&this); pump();
    assert!(!this.borrow().window.is_visible());
    assert_eq!(crate::core::frontmatter::parse(&std::fs::read_to_string(&this.borrow().cards[&first].path).unwrap()).body,"# Draft\n\n- [ ] Task\n");
    Controller::toggle(&this); pump();
    assert!(!this.borrow().cards[&first].editing());
    assert_eq!(this.borrow().cards.len(),1,"Summon must not create duplicates");
    assert_eq!(app.windows().len(),1);
    let check=checkbox(&this.borrow().cards[&first].preview.clone().upcast()).unwrap();
    check.set_active(true); pump();
    assert!(this.borrow().cards[&first].note.body.contains("- [x] Task"));
    assert!(!this.borrow().cards[&first].editing(),"Checkbox does not enter edit mode");
    Controller::edit(&this,&first,None); pump();
    editor.buffer().set_text("Uncommitted newest content");
    assert_eq!(this.borrow().cards[&first].raw(),"Uncommitted newest content","Copy source must include newest draft");
    Controller::create(&this); pump();
    assert!(!this.borrow().cards[&first].editing());
    assert_eq!(this.borrow().cards[&first].saved_body,"Uncommitted newest content");
    assert_eq!(app.windows().len(),1,"Every note belongs to the same board");
    {
        let mut board=this.borrow_mut();
        board.finish_edit();
        let card=board.cards.get_mut(&first).unwrap();
        card.note.locked=true; card.dirty=true;
        card.editor.buffer().set_text("- [ ] Locked task");
        assert!(board.save_card(&first)); board.rebuild_card(&first);
    }
    let check=checkbox(&this.borrow().cards[&first].preview.clone().upcast()).unwrap();
    assert!(!check.is_sensitive());
    Controller::edit(&this,&first,None); pump();
    assert!(!this.borrow().cards[&first].editing());
    {
        let mut board=this.borrow_mut();
        let card=board.cards.get_mut(&first).unwrap();
        card.note.locked=false;
        card.editor.buffer().set_text("Local unsaved draft");
        std::fs::write(&card.path,"External content").unwrap();
        assert!(!board.save_card(&first));
        assert!(!board.hide(),"Save conflict must not discard a hidden draft");
        assert!(board.cards[&first].blocked);
        assert_eq!(std::fs::read_to_string(&board.cards[&first].path).unwrap(),"External content");
        assert_eq!(board.cards[&first].raw(),"Local unsaved draft");
        // Explicit discard is tested only after verifying both copies survived.
        board.remove_card(&first); board.reload();
        let remaining:Vec<_>=board.cards.keys().cloned().collect();
        for id in remaining { board.delete(&id); }
        assert!(board.cards.is_empty()); assert!(board.empty.is_visible());
        assert!(board.hide());
    }
    Controller::show(&this); pump();
    assert!(this.borrow().cards.is_empty(),"Deleting the final note leaves an empty board");
    assert_eq!(app.windows().len(),1);
    this.borrow().window.destroy();
}
