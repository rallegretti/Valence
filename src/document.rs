//! The open lab "document": its file path, unsaved-changes tracking, the
//! Save / Save As / Open flows with native file dialogs, and the guard that
//! asks before discarding unsaved work (New, Open, example shelf, Quit and
//! the window's close button).

use std::{future::Future, path::PathBuf, pin::Pin};

use bevy::{
    prelude::*,
    tasks::{block_on, poll_once},
    window::{PrimaryWindow, WindowCloseRequested},
};
use bevy_egui::egui::{self, Color32, RichText};

use crate::camera::OrbitCam;
use crate::labfile::{self, LabFile, SavedCamera, SavedReactionPart};
use crate::library::MyLibrary;
use crate::reactions_ui::ReactionState;
use crate::state::{Chem, Edit, Selection};

/// Things that may discard unsaved work.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    New,
    Open,
    OpenExample,
    Quit,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Request {
    Save,
    SaveAs,
    /// Ask about unsaved changes first.
    Do(Action),
    /// The user chose "Don't save".
    Discard(Action),
}

#[derive(Resource, Default)]
pub struct Document {
    pub path: Option<PathBuf>,
    saved_lab_rev: u64,
    saved_fingerprint: u64,
    initialised: bool,
    dirty: bool,
    requests: Vec<Request>,
    /// Unsaved-changes prompt being shown for this action.
    confirm: Option<Action>,
    /// Action to carry out once a save succeeds ("Save" in the prompt).
    after_save: Option<Action>,
    toast: Option<(String, bool, f64)>,
}

impl Document {
    pub fn request(&mut self, r: Request) {
        self.requests.push(r);
    }

    pub fn display_name(&self) -> String {
        self.path.as_ref().and_then(|p| p.file_stem()).map_or_else(
            || "Untitled lab".to_string(),
            |s| s.to_string_lossy().into_owned(),
        )
    }

    fn toast(&mut self, text: impl Into<String>, ok: bool, now: f64) {
        self.toast = Some((text.into(), ok, now));
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Purpose {
    Open,
    SaveAs,
}

type DialogFuture = Pin<Box<dyn Future<Output = Option<rfd::FileHandle>>>>;

/// An open native file dialog (polled each frame, never blocking).
#[derive(Default)]
pub struct DialogSlot(Option<(Purpose, DialogFuture)>);

fn fingerprint(reaction: &ReactionState, mine: &MyLibrary) -> u64 {
    reaction.fingerprint().rotate_left(17) ^ mine.fingerprint()
}

fn open_dialog(slot: &mut DialogSlot, purpose: Purpose, current: Option<&PathBuf>) {
    let mut d = rfd::AsyncFileDialog::new().add_filter("Valence lab", &[labfile::EXTENSION]);
    if let Some(dir) = current.and_then(|p| p.parent()) {
        d = d.set_directory(dir);
    }
    let fut: DialogFuture = match purpose {
        Purpose::Open => Box::pin(d.set_title("Open Lab Data").pick_file()),
        Purpose::SaveAs => {
            let name = current.and_then(|p| p.file_name()).map_or_else(
                || "Untitled lab.valence".to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            Box::pin(d.set_title("Save Lab Data").set_file_name(name).save_file())
        }
    };
    slot.0 = Some((purpose, fut));
}

/// Everything saved in a lab data file, gathered from the app state.
pub fn snapshot(
    chem: &Chem,
    reaction: &ReactionState,
    mine: &MyLibrary,
    rig: &OrbitCam,
) -> LabFile {
    let (data, ids) = chem.lab.to_data();
    let index_of = |id: valence_chem::AtomId| ids.iter().position(|&x| x == id).map(|i| i as u32);
    let mut file = LabFile::new(data);
    file.reactions = reaction
        .to_saved(chem)
        .into_iter()
        .filter_map(|(anchor, role, coeff)| {
            Some(SavedReactionPart {
                atom: index_of(anchor)?,
                role: role.key().to_string(),
                coeff,
            })
        })
        .collect();
    file.my_molecules = mine.molecules.clone();
    file.camera = Some(SavedCamera {
        focus: rig.focus.to_array(),
        yaw: rig.yaw,
        pitch: rig.pitch,
        dist: rig.dist,
    });
    file
}

/// Save to `path` atomically.
pub fn save_lab(
    path: &std::path::Path,
    chem: &Chem,
    reaction: &ReactionState,
    mine: &MyLibrary,
    rig: &OrbitCam,
) -> Result<(), String> {
    let bytes = snapshot(chem, reaction, mine, rig).to_bytes()?;
    labfile::save_atomic(path, &bytes)
}

/// Load from `path`. Everything is read and validated first; the open lab
/// is only replaced once that all succeeded (and the replacement is an undo
/// step).
pub fn load_lab(
    path: &std::path::Path,
    chem: &mut Chem,
    reaction: &mut ReactionState,
    mine: &mut MyLibrary,
    rig: &mut OrbitCam,
) -> Result<(), String> {
    let loaded = labfile::read(path)?.validate()?;
    let new_lab = loaded.lab;
    match chem.edit("open", move |lab| {
        lab.restore(new_lab);
        Ok(())
    }) {
        Edit::Done(()) => {}
        Edit::Rejected(e) => return Err(e.0),
        Edit::Crashed => {
            return Err("Something went wrong while opening; nothing was changed.".into());
        }
    }
    chem.refresh();
    reaction.replace(loaded.reactions);
    mine.molecules = loaded.molecules;
    if let Some(c) = loaded.camera {
        rig.focus = Vec3::from_array(c.focus);
        rig.yaw = c.yaw;
        rig.pitch = c.pitch;
        rig.dist = c.dist.clamp(2.5, 200.0);
        rig.pitch = c.pitch.clamp(-1.5, 1.5);
        rig.sanitize();
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn document_system(
    mut doc: ResMut<Document>,
    mut slot: NonSendMut<DialogSlot>,
    mut chem: ResMut<Chem>,
    mut reaction: ResMut<ReactionState>,
    mut mine: ResMut<MyLibrary>,
    mut rig: Single<&mut OrbitCam>,
    mut selection: ResMut<Selection>,
    keys: Res<ButtonInput<KeyCode>>,
    mut close: MessageReader<WindowCloseRequested>,
    mut exit: MessageWriter<AppExit>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs_f64();
    let mark_clean =
        |doc: &mut Document, chem: &mut Chem, reaction: &ReactionState, mine: &MyLibrary| {
            doc.saved_lab_rev = chem.lab.revision();
            doc.saved_fingerprint = fingerprint(reaction, mine);
            chem.user_moved = false;
        };
    if !doc.initialised {
        doc.initialised = true;
        mark_clean(&mut doc, &mut chem, &reaction, &mine);
    }
    doc.dirty = chem.lab.revision() != doc.saved_lab_rev
        || fingerprint(&reaction, &mine) != doc.saved_fingerprint
        || chem.user_moved;

    // Window title: "Valence — my lab •" (• = unsaved changes).
    let title = format!(
        "Valence — {}{}",
        doc.display_name(),
        if doc.dirty { " •" } else { "" }
    );
    if window.title != title {
        window.title = title;
    }

    // Keyboard shortcuts (ignored while a dialog or prompt is up).
    let busy = slot.0.is_some() || doc.confirm.is_some();
    let cmd = keys.any_pressed([
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    if !busy && cmd && keys.just_pressed(KeyCode::KeyS) {
        doc.request(if shift {
            Request::SaveAs
        } else {
            Request::Save
        });
    }
    if !busy && cmd && keys.just_pressed(KeyCode::KeyO) {
        doc.request(Request::Do(Action::Open));
    }
    if !busy && cmd && keys.just_pressed(KeyCode::KeyN) {
        doc.request(Request::Do(Action::New));
    }
    for _ in close.read() {
        doc.request(Request::Do(Action::Quit));
    }

    // A file dialog in progress?
    if let Some((purpose, fut)) = slot.0.as_mut() {
        let purpose = *purpose;
        let Some(result) = block_on(poll_once(fut)) else {
            return;
        };
        slot.0 = None;
        match (purpose, result) {
            (_, None) => doc.after_save = None, // cancelled: don't continue to quit/open/new
            (Purpose::Open, Some(h)) => {
                let path = h.path().to_path_buf();
                match load_lab(&path, &mut chem, &mut reaction, &mut mine, &mut rig) {
                    Ok(()) => {
                        selection.atom = None;
                        doc.path = Some(path);
                        mark_clean(&mut doc, &mut chem, &reaction, &mine);
                        let name = doc.display_name();
                        doc.toast(format!("Opened {name}"), true, now);
                    }
                    Err(e) => doc.toast(format!("Couldn't open: {e}"), false, now),
                }
            }
            (Purpose::SaveAs, Some(h)) => {
                let path = labfile::with_extension(h.path().to_path_buf());
                do_save(path, &mut doc, &mut chem, &reaction, &mine, &rig, now);
            }
        }
    }
    if slot.0.is_some() || doc.confirm.is_some() {
        return;
    }

    let requests = std::mem::take(&mut doc.requests);
    for r in requests {
        let action = match r {
            Request::Save => match doc.path.clone() {
                Some(path) => {
                    do_save(path, &mut doc, &mut chem, &reaction, &mine, &rig, now);
                    continue;
                }
                None => {
                    open_dialog(&mut slot, Purpose::SaveAs, None);
                    break;
                }
            },
            Request::SaveAs => {
                open_dialog(&mut slot, Purpose::SaveAs, doc.path.as_ref());
                break;
            }
            Request::Do(a) if doc.dirty => {
                doc.confirm = Some(a);
                break;
            }
            Request::Do(a) | Request::Discard(a) => a,
        };
        match action {
            Action::Quit => {
                exit.write(AppExit::Success);
            }
            Action::Open => {
                open_dialog(&mut slot, Purpose::Open, doc.path.as_ref());
                break;
            }
            Action::New | Action::OpenExample => {
                let edit = chem.edit("new lab", |lab| {
                    lab.clear();
                    if action == Action::OpenExample {
                        crate::debug::build_demo(lab).map(|_| ())
                    } else {
                        Ok(())
                    }
                });
                if let Edit::Done(()) = edit {
                    reaction.replace(Vec::new());
                    selection.atom = None;
                    doc.path = None;
                    chem.refresh();
                    if action == Action::OpenExample {
                        crate::camera::frame_all(&mut rig, &chem);
                    } else {
                        **rig = OrbitCam::default();
                    }
                    mark_clean(&mut doc, &mut chem, &reaction, &mine);
                }
            }
        }
    }
}

fn do_save(
    path: PathBuf,
    doc: &mut Document,
    chem: &mut Chem,
    reaction: &ReactionState,
    mine: &MyLibrary,
    rig: &OrbitCam,
    now: f64,
) {
    match save_lab(&path, chem, reaction, mine, rig) {
        Ok(()) => {
            doc.path = Some(path);
            doc.saved_lab_rev = chem.lab.revision();
            doc.saved_fingerprint = fingerprint(reaction, mine);
            chem.user_moved = false;
            doc.dirty = false;
            let name = doc.display_name();
            doc.toast(format!("Saved {name}"), true, now);
            // "Save" from the unsaved-changes prompt: now carry on.
            if let Some(a) = doc.after_save.take() {
                doc.request(Request::Discard(a));
            }
        }
        Err(e) => {
            doc.after_save = None;
            doc.toast(e, false, now);
        }
    }
}

/// Unsaved-changes prompt and save/open notifications.
pub fn document_ui(ctx: &egui::Context, doc: &mut Document, view: egui::Rect, now: f64) {
    if let Some(action) = doc.confirm {
        let what = match action {
            Action::New => "starting a new lab",
            Action::Open => "opening another file",
            Action::OpenExample => "opening the example shelf",
            Action::Quit => "quitting",
        };
        let name = doc.display_name();
        let response = egui::Modal::new("unsaved".into()).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(
                RichText::new(format!("Save changes to \"{name}\"?"))
                    .size(18.0)
                    .strong()
                    .color(Color32::WHITE),
            );
            ui.label(format!(
                "You have unsaved changes. They'll be lost if you continue {what} without saving."
            ));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(RichText::new("Save").strong()).clicked() {
                    doc.confirm = None;
                    doc.after_save = Some(action);
                    doc.request(Request::Save);
                }
                if ui.button("Don't save").clicked() {
                    doc.confirm = None;
                    doc.request(Request::Discard(action));
                }
                if ui.button("Cancel").clicked() {
                    doc.confirm = None;
                }
            });
        });
        if response.should_close() {
            doc.confirm = None;
        }
    }
    if let Some((text, ok, born)) = &doc.toast {
        let age = (now - born) as f32;
        if age > 4.0 {
            doc.toast = None;
            return;
        }
        let alpha = (1.0 - ((age - 3.0).max(0.0))).clamp(0.0, 1.0);
        let col = if *ok {
            Color32::from_rgb(80, 220, 120)
        } else {
            Color32::from_rgb(255, 110, 110)
        }
        .gamma_multiply(alpha);
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            "doc_toast".into(),
        ));
        let galley = ctx.fonts_mut(|f| {
            f.layout(
                text.clone(),
                egui::FontId::proportional(15.0),
                col,
                (view.width() - 60.0).max(200.0),
            )
        });
        let rect = egui::Rect::from_center_size(
            egui::pos2(view.center().x, view.top() + 34.0),
            galley.size() + egui::vec2(28.0, 16.0),
        );
        painter.rect_filled(rect, 8.0, Color32::from_black_alpha((220.0 * alpha) as u8));
        painter.galley(rect.min + egui::vec2(14.0, 8.0), galley, col);
    }
}
