//! The pages shown beside each other. Each pane glides from the rectangle
//! it had to the rectangle the split asks for.

use crate::layout::Rect;
use crate::motion::{Curve, Tween};
use adw::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use webkit6::WebView;

struct Slot {
    /// What the board moves and sizes. The page sits in `body`; a label
    /// fills the shell so the pane's rectangle is an accessible object.
    shell: gtk::Overlay,
    body: gtk::Box,
    x: Tween,
    y: Tween,
    w: Tween,
    h: Tween,
}

impl Slot {
    fn new(board: &gtk::Fixed, start: Rect) -> Slot {
        let shell = gtk::Overlay::new();
        shell.set_overflow(gtk::Overflow::Hidden);
        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        body.add_css_class("pane");
        body.set_hexpand(true);
        body.set_vexpand(true);
        shell.set_child(Some(&body));
        // A generic box never appears in the accessibility tree here. The
        // label's text is the pane's name, and it fills the shell so its
        // box is the pane's rectangle. The text itself is not drawn.
        let name = gtk::Label::new(Some("Split pane"));
        name.add_css_class("pane-name");
        name.set_can_target(false);
        name.set_halign(gtk::Align::Fill);
        name.set_valign(gtk::Align::Fill);
        shell.add_overlay(&name);
        board.put(&shell, start.x, start.y);
        shell.set_size_request(start.w.round() as i32, start.h.round() as i32);

        let at = Rc::new(Cell::new((start.x, start.y)));
        let size = Rc::new(Cell::new((start.w, start.h)));
        let (board_x, shell_x, at_x) = (board.clone(), shell.clone(), at.clone());
        let x = Tween::new(board, start.x, move |v| {
            at_x.set((v, at_x.get().1));
            if shell_x.parent().is_some() {
                board_x.move_(&shell_x, at_x.get().0, at_x.get().1);
            }
        });
        let (board_y, shell_y, at_y) = (board.clone(), shell.clone(), at.clone());
        let y = Tween::new(board, start.y, move |v| {
            at_y.set((at_y.get().0, v));
            if shell_y.parent().is_some() {
                board_y.move_(&shell_y, at_y.get().0, at_y.get().1);
            }
        });
        let (shell_w, size_w) = (shell.clone(), size.clone());
        let w = Tween::new(&shell, start.w, move |v| {
            size_w.set((v, size_w.get().1));
            let (width, height) = size_w.get();
            shell_w.set_size_request(width.round() as i32, height.round() as i32);
        });
        let (shell_h, size_h) = (shell.clone(), size);
        let h = Tween::new(&shell, start.h, move |v| {
            size_h.set((size_h.get().0, v));
            let (width, height) = size_h.get();
            shell_h.set_size_request(width.round() as i32, height.round() as i32);
        });
        Slot { shell, body, x, y, w, h }
    }

    fn to(&self, rect: Rect) {
        self.x.to(rect.x, Curve::Glide);
        self.y.to(rect.y, Curve::Glide);
        self.w.to(rect.w, Curve::Glide);
        self.h.to(rect.h, Curve::Glide);
    }

    /// Glide when the pane can. libadwaita skips an animation whose widget
    /// is not mapped yet, which would land the pane on its final rectangle
    /// in one step. Aim first so a resize during the wait does not replace
    /// the glide with a jump.
    fn go(&self, rect: Rect, animate: bool) {
        if !animate {
            self.set(rect);
            return;
        }
        self.x.aim(rect.x);
        self.y.aim(rect.y);
        self.w.aim(rect.w);
        self.h.aim(rect.h);
        if self.shell.is_mapped() {
            self.to(rect);
            return;
        }
        let once = Rc::new(Cell::new(false));
        let (x, y, w, h) = (self.x.clone(), self.y.clone(), self.w.clone(), self.h.clone());
        self.shell.connect_map(move |_| {
            if once.replace(true) {
                return;
            }
            x.to(rect.x, Curve::Glide);
            y.to(rect.y, Curve::Glide);
            w.to(rect.w, Curve::Glide);
            h.to(rect.h, Curve::Glide);
        });
    }

    fn set(&self, rect: Rect) {
        self.x.set(rect.x);
        self.y.set(rect.y);
        self.w.set(rect.w);
        self.h.set(rect.h);
    }
}

type PaneFocus = Rc<dyn Fn(u64)>;

pub struct Panes {
    pub root: gtk::Fixed,
    slots: RefCell<HashMap<u64, Slot>>,
    watched: RefCell<Vec<u64>>,
    focus: RefCell<Option<PaneFocus>>,
}

impl Panes {
    pub fn new() -> Panes {
        let root = gtk::Fixed::new();
        root.set_hexpand(true);
        root.set_vexpand(true);
        root.set_visible(false);
        root.set_can_target(true);
        Panes { root, slots: RefCell::default(), watched: RefCell::default(), focus: RefCell::default() }
    }

    pub fn bind(&self, focus: impl Fn(u64) + 'static) {
        *self.focus.borrow_mut() = Some(Rc::new(focus));
    }

    pub fn has(&self, id: u64) -> bool {
        self.slots.borrow().contains_key(&id)
    }

    /// Whether every pane is already heading for `rects`.
    pub fn matches(&self, rects: &[(u64, Rect)]) -> bool {
        let slots = self.slots.borrow();
        slots.len() == rects.len()
            && rects.iter().all(|(id, rect)| {
                slots.get(id).is_some_and(|slot| {
                    (slot.x.target() - rect.x).abs() < 0.5
                        && (slot.y.target() - rect.y).abs() < 0.5
                        && (slot.w.target() - rect.w).abs() < 0.5
                        && (slot.h.target() - rect.h).abs() < 0.5
                })
            })
    }

    /// Put `rects` on the board. A pane that is new begins at `starts` and,
    /// when `animate` is set, glides to its place. Views already parented
    /// elsewhere are moved onto their pane.
    pub fn show(
        &self,
        rects: &[(u64, Rect)],
        starts: &HashMap<u64, Rect>,
        animate: bool,
        mut view_for: impl FnMut(u64) -> Option<WebView>,
    ) {
        let keep: Vec<u64> = rects.iter().map(|(id, _)| *id).collect();
        let gone: Vec<u64> = self.slots.borrow().keys().copied().filter(|id| !keep.contains(id)).collect();
        for id in gone {
            self.detach(id);
        }
        // Mapped before the glide starts, so the spring is allowed to run.
        self.root.set_visible(true);
        for (id, rect) in rects {
            let fresh = !self.has(*id);
            if fresh {
                let start = starts.get(id).copied().unwrap_or(*rect);
                let slot = Slot::new(&self.root, start);
                self.slots.borrow_mut().insert(*id, slot);
            }
            if let Some(view) = view_for(*id) {
                self.attach(*id, &view);
            }
            let slots = self.slots.borrow();
            let Some(slot) = slots.get(id) else { continue };
            slot.go(*rect, animate);
        }
    }

    pub fn choose(&self, id: Option<u64>) {
        for (slot_id, slot) in self.slots.borrow().iter() {
            if Some(*slot_id) == id {
                slot.body.add_css_class("chosen");
            } else {
                slot.body.remove_css_class("chosen");
            }
        }
    }

    pub fn hide(&self) {
        self.detach_all();
    }

    /// The view left, so the next one has to be wired for focus again.
    pub fn unwatch(&self, id: u64) {
        self.watched.borrow_mut().retain(|kept| *kept != id);
    }

    /// Views that were on the board, now parentless, in no particular order.
    pub fn detach_all(&self) -> Vec<WebView> {
        let ids: Vec<u64> = self.slots.borrow().keys().copied().collect();
        let mut views = vec![];
        for id in ids {
            if let Some(view) = self.detach(id) {
                views.push(view);
            }
        }
        self.root.set_visible(false);
        views
    }

    fn attach(&self, id: u64, view: &WebView) {
        let Some(body) = self.slots.borrow().get(&id).map(|slot| slot.body.clone()) else { return };
        let same = view.parent().is_some_and(|parent| parent == body.clone().upcast::<gtk::Widget>());
        if !same {
            view.unparent();
            view.set_hexpand(true);
            view.set_vexpand(true);
            body.append(view);
        }
        if self.watched.borrow().contains(&id) {
            return;
        }
        self.watched.borrow_mut().push(id);
        let focus = self.focus.borrow().clone();
        view.connect_notify_local(Some("has-focus"), move |view, _| {
            if view.has_focus()
                && let Some(focus) = focus.as_ref()
            {
                focus(id);
            }
        });
    }

    fn detach(&self, id: u64) -> Option<WebView> {
        self.unwatch(id);
        let slot = self.slots.borrow_mut().remove(&id)?;
        let view = slot.body.first_child().and_then(|child| child.downcast::<WebView>().ok());
        if let Some(view) = view.clone() {
            view.unparent();
        }
        if slot.shell.parent().is_some() {
            self.root.remove(&slot.shell);
        }
        view
    }
}
