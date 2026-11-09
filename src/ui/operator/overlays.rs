//! The toasts and the jobs box over the operator (`toast()` and `renderJobs()` in `operator.js:56-60, 281-290`).
//! A toast leaves after 2.6 s, or 6 s for an error. The jobs box lists this window's own uploads (client jobs)
//! first, then the core's jobs from the snapshot. A failed job, or a finished one with a note, has a close button that
//! sends `job/dismiss`; a running cancellable job has a Cancel button that sends `job/cancel`.
use super::window::Operator;
use crate::ui::{JobRow, ToastRow};
use slint::{Model, ModelRc, Timer, TimerMode, VecModel};
use std::{cell::{Cell, RefCell}, rc::{Rc, Weak}, sync::{Arc, atomic::{AtomicU8, Ordering}}, time::Duration};
use uuid::Uuid;

/// How long a toast stays (`operator.js:59`).
pub const TOAST_FOR: Duration = Duration::from_millis(2600);
pub const ERROR_TOAST_FOR: Duration = Duration::from_millis(6000);
/// How often a client job's progress, set from another thread, is copied to its card.
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

/// One of this window's own jobs, e.g. "Uploading `<name>`" (the web's `clientJobs`).
struct ClientJob { id: Uuid, label: String, percent: Arc<AtomicU8> }

/// The overlays' models and timers; one per window.
#[derive(Default)]
pub struct Overlays {
    toasts: Rc<VecModel<ToastRow>>,
    next_toast: Cell<i32>,
    jobs: Rc<VecModel<JobRow>>,
    client: RefCell<Vec<ClientJob>>,
    /// Runs while a client job exists, so progress reported off the Slint thread reaches its card.
    progress: Timer,
}

/// A client job's card, shown until this is dropped. Made by `Operator::client_job`.
pub struct JobHandle { op: Weak<Operator>, id: Uuid, percent: Arc<AtomicU8> }

impl JobHandle {
    /// Its card's id (an "Importing…" row follows its bar).
    pub fn id(&self) -> Uuid { self.id }

    /// Sets the bar (0–100) at once; on the Slint thread.
    pub fn set_percent(&self, percent: u8) {
        self.percent.store(percent.min(100), Ordering::Relaxed);
        if let Some(op) = self.op.upgrade() { op.render_jobs(); }
    }

    /// A setter for any thread, e.g. `import_path`'s progress callback; the card follows within 100 ms.
    pub fn progress(&self) -> impl Fn(u8) + Send + Sync + 'static {
        let percent = self.percent.clone();
        move |p| percent.store(p.min(100), Ordering::Relaxed)
    }
}

impl Drop for JobHandle {
    fn drop(&mut self) {
        let Some(op) = self.op.upgrade() else { return };
        let empty = {
            let mut client = op.overlays.client.borrow_mut();
            client.retain(|j| j.id != self.id);
            client.is_empty()
        };
        if empty { op.overlays.progress.stop(); }
        op.render_jobs();
    }
}

/// Hands the models to the window and wires the cards' close and Cancel buttons.
pub(super) fn wire(op: &Rc<Operator>) {
    op.ui.set_toasts(ModelRc::from(op.overlays.toasts.clone()));
    op.ui.set_jobs(ModelRc::from(op.overlays.jobs.clone()));
    let me = Rc::downgrade(op);
    op.ui.on_job_dismiss(move |id| {
        let Some(op) = me.upgrade() else { return };
        let me = me.clone();
        op.ops.call::<serde_json::Value>("job/dismiss", serde_json::json!({ "id": id.as_str() }), move |r| {
            if let (Some(op), Err(e)) = (me.upgrade(), r) { op.toast(e, true); }
        });
    });
    let me = Rc::downgrade(op);
    op.ui.on_job_cancel(move |id| {
        let Some(op) = me.upgrade() else { return };
        let me = me.clone();
        op.ops.call::<serde_json::Value>("job/cancel", serde_json::json!({ "id": id.as_str() }), move |r| {
            if let (Some(op), Err(e)) = (me.upgrade(), r) { op.toast(e, true); }
        });
    });
}

impl Operator {
    /// A short message at the bottom of the window (`toast()`): 2.6 s, or 6 s for an error.
    pub fn toast(&self, text: impl Into<String>, err: bool) {
        let text = text.into();
        #[cfg(test)]
        self.state.borrow_mut().toasts.push((text.clone(), err));
        let o = &self.overlays;
        let id = o.next_toast.get();
        o.next_toast.set(id.wrapping_add(1));
        o.toasts.push(ToastRow { id, text: text.into(), err });
        let me = self.me.clone();
        Timer::single_shot(if err { ERROR_TOAST_FOR } else { TOAST_FOR }, move || {
            let Some(op) = me.upgrade() else { return };
            let toasts = &op.overlays.toasts;
            if let Some(i) = toasts.iter().position(|t| t.id == id) { toasts.remove(i); }
        });
    }

    /// Shows a card for work this window does itself (an upload) until the handle is dropped.
    pub fn client_job(&self, label: impl Into<String>) -> JobHandle {
        let o = &self.overlays;
        let percent = Arc::new(AtomicU8::new(0));
        let id = Uuid::new_v4();
        o.client.borrow_mut().push(ClientJob { id, label: label.into(), percent: percent.clone() });
        if !o.progress.running() {
            let me = self.me.clone();
            o.progress.start(TimerMode::Repeated, PROGRESS_EVERY, move || if let Some(op) = me.upgrade() { op.render_jobs() });
        }
        self.render_jobs();
        JobHandle { op: self.me.clone(), id, percent }
    }

    /// How far a client job is, while it runs.
    pub(super) fn client_percent(&self, id: Uuid) -> Option<u8> {
        self.overlays.client.borrow().iter().find(|j| j.id == id).map(|j| j.percent.load(Ordering::Relaxed))
    }

    /// `renderJobs()`: client jobs, then the snapshot's. Rows are changed in place, so a card under the mouse
    /// keeps its hover while a bar moves.
    pub(super) fn render_jobs(&self) {
        self.render_importing();
        let mut rows: Vec<JobRow> = self.overlays.client.borrow().iter().map(|j| JobRow {
            id: j.id.to_string().into(), label: j.label.as_str().into(), percent: j.percent.load(Ordering::Relaxed) as i32, ..JobRow::default()
        }).collect();
        if let Some(s) = self.state.borrow().snap.as_ref() {
            rows.extend(s.jobs.iter().map(|j| JobRow {
                id: j.id.to_string().into(), label: j.label.as_str().into(), percent: j.percent as i32,
                error: j.error.as_deref().unwrap_or_default().into(), note: j.note.as_deref().unwrap_or_default().into(),
                cancellable: j.cancellable, done: j.done,
            }));
        }
        let model = &self.overlays.jobs;
        if model.row_count() != rows.len() { return model.set_vec(rows); }
        for (i, row) in rows.into_iter().enumerate() {
            if model.row_data(i).as_ref() != Some(&row) { model.set_row_data(i, row); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::{Job, test_app}, ui::operator::window::testing::{find, find_all, open, refresh}};
    use i_slint_backend_testing::mock_elapsed_time;
    use slint::platform::PointerEventButton;

    fn toast_texts(op: &Operator) -> Vec<String> {
        find_all(op, "ToastStack::toast").iter().map(|t| t.accessible_label().unwrap_or_default().to_string()).collect()
    }

    fn cards(op: &Operator) -> Vec<(String, String)> {
        find_all(op, "JobsBox::job").iter()
            .map(|c| (c.accessible_label().unwrap_or_default().to_string(), c.accessible_value().unwrap_or_default().to_string()))
            .collect()
    }

    #[test]
    fn toast_expires_after_2_6s_or_6s_for_errors() {
        let app = test_app();
        let op = open(&app);
        op.toast("Saved", false);
        op.toast(String::from("That item is gone."), true);
        assert_eq!(toast_texts(&op), ["Saved", "That item is gone."]);
        let err = find_all(&op, "ToastStack::toast").pop().unwrap();
        assert_eq!(err.accessible_value().unwrap_or_default(), "err");
        mock_elapsed_time(Duration::from_millis(2550));
        assert_eq!(toast_texts(&op).len(), 2, "still up just before 2.6 s");
        mock_elapsed_time(Duration::from_millis(100));
        assert_eq!(toast_texts(&op), ["That item is gone."], "the plain toast leaves at 2.6 s");
        mock_elapsed_time(Duration::from_millis(3300));
        assert_eq!(toast_texts(&op).len(), 1, "still up just before 6 s");
        mock_elapsed_time(Duration::from_millis(100));
        assert!(toast_texts(&op).is_empty(), "the error leaves at 6 s");
    }

    #[test]
    fn job_error_card_dismisses() {
        let app = test_app();
        let (running, failed) = (Uuid::new_v4(), Uuid::new_v4());
        app.jobs.lock().unwrap().extend([
            Job { id: running, label: "Importing slides".into(), percent: 30, ..Job::default() },
            Job { id: failed, label: "Importing talk.pptx".into(), error: Some("PowerPoint is not installed.".into()), ..Job::default() },
        ]);
        let op = open(&app);
        refresh(&op);
        assert_eq!(cards(&op), [("Importing slides".into(), "30".into()), ("Importing talk.pptx".into(), "PowerPoint is not installed.".into())]);
        let close = find_all(&op, "JobCard::close");
        assert_eq!(close.len(), 1, "only the failed job has a close button");
        close[0].mock_single_click(PointerEventButton::Left);
        assert_eq!(app.jobs.lock().unwrap().iter().map(|j| j.id).collect::<Vec<_>>(), [running]);
        refresh(&op);
        assert_eq!(cards(&op), [("Importing slides".into(), "30".into())]);
        assert!(find(&op, "JobCard::close").is_none());
    }

    #[test]
    fn running_cancellable_job_shows_cancel_button() {
        let app = test_app();
        app.job_start("Waiting for an old job");
        let (id, token) = app.job_start_cancellable("Importing talk.pptx");
        let op = open(&app);
        refresh(&op);
        let cancel = find_all(&op, "JobCard::cancel");
        assert_eq!(cancel.len(), 1, "only the cancellable job has one");
        assert_eq!(cancel[0].accessible_label().unwrap_or_default(), "Cancel");
        assert!(find(&op, "JobCard::close").is_none());
        assert_eq!(find_all(&op, "JobCard::bar").len(), 2);
        cancel[0].mock_single_click(PointerEventButton::Left);
        assert!(token.is_cancelled());
        app.job_end_cancelled(id);
        refresh(&op);
        assert!(find(&op, "JobCard::cancel").is_none());
    }

    #[test]
    fn finished_job_note_shows_and_dismisses() {
        let app = test_app();
        let (id, _) = app.job_start_cancellable("Importing talk.pptx");
        app.job_note(id, "Imported with LibreOffice — check the slides.".into());
        let op = open(&app);
        refresh(&op);
        assert_eq!(find_all(&op, "JobCard::note").len(), 1, "a note shows while the job runs too");
        assert!(find(&op, "JobCard::close").is_none(), "a running job cannot be dismissed");
        app.job_end(id, None);
        refresh(&op);
        assert_eq!(cards(&op), [("Importing talk.pptx".into(), "Imported with LibreOffice — check the slides.".into())]);
        assert!(find(&op, "JobCard::cancel").is_none());
        assert!(find(&op, "JobCard::bar").is_none(), "a finished job has no bar");
        find(&op, "JobCard::close").unwrap().mock_single_click(PointerEventButton::Left);
        assert!(app.jobs.lock().unwrap().is_empty());
    }

    #[test]
    fn client_upload_job_has_no_cancel() {
        let app = test_app();
        let op = open(&app);
        let _job = op.client_job("Uploading talk.pptx");
        assert_eq!(cards(&op).len(), 1);
        assert!(find(&op, "JobCard::cancel").is_none());
        assert!(find(&op, "JobCard::close").is_none());
    }

    #[test]
    fn client_job_shows_progress_and_goes_away() {
        let app = test_app();
        app.jobs.lock().unwrap().push(Job { id: Uuid::new_v4(), label: "Importing slides".into(), percent: 10, ..Job::default() });
        let op = open(&app);
        refresh(&op);
        let job = op.client_job("Uploading talk.pptx");
        assert_eq!(cards(&op), [("Uploading talk.pptx".into(), "0".into()), ("Importing slides".into(), "10".into())], "client jobs first");
        job.set_percent(40);
        assert_eq!(cards(&op)[0].1, "40");
        let progress = job.progress();
        std::thread::spawn(move || progress(75)).join().unwrap();
        mock_elapsed_time(PROGRESS_EVERY + Duration::from_millis(10));
        assert_eq!(cards(&op)[0].1, "75", "progress from another thread reaches the card");
        drop(job);
        assert_eq!(cards(&op), [("Importing slides".into(), "10".into())]);
        assert!(!op.overlays.progress.running());
    }
}
