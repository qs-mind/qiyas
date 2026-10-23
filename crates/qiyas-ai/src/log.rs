//! The agent's log: a plain-text file the user can read, with every conversation, request, answer,
//! tool run and error, so a turn that went wrong can be looked into afterwards. It never holds the
//! API key: errors are scrubbed before they get here.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Past this size the log moves to `<file>.1` (dropping the one before) and starts afresh.
const MAX_BYTES: u64 = 5 * 1024 * 1024;
/// Most characters kept from one text in an entry (an answer, a tool's input or result).
pub const MAX_TEXT: usize = 4000;

#[derive(Debug, Clone)]
pub struct AgentLog {
    path: PathBuf,
    rotate: bool,
}

impl AgentLog {
    /// A log at `path`; its folder is made at the first entry.
    pub fn new(path: PathBuf) -> AgentLog {
        AgentLog { path, rotate: true }
    }

    /// Evaluation keeps every step for the complete run.
    pub fn evaluation(path: PathBuf) -> AgentLog {
        AgentLog {
            path,
            rotate: false,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Adds one entry: a line with the local time and `title`, then `body` indented under it. A
    /// log that can't be written is skipped: the agent goes on without it.
    pub fn write(&self, title: &str, body: &str) {
        if let Err(e) = self.append(title, body) {
            eprintln!("The agent log could not be written: {e}");
        }
    }

    fn append(&self, title: &str, body: &str) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        if self.rotate && fs::metadata(&self.path).is_ok_and(|m| m.len() > MAX_BYTES) {
            let mut old = self.path.clone().into_os_string();
            old.push(".1");
            fs::rename(&self.path, old)?;
        }
        let mut entry = format!(
            "{}  {title}\n",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f")
        );
        for line in body.lines() {
            entry.push_str("    ");
            entry.push_str(line);
            entry.push('\n');
        }
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        // It holds the user's messages and what was read from the drawing: the user's alone.
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        options.open(&self.path)?.write_all(entry.as_bytes())
    }
}

/// `text` cut to `MAX_TEXT` characters, saying how much was left out.
pub fn clip(text: &str) -> String {
    let total = text.chars().count();
    if total <= MAX_TEXT {
        return text.to_string();
    }
    let kept: String = text.chars().take(MAX_TEXT).collect();
    format!("{kept}… ({} more characters)", total - MAX_TEXT)
}

/// The last `MAX_TEXT` characters of `text` after a `…`, or all of it when it is short enough: for
/// a request, whose last messages are the ones that matter.
pub fn clip_tail(text: &str) -> String {
    let total = text.chars().count();
    if total <= MAX_TEXT {
        return text.to_string();
    }
    let kept: String = text.chars().skip(total - MAX_TEXT).collect();
    format!("…{kept}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_are_timed_and_indented_and_a_full_log_moves_aside() {
        let dir = tempfile::tempdir().unwrap();
        let log = AgentLog::new(dir.path().join("logs").join("agent.log"));
        log.write("REQUEST 1", "3 messages\nsecond line");
        let text = fs::read_to_string(log.path()).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[0].ends_with("  REQUEST 1"), "{text}");
        assert_eq!(lines[0].as_bytes()[4], b'-', "starts with the date: {text}");
        assert_eq!(&lines[1..], ["    3 messages", "    second line"]);

        fs::write(log.path(), vec![b'x'; MAX_BYTES as usize + 1]).unwrap();
        log.write("DONE", "");
        assert!(dir.path().join("logs").join("agent.log.1").exists());
        assert!(fs::read_to_string(log.path())
            .unwrap()
            .ends_with("  DONE\n"));
    }

    #[test]
    fn long_text_is_clipped() {
        assert_eq!(clip("short"), "short");
        let long = "a".repeat(MAX_TEXT + 5);
        assert!(clip(&long).ends_with("… (5 more characters)"));
    }

    #[test]
    fn a_long_text_can_keep_its_tail() {
        assert_eq!(clip_tail("short"), "short");
        let long = format!("{}{}", "a".repeat(5), "b".repeat(MAX_TEXT));
        let tail = clip_tail(&long);
        assert_eq!(tail, format!("…{}", "b".repeat(MAX_TEXT)));
    }
}
