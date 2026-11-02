//! stand-in until Tasks 14 and 15 merge: the file picker (`files::pick`, Task 15) and adding the picked files
//! (`files::add_files` and `files::images_to_deck`, Task 14) are built on other branches. These keep the plan's
//! signatures so the Add dialog's buttons are wired already; at the merge each call moves to
//! `crate::ui::operator::files` and this file is deleted.
use crate::ui::operator::{file_input::Filter, window::Operator};
use std::{path::PathBuf, rc::Rc};

/// stand-in until Task 15 merges: `files::pick(op, filter, multi, done)` (native `rfd` dialog). Picks nothing.
pub fn pick(_op: &Rc<Operator>, _filter: Filter, _multi: bool, _done: impl FnOnce(Vec<PathBuf>) + 'static) {}

/// stand-in until Task 14 merges: `files::add_files(op, paths)`.
pub fn add_files(_op: &Rc<Operator>, _paths: Vec<PathBuf>) {}

/// stand-in until Task 14 merges: `files::images_to_deck(op, paths)`.
pub fn images_to_deck(_op: &Rc<Operator>, _paths: Vec<PathBuf>) {}
