#!/bin/sh

useradd -r antimony
chown antimony:antimony -R /usr/share/antimony
chown antimony:antimony /usr/bin/antimony

useradd -r antimony-lockdown
mkdir /usr/share/antimony/lockdown
chown antimony-lockdown:antimony-lockdown -R /usr/share/antimony/lockdown
chown antimony-lockdown:antimony-lockdown -R /usr/share/antimony/utilities/antimony-lockdown

chmod ug+s /usr/bin/antimony
chmod ug+s /usr/share/antimony/utilities/antimony-lockdown
setcap cap_sys_ptrace+ep /usr/share/antimony/utilities/antimony-dumper
setcap cap_audit_read+ep /usr/share/antimony/utilities/antimony-monitor
