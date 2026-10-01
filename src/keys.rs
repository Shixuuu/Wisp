//! The keys, as Search has them, with Ctrl where the Mac has ⌘.
//!
//! A page gets the first go at the keys it may want for itself (Ctrl+F in a
//! document editor, Ctrl+S in a code editor) and the key is Wisp's only if
//! the page leaves it; the keys that make, close and move between tabs, and
//! Ctrl+K, are always Wisp's.

use crate::browser::Browser;
use crate::panels::{Page, Panel};
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use std::rc::Rc;
use webkit6::prelude::*;

pub fn install(b: &Rc<Browser>) {
    actions(b);
    for phase in [gtk::PropagationPhase::Capture, gtk::PropagationPhase::Bubble] {
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(phase);
        let weak = b.weak();
        keys.connect_key_pressed(move |_, key, code, mods| {
            let Some(b) = weak.upgrade() else { return glib::Propagation::Proceed };
            let first = phase == gtk::PropagationPhase::Capture;
            if first && page_first(&b, key, mods) {
                return glib::Propagation::Proceed;
            }
            if take(&b, key, code, mods) { glib::Propagation::Stop } else { glib::Propagation::Proceed }
        });
        if phase == gtk::PropagationPhase::Capture {
            let weak = b.weak();
            keys.connect_key_released(move |_, key, _, _| {
                let Some(b) = weak.upgrade() else { return };
                if matches!(key, gdk::Key::Control_L | gdk::Key::Control_R) {
                    if b.ui().switcher.active() {
                        b.ui().switcher.commit();
                    }
                    b.ui().field.land();
                }
            });
        }
        b.window.add_controller(keys);
    }
}

/// Whether a key goes to the page before Wisp looks at it.
fn page_first(b: &Browser, key: gdk::Key, mods: gdk::ModifierType) -> bool {
    let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
    let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
    if !ctrl || !focus_on_page(b) {
        return false;
    }
    let k = key.to_lower();
    let reserved = k == gdk::Key::t
        || (k == gdk::Key::w && !shift)
        || (k == gdk::Key::n && shift)
        || (shift
            && matches!(
                k,
                gdk::Key::bracketleft | gdk::Key::bracketright | gdk::Key::braceleft | gdk::Key::braceright
            ))
        || (k == gdk::Key::z && b.veiling.get())
        || (k == gdk::Key::k && !shift)
        || matches!(k, gdk::Key::Tab | gdk::Key::ISO_Left_Tab);
    !reserved
}

fn focus_on_page(b: &Browser) -> bool {
    gtk::prelude::RootExt::focus(&b.window)
        .is_some_and(|w| w.is::<webkit6::WebView>() || w.ancestor(webkit6::WebView::static_type()).is_some())
}

fn typing(b: &Browser) -> bool {
    gtk::prelude::RootExt::focus(&b.window).is_some_and(|w| w.is::<gtk::Text>())
}

/// The top row of digits, by where the key sits rather than what it types,
/// so Ctrl+1 works on every keyboard layout.
fn digit(code: u32) -> Option<usize> {
    match code {
        10..=18 => Some(code as usize - 9),
        19 => Some(0),
        _ => None,
    }
}

fn take(b: &Rc<Browser>, key: gdk::Key, code: u32, mods: gdk::ModifierType) -> bool {
    let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
    let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
    let alt = mods.contains(gdk::ModifierType::ALT_MASK);
    let ui = b.ui();
    let tab_key = matches!(key, gdk::Key::Tab | gdk::Key::ISO_Left_Tab);

    // While the switcher is up, Ctrl and the arrows move through it and
    // Ctrl+Tab goes on; any other key puts it away.
    if ui.switcher.active() && !(ctrl && tab_key) {
        if ctrl {
            let moved = match key {
                gdk::Key::Left => Some((-1, 0)),
                gdk::Key::Right => Some((1, 0)),
                gdk::Key::Up => Some((0, -1)),
                gdk::Key::Down => Some((0, 1)),
                _ => None,
            };
            if let Some((dx, dy)) = moved {
                ui.switcher.nudge(dx, dy);
                return true;
            }
            if matches!(key, gdk::Key::Control_L | gdk::Key::Control_R) {
                return false;
            }
        }
        ui.switcher.cancel();
        if key == gdk::Key::Escape {
            return true;
        }
    }

    if key == gdk::Key::Escape {
        return escape(b);
    }

    if ctrl && tab_key && !alt {
        if ui.panels.open.get().is_none() && !b.veiling.get() && !ui.field.showing() {
            ui.switcher.step(key == gdk::Key::ISO_Left_Tab || shift);
        } else {
            b.step(if shift || key == gdk::Key::ISO_Left_Tab { -1 } else { 1 });
        }
        return true;
    }

    match key {
        gdk::Key::F12 => {
            if let Some(i) = b.active().and_then(|t| t.view.borrow().clone()).and_then(|v| v.inspector()) {
                i.show();
            }
            return true;
        }
        gdk::Key::F11 => {
            if b.window.is_fullscreen() {
                b.window.unfullscreen()
            } else {
                b.window.fullscreen()
            }
            return true;
        }
        gdk::Key::F5 => {
            reload(b, shift);
            return true;
        }
        _ => {}
    }

    if !ctrl {
        return false;
    }
    if alt {
        if key.to_lower() == gdk::Key::r && !shift {
            reload(b, true);
            return true;
        }
        return false;
    }

    if let (false, Some(n)) = (shift, digit(code)) {
        if n == 0 {
            b.zoom(None)
        } else {
            b.select_index(n)
        }
        return true;
    }

    let view = || b.active().and_then(|t| t.view.borrow().clone());
    match key.to_lower() {
        gdk::Key::t if !shift => b.new_tab(),
        gdk::Key::t => b.reopen(),
        gdk::Key::c if shift => b.copy_address(),
        gdk::Key::d if !shift => {
            if let Some(t) = b.active() {
                b.duplicate(&t);
            }
        }
        gdk::Key::n if shift => b.new_shy_tab(),
        gdk::Key::n => b.new_tab(),
        gdk::Key::y if !shift => ui.panels.toggle(Panel::History),
        gdk::Key::j if shift => ui.panels.toggle(Panel::Downloads),
        gdk::Key::v if shift => {
            if typing(b) {
                return false;
            }
            b.paste_and_go();
        }
        gdk::Key::p if shift => {
            if let Some(t) = b.active() {
                if t.pin.borrow().is_some() {
                    b.unpin(&t);
                } else {
                    b.pin(&t);
                }
            }
        }
        gdk::Key::p if !shift => {
            if let Some(v) = view() {
                webkit6::PrintOperation::new(&v).run_dialog(Some(&b.window));
            }
        }
        gdk::Key::f if !shift => ui.bars.open_find(),
        gdk::Key::g => ui.bars.look(!shift),
        gdk::Key::m if shift => {
            if let Some(t) = b.active() {
                t.js("document.querySelectorAll('video,audio').forEach(function(m){m.pause()})");
            }
        }
        gdk::Key::k if !shift => {
            if ui.field.summoning() && ui.field.showing() {
                ui.field.step_summon();
            } else {
                ui.field.summon();
            }
        }
        gdk::Key::s if shift => b.toggle_sidebar(),
        gdk::Key::s => b.toggle_fold(),
        gdk::Key::b if shift => bookmark(b),
        gdk::Key::comma if !shift => ui.panels.toggle(Panel::Settings),
        gdk::Key::h if shift => b.toggle_hiding(),
        gdk::Key::u if shift => ui.panels.show_hidden(!ui.panels.hidden_showing()),
        gdk::Key::z if !shift => {
            if !b.veiling.get() {
                return false;
            }
            b.undo_hiding();
        }
        gdk::Key::equal | gdk::Key::plus | gdk::Key::KP_Add => b.zoom(Some(1.1)),
        gdk::Key::minus | gdk::Key::KP_Subtract => b.zoom(Some(1.0 / 1.1)),
        gdk::Key::w if !shift => {
            if !ui.panels.close()
                && let Some(t) = b.active()
            {
                b.close(&t);
            }
        }
        gdk::Key::l if !shift => ui.field.edit(),
        gdk::Key::r if !shift => reload(b, false),
        gdk::Key::r => b.toggle_reader(),
        gdk::Key::q => b.window.close(),
        gdk::Key::BackSpace if shift => ui.panels.show(Panel::History),
        gdk::Key::bracketleft | gdk::Key::braceleft => {
            if shift {
                b.step(-1);
            } else if let Some(v) = view() {
                v.go_back();
            }
        }
        gdk::Key::bracketright | gdk::Key::braceright => {
            if shift {
                b.step(1);
            } else if let Some(v) = view() {
                v.go_forward();
            }
        }
        gdk::Key::Left | gdk::Key::Right if !shift && !typing(b) => {
            if let Some(v) = view() {
                if key == gdk::Key::Left { v.go_back() } else { v.go_forward() }
            }
        }
        _ => return false,
    }
    true
}

/// Escape puts things away, one at a time, the most recent first.
fn escape(b: &Rc<Browser>) -> bool {
    let ui = b.ui();
    if ui.panels.close() {
        return true;
    }
    if b.veiling.get() {
        b.toggle_hiding();
        return true;
    }
    if ui.panels.hidden_showing() {
        ui.panels.show_hidden(false);
        return true;
    }
    if ui.bars.finding() {
        ui.bars.close_find(b);
        return true;
    }
    ui.field.escape()
}

fn reload(b: &Browser, from_origin: bool) {
    if let Some(v) = b.active().and_then(|t| t.view.borrow().clone()) {
        if from_origin { v.reload_bypass_cache() } else { v.reload() }
    }
}

fn bookmark(b: &Rc<Browser>) {
    let door = b.ui().tabs.borrow().as_ref().map(|l| l.bookmark_door());
    b.ui().panels.bookmark_card(door.as_ref());
}

/// The same commands for the menu, a tab's menu, and a panel's buttons.
fn actions(b: &Rc<Browser>) {
    let simple = |name: &str, run: fn(&Rc<Browser>)| {
        let action = gio::SimpleAction::new(name, None);
        let weak = b.weak();
        action.connect_activate(move |_, _| {
            if let Some(b) = weak.upgrade() {
                run(&b);
            }
        });
        b.window.add_action(&action);
    };
    simple("new-tab", |b| b.new_tab());
    simple("new-private-tab", |b| b.new_shy_tab());
    simple("reopen", |b| b.reopen());
    simple("history", |b| b.ui().panels.show(Panel::History));
    simple("downloads", |b| b.ui().panels.show(Panel::Downloads));
    simple("bookmarks", |b| b.ui().panels.show(Panel::Bookmarks));
    simple("bookmark", bookmark);
    simple("hide", |b| b.toggle_hiding());
    simple("hidden", |b| b.ui().panels.show_hidden(true));
    simple("reader", |b| b.toggle_reader());
    simple("find", |b| b.ui().bars.open_find());
    simple("toggle-sidebar", |b| b.toggle_sidebar());
    simple("settings", |b| b.ui().panels.show(Panel::Settings));
    simple("shortcuts", |b| b.ui().panels.show_settings(Page::Shortcuts));
    simple("quit", |b| b.window.close());

    let announce = gio::SimpleAction::new("announce", Some(glib::VariantTy::STRING));
    let weak = b.weak();
    announce.connect_activate(move |_, v| {
        if let (Some(b), Some(text)) = (weak.upgrade(), v.and_then(|v| v.get::<String>())) {
            b.announce(&text);
        }
    });
    b.window.add_action(&announce);

    let on_tab = |name: &str, run: fn(&Rc<Browser>, &Rc<crate::tab::Tab>)| {
        let action = gio::SimpleAction::new(name, Some(glib::VariantTy::UINT64));
        let weak = b.weak();
        action.connect_activate(move |_, v| {
            let Some(b) = weak.upgrade() else { return };
            if let Some(tab) = v.and_then(|v| v.get::<u64>()).and_then(|id| b.tab(id)) {
                run(&b, &tab);
            }
        });
        b.window.add_action(&action);
    };
    on_tab("tab-pin", |b, t| b.pin(t));
    on_tab("tab-unpin", |b, t| b.unpin(t));
    on_tab("tab-letter", |b, t| {
        if let Some(list) = b.ui().tabs.borrow().as_ref() {
            list.edit_letter(t);
        }
    });
    on_tab("tab-rename", |b, t| {
        b.select(t);
        if let Some(list) = b.ui().tabs.borrow().as_ref() {
            list.begin_edit(t, true);
        }
    });
    on_tab("tab-duplicate", |b, t| b.duplicate(t));
    on_tab("tab-copy", |b, t| {
        b.window.clipboard().set_text(&t.address());
        b.announce("Address copied");
    });
    on_tab("tab-markdown", |b, t| b.copy_markdown(t));
    on_tab("tab-mute", |b, t| {
        if let Some(v) = t.view.borrow().as_ref() {
            v.set_is_muted(!v.is_muted());
            t.muted.set(v.is_muted());
        }
        b.changed(t, crate::tab::Change::Sound);
    });
    on_tab("tab-sleep", |b, t| b.put_to_sleep(t));
    on_tab("tab-close", |b, t| b.close(t));
    on_tab("tab-close-others", |b, t| b.close_others(t));
}
