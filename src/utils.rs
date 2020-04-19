use std::fmt::Display;

use twilight::{
    http::Client as HttpClient,
    model::id::{ChannelId, UserId},
};

pub type Result<T> = std::result::Result<T, anyhow::Error>;

pub async fn send_message(
    http: &HttpClient,
    channel_id: ChannelId,
    user_id: UserId,
    content: impl Into<String> + Display,
) -> Result<()> {
    http.create_message(channel_id)
        .content(format!("<@{}> {}", user_id, content))
        .await?;
    Ok(())
}
