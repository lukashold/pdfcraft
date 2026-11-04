//! Stylus mode for pen tablets in the browser: the pen uses the chosen tool, a finger moves the
//! page, and a palm resting on the screen while the pen writes is ignored.
//!
//! eframe passes pen and finger input on as the same pointer, so [`install`] listens ahead of it
//! (capture phase on the window) for the browser's `pointerType`. Stylus mode turns on the first
//! time a pen is seen and is remembered for later visits; `?stylus=off` keeps it off and
//! `?stylus=on` starts with it on.
//!
//! The listeners only drop palm events and note what made the latest press. The app reads that
//! once per frame ([`poll`]) into [`StylusInput`]; the pages see the kind with the rest of their pointer state,
//! and the page view lets a finger pan (and cancels the press it would otherwise give the tool)
//! by [`finger_pans`]. Native shells never poll, so their input is unchanged.

use crate::QuickTool;

/// What made the latest press.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PointerKind {
    /// A mouse, or a pointer the browser did not name. Also every native press.
    #[default]
    Mouse,
    Pen,
    Touch,
}

/// One frame's stylus state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StylusInput {
    /// A pen has been used here (or `?stylus=on`): fingers pan with the pen's tools and palms are
    /// ignored while the pen is near.
    pub enabled: bool,
    /// What made the latest press, until the next one.
    pub kind: PointerKind,
}

/// Whether a press by `input` with `tool` moves the page instead of using the tool: a finger in
/// stylus mode, with a tool the pen should own. Tap-to-place tools keep the finger.
pub(crate) fn finger_pans(tool: QuickTool, input: StylusInput) -> bool {
    input.enabled && input.kind == PointerKind::Touch && pen_owned(tool)
}

fn pen_owned(tool: QuickTool) -> bool {
    matches!(
        tool,
        QuickTool::Select
            | QuickTool::Comment(_)
            | QuickTool::Redact
            | QuickTool::ColumnSelect
            | QuickTool::Measure(_)
            | QuickTool::MarqueeZoom
            | QuickTool::Snapshot
    )
}

/// Start listening for pen and touch input. Call once, before the app starts.
pub fn install() {
    #[cfg(target_arch = "wasm32")]
    web::install();
}

/// This frame's stylus state from the browser listeners, or `None` where there are none (native
/// shells, or before [`install`]).
pub(crate) fn poll() -> Option<StylusInput> {
    #[cfg(target_arch = "wasm32")]
    {
        web::poll()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use std::cell::RefCell;

    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;
    use web_sys::{AddEventListenerOptions, Event, PointerEvent, TouchEvent};

    use super::{PointerKind, StylusInput};

    /// A touch while the pen is down or this soon after it lifts is a palm. Android already
    /// ignores a palm resting on the screen, and a Pixel Tablet showed deliberate finger swipes
    /// about half a second after the pen, so the window only covers the pen being lifted.
    const PEN_GRACE_MS: f64 = 300.0;
    /// A palm's `touchstart` follows its `pointerdown` within this long.
    const SAME_CONTACT_MS: f64 = 100.0;
    const STORAGE_KEY: &str = "pdfcraft-stylus";
    /// More palm contacts than a hand can make means some lifts were never reported: start over.
    const MAX_PALMS: usize = 16;

    #[derive(Default)]
    struct State {
        installed: bool,
        input: StylusInput,
        pen_down: bool,
        /// `performance.now()` of the last pen lift.
        pen_lift: f64,
        /// When the latest palm `pointerdown` was dropped, until its `touchstart` arrives.
        palm_pressed: Option<f64>,
        /// Touches set aside as palms: every later event of theirs is dropped too.
        palm_pointers: Vec<i32>,
        palm_touches: Vec<i32>,
    }

    thread_local! {
        static STATE: RefCell<State> = RefCell::new(State::default());
    }

    pub(super) fn poll() -> Option<StylusInput> {
        STATE.with_borrow(|s| s.installed.then_some(s.input))
    }

    fn now() -> f64 {
        web_sys::window().and_then(|w| w.performance()).map_or(0.0, |p| p.now())
    }

    fn pen_near(s: &State) -> bool {
        s.pen_down || now() - s.pen_lift < PEN_GRACE_MS
    }

    fn drop_event(e: &Event) {
        e.stop_immediate_propagation();
        e.prevent_default();
    }

    fn remember_enabled() {
        if let Some(Ok(Some(storage))) = web_sys::window().map(|w| w.local_storage()) {
            let _ = storage.set_item(STORAGE_KEY, "1");
        }
    }

    fn on_pointer(e: PointerEvent) {
        let kind = e.pointer_type();
        let ty = e.type_();
        let (down, up) = (ty == "pointerdown", ty == "pointerup" || ty == "pointercancel");
        STATE.with_borrow_mut(|s| match kind.as_str() {
            "pen" => {
                if down {
                    s.pen_down = true;
                    s.input.kind = PointerKind::Pen;
                } else if up {
                    s.pen_down = false;
                    s.pen_lift = now();
                }
                if !std::mem::replace(&mut s.input.enabled, true) {
                    remember_enabled();
                }
            }
            "touch" if s.input.enabled => {
                let id = e.pointer_id();
                if let Some(i) = s.palm_pointers.iter().position(|p| *p == id) {
                    if up {
                        s.palm_pointers.swap_remove(i);
                    }
                    drop_event(&e);
                } else if down {
                    if pen_near(s) {
                        if s.palm_pointers.len() >= MAX_PALMS {
                            s.palm_pointers.clear();
                        }
                        s.palm_pointers.push(id);
                        s.palm_pressed = Some(now());
                        drop_event(&e);
                    } else {
                        s.input.kind = PointerKind::Touch;
                    }
                }
            }
            "touch" if down => s.input.kind = PointerKind::Touch,
            _ if down => s.input.kind = PointerKind::Mouse,
            _ => {}
        });
    }

    /// eframe also reads touch events; drop the ones made only by palms. The pen makes touch
    /// events too, and Chrome on Android leaves their `touchType` unset, so a touch counts as a
    /// palm only when it starts right after a palm's `pointerdown` (browsers send that first).
    fn on_touch(e: TouchEvent) {
        let ty = e.type_();
        let changed = e.changed_touches();
        let ids: Vec<i32> = (0..changed.length()).filter_map(|i| changed.get(i)).map(|t| t.identifier()).collect();
        if ids.is_empty() {
            return;
        }
        STATE.with_borrow_mut(|s| {
            if !s.input.enabled {
                return;
            }
            let palms = if ty == "touchstart" {
                s.palm_pressed.take().is_some_and(|at| now() - at < SAME_CONTACT_MS)
            } else {
                ids.iter().all(|id| s.palm_touches.contains(id))
            };
            if !palms {
                return;
            }
            if ty == "touchstart" {
                if s.palm_touches.len().saturating_add(ids.len()) > MAX_PALMS {
                    s.palm_touches.clear();
                }
                s.palm_touches.extend(ids.iter().take(MAX_PALMS));
            } else if ty == "touchend" || ty == "touchcancel" {
                s.palm_touches.retain(|id| !ids.contains(id));
            }
            drop_event(&e);
        });
    }

    pub(super) fn install() {
        let Some(window) = web_sys::window() else { return };
        let flag = window.location().search().ok().and_then(|q| web_sys::UrlSearchParams::new_with_str(&q).ok()).and_then(|p| p.get("stylus"));
        let enabled = match flag.as_deref() {
            Some("off") => return,
            Some("on") => true,
            _ => window.local_storage().ok().flatten().and_then(|s| s.get_item(STORAGE_KEY).ok().flatten()).as_deref() == Some("1"),
        };
        STATE.with_borrow_mut(|s| {
            s.installed = true;
            s.input.enabled = enabled;
        });
        let opts = AddEventListenerOptions::new();
        opts.set_capture(true);
        opts.set_passive(false);
        let pointer = Closure::<dyn FnMut(PointerEvent)>::new(on_pointer);
        for ty in ["pointerdown", "pointermove", "pointerup", "pointercancel"] {
            let _ = window.add_event_listener_with_callback_and_add_event_listener_options(ty, pointer.as_ref().unchecked_ref(), &opts);
        }
        let touch = Closure::<dyn FnMut(TouchEvent)>::new(on_touch);
        for ty in ["touchstart", "touchmove", "touchend", "touchcancel"] {
            let _ = window.add_event_listener_with_callback_and_add_event_listener_options(ty, touch.as_ref().unchecked_ref(), &opts);
        }
        // The listeners live as long as the page.
        pointer.forget();
        touch.forget();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comments::CommentTool;

    fn input(enabled: bool, kind: PointerKind) -> StylusInput {
        StylusInput { enabled, kind }
    }

    #[test]
    fn a_finger_pans_with_pen_tools_only_in_stylus_mode() {
        let highlight = QuickTool::Comment(CommentTool::Highlight);
        assert!(!finger_pans(highlight, input(false, PointerKind::Touch)), "stylus mode off: the finger marks as before");
        assert!(finger_pans(highlight, input(true, PointerKind::Touch)), "a finger moves the page");
        assert!(finger_pans(QuickTool::Comment(CommentTool::Ink), input(true, PointerKind::Touch)));
        assert!(finger_pans(QuickTool::Select, input(true, PointerKind::Touch)));
        assert!(!finger_pans(QuickTool::AddText, input(true, PointerKind::Touch)), "tap-to-place tools keep the finger");
        assert!(!finger_pans(QuickTool::Hand, input(true, PointerKind::Touch)), "the Hand tool pans on its own");
        assert!(!finger_pans(highlight, input(true, PointerKind::Pen)), "the pen keeps the chosen tool");
        assert!(!finger_pans(highlight, input(true, PointerKind::Mouse)), "so does a mouse");
    }

    #[test]
    fn native_shells_have_no_stylus_input() {
        assert_eq!(poll(), None, "nothing changes outside the browser");
        assert_eq!(StylusInput::default(), input(false, PointerKind::Mouse));
    }
}
