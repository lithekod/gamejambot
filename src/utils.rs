use std::fmt::Display;

use twilight::{
    http::{
        Client as HttpClient,
        error::{
            Error as DiscordError,
            ResponseError,
        },
    },
    model::id::{ChannelId, UserId},
};

pub type Result<T> = std::result::Result<T, anyhow::Error>;

pub async fn send_message(
    http: &HttpClient,
    channel_id: ChannelId,
    user_id: UserId,
    content: impl Into<String> + Display,
) -> Result<()> {
    let context = "send_message";
    match http.create_message(channel_id)
        .content(format!("<@{}> {}", user_id, content))
        .await {
        Err(DiscordError::Response{source: ResponseError::Client{response: r}}) => {
            println!("{}: The response was a client side error: {}", context,
                match r.text().await {
                    Ok(text) => text,
                    _ => "(Response unavailable)".to_string(),
                }
            );
        },
        Err(e) => println!("{}: The response was an unknown error: {:?}", context, e),
        _ => {}
    };
    Ok(())
}
