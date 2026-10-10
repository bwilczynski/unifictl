//! Table/tree rendering, ported from hlctl's network templates
//! (homelab-cli/internal/cli/network/templates).

pub mod clients;
pub mod devices;
pub mod format;
pub mod ports;
pub mod ssids;
pub mod topology;
pub mod vlans;
pub mod wans;

use std::io::Write;

use tabwriter::TabWriter;

/// Tab-separated rows aligned like Go's `tabwriter.NewWriter(w, 0, 0, 2, ' ', 0)`.
/// `flush()` ends a column block (Go templates' `{{ flush }}`), so sections
/// get independent widths.
pub struct Table {
    tw: TabWriter<Vec<u8>>,
}

impl Default for Table {
    fn default() -> Self {
        Self::new()
    }
}

// Writes go to an in-memory Vec<u8> and cannot fail.
impl Table {
    pub fn new() -> Self {
        Self {
            tw: TabWriter::new(Vec::new()).minwidth(0).padding(2),
        }
    }

    pub fn row(&mut self, cells: &[&str]) {
        writeln!(self.tw, "{}", cells.join("\t")).expect("write to memory");
    }

    /// A line with no columns, e.g. `--- PORTS ---`.
    pub fn line(&mut self, text: &str) {
        writeln!(self.tw, "{text}").expect("write to memory");
    }

    pub fn blank(&mut self) {
        writeln!(self.tw).expect("write to memory");
    }

    pub fn flush(&mut self) {
        self.tw.flush().expect("write to memory");
    }

    pub fn finish(self) -> String {
        let bytes = self.tw.into_inner().expect("write to memory");
        String::from_utf8(bytes).expect("renderers only write UTF-8")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aligns_columns_with_two_space_padding() {
        let mut t = Table::new();
        t.row(&["ID", "NAME"]);
        t.row(&["a", "longer"]);
        t.row(&["abcd", "x"]);
        assert_eq!(t.finish(), "ID    NAME\na     longer\nabcd  x\n");
    }

    #[test]
    fn flush_starts_independent_block() {
        let mut t = Table::new();
        t.row(&["A", "B"]);
        t.flush();
        t.blank();
        t.line("--- X ---");
        t.row(&["LONGVALUE", "C"]);
        assert_eq!(t.finish(), "A  B\n\n--- X ---\nLONGVALUE  C\n");
    }
}
