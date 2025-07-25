#!/bin/sh

useradd -r antimony
chown antimony:antimony -R /usr/share/antimony
chown antimony:antimony /usr/bin/antimony

chmod ug+s /usr/bin/antimony

setcap cap_fowner+ep /usr/bin/antimony
setcap cap_audit_read+ep /usr/bin/antimony-monitor
