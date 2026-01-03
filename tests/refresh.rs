use antimony::cli::Run;
use anyhow::Result;

#[test]
fn test() -> Result<()> {
    let cmd = antimony::cli::refresh::Args {
        profile: Some("sh".to_string()),
        dry: true,
        hard: true,
        ..Default::default()
    };
    cmd.run()?;
    Ok(())
}
