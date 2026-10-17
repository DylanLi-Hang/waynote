use super::controller::Controller;
use gtk::{gio, prelude::*};
use std::{cell::RefCell, rc::Rc};

pub fn register(app: &gtk::Application, board: &Rc<RefCell<Controller>>) {
    for name in [
        "summon", "toggle", "show-all", "hide-all", "new-note", "quit",
    ] {
        let action = gio::SimpleAction::new(name, None);
        let board = board.clone();
        action.connect_activate(move |_, _| match name {
            "summon" | "toggle" => Controller::toggle(&board),
            "show-all" => Controller::show(&board),
            "hide-all" => {
                board.borrow_mut().hide();
            }
            "new-note" => Controller::create(&board),
            "quit" => {
                board.borrow_mut().quit();
            }
            _ => unreachable!(),
        });
        app.add_action(&action);
    }
}
