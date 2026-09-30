//! Wisp: a small, fast, quiet web browser for Arch Linux.
//!
//! A Rust rewrite of Search (github.com/driceroland/Search), with Apple's
//! WebKit swapped for WebKitGTK and SwiftUI/AppKit for GTK 4 and libadwaita.

mod address;
mod bars;
mod bookmarks;
mod browser;
mod curtain;
mod history;
mod keys;
mod loot;
mod motion;
mod omnibox;
mod panels;
mod settings;
mod shield;
mod store;
mod switcher;
mod tab;
mod tabs;
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
        style();
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
            browser.new_shy_tab();
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
        None => settings::Prefs::load().search_url(arg),
    }
}

/// Search's greys, one pair for a light window and one for a dark. The
/// stylesheet names the colours; which pair they mean follows the look.
const LIGHT: &str = "@define-color ground #ffffff; @define-color ink #171717; @define-color muted #8c8c8c;
@define-color faint #d4d4d4; @define-color hairline #e8e8e8; @define-color wash #efefef;
@define-color pinlive #e6e6e6; @define-color hover #f6f6f6;";
const DARK: &str = "@define-color ground #1c1c1c; @define-color ink #ededed; @define-color muted #949494;
@define-color faint #525252; @define-color hairline #333333; @define-color wash #2d2d2d;
@define-color pinlive #363636; @define-color hover #262626;";

fn style() {
    let Some(display) = gdk::Display::default() else { return };
    let base = gtk::CssProvider::new();
    base.load_from_string(include_str!("style.css"));
    gtk::style_context_add_provider_for_display(&display, &base, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    let palette = gtk::CssProvider::new();
    gtk::style_context_add_provider_for_display(&display, &palette, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);
    let manager = adw::StyleManager::default();
    let paint = move |m: &adw::StyleManager| palette.load_from_string(if m.is_dark() { DARK } else { LIGHT });
    paint(&manager);
    manager.connect_dark_notify(paint);
}

/// A place to type. GTK gives a bare text box no accessible role, which
/// leaves it invisible to screen readers; this one is a named text box.
pub fn text_box(name: &str) -> gtk::Text {
    let text: gtk::Text = glib::Object::builder().property("accessible-role", gtk::AccessibleRole::TextBox).build();
    text.update_property(&[gtk::accessible::Property::Label(name)]);
    text
}
