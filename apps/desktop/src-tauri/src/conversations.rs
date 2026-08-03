use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

const STORE_SCHEMA_VERSION: u32 = 1;
const CONVERSATION_SCHEMA_VERSION: u32 = 1;
const MAX_CONVERSATIONS: usize = 500;
const MAX_MESSAGES: usize = 2_000;
const MAX_MESSAGE_BYTES: usize = 256 * 1024;
const MAX_CONVERSATION_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMessage {
    pub id: String,
    pub request_id: Option<String>,
    pub role: String,
    pub content: String,
    pub created_at: String,
    pub status: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ConversationParameters {
    pub temperature: Option<f32>,
    pub max_output_tokens: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub schema_version: u32,
    pub id: String,
    pub title: String,
    pub model_entry_id: Option<String>,
    pub model_name_snapshot: Option<String>,
    pub system_prompt: String,
    pub system_prompt_name: Option<String>,
    pub save_history: bool,
    pub created_at: String,
    pub updated_at: String,
    pub parameters: ConversationParameters,
    pub messages: Vec<ConversationMessage>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct ConversationStoreDocument {
    schema_version: u32,
    conversations: Vec<Conversation>,
}

pub struct ConversationStore {
    path: PathBuf,
    write: Mutex<()>,
}

impl ConversationStore {
    pub fn new(data_root: &Path) -> Self {
        Self {
            path: data_root.join("lumen-source/conversations.json"),
            write: Mutex::new(()),
        }
    }

    pub async fn list(&self) -> Vec<Conversation> {
        let mut conversations = load_document(&self.path).conversations;
        conversations.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        conversations
    }

    pub async fn save(&self, conversation: Conversation) -> Result<Conversation, String> {
        validate_conversation(&conversation)?;
        if !conversation.save_history {
            self.delete(&conversation.id).await?;
            return Ok(conversation);
        }

        let _write = self.write.lock().await;
        let mut document = load_document(&self.path);
        document.schema_version = STORE_SCHEMA_VERSION;
        if let Some(existing) = document
            .conversations
            .iter_mut()
            .find(|existing| existing.id == conversation.id)
        {
            *existing = conversation.clone();
        } else {
            if document.conversations.len() >= MAX_CONVERSATIONS {
                return Err(format!(
                    "Delete an older conversation before saving more than {MAX_CONVERSATIONS} chats."
                ));
            }
            document.conversations.push(conversation.clone());
        }
        write_document(&self.path, &document).await?;
        Ok(conversation)
    }

    pub async fn delete(&self, conversation_id: &str) -> Result<bool, String> {
        if conversation_id.trim().is_empty() {
            return Err("A conversation ID is required.".to_owned());
        }
        let _write = self.write.lock().await;
        let mut document = load_document(&self.path);
        let before = document.conversations.len();
        document
            .conversations
            .retain(|conversation| conversation.id != conversation_id);
        let removed = before != document.conversations.len();
        if removed {
            document.schema_version = STORE_SCHEMA_VERSION;
            write_document(&self.path, &document).await?;
        }
        Ok(removed)
    }
}

async fn overwrite_backup(path: &Path, document: &ConversationStoreDocument) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(document).map_err(|error| error.to_string())?;
    tokio::fs::write(path.with_extension("backup.json"), bytes)
        .await
        .map_err(|error| error.to_string())
}

fn load_document(path: &Path) -> ConversationStoreDocument {
    [path.to_path_buf(), path.with_extension("backup.json")]
        .into_iter()
        .find_map(|candidate| {
            let bytes = std::fs::read(candidate).ok()?;
            let document = serde_json::from_slice::<ConversationStoreDocument>(&bytes).ok()?;
            (document.schema_version == STORE_SCHEMA_VERSION).then_some(document)
        })
        .unwrap_or_else(|| ConversationStoreDocument {
            schema_version: STORE_SCHEMA_VERSION,
            conversations: Vec::new(),
        })
}

async fn write_document(path: &Path, document: &ConversationStoreDocument) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(document).map_err(|error| error.to_string())?;
    let parent = path
        .parent()
        .ok_or_else(|| "Conversation store has no parent directory.".to_owned())?;
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|error| error.to_string())?;

    let backup = path.with_extension("backup.json");
    if let Ok(existing) = tokio::fs::read(path).await {
        if serde_json::from_slice::<ConversationStoreDocument>(&existing).is_ok() {
            tokio::fs::write(&backup, existing)
                .await
                .map_err(|error| error.to_string())?;
        }
    }

    let temporary = path.with_extension("json.tmp");
    let mut file = tokio::fs::File::create(&temporary)
        .await
        .map_err(|error| error.to_string())?;
    file.write_all(&bytes)
        .await
        .map_err(|error| error.to_string())?;
    file.sync_all().await.map_err(|error| error.to_string())?;
    drop(file);
    if let Err(first_error) = tokio::fs::rename(&temporary, path).await {
        if tokio::fs::try_exists(path).await.unwrap_or(false) {
            tokio::fs::remove_file(path)
                .await
                .map_err(|error| error.to_string())?;
            tokio::fs::rename(&temporary, path)
                .await
                .map_err(|error| error.to_string())?;
        } else {
            return Err(first_error.to_string());
        }
    }
    overwrite_backup(path, document).await
}

fn validate_conversation(conversation: &Conversation) -> Result<(), String> {
    if conversation.schema_version != CONVERSATION_SCHEMA_VERSION {
        return Err(format!(
            "Unsupported conversation version {}.",
            conversation.schema_version
        ));
    }
    if conversation.id.trim().is_empty() {
        return Err("A conversation ID is required.".to_owned());
    }
    if conversation.title.trim().is_empty() || conversation.title.len() > 160 {
        return Err("Conversation titles must contain between 1 and 160 characters.".to_owned());
    }
    if conversation.system_prompt.len() > MAX_MESSAGE_BYTES {
        return Err("The system prompt is too large.".to_owned());
    }
    if conversation
        .system_prompt_name
        .as_ref()
        .is_some_and(|name| name.trim().is_empty() || name.chars().count() > 255)
    {
        return Err(
            "The system prompt filename must contain between 1 and 255 characters.".to_owned(),
        );
    }
    if conversation.messages.len() > MAX_MESSAGES {
        return Err(format!(
            "A conversation cannot contain more than {MAX_MESSAGES} messages."
        ));
    }
    let mut total_bytes = conversation.system_prompt.len();
    for message in &conversation.messages {
        if message.id.trim().is_empty()
            || message
                .request_id
                .as_ref()
                .is_some_and(|request_id| request_id.trim().is_empty() || request_id.len() > 128)
            || !matches!(
                message.role.as_str(),
                "system" | "user" | "assistant" | "tool"
            )
            || !matches!(
                message.status.as_str(),
                "complete" | "generating" | "failed" | "stopped"
            )
        {
            return Err("The conversation contains an invalid message record.".to_owned());
        }
        if message.content.len() > MAX_MESSAGE_BYTES {
            return Err("A conversation message is too large.".to_owned());
        }
        total_bytes = total_bytes.saturating_add(message.content.len());
    }
    if total_bytes > MAX_CONVERSATION_BYTES {
        return Err("The conversation exceeds the 4 MiB local history limit.".to_owned());
    }
    if let Some(temperature) = conversation.parameters.temperature {
        if !(0.0..=2.0).contains(&temperature) {
            return Err("Conversation temperature must be between 0 and 2.".to_owned());
        }
    }
    if conversation.parameters.max_output_tokens == Some(0) {
        return Err("Maximum output tokens must be greater than zero.".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conversation(id: &str) -> Conversation {
        Conversation {
            schema_version: CONVERSATION_SCHEMA_VERSION,
            id: id.to_owned(),
            title: "Private chat".to_owned(),
            model_entry_id: Some("model".to_owned()),
            model_name_snapshot: Some("Model".to_owned()),
            system_prompt: String::new(),
            system_prompt_name: None,
            save_history: true,
            created_at: "2026-08-03T00:00:00Z".to_owned(),
            updated_at: "2026-08-03T00:00:00Z".to_owned(),
            parameters: ConversationParameters::default(),
            messages: vec![ConversationMessage {
                id: "message".to_owned(),
                request_id: None,
                role: "user".to_owned(),
                content: "Hello".to_owned(),
                created_at: "2026-08-03T00:00:00Z".to_owned(),
                status: "complete".to_owned(),
            }],
        }
    }

    #[tokio::test]
    async fn conversations_survive_reload_and_recover_from_a_truncated_primary() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let store = ConversationStore::new(directory.path());
        let mut first = conversation("first");
        first.system_prompt = "Answer as a careful reviewer.".to_owned();
        first.system_prompt_name = Some("reviewer.md".to_owned());
        store
            .save(first)
            .await
            .expect("first conversation should save");
        store
            .save(conversation("second"))
            .await
            .expect("second conversation should save and create a backup");

        std::fs::write(&store.path, b"{truncated").expect("primary should be truncated");
        let recovered = ConversationStore::new(directory.path()).list().await;

        assert_eq!(recovered.len(), 2);
        assert!(recovered
            .iter()
            .any(|conversation| conversation.id == "first"
                && conversation.system_prompt == "Answer as a careful reviewer."
                && conversation.system_prompt_name.as_deref() == Some("reviewer.md")));
        assert!(recovered
            .iter()
            .any(|conversation| conversation.id == "second"));
    }

    #[tokio::test]
    async fn unsaved_conversations_remove_any_persisted_copy() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let store = ConversationStore::new(directory.path());
        store
            .save(conversation("private"))
            .await
            .expect("conversation should save");
        let mut unsaved = conversation("private");
        unsaved.save_history = false;

        store
            .save(unsaved)
            .await
            .expect("unsaved conversation should remove persisted content");

        assert!(store.list().await.is_empty());
        std::fs::write(&store.path, b"{truncated").expect("primary should be truncated");
        assert!(
            store.list().await.is_empty(),
            "backup must not resurrect private content"
        );
    }
}
