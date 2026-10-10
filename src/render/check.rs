//! `config check` report: one row per step, hints indented under the detail.

use super::Table;
use crate::setup::check::{Report, Status};

pub fn table(report: &Report) -> String {
    let mut t = Table::new();
    for c in &report.checks {
        let mark = match c.status {
            Status::Pass => "✓",
            Status::Warn => "!",
            Status::Fail => "✗",
            Status::Skip => "–",
        };
        t.row(&[mark, c.step, &c.detail]);
        if let Some(hint) = &c.hint {
            t.row(&["", "", &format!("hint: {hint}")]);
        }
    }
    t.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup::check::{Check, Report, Status};

    #[test]
    fn marks_and_hint_lines() {
        let r = Report::new(vec![
            Check::new("config", Status::Pass, "/p"),
            Check::new("tls", Status::Fail, "certificate not trusted: X").hint("h"),
            Check::new("auth", Status::Skip, "skipped: tls failed"),
            Check::new("permissions", Status::Warn, "0644"),
        ]);
        assert_eq!(
            table(&r),
            "✓  config       /p\n\
             ✗  tls          certificate not trusted: X\n\
             \x20               hint: h\n\
             –  auth         skipped: tls failed\n\
             !  permissions  0644\n"
        );
    }
}
