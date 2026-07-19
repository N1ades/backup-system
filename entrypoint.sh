#!/bin/sh
set -e

# Scheduling lives per-project inside each .backup file now (cron/keep_versions/
# keep_days/max_total_size), re-read on every run. So this trigger just fires
# every minute and "app.main backup --due" figures out which projects, if any,
# are actually due -- no separate schedule to configure or push here.

printenv | grep -v '^HOME=\|^PWD=\|^SHLVL=' > /etc/environment
echo "* * * * * cd /opt/backup-system && . /etc/environment && python -m app.main backup --due >> /var/log/backup.log 2>&1" | crontab -
touch /var/log/backup.log

if [ "$1" = "cli" ] || [ "$1" = "backup" ] || [ "$1" = "audit" ] || [ "$1" = "restore" ]; then
    exec python -m app.main "$@"
fi

echo "Per-project schedules are read from each .backup file (checked every minute)."
echo "For the interactive menu run: docker exec -it <container> python -m app.main cli"
cron -f
