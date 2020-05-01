use anyhow::Context;
use rand::seq::{IteratorRandom, SliceRandom};
use twilight::{
    http::Client as HttpClient,
    model::{
        channel::Message,
        id::{ChannelId, GuildId, UserId},
        user::User,
    },
};

use crate::role::has_role;
use crate::roles::ORGANIZER;
use crate::state::PersistentState;
use crate::utils::{Result, send_message};

enum SubmissionResult {
    Done,
    AlreadySubmitted{previous_submission: String},
}

impl PersistentState {
    /**
      Tries to add a theme submission by the user. Replaces the previous theme
      if the user had one previously. If file saving fails, returns Err
    */
    fn try_add_theme(
        &mut self,
        user: UserId,
        idea: &str
    ) -> Result<SubmissionResult> {
        if self.theme_ideas.contains_key(&user) {
            let previous_submission = self.theme_ideas.get(&user).unwrap().to_string();
            self.theme_ideas.insert(user, idea.into());
            self.save().context("Failed to write current themes")?;
            Ok(SubmissionResult::AlreadySubmitted{previous_submission})
        }
        else {
            self.theme_ideas.insert(user, idea.into());
            self.save().context("Failed to write current themes")?;
            Ok(SubmissionResult::Done)
        }
    }
}

pub async fn handle_add_theme(
    http: &HttpClient,
    msg: &Message,
) -> Result<()> {
    // Check if the message is a single word
    if msg.content.split_ascii_whitespace().count() != 1 {
        http.create_message(msg.channel_id)
            .content("Themes ideas should only be a single word.")
            .await?;
    }
    else {
        let had_old_theme = PersistentState::instance().lock()
            .unwrap()
            .try_add_theme(msg.author.id, &msg.content)
            .context("Failed to save theme")?;

        match had_old_theme {
            SubmissionResult::Done => {
                // Check if the message is a PM
                http.create_message(msg.channel_id)
                    .content(format!(
                        "Theme idea \"{}\" registered, thanks!",
                        &msg.content
                    ))
                    .await?;
            }
            SubmissionResult::AlreadySubmitted{previous_submission} => {
                // Check if the message is a PM
                http.create_message(msg.channel_id)
                    .content(format!(
                        "You can only submit one idea.\n\
                        Theme idea \"{}\" registered, \
                        replacing your previous submission \"{}\".",
                        &msg.content, previous_submission
                    ))
                    .await?;
            }
        }
    }
    Ok(())
}

pub async fn handle_generate_theme(
    original_channel: ChannelId,
    guild: GuildId,
    author: &User,
    http: HttpClient
) -> Result<()> {
    if has_role(
        &http,
        guild,
        author.id,
        ORGANIZER,
    ).await? {
        let theme = do_theme_generation();
        let send_result = send_message(&http, original_channel, author.id,
            &theme
        )
        .await
        .context("Failed to send theme");
        match send_result {
            Ok(_) => {},
            Err(e) => {
                send_message(&http, original_channel, author.id,
                    "Failed to send theme. Has someone been naughty? 🤔"
                ).await?;
                println!("Failed to send theme message {:?}", e);
                println!("Message should have been: {:?}", theme);
            }
        }
    }
    else {
        send_message(&http, original_channel, author.id,
            format!(
                "Since you lack the required role **{}**, you do \
                not have permission to generate themes.", ORGANIZER)
        ).await?;
        println!("Tried to generate theme without required role \"{}\"", ORGANIZER);
    }

    Ok(())
}

pub async fn handle_show_all_themes(
    original_channel: ChannelId,
    guild: GuildId,
    author: &User,
    http: HttpClient
) -> Result<()> {
    if has_role(
        &http,
        guild,
        author.id,
        ORGANIZER,
    ).await? {
        let all_ideas = format_all_ideas();
        let send_result = send_message(&http, original_channel, author.id,
            format!("The theme ideas submitted are ```{}```", all_ideas)
        )
        .await
        .context("Failed to send all themes");

        match send_result {
            Ok(_) => {},
            Err(e) => {
                send_message(&http, original_channel, author.id,
                    "Failed to send all themes. I don't know how this happened."
                )
                .await?;
                println!("Tried to send all themes but something went wrong {:?}", e);
            }
        }
    }
    else {
        send_message(&http, original_channel, author.id,
            format!(
                "Since you lack the required role **{}**, you do \
                not have permission to see all the theme ideas.", ORGANIZER)
        ).await?;
        println!("Tried to see all theme ideas without required role \"{}\"", ORGANIZER);
    }
    Ok(())
}

fn do_theme_generation() -> String {
    let mut rng = rand::thread_rng();
    let ref theme_ideas = PersistentState::instance().lock().unwrap().theme_ideas;
    let mut selected = theme_ideas
        .iter()
        .map(|(_, idea)| idea)
        .choose_multiple(&mut rng, 2);

    // Per documetation: The order of chose_multiple is not random. To achieve
    // that, shuffle the result
    selected.shuffle(&mut rng);

    if selected.len() != 2 {
        "Not enough ideas have been submitted yet.".to_string()
    }
    else {
        format!("The theme is: {} {}", selected[0], selected[1])
    }
}

fn format_all_ideas() -> String {
    let ref theme_ideas = PersistentState::instance().lock().unwrap().theme_ideas;

    let all_ideas = theme_ideas
        .iter()
        .map(|(_, idea)| idea.to_string())
        .collect::<Vec<String>>()
        .join(", ");

    all_ideas
}
