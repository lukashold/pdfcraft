//! Touch/tablet input shared by the native and WebAssembly shells.
//!
//! OS pen drivers already provide pointer positions. Pressure is deliberately not used to
//! guess a pen: touchscreens can report finger pressure too. A second contact cancels a mark
//! and reserves the whole contact sequence for navigation, including the final finger release.

use egui::{Event, TouchDeviceId, TouchId, TouchPhase};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TouchMode {
    #[default]
    Auto,
    On,
    Off,
}

impl TouchMode {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "on" => Ok(Self::On),
            "off" => Ok(Self::Off),
            _ => Err("touch-mode must be auto, on or off".into()),
        }
    }

    pub fn active(self, detected: bool) -> bool {
        match self {
            Self::Auto => detected,
            Self::On => true,
            Self::Off => false,
        }
    }
}

#[derive(Default)]
pub(crate) struct TouchState {
    contacts: Vec<(TouchDeviceId, TouchId, egui::Pos2)>,
    pub origin: Option<egui::Pos2>,
    pub translation: egui::Vec2,
    pub zoom: Option<f32>,
    pub center: Option<egui::Pos2>,
    detected: bool,
    saw_touch: bool,
    navigation: bool,
    /// Also true on the last release frame; no last-finger tap may create an annotation.
    pub blocked: bool,
}

impl TouchState {
    pub fn detected(&self) -> bool {
        self.detected
    }

    pub fn observe(&mut self, events: &[Event], focused: bool, pointer_down: bool) {
        let previous = self.pair();
        let mut changed_contacts = false;
        self.translation = egui::Vec2::ZERO;
        self.zoom = None;
        self.center = None;
        self.blocked = self.navigation;
        if self.contacts.is_empty() && !self.navigation {
            self.origin = None;
        }
        if !focused {
            self.blocked = true;
            self.contacts.clear();
            self.navigation = false;
            return;
        }
        for event in events {
            if let Event::Touch { device_id, id, phase, pos, .. } = event {
                self.saw_touch = true;
                let matches = |contact: &(TouchDeviceId, TouchId, egui::Pos2)| contact.0 == *device_id && contact.1 == *id;
                match phase {
                    TouchPhase::Start => {
                        if self.contacts.is_empty() {
                            self.origin = Some(*pos);
                        }
                        if !self.contacts.iter().any(matches) {
                            changed_contacts = true;
                            // Bound even a hostile/synthetic input stream. More contacts can
                            // only navigate; retaining their coordinates is unnecessary.
                            if self.contacts.len() < 32 {
                                self.contacts.push((*device_id, *id, *pos));
                            }
                            if self.contacts.len() > 1 {
                                self.navigation = true;
                                self.blocked = true;
                            }
                        }
                    }
                    TouchPhase::End | TouchPhase::Cancel => {
                        changed_contacts = true;
                        self.contacts.retain(|contact| !matches(contact));
                        if *phase == TouchPhase::Cancel {
                            self.navigation = true;
                            self.blocked = true;
                        }
                    }
                    TouchPhase::Move => {
                        if pos.x.is_finite()
                            && pos.y.is_finite()
                            && let Some(contact) = self.contacts.iter_mut().find(|contact| matches(contact))
                        {
                            contact.2 = *pos;
                        }
                    }
                }
            }
        }
        // Use the delivered contact positions: egui's first gesture can have no previous
        // pointer position on a touchscreen, losing the entire first movement otherwise.
        if !changed_contacts && let (Some((before, distance)), Some((after, new_distance))) = (previous, self.pair()) {
            self.translation = after - before;
            // Zoom about the old midpoint, then translate to the new one. Anchoring at
            // `after` would apply part of the pan twice during simultaneous pinch/pan.
            self.center = Some(before);
            if distance > 1.0 {
                let ratio = new_distance / distance;
                self.zoom = (ratio.is_finite() && ratio > 0.0).then_some(ratio.clamp(0.1, 10.0));
            }
        }
        if self.contacts.is_empty() {
            self.navigation = false;
            // Do not move the page or its tools under the user's first finger/stylus stroke.
            if !pointer_down && !events.iter().any(|event| matches!(event, Event::Touch { .. })) {
                self.detected |= self.saw_touch;
            }
        }
    }

    fn pair(&self) -> Option<(egui::Pos2, f32)> {
        let [a, b] = self.contacts.as_slice() else { return None };
        let center = a.2 + (b.2 - a.2) * 0.5;
        let distance = a.2.distance(b.2);
        (center.x.is_finite() && center.y.is_finite() && distance.is_finite()).then_some((center, distance))
    }
}

/// A scrollable 44-point toolbar keeps drawing and undo reachable on small touch screens.
pub(crate) fn toolbar(ui: &mut egui::Ui, app: &mut crate::PdfCraftApp) {
    use crate::{QuickTool, comments::CommentTool, icons};
    if !app.touch_active() || app.active.is_none() {
        return;
    }
    ui.scope(|ui| {
        // A visible rail explains that more tools are off-screen even without a hover cursor.
        ui.spacing_mut().scroll = egui::style::ScrollStyle { foreground_color: true, ..egui::style::ScrollStyle::solid() };
        egui::ScrollArea::horizontal()
            .id_salt("touch-tools")
            .auto_shrink([false, true])
            // Register the drag target before the first touch arrives. OnTouch only adds it
            // after detection, too late for the first press when touch mode is explicitly On.
            .scroll_source(egui::scroll_area::ScrollSource { drag: egui::scroll_area::DragScroll::Always, ..Default::default() })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for (tool, icon, label) in [
                        (QuickTool::Hand, "hand", "Hand (H)"),
                        (QuickTool::Select, "mouse-pointer-2", "Select (V)"),
                        (QuickTool::Comment(CommentTool::Ink), "pencil", "Draw"),
                        (QuickTool::Comment(CommentTool::Highlight), "highlighter", "Highlight"),
                        (QuickTool::Comment(CommentTool::Note), "message-square-plus", "Add a sticky note"),
                        (QuickTool::Comment(CommentTool::TextBox), "type", "Add a text box"),
                        (QuickTool::Comment(CommentTool::Eraser), "eraser", "Eraser"),
                    ] {
                        if icons::button(ui, icon, 44.0, app.quick_tool == tool, tl!(label)).clicked() {
                            if let QuickTool::Comment(comment) = tool {
                                app.execute(comment.command());
                            } else {
                                app.quick_tool = tool;
                            }
                        }
                    }
                    ui.separator();
                    if icons::button(ui, "undo-2", 44.0, false, tl!("Undo")).clicked() {
                        app.undo();
                    }
                    if icons::button(ui, "redo-2", 44.0, false, tl!("Redo")).clicked() {
                        app.redo();
                    }
                    if let QuickTool::Comment(tool) = app.quick_tool {
                        ui.separator();
                        crate::comments::quick_bar_controls(ui, tool, &mut app.comment_prefs);
                    }
                });
            });
    });
}

pub(crate) fn control_size(ctx: &egui::Context, size: f32) -> f32 {
    if ctx.data(|data| data.get_temp::<bool>(egui::Id::new("pdfcraft-touch"))).unwrap_or(false) { size.max(44.0) } else { size }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(id: u64, phase: TouchPhase) -> Event {
        Event::Touch { device_id: TouchDeviceId(1), id: TouchId(id), phase, pos: egui::pos2(20.0, 40.0), force: None }
    }

    #[test]
    fn navigation_cancels_until_the_last_finger_is_released() {
        let mut state = TouchState::default();
        state.observe(&[touch(1, TouchPhase::Start)], true, true);
        assert!(!state.blocked);
        assert!(!state.detected(), "no toolbar jump during the first stroke");
        state.observe(&[touch(2, TouchPhase::Start), touch(2, TouchPhase::End)], true, true);
        assert!(state.blocked, "two fingers within one frame also cancel");
        state.observe(&[touch(1, TouchPhase::Move)], true, true);
        assert!(state.blocked);
        state.observe(&[touch(1, TouchPhase::End)], true, false);
        assert!(state.blocked);
        state.observe(&[], true, false);
        assert!(state.detected());
        assert!(!state.blocked);
    }

    #[test]
    fn cancelled_contacts_and_focus_loss_never_commit_a_mark() {
        let mut state = TouchState::default();
        state.observe(&[touch(1, TouchPhase::Start), touch(1, TouchPhase::Cancel)], true, false);
        assert!(state.blocked);
        state.observe(&[touch(2, TouchPhase::Start)], true, true);
        state.observe(&[], false, false);
        assert!(state.blocked);
        state.observe(&[], true, false);
        assert!(!state.blocked);
    }

    #[test]
    fn preference_round_trips_and_invalid_settings_fall_back_to_auto() {
        let mut app = crate::PdfCraftApp::new();
        app.set_option("touch-mode", "on").unwrap();
        assert!(app.touch_active());
        let saved = app.persist();
        let mut restored = crate::PdfCraftApp::new();
        restored.restore(&saved);
        assert_eq!(restored.touch_mode, TouchMode::On);
        restored.restore(&saved.replace("\"touch_mode\":\"on\"", "\"touch_mode\":\"broken\""));
        assert_eq!(restored.touch_mode, TouchMode::Auto);
        assert!(app.set_option("touch-mode", "sometimes").is_err());
        assert_eq!(app.touch_mode, TouchMode::On, "invalid control requests keep the choice");
    }
}
