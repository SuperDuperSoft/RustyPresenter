//! Printing a service (FEAT-6b, spec §6): the run sheet (service order) and the lyric sheet, as one web page the
//! browser prints or saves as PDF. `doc` builds a plain document from the service; `html` writes it.
#[path = "../strings/print.rs"]
pub mod strings;
pub mod doc;
pub mod html;

use crate::model::{Data, PrintSettings, PrintWhat, Service};
use chrono::NaiveDate;

/// The whole page for `service`.
pub fn html(data: &Data, service: &Service, options: &PrintSettings, today: NaiveDate) -> String {
    let order = crate::planning::date_order(&data.settings);
    let paper = options.paper.unwrap_or_else(|| crate::planning::paper(&data.settings));
    let doc = doc::build(data, service, options, order);
    html::render(&doc, paper, &strings::printed(&crate::planning::format_date(today, order)))
}

pub fn file_stem(name: &str, what: PrintWhat) -> String { strings::file_stem(name, what) }

#[cfg(test)]
mod tests;
