use console::{StyledObject, style};
use inflector::Inflector;
use spawn::{HandleError, SpawnError, Spawner, StreamMode};
use std::{
    borrow::Cow,
    io::{Write, stdout},
    sync::LazyLock,
    thread,
    time::Duration,
};

pub static LEVEL: LazyLock<log::Level> = LazyLock::new(|| match std::env::var("RUST_LOG") {
    Ok(e) => match e.to_lowercase().as_str() {
        "trace" => log::Level::Trace,
        "warn" => log::Level::Warn,
        "info" => log::Level::Info,
        "debug" => log::Level::Debug,
        _ => log::Level::Error,
    },
    Err(_) => log::Level::Error,
});

pub static PROMPT_LEVEL: LazyLock<Option<log::Level>> =
    LazyLock::new(|| match std::env::var("NOTIFY") {
        Ok(e) => match e.to_lowercase().as_str() {
            "none" => None,
            "trace" => Some(log::Level::Trace),
            "warn" => Some(log::Level::Warn),
            "info" => Some(log::Level::Info),
            "debug" => Some(log::Level::Debug),
            _ => Some(log::Level::Error),
        },
        Err(_) => Some(log::Level::Error),
    });

static LOGGER: NotifyLogger = NotifyLogger::new();

#[derive(Debug)]
pub enum Error {
    Spawn(SpawnError),
    Handle(HandleError),
    Init,
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn(e) => Some(e),
            Self::Handle(e) => Some(e),
            _ => None,
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(e) => write!(f, "Failed to spawn notify-send: {e}"),
            Self::Handle(e) => write!(f, "Failed to handle notify-send: {e}"),
            Self::Init => write!(f, "Failed to initialize logger"),
        }
    }
}
impl From<SpawnError> for Error {
    fn from(value: SpawnError) -> Self {
        Self::Spawn(value)
    }
}
impl From<HandleError> for Error {
    fn from(value: HandleError) -> Self {
        Self::Handle(value)
    }
}

pub enum Urgency {
    Low,
    Normal,
    Critical,
}
impl std::fmt::Display for Urgency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Urgency::Low => write!(f, "low"),
            Urgency::Normal => write!(f, "normal"),
            Urgency::Critical => write!(f, "critical"),
        }
    }
}

pub fn notify(
    title: impl Into<Cow<'static, str>>,
    body: impl Into<Cow<'static, str>>,
    timeout: Option<Duration>,
    urgency: Option<Urgency>,
) -> Result<(), Error> {
    #[rustfmt::skip]
    let handle = Spawner::new("notify-send")?
        .args([title.into(), body.into()])?
        .arg("-a")?
        .arg(if let Ok(path) = std::env::current_exe() && let Some(name) = path.file_name() {
                name.to_string_lossy().to_title_case()
            } else {
                    "Notify".to_string()
            }
        )?
        .mode(user::Mode::Real)
        .preserve_env(true);

    if let Some(timeout) = timeout {
        handle.args_i(["-t", &timeout.as_millis().to_string()])?;
    }

    if let Some(urgency) = urgency {
        handle.args_i(["-u", &urgency.to_string()])?;
    }

    handle.spawn()?.wait()?;
    Ok(())
}

pub fn action(
    title: impl Into<Cow<'static, str>>,
    body: impl Into<Cow<'static, str>>,
    timeout: Option<Duration>,
    urgency: Option<Urgency>,
    actions: Vec<(impl Into<Cow<'static, str>>, impl Into<Cow<'static, str>>)>,
) -> Result<String, Error> {
    #[rustfmt::skip]
    let handle = Spawner::new("notify-send")?
        .args([title.into(), body.into()])?
        .arg("-a")?
        .arg(if let Ok(path) = std::env::current_exe() && let Some(name) = path.file_name() {
                name.to_string_lossy().to_title_case()
            } else {
                    "Notify".to_string()
            }
        )?
        .mode(user::Mode::Real)
        .preserve_env(true)
        .output(StreamMode::Pipe);

    if let Some(timeout) = timeout {
        handle.args_i(["-t", &timeout.as_millis().to_string()])?;
    }

    if let Some(urgency) = urgency {
        handle.args_i(["-u", &urgency.to_string()])?;
    }

    for (key, value) in actions {
        handle.args_i(["-A", &format!("{}={}", key.into(), value.into())])?;
    }

    handle.spawn()?.output_all().map_err(Error::Handle)
}

struct NotifyLogger {}
impl NotifyLogger {
    const fn new() -> Self {
        Self {}
    }

    fn level_color(level: log::Level) -> StyledObject<&'static str> {
        match level {
            log::Level::Error => style("ERROR").red().bold().blink(),
            log::Level::Warn => style("WARN").yellow().bold(),
            log::Level::Info => style("INFO").green().bold(),
            log::Level::Debug => style("DEBUG").blue().bold(),
            log::Level::Trace => style("TRACE").cyan().bold(),
        }
    }

    fn level_name(level: log::Level) -> &'static str {
        match level {
            log::Level::Error => "Error",
            log::Level::Warn => "Warning",
            log::Level::Info => "Info",
            log::Level::Debug => "Debug",
            log::Level::Trace => "Trace",
        }
    }

    fn level_urgency(level: log::Level) -> Urgency {
        match level {
            log::Level::Error => Urgency::Critical,
            log::Level::Warn => Urgency::Normal,
            log::Level::Info => Urgency::Low,
            log::Level::Debug => Urgency::Low,
            log::Level::Trace => Urgency::Low,
        }
    }
}
impl log::Log for NotifyLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= *LEVEL
    }

    fn log(&self, record: &log::Record) {
        let level = record.level();
        if !self.enabled(record.metadata()) {
            return;
        }

        let mut out = stdout();
        let mut msg = String::new();
        msg.push_str(&format!(
            "[{} {} {:?}] {}",
            Self::level_color(record.level()),
            style(record.target()).bold().italic(),
            thread::current().id(),
            record.args()
        ));

        if !msg.ends_with('\n') {
            msg.push('\n')
        }

        let _ = write!(out, "{msg}");

        if let Some(prompt) = *PROMPT_LEVEL
            && level <= prompt
        {
            let _ = notify(
                format!("{}: {}", Self::level_name(level), record.target()),
                format!("{}", record.args()),
                None,
                Some(Self::level_urgency(level)),
            );
        }
    }

    fn flush(&self) {}
}

pub fn init() -> Result<(), Error> {
    log::set_logger(&LOGGER).map_err(|_| Error::Init)?;
    log::set_max_level(log::LevelFilter::Trace);
    Ok(())
}
