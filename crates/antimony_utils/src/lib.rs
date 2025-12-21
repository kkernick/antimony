use anyhow::Result;
use spawn::{Handle, Spawner};
use std::path::Path;

pub fn set_capabilities(root: &str, path: &Path) -> Result<Handle> {
    let name = path.to_string_lossy();

    let command = format!(
        r#"
        chown antimony:antimony {path:?} &&
        chmod ug+s {path:?} &&
        chmod o+x {path:?} &&
        setcap cap_fowner+ep {path:?}


        TEMP=$(mktemp -d)
        mount --bind "{root}/config/profiles" /usr/share/antimony/profiles
        mount --bind "{root}/config/features" /usr/share/antimony/features
        mount --bind "${{TEMP}}" /usr/share/antimony/config
        mount --bind "${{TEMP}}" /usr/share/antimony/seccomp

        touch "{name}-ready"
        read

        umount /usr/share/antimony/{{profiles,features,config,seccomp}}

        rm {path:?} "{name}-ready"
        rmdir $TEMP
    "#,
    );

    let handle = Spawner::new("sh")
        .elevate(true)
        .arg("-c")?
        .arg(command)?
        .input(true)
        .spawn()?;
    Ok(handle)
}
