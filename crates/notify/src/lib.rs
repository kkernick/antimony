#![doc = include_str!("../README.md")]

use console::{StyledObject, style};
use dbus::{
    Message, MessageType,
    arg::Variant,
    blocking::{BlockingSender, LocalConnection},
    channel::MatchingReceiver,
    message::MatchRule,
};
use heck::ToTitleCase;
use log::{Level, Record};
use nix::errno;
use parking_lot::{Mutex, ReentrantMutex};
use std::{
    borrow::Cow,
    collections::HashMap,
    env,
    fmt::{self, Write},
    io::{IsTerminal, stdout},
    process,
    sync::{
        Arc, LazyLock, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use thiserror::Error;

/// The current log level, set by the `RUST_LOG` environment variable
pub static LEVEL: LazyLock<log::Level> = LazyLock::new(|| {
    env::var("RUST_LOG").map_or(Level::Error, |e| match e.to_lowercase().as_str() {
        "trace" => log::Level::Trace,
        "warn" => log::Level::Warn,
        "info" => log::Level::Info,
        "debug" => log::Level::Debug,
        _ => log::Level::Error,
    })
});

/// The current level to which messages should be notified.
///
/// By default, only errors cause prompts. This is controlled via the
/// NOTIFY environment variable.
pub static PROMPT_LEVEL: LazyLock<Option<log::Level>> = LazyLock::new(|| {
    env::var("NOTIFY").map_or(Some(Level::Error), |e| match e.to_lowercase().as_str() {
        "none" => None,
        "trace" => Some(log::Level::Trace),
        "warn" => Some(log::Level::Warn),
        "info" => Some(log::Level::Info),
        "debug" => Some(log::Level::Debug),
        _ => Some(log::Level::Error),
    })
});

/// The global Logger
static LOGGER: OnceLock<NotifyLogger> = OnceLock::new();

/// A lock to ensure only a single thread can log at a time.
static LOCK: LazyLock<ReentrantMutex<()>> = LazyLock::new(ReentrantMutex::default);

/// A custom Notifier that the LOGGER will use, if set.
static NOTIFIER: OnceLock<Box<Notifier>> = OnceLock::new();

/// A D-Bus Map
#[allow(clippy::absolute_paths)]
type VariantMap<'a> = HashMap<&'a str, Variant<Box<dyn dbus::arg::RefArg>>>;

/// A Notifier function, which accepts the Record and Level of each Log, and
/// returns whether the logging was successful.
type Notifier = dyn Fn(&Record, Level) -> bool + Send + Sync + 'static;

/// Errors for the `NotifyLogger`.
#[derive(Debug, Error)]
pub enum Error {
    /// Errors for when the console-fallback fails to receive
    /// user input for an action.
    #[error("Failed to query user: {0}")]
    Dialog(#[from] dialoguer::Error),

    /// Errors with the dbus crate
    #[error("Failed to communicate with the user bus: {0}")]
    Dbus(#[from] dbus::Error),

    /// Miscellaneous errors returning an Errno.
    #[error("System error: {0}")]
    Errno(#[from] errno::Errno),

    /// Errors initializing the Logger.
    #[error("Failed to initialize logger")]
    Init,

    /// Errors setting the Notifier.
    #[error("Failed to set notify logger")]
    Set,

    /// Errors setting the Notifier.
    #[error("Unexpected response from the user-bus")]
    Connection,
}

/// The urgency level of a Notification.
/// The Desktop Environment is free to interpret this as it wants.
/// This should, therefore, be seen as a suggestion.
#[derive(Default, Clone, Copy, Debug, clap::ValueEnum)]
pub enum Urgency {
    Low,

    #[default]
    Normal,

    Critical,
}
impl Urgency {
    /// Get the Byte for this Urgency Level, to send across the Bus.
    const fn byte(self) -> u8 {
        match self {
            Self::Low => 0,
            Self::Normal => 1,
            Self::Critical => 2,
        }
    }
}
impl fmt::Display for Urgency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Low => write!(f, "low"),
            Self::Normal => write!(f, "normal"),
            Self::Critical => write!(f, "critical"),
        }
    }
}

/// Format a message so it can be displayed on the console.
fn console_msg(title: &str, body: &str, urgency: Option<Urgency>) -> String {
    let msg = format!(
        "{}: {body}",
        match urgency.unwrap_or_default() {
            Urgency::Low => style(title).blue().bold(),
            Urgency::Normal => style(title).bold(),
            Urgency::Critical => style(title).red().bold(),
        }
        .force_styling(true),
    );
    console_format(&msg)
}

/// Convert the HTML tags defined by the Notification Spec to console formatting.
fn style_tag<F>(mut msg: String, open: &str, close: &str, style: F) -> String
where
    F: Fn(&str) -> StyledObject<&str>,
{
    while let Some(start) = msg.find(open)
        && let Some(end) = msg.find(close)
    {
        let pre = &msg[..start];
        if let Some(pi) = end.checked_add(close.len())
            && let Some(start) = start.checked_add(open.len())
        {
            let post = &msg[pi..];
            let extract = style(&msg[start..end]).force_styling(true);
            msg = format!("{pre}{extract}{post}");
        }
    }
    msg
}

/// Format style tags.
fn console_format(content: &str) -> String {
    let mut content = content.to_owned();
    content = style_tag(content, "<b>", "</b>", |tag: &str| style(tag).bold());
    content = style_tag(content, "<i>", "</i>", |tag: &str| style(tag).italic());
    content
}

/// Get actions from the console.
fn console_actions(
    title: &str,
    body: &str,
    urgency: Option<Urgency>,
    actions: &Vec<String>,
) -> Result<String, Error> {
    let msg = console_msg(title, body, urgency);
    let _lock = LOCK.lock();
    let result = dialoguer::Select::new()
        .with_prompt(msg)
        .default(0)
        .items(actions)
        .interact()?;
    Ok(actions[result].clone())
}

/// Construct a Message to send across the User Bus.
#[allow(
    clippy::ref_option,
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation
)]
#[allow(clippy::unwrap_used, reason = "All arguments are hard-coded.")]
fn get_msg(
    title: &str,
    body: &str,
    timeout: &Option<Duration>,
    urgency: Option<Urgency>,
    actions: Option<&Vec<String>>,
) -> Message {
    let mut hints = VariantMap::new();
    if let Some(urgency) = urgency {
        let _ = hints.insert("urgency", Variant(Box::new(urgency.byte())));
    }
    let _ = hints.insert("sender-pid", Variant(Box::new(process::id() as i32)));

    let a_placeholder = Vec::new();
    let a = actions.unwrap_or(&a_placeholder);

    Message::new_method_call(
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
        "Notify",
    )
    .unwrap()
    // App Name
    .append1(
        if let Ok(path) = env::current_exe()
            && let Some(name) = path.file_name()
        {
            name.to_string_lossy().to_title_case()
        } else {
            "Notify".to_owned()
        },
    )
    // Replace ID and Icon are empty
    .append2(0u32, "")
    // Summary and Body
    .append2(title, body)
    // Actions and Hints
    .append2(a, hints)
    // Timeout
    .append1(timeout.map_or(-1, |timeout| timeout.as_millis() as i32))
}

/// Send a stateless Notification across the User Bus.
/// If there is a failure sending the message, it will be sent
/// to the terminal.
///
/// ## Errors
/// This function will fail if a connection to the user bus could not be made, or if
/// the message could not be sent.
pub fn notify(
    title: impl Into<Cow<'static, str>>,
    body: impl Into<Cow<'static, str>>,
    timeout: Option<Duration>,
    urgency: Option<Urgency>,
) -> Result<(), Error> {
    let title = title.into();
    let body = body.into();

    let msg = get_msg(&title, &body, &timeout, urgency, None);
    let result = || -> Result<(), Error> {
        let connection = LocalConnection::new_session()?;
        let _ = connection.send_with_reply_and_block(msg, Duration::from_secs(1))?;
        Ok(())
    };

    if result().is_err() {
        let _lock = LOCK.lock();
        println!("{}", console_msg(&title, &body, urgency));
    }
    Ok(())
}

/// Send a Notification across the User Bus with a set of Actions.
/// The selected Action, if the user choose one, is returned.
///
/// Should an error preventing sending a Notification, the dialoguer
/// crate will be used to prompt from the terminal.
///
/// ## Errors
/// This function will fail if a connection to the user bus could not be made, or if
/// the message could not be sent.
#[allow(
    clippy::missing_panics_doc,
    clippy::unwrap_used,
    reason = "We will never panic unwrapping the Arc"
)]
pub fn action(
    title: impl Into<Cow<'static, str>>,
    body: impl Into<Cow<'static, str>>,
    timeout: Option<Duration>,
    urgency: Option<Urgency>,
    actions: Vec<(String, String)>,
) -> Result<String, Error> {
    let title = title.into();
    let body = body.into();

    // Format.
    let mut a = Vec::new();
    for (key, value) in actions.clone() {
        a.push(key);
        a.push(value);
    }

    let result = || -> Result<String, Error> {
        // Get a connection, and send the notification.
        let connection = LocalConnection::new_session()?;
        let msg = get_msg(&title, &body, &timeout, urgency, Some(&a));
        let response = connection.send_with_reply_and_block(msg, Duration::from_secs(1))?;
        let Some(id) = response.get1::<u32>() else {
            return Err(Error::Connection);
        };

        // The Bus will Broadcast the response through an ActionInvoked Member,
        // with a pair containing the response, and the ID we got from the original call.
        let found = Arc::new(AtomicBool::new(false));
        let action = Arc::<Mutex<String>>::default();
        let found_clone = Arc::clone(&found);
        let action_clone = Arc::clone(&action);
        let rule = MatchRule::new()
            .with_path("/org/freedesktop/Notifications")
            .with_interface("org.freedesktop.Notifications")
            .with_member("ActionInvoked")
            .with_type(MessageType::Signal);

        // Monitor the Bus.
        let monitor = LocalConnection::new_session()?;
        let proxy = monitor.with_proxy(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            Duration::from_secs(1),
        );
        let _: () = proxy.method_call(
            "org.freedesktop.DBus.Monitoring",
            "BecomeMonitor",
            (vec![rule.match_str()], 0u32),
        )?;

        // Watch the Bus for our desired notification.
        let _ = monitor.start_receive(
            MatchRule::new(),
            Box::new(move |msg: Message, _conn: &LocalConnection| -> bool {
                if found_clone.load(Ordering::Relaxed) {
                    true
                } else {
                    let (notif_id, action_key): (u32, String) = match msg.read2() {
                        Ok(v) => v,
                        Err(_) => {
                            return true;
                        }
                    };

                    if notif_id == id {
                        *action_clone.lock() = action_key;
                        found_clone.store(true, Ordering::Relaxed);
                        false
                    } else {
                        true
                    }
                }
            }),
        );

        // Wait until the callback fires.
        let timeout = timeout.unwrap_or(Duration::from_secs(10));
        let start = Instant::now();
        while !found.load(Ordering::Relaxed) && start.elapsed() < timeout {
            let _ = monitor.process(Duration::from_secs(1))?;
        }
        Ok(Arc::into_inner(action).unwrap().into_inner())
    };

    // If we couldn't get a response, ask on the terminal, instead.
    if let Ok(result) = result() {
        Ok(result)
    } else {
        let _lock = LOCK.lock();
        let response = console_actions(
            &title,
            &body,
            urgency,
            &actions.iter().map(|(_, v)| v.to_owned()).collect(),
        )?;

        Ok(actions
            .into_iter()
            .find_map(|(k, v)| if v == response { Some(k) } else { None })
            .iter()
            .next()
            .unwrap()
            .to_owned())
    }
}

/// Get the color that should be used for particular log level.
#[must_use]
pub fn level_color(level: log::Level) -> StyledObject<&'static str> {
    match level {
        log::Level::Error => style("ERROR").red().bold().blink(),
        log::Level::Warn => style("WARN").yellow().bold(),
        log::Level::Info => style("INFO").green().bold(),
        log::Level::Debug => style("DEBUG").blue().bold(),
        log::Level::Trace => style("TRACE").cyan().bold(),
    }
}

/// Get the pretty name of a log level.
#[must_use]
pub const fn level_name(level: log::Level) -> &'static str {
    match level {
        log::Level::Error => "Error",
        log::Level::Warn => "Warning",
        log::Level::Info => "Info",
        log::Level::Debug => "Debug",
        log::Level::Trace => "Trace",
    }
}

/// Get the recommended notification urgency for each level.
#[must_use]
pub const fn level_urgency(level: log::Level) -> Urgency {
    match level {
        log::Level::Error => Urgency::Critical,
        log::Level::Warn => Urgency::Normal,
        _ => Urgency::Low,
    }
}

/// A `log::Log` implementation that is controlled by `RUST_LOG` for console logs,
/// and NOTIFY for which of those should be promoted to Notifications.
struct NotifyLogger {
    /// Whether to print to stderr or stdin
    error: bool,
}
impl NotifyLogger {
    /// Create a new logger.
    const fn new(error: bool) -> Self {
        Self { error }
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

        let mut msg = String::new();
        let _ = write!(
            msg,
            "[{} {}] {}",
            level_color(record.level()),
            style(record.target()).bold().italic(),
            record.args()
        );

        if !msg.ends_with('\n') {
            msg.push('\n');
        }

        {
            let _lock = LOCK.lock();
            if self.error {
                eprint!("{msg}");
            } else {
                print!("{msg}");
            }
        }

        if let Some(prompt) = *PROMPT_LEVEL
            && level <= prompt
            && !stdout().is_terminal()
        {
            if let Some(cb) = NOTIFIER.get()
                && cb(record, record.level())
            {
            } else {
                let _ = notify(
                    format!("{}: {}", level_name(level), record.target()),
                    format!("{}", record.args()),
                    None,
                    Some(level_urgency(level)),
                );
            }
        }
    }

    fn flush(&self) {}
}

/// Initialize the `NotifyLogger` as the program's Logger.
///
/// ## Errors
/// `Error::Init`: If the logger could not be set, or already was.
#[allow(
    clippy::missing_panics_doc,
    clippy::unwrap_used,
    reason = "This function will not panic, as we check the value."
)]
pub fn init() -> Result<(), Error> {
    if LOGGER.set(NotifyLogger::new(false)).is_ok() {
        log::set_logger(LOGGER.get().unwrap()).map_err(|_| Error::Init)?;
        log::set_max_level(log::LevelFilter::Trace);
        Ok(())
    } else {
        Err(Error::Init)
    }
}

/// Initialize the `NotifyLogger` as the program's Logger, but print to
/// stderr instead of stdout. Note that this only applies to direct logging,
/// notification call fallback still prints to stdout.
///
/// ## Errors
/// `Error::Init`: If the logger could not be set, or already was.
#[allow(
    clippy::missing_panics_doc,
    clippy::unwrap_used,
    reason = "This function will not panic, as we check the value."
)]
pub fn init_error() -> Result<(), Error> {
    if LOGGER.set(NotifyLogger::new(true)).is_ok() {
        log::set_logger(LOGGER.get().unwrap()).map_err(|_| Error::Init)?;
        log::set_max_level(log::LevelFilter::Trace);
        Ok(())
    } else {
        Err(Error::Init)
    }
}

/// Set an optional Notifier function that is called instead of the
/// `notify()` function defined here.
///
/// ## Errors
/// `Error::Set`: If the logger could not be set, or already was.
pub fn set_notifier(function: Box<Notifier>) -> Result<(), Error> {
    NOTIFIER.set(function).map_err(|_| Error::Set)
}

#[cfg(test)]
mod tests {
    use crate::{Error, action, init, notify};

    #[test]
    pub fn simple_notify() -> Result<(), Error> {
        notify("Notify Test", "This is a notification test!", None, None)
    }

    #[test]
    pub fn notify_action() -> Result<(), Error> {
        assert!(
            action(
                "Notify Test",
                "This is an action test!",
                None,
                None,
                vec![("Test".to_owned(), "Test".to_owned())]
            )? == "Test"
        );
        Ok(())
    }

    #[test]
    pub fn test_logger() {
        init().expect("Failed to init");
        log::info!("Test!");
    }
}
