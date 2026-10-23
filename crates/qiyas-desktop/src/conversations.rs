//! Private, app-wide conversations. The small index is enough to show the history list.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use qiyas_ai::conversation::Conversation;
use serde::{Deserialize, Serialize};

pub const MAX_CONVERSATIONS: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Summary {
    pub id: String,
    pub title: String,
    pub drawing: Option<PathBuf>,
    pub updated: u64,
    pub turns: usize,
}

impl From<&Conversation> for Summary {
    fn from(conversation: &Conversation) -> Self {
        Self {
            id: conversation.id.clone(),
            title: conversation.title.clone(),
            drawing: conversation.drawing.clone(),
            updated: conversation.updated,
            turns: conversation.turns(),
        }
    }
}

#[derive(Clone)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(config_dir: &Path) -> Self {
        Self {
            dir: config_dir.join("conversations"),
        }
    }

    fn path(&self, id: &str) -> Result<PathBuf, String> {
        if id.is_empty()
            || id.eq_ignore_ascii_case("index")
            || id.len() > 64
            || !id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
        {
            return Err("Invalid conversation id".into());
        }
        Ok(self.dir.join(format!("{id}.json")))
    }

    fn index(&self) -> Result<Vec<Summary>, String> {
        match fs::read(self.dir.join("index.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Once at startup, recover a chat whose file was renamed just before a crash interrupted
    /// its index write. Normal listing never reads conversation bodies.
    pub fn recover(&self) -> Result<(), String> {
        if !self.dir.exists() {
            return Ok(());
        }
        let index = self.index();
        let rebuild = index.is_err() || !self.dir.join("index.json").is_file();
        let mut entries = index.unwrap_or_default();
        let previous = entries.clone();
        entries.retain(|entry| self.path(&entry.id).is_ok_and(|path| path.is_file()));
        for file in fs::read_dir(&self.dir).map_err(|e| e.to_string())? {
            let path = file.map_err(|e| e.to_string())?.path();
            if path.extension().is_some_and(|extension| extension == "tmp") {
                remove_if_present(&path)?;
                continue;
            }
            if !path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|id| id.to_str()) else {
                continue;
            };
            if self.path(id).is_err() || entries.iter().any(|entry| entry.id == id) {
                continue;
            }
            if let Ok(conversation) = self.load(id) {
                entries.push(Summary::from(&conversation));
            }
        }
        entries.sort_by(|a, b| b.updated.cmp(&a.updated).then_with(|| a.id.cmp(&b.id)));
        let expired = entries.split_off(entries.len().min(MAX_CONVERSATIONS));
        if rebuild || entries != previous {
            write_json(&self.dir.join("index.json"), &entries)?;
        }
        for entry in expired {
            remove_if_present(&self.path(&entry.id)?)?;
        }
        Ok(())
    }

    pub fn list(&self, drawing: Option<&Path>) -> Result<Vec<Summary>, String> {
        let mut entries = self.index()?;
        if let Some(drawing) = drawing {
            entries.retain(|entry| {
                entry
                    .drawing
                    .as_deref()
                    .is_some_and(|path| crate::settings::same_file(path, drawing))
            });
        }
        entries.sort_by(|a, b| b.updated.cmp(&a.updated).then_with(|| a.id.cmp(&b.id)));
        Ok(entries)
    }

    pub fn load(&self, id: &str) -> Result<Conversation, String> {
        let path = self.path(id)?;
        let bytes = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let conversation: Conversation =
            serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if conversation.id != id {
            return Err("Conversation id does not match its file".into());
        }
        Ok(conversation)
    }

    pub fn save(&self, conversation: &Conversation) -> Result<(), String> {
        if conversation.id.is_empty() || conversation.transcript.is_empty() {
            return Ok(());
        }
        let conversation = conversation.clone().without_pictures();
        let path = self.path(&conversation.id)?;
        let mut entries = self.index()?;
        entries.retain(|entry| entry.id != conversation.id);
        entries.push(Summary::from(&conversation));
        entries.sort_by(|a, b| b.updated.cmp(&a.updated).then_with(|| a.id.cmp(&b.id)));
        let expired = entries.split_off(entries.len().min(MAX_CONVERSATIONS));
        fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        write_json(&path, &conversation)?;
        write_json(&self.dir.join("index.json"), &entries)?;
        for entry in expired {
            remove_if_present(&self.path(&entry.id)?)?;
        }
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        let path = self.path(id)?;
        let mut entries = self.index()?;
        entries.retain(|entry| entry.id != id);
        // Recovery removes a stale index entry; it must never resurrect a deleted body.
        remove_if_present(&path)?;
        if self.dir.exists() {
            write_json(&self.dir.join("index.json"), &entries)?;
        }
        Ok(())
    }

    pub fn delete_all(&self) -> Result<(), String> {
        if !self.dir.exists() {
            return Ok(());
        }
        // Include files orphaned by an interruption between a chat and index write.
        for entry in fs::read_dir(&self.dir).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path
                .extension()
                .is_some_and(|extension| extension == "json" || extension == "tmp")
            {
                remove_if_present(&path)?;
            }
        }
        Ok(())
    }
}

fn remove_if_present(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    write_atomic(path, &bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// A private temporary file, synced before rename. Existing permissive modes never survive.
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).map_err(|error| io::Error::other(error.to_string()))?;
    let temp = path.with_extension(format!("{:016x}.tmp", u64::from_le_bytes(random)));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        #[cfg(unix)]
        if let Some(parent) = path.parent() {
            fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use qiyas_ai::conversation::TranscriptEntry;
    use qiyas_ai::prompt::Lang;
    use serde_json::json;

    fn conversation(id: &str, updated: u64, drawing: Option<&Path>) -> Conversation {
        Conversation {
            id: id.into(),
            title: format!("Conversation {id}"),
            created: 1,
            updated,
            drawing: drawing.map(Path::to_path_buf),
            provider: "mock".into(),
            model: "mock".into(),
            language: Lang::En,
            system: Some("unchanged\r\nprompt".into()),
            history: Vec::new(),
            image_bytes: 0,
            provider_state: json!({}),
            transcript: vec![TranscriptEntry::User {
                text: "Draw a demo".into(),
                pointed: vec!["Selection · 2".into()],
                images: Vec::new(),
            }],
            phase_state: Default::default(),
        }
    }

    #[test]
    fn saves_lists_filters_loads_and_deletes_without_loading_bodies_for_the_list() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        assert!(store.list(None).unwrap().is_empty());
        let drawing = dir.path().join("plan.dwg");
        let first = conversation("first", 1, Some(&drawing));
        store.save(&first).unwrap();
        store.save(&conversation("second", 2, None)).unwrap();
        assert_eq!(store.load("first").unwrap(), first);
        let all = store.list(None).unwrap();
        assert_eq!(
            all.iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            ["second", "first"]
        );
        assert_eq!(all[0].turns, 1);
        assert_eq!(store.list(Some(&drawing)).unwrap().len(), 1);
        fs::write(store.path("first").unwrap(), "invalid body").unwrap();
        assert_eq!(store.list(Some(&drawing)).unwrap().len(), 1);
        assert!(store.load("first").is_err());
        store.delete("first").unwrap();
        assert!(!store.path("first").unwrap().exists());
        assert_eq!(store.list(None).unwrap().len(), 1);
        store.delete_all().unwrap();
        assert!(store.list(None).unwrap().is_empty());
        assert!(!store.path("second").unwrap().exists());
    }

    #[test]
    fn saves_private_files_and_never_pictures() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let mut chat = conversation("private", 1, None);
        chat.history.push(qiyas_ai::Message {
            role: qiyas_ai::Role::User,
            content: vec![qiyas_ai::ContentBlock::Image {
                media_type: "image/png".into(),
                data: "SECRET-PICTURE".into(),
            }],
        });
        if let TranscriptEntry::User { images, .. } = &mut chat.transcript[0] {
            images.push("data:image/png;base64,SECRET-PICTURE".into());
        }
        store.save(&chat).unwrap();
        let text = fs::read_to_string(store.path("private").unwrap()).unwrap();
        assert!(!text.contains("SECRET-PICTURE"));
        assert!(text.contains("[picture removed]"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for path in [store.path("private").unwrap(), store.dir.join("index.json")] {
                assert_eq!(
                    fs::metadata(path).unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
        }
    }

    #[test]
    fn keeps_the_one_hundred_newest_and_refuses_paths_as_ids() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        for n in 0..=MAX_CONVERSATIONS {
            store
                .save(&conversation(&format!("chat-{n}"), n as u64, None))
                .unwrap();
        }
        assert_eq!(store.list(None).unwrap().len(), MAX_CONVERSATIONS);
        assert!(!store.path("chat-0").unwrap().exists());
        assert_eq!(store.list(None).unwrap()[0].id, "chat-100");
        assert!(store.load("../outside").is_err());
        assert!(store.delete("../outside").is_err());
        assert!(store.delete("index").is_err());
    }

    #[test]
    fn startup_recovers_a_chat_written_before_its_index_and_rebuilds_a_broken_index() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        store.save(&conversation("indexed", 1, None)).unwrap();
        let orphan = conversation("orphan", 2, None);
        write_json(&store.path("orphan").unwrap(), &orphan).unwrap();
        assert_eq!(
            store.list(None).unwrap().len(),
            1,
            "the live list reads only its index"
        );
        let restarted = Store::new(dir.path());
        restarted.recover().unwrap();
        assert_eq!(restarted.list(None).unwrap().len(), 2);
        assert_eq!(restarted.list(None).unwrap()[0].id, "orphan");
        assert_eq!(restarted.load("orphan").unwrap(), orphan);
        fs::write(restarted.dir.join("index.json"), "interrupted old index").unwrap();
        restarted.recover().unwrap();
        assert_eq!(restarted.list(None).unwrap().len(), 2);
        fs::remove_file(restarted.dir.join("index.json")).unwrap();
        restarted.recover().unwrap();
        assert_eq!(restarted.list(None).unwrap().len(), 2);
    }
}
