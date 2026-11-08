# Lists a host's scheduled jobs: the crontab of whoever is connected, the system's cron files, other users' crontabs where
# they can be read, the daily/weekly directories, and systemd timers. Read-only. Output is sections `@@NAME`.
LC_ALL=C; export LC_ALL
PATH="$PATH:/usr/local/bin:/usr/sbin:/sbin"; export PATH
have() { command -v "$1" >/dev/null 2>&1; }
echo "@@NOW"; date +%s 2>/dev/null
echo "@@ME"; id -un 2>/dev/null
echo "@@DAEMON"
if have systemctl; then for u in cron crond cronie anacron; do echo "$u $(systemctl is-active "$u" 2>/dev/null)"; done; fi
echo "@@USER"; if have crontab; then crontab -l 2>&1 | head -n 500; else echo "missing crontab"; fi
echo "@@SYSTEM"
for f in /etc/crontab /etc/cron.d/*; do
  [ -f "$f" ] && [ -r "$f" ] || continue
  echo "#FILE $f"; head -n 300 "$f"
done
echo "@@SPOOL"
for d in /var/spool/cron/crontabs /var/spool/cron; do
  [ -d "$d" ] || continue
  if [ ! -r "$d" ]; then echo "#UNREADABLE $d"; break; fi
  for f in "$d"/*; do
    [ -f "$f" ] && [ -r "$f" ] || continue
    echo "#FILE $f"; head -n 300 "$f"
  done
  break
done
echo "@@PERIODIC"
for p in hourly daily weekly monthly; do
  for f in /etc/cron.$p/*; do [ -f "$f" ] && echo "$p ${f##*/}"; done
done
echo "@@TIMERS"
if have systemctl; then systemctl show '*.timer' --property=Id,Description,ActiveState,NextElapseUSecRealtime,LastTriggerUSec,Unit --no-pager 2>/dev/null | head -n 2000; fi
echo "@@END"
