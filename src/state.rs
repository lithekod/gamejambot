use std::collections::HashMap;
use std::fs::File;
use std::io::prelude::*;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::Context;
use lazy_static::lazy_static;
use serde_derive::{Serialize, Deserialize};
use serde_json;
use twilight::model::id::{ChannelId, MessageId, UserId};

use crate::utils::Result;

const FILENAME: &'static str = "state.json";

/**
  Stores state that should persist between bot restarts.

  The data is stored as json and is loaded lazily on the first use
  of the struct.

  Data is not automatically reloaded on file changes
*/
#[derive(Serialize, Deserialize)]
pub struct PersistentState {
    pub theme_ideas: HashMap<UserId, String>,
    pub channel_creators: HashMap<UserId, (String, ChannelId)>,
    eula_channel_id: ChannelId,
    eula_message_id: MessageId,
}

impl PersistentState {
    /// Load the data from disk, or default initialise it if the file doesn't exist
    fn load() -> Result<Self> {
        if PathBuf::from(FILENAME).exists() {
            let mut file = File::open(FILENAME)?;
            let mut content = String::new();
            file.read_to_string(&mut content)?;
            Ok(serde_json::from_str(&content)?)
        }
        else {
            Ok(Self {
                theme_ideas: HashMap::new(),
                channel_creators: HashMap::new(),
                eula_channel_id: ChannelId {0: 0},
                eula_message_id: MessageId {0: 0},
            })
        }
    }

    /**
      Return a global instance of the struct. The instance is global to
      avoid race conditions, especially with data stored on disk
    */
    pub fn instance() -> &'static Mutex<Self> {
        lazy_static! {
            static ref INSTANCE: Mutex<PersistentState> = Mutex::new(
                PersistentState::load().unwrap()
            );
        }
        &INSTANCE
    }

    /// Checks if the user is allowed to create a channel
    pub fn is_allowed_channel(&mut self, id: UserId) -> bool {
        !self.channel_creators.contains_key(&id)
    }

    /// Gets the user's current channel
    pub fn get_channel_info(&mut self, id: UserId) -> Option<&(String, ChannelId)> {
        self.channel_creators.get(&id)
    }

    /// Registers that the user has created a channel
    pub fn register_channel_creation(&mut self, user_id: UserId, game_name: &String, text_id: ChannelId) -> Result<()> {
        self.channel_creators.insert(user_id, (game_name.to_string(), text_id));
        self.save()
    }

    /// Sets the message acting as the server's EULA
    pub fn set_eula(&mut self, channel_id: ChannelId, message_id: MessageId) -> Result<()> {
        self.eula_channel_id = channel_id;
        self.eula_message_id = message_id;
        self.save()
    }

    /// Gets the channel containing the server's EULA
    pub fn get_eula_channel(&mut self) -> ChannelId {
        self.eula_channel_id
    }

    /// Gets the message acting as the server's EULA
    pub fn get_eula_message(&mut self) -> MessageId {
        self.eula_message_id
    }

    /// Save the state to disk. Should be called after all modifications
    pub fn save(&self) -> Result<()> {
        let mut file = File::create(FILENAME)
            .with_context(|| format!("Failed to open {} for writing", FILENAME))?;
        file.write_all(serde_json::to_string(&self)?.as_bytes())
            .with_context(|| format!("Failed to write to {}", FILENAME))?;
        Ok(())
    }
}
