#!/bin/sh

chown antimony:antimony -R /usr/share/antimony
chown antimony:antimony /usr/bin/antimony

chmod ug+s /usr/bin/antimony
setcap cap_sys_ptrace+ep /usr/share/antimony/utilities/antimony-dumper
setcap cap_audit_read+ep /usr/share/antimony/utilities/antimony-monitor
