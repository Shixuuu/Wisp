//! Motion, as Search has it: one spring for anything that moves between two
//! places (glide), one for anything that arrives or leaves (settle), and a
//! short ease-out for everything small (quick). Using the same three
//! everywhere is most of why it feels like one piece of software.
//!
//! `Slide` is a bin that draws its child moved, scaled and faded without
//! laying it out again, so a moving thing costs a redraw and not a layout.
//! libadwaita's animations turn themselves off when the desktop asks for
//! reduced motion (gtk-enable-animations).

use adw::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{glib, graphene};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

#[derive(Clone, Copy)]
pub enum Curve {
    /// Between two places: response 0.34 s, damping 0.82.
    Glide,
    /// Arriving or leaving: response 0.30 s, damping 0.86.
    Settle,
    /// Hover, fades: 0.14 s ease-out.
    Quick,
    /// A timed ease-out of any length, in milliseconds.
    Ease(u32),
}

fn spring(response: f64, damping: f64) -> adw::SpringParams {
    let stiffness = (2.0 * std::f64::consts::PI / response).powi(2);
    adw::SpringParams::new(damping, 1.0, stiffness)
}

/// Run `apply` from `from` to `to` along a curve. Returns the animation so
/// it can be stopped.
pub fn animate(
    widget: &impl IsA<gtk::Widget>,
    from: f64,
    to: f64,
    curve: Curve,
    apply: impl Fn(f64) + 'static,
) -> adw::Animation {
    let target = adw::CallbackAnimationTarget::new(apply);
    let animation: adw::Animation = match curve {
        Curve::Glide => adw::SpringAnimation::new(widget, from, to, spring(0.34, 0.82), target).upcast(),
        Curve::Settle => adw::SpringAnimation::new(widget, from, to, spring(0.30, 0.86), target).upcast(),
        Curve::Quick | Curve::Ease(_) => {
            let ms = if let Curve::Ease(ms) = curve { ms } else { 140 };
            let t = adw::TimedAnimation::new(widget, from, to, ms, target);
            t.set_easing(adw::Easing::EaseOutCubic);
            t.upcast()
        }
    };
    animation.play();
    animation
}

/// A value that moves to wherever it is sent, from wherever it is now.
#[derive(Clone)]
pub struct Tween {
    inner: Rc<TweenInner>,
}

struct TweenInner {
    value: Cell<f64>,
    target: Cell<f64>,
    running: RefCell<Option<adw::Animation>>,
    widget: gtk::Widget,
    apply: Box<dyn Fn(f64)>,
}

impl Tween {
    pub fn new(widget: &impl IsA<gtk::Widget>, value: f64, apply: impl Fn(f64) + 'static) -> Tween {
        let inner = TweenInner {
            value: Cell::new(value),
            target: Cell::new(value),
            running: RefCell::default(),
            widget: widget.clone().upcast(),
            apply: Box::new(apply),
        };
        (inner.apply)(value);
        Tween { inner: Rc::new(inner) }
    }

    pub fn value(&self) -> f64 {
        self.inner.value.get()
    }

    pub fn target(&self) -> f64 {
        self.inner.target.get()
    }

    /// Remember where this is going without moving it yet.
    pub fn aim(&self, to: f64) {
        self.inner.target.set(to);
    }

    /// Move there along a curve. Sending it where it is already going does
    /// nothing, so it can be asked every time something might have changed.
    pub fn to(&self, to: f64, curve: Curve) {
        if (self.inner.target.get() - to).abs() < 0.01 && self.inner.running.borrow().is_some() {
            return;
        }
        self.inner.target.set(to);
        if let Some(old) = self.inner.running.take() {
            old.pause();
        }
        let weak = Rc::downgrade(&self.inner);
        let animation = animate(&self.inner.widget, self.inner.value.get(), to, curve, move |v| {
            if let Some(inner) = weak.upgrade() {
                inner.value.set(v);
                (inner.apply)(v);
            }
        });
        let weak = Rc::downgrade(&self.inner);
        animation.connect_done(move |_| {
            if let Some(inner) = weak.upgrade() {
                inner.running.take();
            }
        });
        let finished = animation.state() == adw::AnimationState::Finished;
        if finished {
            self.inner.running.take();
        } else {
            *self.inner.running.borrow_mut() = Some(animation);
        }
    }

    /// Be there at once.
    pub fn set(&self, to: f64) {
        if let Some(old) = self.inner.running.take() {
            old.pause();
        }
        self.inner.target.set(to);
        self.inner.value.set(to);
        (self.inner.apply)(to);
    }
}

glib::wrapper! {
    pub struct Slide(ObjectSubclass<imp::Slide>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

mod imp {
    use super::*;

    pub struct Slide {
        pub dx: Cell<f32>,
        pub dy: Cell<f32>,
        pub scale: Cell<f32>,
        pub anchor: Cell<(f32, f32)>,
    }

    impl Default for Slide {
        fn default() -> Self {
            Slide { dx: Cell::new(0.0), dy: Cell::new(0.0), scale: Cell::new(1.0), anchor: Cell::new((0.5, 0.5)) }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Slide {
        const NAME: &'static str = "WispSlide";
        type Type = super::Slide;
        type ParentType = gtk::Widget;

        fn class_init(class: &mut Self::Class) {
            class.set_layout_manager_type::<gtk::BinLayout>();
        }
    }

    impl ObjectImpl for Slide {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for Slide {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let (ax, ay) = self.anchor.get();
            let (cx, cy) = (obj.width() as f32 * ax, obj.height() as f32 * ay);
            let k = self.scale.get();
            snapshot.save();
            snapshot.translate(&graphene::Point::new(cx + self.dx.get(), cy + self.dy.get()));
            snapshot.scale(k, k);
            snapshot.translate(&graphene::Point::new(-cx, -cy));
            let mut child = obj.first_child();
            while let Some(c) = child {
                obj.snapshot_child(&c, snapshot);
                child = c.next_sibling();
            }
            snapshot.restore();
        }
    }
}

impl Slide {
    pub fn new(child: &impl IsA<gtk::Widget>) -> Slide {
        let slide: Slide = glib::Object::new();
        child.set_parent(&slide);
        slide
    }

    pub fn child(&self) -> Option<gtk::Widget> {
        self.first_child()
    }

    /// Where scaling grows from, as fractions of the width and height.
    pub fn set_anchor(&self, x: f32, y: f32) {
        self.imp().anchor.set((x, y));
    }

    pub fn set_offset(&self, dx: f64, dy: f64) {
        self.imp().dx.set(dx as f32);
        self.imp().dy.set(dy as f32);
        self.queue_draw();
    }

    pub fn set_scale(&self, k: f64) {
        self.imp().scale.set(k as f32);
        self.queue_draw();
    }
}

/// Somewhere a thing is put when it shows and taken from when it hides.
/// GTK's accessibility tree hears of a child when it is added, not when a
/// hidden one is shown again, so things come and go this way; screen
/// readers and the end-to-end tests then see what is on screen.
#[derive(Clone)]
pub struct Place {
    pub root: gtk::Box,
    child: gtk::Widget,
    wanted: Rc<Cell<bool>>,
    pending: Rc<Cell<bool>>,
}

impl Place {
    pub fn new(child: &impl IsA<gtk::Widget>) -> Place {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.set_can_target(false);
        Place { root, child: child.clone().upcast(), wanted: Rc::default(), pending: Rc::default() }
    }

    /// Asked for from anywhere, done just after: adding or removing a
    /// widget inside an animation frame crashes GTK 4.14's accessibility.
    /// In at once; out at once too, though the removal itself waits for
    /// the frame to end (see `settle`).
    pub fn show(&self, on: bool) {
        if on {
            self.wanted.set(true);
            if self.child.parent().is_none() {
                self.root.append(&self.child);
            }
            self.root.set_can_target(true);
        } else {
            self.release();
            self.settle();
        }
    }

    /// No longer wanted; it stays until `settle`, so it can fade out first.
    pub fn release(&self) {
        self.wanted.set(false);
    }

    /// Take it out if it is no longer wanted. Just after the frame, not in
    /// it: removing a widget inside an animation frame crashes GTK 4.14's
    /// accessibility.
    pub fn settle(&self) {
        if self.wanted.get() || self.pending.replace(true) {
            return;
        }
        let me = self.clone();
        glib::idle_add_local_once(move || {
            me.pending.set(false);
            if !me.wanted.get() && me.child.parent().is_some() {
                me.root.remove(&me.child);
                // Empty, it must not take the clicks meant for what is under it.
                me.root.set_can_target(false);
            }
        });
    }
}

/// Something that comes and goes: in on `settle`, growing from `scale` to
/// full and fading in, out on `quick`. Lay out `root`; `slide` moves.
#[derive(Clone)]
pub struct Presence {
    pub root: gtk::Box,
    pub slide: Slide,
    place: Place,
    tween: Tween,
    shown: Rc<Cell<bool>>,
}

impl Presence {
    /// `lift`: how far below its place it starts, for things that rise.
    pub fn new(child: &impl IsA<gtk::Widget>, scale: f64, lift: f64) -> Presence {
        let slide = Slide::new(child);
        let place = Place::new(&slide);
        let root = place.root.clone();
        let (s, p) = (slide.clone(), place.clone());
        let tween = Tween::new(&slide, 0.0, move |t| {
            s.set_opacity(t.clamp(0.0, 1.0));
            s.set_scale(scale + (1.0 - scale) * t);
            s.set_offset(0.0, lift * (1.0 - t));
            if t <= 0.001 {
                p.settle();
            }
        });
        Presence { root, slide, place, tween, shown: Rc::new(Cell::new(false)) }
    }

    pub fn shown(&self) -> bool {
        self.shown.get()
    }

    pub fn show(&self, on: bool) {
        if self.shown.replace(on) == on {
            return;
        }
        if on {
            // In at once, so what is inside can take the keyboard.
            self.place.show(true);
        } else {
            self.place.release();
        }
        self.tween.to(if on { 1.0 } else { 0.0 }, if on { Curve::Settle } else { Curve::Quick });
    }

    /// Present at full size with no entrance of its own. The rows inside move.
    pub fn reveal(&self) {
        if !self.shown.replace(true) {
            self.place.show(true);
        }
        self.tween.set(1.0);
    }
}

/// The field shivers and stops: three there-and-backs, tapering to nothing.
pub fn shake(slide: &Slide) {
    let s = slide.clone();
    animate(slide, 0.0, 1.0, Curve::Ease(500), move |t| {
        let decay = 1.0 - t;
        s.set_offset((t * std::f64::consts::PI * 6.0).sin() * 7.0 * decay, 0.0);
    });
}
