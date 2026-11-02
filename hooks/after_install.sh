#!/bin/sh

useradd -r antimony 2>/dev/null
rm -r /usr/share/antimony/cache
chown antimony:antimony -R /usr/share/antimony
chown antimony:antimony /usr/bin/antimony

useradd -r antimony-lockdown 2>/dev/null

if ! [ -d /usr/share/antimony/lockdown ]; then
    mkdir /usr/share/antimony/lockdown
fi

chown antimony-lockdown:antimony-lockdown -R /usr/share/antimony/lockdown
chown antimony-lockdown:antimony-lockdown /usr/share/antimony/utilities/antimony-lockdown

chmod ug+s /usr/bin/antimony
chmod ug+s /usr/share/antimony/utilities/antimony-lockdown
setcap cap_sys_ptrace+ep /usr/share/antimony/utilities/antimony-dumper
setcap cap_audit_read+ep /usr/share/antimony/utilities/antimony-monitor
setcap cap_fowner+ep /usr/share/antimony/utilities/antimony-writer

if [ -f /usr/bin/aa-status ] && [ -f /usr/bin/systemctl ]; then
    systemctl reload apparmor
fi
