//! One run of the engine at a time, shown in the window as it goes.
//!
//! The engine asks its questions and waits for the answer, as it does at a terminal, so a run has a thread of its own. What it
//! says goes to the window as events, and a question waits here for the window's answer.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, PoisonError};
use std::thread;

use libnima_core::flow;
use libnima_core::report::{Line, Question, Style, Ui, span};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::Asked;

/// The way to a run's questions, while one is under way.
#[derive(Default)]
pub struct Running {
    answers: Mutex<Option<Sender<Option<String>>>>,
}

/// A run, as the window shows it.
struct Shown {
    app: AppHandle,
    answers: Receiver<Option<String>>,
}

impl Shown {
    fn line(&self, line: &Line) {
        let _ = self.app.emit("run-line", line);
    }
}

impl Ui for Shown {
    fn say(&mut self, line: Line) {
        self.line(&line);
    }

    fn new_part(&mut self) {
        let _ = self.app.emit("run-part", ());
    }

    fn error(&mut self, message: &str) {
        for text in message.split('\n') {
            self.line(&vec![span(text, Style::Bad)]);
        }
    }

    fn ask(&mut self, question: &Question) -> Option<String> {
        let _ = self.app.emit("run-ask", question);

        self.answers.recv().ok().flatten()
    }

    fn shows_output(&self) -> bool {
        false
    }
}

/// Starts a run. What it says arrives as run-line, run-part and run-ask, and run-done with its exit code ends it.
#[tauri::command]
pub fn start_run(app: AppHandle, asked: Asked, running: State<Running>) -> Result<(), String> {
    let mut slot = running
        .answers
        .lock()
        .unwrap_or_else(PoisonError::into_inner);

    if slot.is_some() {
        return Err("A run is under way. Wait for it to finish.".to_string());
    }

    let (send, answers) = mpsc::channel();

    *slot = Some(send);

    thread::spawn(move || {
        let request = asked.request();
        let mut shown = Shown {
            app: app.clone(),
            answers,
        };

        // Whatever happens in a run, the window hears that it ended, or it would wait for ever.
        let code =
            catch_unwind(AssertUnwindSafe(|| flow::run(&request, &mut shown))).unwrap_or(101);

        *app.state::<Running>()
            .answers
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;

        let _ = app.emit("run-done", code);
    });

    Ok(())
}

/// The answer to the question the run is waiting on. None takes what the question offers.
#[tauri::command]
pub fn answer_run(answer: Option<String>, running: State<Running>) {
    if let Some(send) = &*running
        .answers
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
    {
        let _ = send.send(answer);
    }
}
