use antimony::cli::Run;
use anyhow::Result;

#[test]
fn test() -> Result<()> {
    let cmd = antimony::cli::run::Args {
        profile: "sh".to_string(),
        passthrough: Some(
            vec!["-c", "echo", "Hello, world!"]
                .into_iter()
                .map(String::from)
                .collect(),
        ),
        ..Default::default()
    };
    cmd.run()?;
    Ok(())
}
