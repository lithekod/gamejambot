# LiU Game Jam Bot

A discord bot for doing various gamejam related things.

- [x] Create channels on demand
- [x] Register theme ideas through PM
- [x] Theme generation based on submitted themes (requires role "Organizer")
- [x] Request roles for skills
- [x] Remove channels (requires role "Organizer")

## Usage

Requires a recent rust compiler. Install it using `rustup`

Create a `.env` file containing 

```
DISCORD_TOKEN=<your bot token>
```

Then run the project using `cargo run`
