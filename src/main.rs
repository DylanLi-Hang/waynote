mod app;
mod core;
mod platform;

use gtk::{gio, glib, prelude::*};
use std::{cell::RefCell, rc::Rc};

fn command(args: &[String]) -> Option<&'static str> {
    match args.first().map(String::as_str) {
        None | Some("show-all") => Some("show-all"),
        Some("summon" | "toggle") => Some("summon"),
        Some("hide-all") => Some("hide-all"),
        Some("new") => Some("new-note"),
        Some("quit") => Some("quit"),
        _ => None,
    }
}
fn main() -> glib::ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if command(&args).is_none() {
        println!("waynote — Markdown notes\n\nUsage: waynote [summon|toggle|new|show-all|hide-all|quit]\nBind Meta+N to `waynote summon`. A visible board always hides, regardless of focus.");
        return glib::ExitCode::SUCCESS;
    }
    let id = if std::env::var("WAYNOTE_DEV").as_deref() == Ok("1") {
        "dev.mryll.waynote.development"
    } else {
        "dev.mryll.waynote"
    };
    let app = gtk::Application::builder()
        .application_id(id)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let board = Rc::new(RefCell::new(
        None::<Rc<RefCell<app::controller::Controller>>>,
    ));
    let slot = board.clone();
    app.connect_command_line(move |app, invocation| {
        let args: Vec<_> = invocation
            .arguments()
            .into_iter()
            .skip(1)
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        let Some(action) = command(&args) else {
            return glib::ExitCode::FAILURE;
        };
        if slot.borrow().is_none() {
            let controller = app::controller::Controller::new(app);
            app::actions::register(app, &controller);
            *slot.borrow_mut() = Some(controller);
        }
        app.activate_action(action, None);
        glib::ExitCode::SUCCESS
    });
    let mut argv = vec!["waynote".to_string()];
    argv.extend(args);
    app.run_with_args(&argv)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn summon_and_legacy_toggle_have_identical_visibility_semantics() {
        assert_eq!(command(&["summon".into()]), command(&["toggle".into()]));
        assert_eq!(command(&[]), Some("show-all"));
        assert_eq!(command(&["new".into()]), Some("new-note"));
        assert!(command(&["arrange".into()]).is_none());
    }
}
