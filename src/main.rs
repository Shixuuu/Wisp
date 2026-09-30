//! Wisp: a small, fast, quiet web browser for Arch Linux.
//!
//! A Rust rewrite of Search (github.com/driceroland/Search), with Apple's
//! WebKit swapped for WebKitGTK and SwiftUI/AppKit for GTK 4 and libadwaita.

mod actions;
mod address;
mod bookmarks;
mod browser;
mod curtain;
mod downloads;
mod history;
mod omnibox;
mod panels;
mod prefs;
mod session;
mod settings;
mod shield;
mod store;
mod tab;
mod view;
mod web;

use adw::prelude::*;
use browser::Browser;
use gtk::{gdk, gio, glib};
use std::cell::RefCell;
use std::rc::Rc;

const APP_ID: &str = "org.wisp.Wisp";

fn main() -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id(APP_ID)
        // One window, one process: `wisp URL` run again, or a link opened
        // from another program, goes to the Wisp already running as a tab.
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    app.add_main_option(
        "private",
        glib::Char::from(b'p'),
        glib::OptionFlags::NONE,
        glib::OptionArg::None,
        "Open a new private tab",
        None,
    );
    app.add_main_option(
        "version",
        glib::Char::from(b'v'),
        glib::OptionFlags::NONE,
        glib::OptionArg::None,
        "Print the version and quit",
        None,
    );
    app.connect_handle_local_options(|_, options| {
        if options.contains("version") {
            println!("wisp {}", env!("CARGO_PKG_VERSION"));
            return std::ops::ControlFlow::Break(glib::ExitCode::SUCCESS);
        }
        std::ops::ControlFlow::Continue(())
    });

    let browser: Rc<RefCell<Option<Rc<Browser>>>> = Rc::new(RefCell::new(None));

    app.connect_startup(|_| {
        load_css();
        gtk::Window::set_default_icon_name(APP_ID);
    });

    let b = browser.clone();
    app.connect_command_line(move |app, line| {
        let existing = b.borrow().clone();
        let browser = match existing {
            Some(browser) => browser,
            None => {
                let browser = Browser::new(app);
                *b.borrow_mut() = Some(browser.clone());
                browser
            }
        };
        let private = line.options_dict().contains("private");
        let targets: Vec<String> = line
            .arguments()
            .iter()
            .skip(1)
            .filter_map(|arg| arg.to_str().map(str::to_string))
            .map(|arg| target_for(line, &arg))
            .collect();
        if private {
            browser.new_tab(true);
        }
        for target in targets {
            browser.open_from_outside(&target);
        }
        browser.present();
        glib::ExitCode::SUCCESS
    });

    app.run()
}

/// What a command-line argument means: a web address as typed, a local file,
/// or else whatever the address field would make of it.
fn target_for(line: &gio::ApplicationCommandLine, arg: &str) -> String {
    let lower = arg.to_ascii_lowercase();
    if lower.contains("://") || lower.starts_with("about:") || lower.starts_with("data:") {
        return arg.to_string();
    }
    let file = line.create_file_for_arg(arg);
    if file.query_exists(None::<&gio::Cancellable>) {
        return file.uri().to_string();
    }
    match address::url_from(arg) {
        Some(url) => url.to_string(),
        None => settings::Settings::load().search_url(arg),
    }
}

fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(include_str!("style.css"));
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    }
}
