use crate::{Command, Context, Error};

mod cleanup;
mod deletebotmessage;
mod punish;

/// Moderator Commands :)
#[poise::command(slash_command, category = "Guild", subcommands())]
pub async fn moderator(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say("This command only has subcommands I'm afraid :)").await?;
    Ok(())
}

pub fn commands() -> [Command; 4] {
    [cleanup::cleanup(), deletebotmessage::delete_botmessage(), punish::punish(), moderator()]
}
