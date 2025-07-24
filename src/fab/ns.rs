use crate::aux::profile::{Namespace, Profile};
use spawn::{SpawnError, Spawner};

pub fn fabricate(profile: &mut Profile, handle: &Spawner) -> Result<(), SpawnError> {
    let mut namespaces = profile.namespaces.take().unwrap_or_default();

    // All overrules None.
    if namespaces.is_empty() || namespaces.contains(&Namespace::All) {
        namespaces.extend([
            Namespace::User,
            Namespace::Ipc,
            Namespace::Pid,
            Namespace::Net,
            Namespace::Uts,
            Namespace::CGroup,
        ]);
    } else if namespaces.contains(&Namespace::None) {
        return Ok(());
    }

    if !namespaces.contains(&Namespace::User) {
        handle.args_i([
            "--unshare-user",
            "--disable-userns",
            "--assert-userns-disabled",
        ])?;
    }
    if !namespaces.contains(&Namespace::Ipc) {
        handle.arg_i("--unshare-ipc")?;
    }
    if !namespaces.contains(&Namespace::Pid) {
        handle.arg_i("--unshare-pid")?;
    }

    if !namespaces.contains(&Namespace::Net) {
        handle.arg_i("--unshare-net")?;
    }
    if !namespaces.contains(&Namespace::Uts) {
        handle.arg_i("--unshare-uts")?;
    }
    if !namespaces.contains(&Namespace::CGroup) {
        handle.arg_i("--unshare-cgroup")?;
    }

    Ok(())
}
