use std::thread;
use std::sync::Mutex;
use std::time::Duration;

use serenity::client::Client;
use serenity::model::channel::Message;
use serenity::prelude::{EventHandler, Context};
use serenity::model::prelude::{GuildStatus, Ready, OnlineStatus};
use serenity::model::user::CurrentUser;
use serenity::framework::standard::{
    StandardFramework,
    CommandResult,
    macros::{
        command,
        group
    }
};

#[group]
#[commands(ping)]
struct General;

use std::env;

// Info which is aquired on `ready`
struct PersistentInfo {
    pub user: CurrentUser,
}

struct Handler {
    info: Mutex<Option<PersistentInfo>>
}

impl Handler {
    pub fn new() -> Self {
        Self {info: Mutex::new(None)}
    }
}

impl EventHandler for Handler {
    fn ready(&self, ctx: Context, ready: Ready) {
        *self.info.lock().unwrap() = Some(PersistentInfo{user: ready.user});
    }

    fn message(&self, context: Context, msg: Message) {
        self.info.lock().unwrap().as_mut().map(|info| {
            if msg.author.id != info.user.id {
                let _ = msg.channel_id.say(&context, msg.content);
            }
        });
    }
}

fn main() {
    dotenv::dotenv().ok();
    // Login with a bot token from the environment
    let mut client = Client::new(
        &env::var("DISCORD_TOKEN").expect("token not found"),
        Handler::new()
    ) .expect("Error creating client");

    client.with_framework(StandardFramework::new()
        .configure(|c| c.prefix("~")) // set the bot's prefix to "~"
        .group(&GENERAL_GROUP));

    println!("Starting");
    // start listening for events by starting a single shard
    if let Err(why) = client.start() {
        println!("An error occurred while running the client: {:?}", why);
    }
}

#[command]
fn ping(ctx: &mut Context, msg: &Message) -> CommandResult {
    msg.reply(ctx, "Pong!")?;

    Ok(())
}
