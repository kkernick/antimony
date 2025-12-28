# Notify

This crate provides an interface to the `org.freedesktop.Notifications` interface via `notify-send`. It also implements a `log:Logger` controlled entirely through the environment that can optionally send log messages to the Desktop Environment via Notifications.

`notify::notify` will send a notification to the client, whereas `notify::action` will send a notification with a list of prospective actions, to which the function will return the action chosen by the user, if any.

To use as a logger, simply call `notify::init` to initialize `notify` as the program logger, after which simply use `warn!`, `log!`, etc as usual. Control over the messages and level is done via two environment variables:

* `RUST_LOG` control messages output to the terminal, identical to how `env_logger` works.
* `NOTIFY` controls notifications sent to the Desktop Environment. By default, logs with `log::Level::Error` will be both printed to the terminal, and displayed via a Notification. Notifications can be displayed entirely at runtime by setting `NOTIFY=none`.

