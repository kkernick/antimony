# Print an optspec for argparse to handle cmd's options that are independent of any subcommand.
function __fish_antimony_global_optspecs
	string join \n h/help V/version
end

function __fish_antimony_needs_command
	# Figure out if the current invocation already has a command.
	set -l cmd (commandline -opc)
	set -e cmd[1]
	argparse -s (__fish_antimony_global_optspecs) -- $cmd 2>/dev/null
	or return
	if set -q argv[1]
		# Also print the command, so this can be used to figure out what it is.
		echo $argv[1]
		return 1
	end
	return 0
end

function __fish_antimony_using_subcommand
	set -l cmd (__fish_antimony_needs_command)
	test -z "$cmd"
	and return 1
	contains -- $cmd[1] $argv
end

complete -c antimony -n "__fish_antimony_needs_command" -s h -l help -d 'Print help'
complete -c antimony -n "__fish_antimony_needs_command" -s V -l version -d 'Print version'
complete -c antimony -n "__fish_antimony_needs_command" -f -a "run" -d 'Run a profile'
complete -c antimony -n "__fish_antimony_needs_command" -f -a "create" -d 'Create a new profile'
complete -c antimony -n "__fish_antimony_needs_command" -f -a "edit" -d 'Edit an existing profile'
complete -c antimony -n "__fish_antimony_needs_command" -f -a "refresh" -d 'Refresh caches'
complete -c antimony -n "__fish_antimony_needs_command" -f -a "integrate" -d 'Integrate a profile into the user environment'
complete -c antimony -n "__fish_antimony_needs_command" -f -a "reset" -d 'Reset a profile back to the system-defined profile'
complete -c antimony -n "__fish_antimony_needs_command" -f -a "trace" -d 'Trace a profile for missing syscalls or files'
complete -c antimony -n "__fish_antimony_needs_command" -f -a "stat" -d 'Collect stats about a profile\'s sandbox'
complete -c antimony -n "__fish_antimony_needs_command" -f -a "info" -d 'List installed profiles and features'
complete -c antimony -n "__fish_antimony_needs_command" -f -a "debug-shell" -d 'Drop into a debugging shell within a profile\'s sandbox'
complete -c antimony -n "__fish_antimony_needs_command" -f -a "seccomp" -d 'Perform operations on the SECCOMP Database'
complete -c antimony -n "__fish_antimony_needs_command" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c antimony -n "__fish_antimony_using_subcommand run" -s c -l config -d 'Use a configuration within the profile' -r
complete -c antimony -n "__fish_antimony_using_subcommand run" -l features -d 'Additional features' -r
complete -c antimony -n "__fish_antimony_using_subcommand run" -l conflicts -d 'Conflicting features' -r
complete -c antimony -n "__fish_antimony_using_subcommand run" -l inherits -d 'Additional inheritance' -r
complete -c antimony -n "__fish_antimony_using_subcommand run" -l home-policy -d 'Override the home policy' -r -f -a "none\t'Do not use a home profile'
enabled\t'The Home Folder is passed read/write. Applications that only permit a single instance, such as Chromium, will get upset if you launch multiple instances of the sandbox'
overlay\t'Once an application has been configured, Overlay effectively freezes it in place by mounting it as a temporary overlay. Changes made in the sandbox are discarded, and it can be shared by multiple instances, even if that application doesn\'t typically support multiple instances (Zed, Chromium, etc)'"
complete -c antimony -n "__fish_antimony_using_subcommand run" -l home-name -d 'Override the home name' -r
complete -c antimony -n "__fish_antimony_using_subcommand run" -l seccomp -d 'Override the seccomp policy' -r -f -a "disabled\t'Disable SECCOMP'
permissive\t'Syscalls are logged to construct a policy for the profile'
enforcing\t'The policy is enforced: unrecognized syscalls return with EPERM'"
complete -c antimony -n "__fish_antimony_using_subcommand run" -l portals -d 'Add portals' -r -f -a "background\t''
camera\t''
clipboard\t''
documents\t''
file-chooser\t''
flatpak\t''
global-shortcuts\t''
inhibit\t''
location\t''
notification\t''
open-uri\t''
proxy-resolver\t''
realtime\t''
screen-cast\t''
screenshot\t''
settings\t''
secret\t''
network-monitor\t''"
complete -c antimony -n "__fish_antimony_using_subcommand run" -l see -d 'Add busses the sandbox can see' -r
complete -c antimony -n "__fish_antimony_using_subcommand run" -l talk -d 'Add busses the sandbox can talk to' -r
complete -c antimony -n "__fish_antimony_using_subcommand run" -l own -d 'Add busses the sandbox owns' -r
complete -c antimony -n "__fish_antimony_using_subcommand run" -l call -d 'Add busses the sandbox can call' -r
complete -c antimony -n "__fish_antimony_using_subcommand run" -l file-passthrough -d 'Override the file passthrough mode' -r -f -a "read-only\t''
read-write\t''
executable\t'Executable files need to be created as copies, so that chmod will work correctly'"
complete -c antimony -n "__fish_antimony_using_subcommand run" -l ro -d 'Add read-only files' -r
complete -c antimony -n "__fish_antimony_using_subcommand run" -l rw -d 'Add read-write files' -r
complete -c antimony -n "__fish_antimony_using_subcommand run" -l binaries -d 'Add binaries' -r
complete -c antimony -n "__fish_antimony_using_subcommand run" -l libraries -d 'Add libraries' -r
complete -c antimony -n "__fish_antimony_using_subcommand run" -l devices -d 'Add devices' -r
complete -c antimony -n "__fish_antimony_using_subcommand run" -l namespaces -d 'Add namespaces' -r -f -a "none\t''
all\t''
user\t'The user namespace is needed to create additional sandboxes (Such as chromium)'
ipc\t''
pid\t''
net\t'Use the network feature instead'
uts\t''
c-group\t''"
complete -c antimony -n "__fish_antimony_using_subcommand run" -l env -d 'Add environment variables in KEY=VALUE syntax' -r
complete -c antimony -n "__fish_antimony_using_subcommand run" -s d -l dry -d 'Generate the profile, but do not run the executable'
complete -c antimony -n "__fish_antimony_using_subcommand run" -l disable-ipc -d 'Disable all IPC. This overrules all other IPC settings'
complete -c antimony -n "__fish_antimony_using_subcommand run" -l system-bus -d 'Provide the system bus'
complete -c antimony -n "__fish_antimony_using_subcommand run" -l user-bus -d 'Provide the user bus. xdg-dbus-proxy is not run'
complete -c antimony -n "__fish_antimony_using_subcommand run" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c antimony -n "__fish_antimony_using_subcommand create" -s b -l blank -d 'Provide an empty file, rather than a documented one'
complete -c antimony -n "__fish_antimony_using_subcommand create" -s h -l help -d 'Print help'
complete -c antimony -n "__fish_antimony_using_subcommand edit" -s h -l help -d 'Print help'
complete -c antimony -n "__fish_antimony_using_subcommand refresh" -s c -l config -d 'Use a configuration within the profile' -r
complete -c antimony -n "__fish_antimony_using_subcommand refresh" -s d -l dry -d 'Just delete the cache, don\'t repopulate'
complete -c antimony -n "__fish_antimony_using_subcommand refresh" -s h -l help -d 'Print help'
complete -c antimony -n "__fish_antimony_using_subcommand integrate" -s c -l config-mode -d 'How to integrate configurations' -r -f -a "action\t'Integrate each configuration as a separate desktop action within the main Desktop File'
file\t'Separate each configuration into its own Desktop File. This can be useful, say, for setting configurations as default application handlers'"
complete -c antimony -n "__fish_antimony_using_subcommand integrate" -s r -l remove -d 'Undo integration for the profile'
complete -c antimony -n "__fish_antimony_using_subcommand integrate" -s s -l shadow -d 'Some desktop environments, particularly Gnome, source their icons via the Flatpak ID (The Profile ID) in this case. This value must be in reverse DNS format, and Antimony automatically prepends "antimony." on those that don\'t. This presents an incongruity between ID and desktop that requires a shadow that hides the original. If an integrated profile lacks an icon, you may need to use this option'
complete -c antimony -n "__fish_antimony_using_subcommand integrate" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c antimony -n "__fish_antimony_using_subcommand reset" -s h -l help -d 'Print help'
complete -c antimony -n "__fish_antimony_using_subcommand trace" -s c -l config -d 'Use a configuration within the profile' -r
complete -c antimony -n "__fish_antimony_using_subcommand trace" -s r -l report -d 'Collect the trace log and list files that the sandbox tried to access, and feature they are available in'
complete -c antimony -n "__fish_antimony_using_subcommand trace" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c antimony -n "__fish_antimony_using_subcommand stat" -s c -l config -d 'Use a configuration within the profile' -r
complete -c antimony -n "__fish_antimony_using_subcommand stat" -s h -l help -d 'Print help'
complete -c antimony -n "__fish_antimony_using_subcommand info" -s v -l verbosity -d 'Generate the profile, but do not run the executable'
complete -c antimony -n "__fish_antimony_using_subcommand info" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c antimony -n "__fish_antimony_using_subcommand debug-shell" -s c -l config -d 'Use a configuration within the profile' -r
complete -c antimony -n "__fish_antimony_using_subcommand debug-shell" -s h -l help -d 'Print help'
complete -c antimony -n "__fish_antimony_using_subcommand seccomp" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c antimony -n "__fish_antimony_using_subcommand help; and not __fish_seen_subcommand_from run create edit refresh integrate reset trace stat info debug-shell seccomp help" -f -a "run" -d 'Run a profile'
complete -c antimony -n "__fish_antimony_using_subcommand help; and not __fish_seen_subcommand_from run create edit refresh integrate reset trace stat info debug-shell seccomp help" -f -a "create" -d 'Create a new profile'
complete -c antimony -n "__fish_antimony_using_subcommand help; and not __fish_seen_subcommand_from run create edit refresh integrate reset trace stat info debug-shell seccomp help" -f -a "edit" -d 'Edit an existing profile'
complete -c antimony -n "__fish_antimony_using_subcommand help; and not __fish_seen_subcommand_from run create edit refresh integrate reset trace stat info debug-shell seccomp help" -f -a "refresh" -d 'Refresh caches'
complete -c antimony -n "__fish_antimony_using_subcommand help; and not __fish_seen_subcommand_from run create edit refresh integrate reset trace stat info debug-shell seccomp help" -f -a "integrate" -d 'Integrate a profile into the user environment'
complete -c antimony -n "__fish_antimony_using_subcommand help; and not __fish_seen_subcommand_from run create edit refresh integrate reset trace stat info debug-shell seccomp help" -f -a "reset" -d 'Reset a profile back to the system-defined profile'
complete -c antimony -n "__fish_antimony_using_subcommand help; and not __fish_seen_subcommand_from run create edit refresh integrate reset trace stat info debug-shell seccomp help" -f -a "trace" -d 'Trace a profile for missing syscalls or files'
complete -c antimony -n "__fish_antimony_using_subcommand help; and not __fish_seen_subcommand_from run create edit refresh integrate reset trace stat info debug-shell seccomp help" -f -a "stat" -d 'Collect stats about a profile\'s sandbox'
complete -c antimony -n "__fish_antimony_using_subcommand help; and not __fish_seen_subcommand_from run create edit refresh integrate reset trace stat info debug-shell seccomp help" -f -a "info" -d 'List installed profiles and features'
complete -c antimony -n "__fish_antimony_using_subcommand help; and not __fish_seen_subcommand_from run create edit refresh integrate reset trace stat info debug-shell seccomp help" -f -a "debug-shell" -d 'Drop into a debugging shell within a profile\'s sandbox'
complete -c antimony -n "__fish_antimony_using_subcommand help; and not __fish_seen_subcommand_from run create edit refresh integrate reset trace stat info debug-shell seccomp help" -f -a "seccomp" -d 'Perform operations on the SECCOMP Database'
complete -c antimony -n "__fish_antimony_using_subcommand help; and not __fish_seen_subcommand_from run create edit refresh integrate reset trace stat info debug-shell seccomp help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
