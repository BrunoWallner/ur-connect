use anyhow::Result;
use ur_connect::UrConnect;

#[tokio::test(flavor = "multi_thread")]
async fn downloads_and_prints_timetable() -> Result<()> {
    let username = std::env::var("UR_USER").unwrap_or_default();
    let password = std::env::var("UR_PASSWORD").unwrap_or_default();

    if username.is_empty() || password.is_empty() {
        println!("Skipping test: set UR_USER and UR_PASSWORD to run.");
        return Ok(());
    }

    let client = UrConnect::new()?;
    client.login(username.as_str(), password.as_str()).await?;
    let entries = client.get_timetable().await?;

    let formatted = UrConnect::format_entries(&entries);
    println!("{}", formatted);

    Ok(())
}
