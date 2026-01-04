use crate::shared::profile::Namespace;
use spawn::SpawnError;

pub fn fabricate(info: &super::FabInfo) -> Result<(), SpawnError> {
    let namespaces = &mut info.profile.lock().namespaces;
    // All overrules None.
    if namespaces.contains(&Namespace::All) {
        namespaces.extend([
            Namespace::User,
            Namespace::Ipc,
            Namespace::Pid,
            Namespace::Net,
            Namespace::Uts,
            Namespace::CGroup,
        ]);
    }

    if !namespaces.contains(&Namespace::User) {
        info.handle.args_i([
            "--unshare-user",
            "--disable-userns",
            "--assert-userns-disabled",
        ])?;
    }
    if !namespaces.contains(&Namespace::Ipc) {
        info.handle.arg_i("--unshare-ipc")?;
    }
    if !namespaces.contains(&Namespace::Pid) {
        info.handle.arg_i("--unshare-pid")?;
    }

    if !namespaces.contains(&Namespace::Net) {
        info.handle.arg_i("--unshare-net")?;
    }
    if !namespaces.contains(&Namespace::Uts) {
        info.handle.arg_i("--unshare-uts")?;
    }
    if !namespaces.contains(&Namespace::CGroup) {
        info.handle.arg_i("--unshare-cgroup")?;
    }

    Ok(())
}
