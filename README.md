# UR Connect

`ur-connect` is a Rust library that automates login and timetable retrieval for the University of Regensburg campus portal.

## Usage
```rust
use ur_connect::UrConnect;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = UrConnect::new()?;
    client.login("username", "password").await?;

    let entries = client.get_timetable().await?;
    println!("{}", UrConnect::format_entries(&entries));

    Ok(())
}
```

For end-to-end testing provide credentials through the environment:

- `UR_USER`
- `UR_PASSWORD`

## Development
- `cargo fmt` – format the codebase.
- `cargo check` – compile without running tests.
- `cargo test --lib` – run unit tests (HTML and ICS parsing coverage).
- `cargo test downloads_and_prints_timetable -- --ignored` – exercise the live timetable flow once credentials are configured.

The core modules reside in `src/`:

- `client.rs` – high-level Campus portal workflow.
- `model.rs` – data structures (`TimetableEntry`).
- `parsing/` – DOM and ICS parsers shared across the client.