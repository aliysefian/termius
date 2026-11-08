# Lists a host's local accounts and groups. Read-only, and it never prints a password hash: where it can read /etc/shadow
# it reduces each line to a state (password, locked, none) and the expiry day before anything leaves the host.
LC_ALL=C; export LC_ALL
PATH="$PATH:/usr/local/bin:/usr/sbin:/sbin"; export PATH
echo "@@NOW"; date +%s 2>/dev/null
echo "@@ME"; id -un 2>/dev/null; id -u 2>/dev/null
echo "@@DEFS"; grep -E '^(UID_MIN|UID_MAX|SYS_UID_MAX)[[:space:]]' /etc/login.defs 2>/dev/null
echo "@@PASSWD"; head -n 5000 /etc/passwd 2>/dev/null
echo "@@GROUP"; head -n 5000 /etc/group 2>/dev/null
echo "@@SHADOW"
if [ -r /etc/shadow ]; then
  awk -F: '{ s = "password"; if ($2 == "") s = "none"; else if ($2 ~ /^[!*]/) s = "locked"; print $1 ":" s ":" $8 }' /etc/shadow 2>/dev/null | head -n 5000
else
  echo "unreadable"
fi
echo "@@HOMES"
awk -F: '$3 >= 1000 || $3 == 0 { print $1, $6 }' /etc/passwd 2>/dev/null | head -n 200 | while read -r u h; do
  [ -n "$h" ] || continue
  d=no; [ -d "$h" ] && d=yes
  k="?"
  if [ -r "$h/.ssh/authorized_keys" ]; then k=$(grep -cEv '^[[:space:]]*(#|$)' "$h/.ssh/authorized_keys" 2>/dev/null); fi
  echo "$u $d $k"
done
echo "@@END"
