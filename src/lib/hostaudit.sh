# Collects what a Linux host's security audit needs (see hostaudit.ts). Run with `sh -c`, so it works whatever the
# login shell is. Output is plain text in sections introduced by a line `@@NAME`; every judgement is made by the app,
# none here.
#
# It only reads: files under /etc, /proc and /sys, `ss`, `find` for set-uid files, `stat`, `docker ps` and `docker
# inspect`, `apt-config`, `systemctl list-units`. It never uses sudo, never asks for a password, installs nothing,
# changes nothing and sends no packet. What the connected account may not read (the sshd drop-ins, sudoers, firewall
# rules) is reported as denied, so the audit can say "could not check" instead of guessing.
#
# Password hashes never leave the host: /etc/shadow is only ever asked "is this field empty?".
LC_ALL=C; export LC_ALL
PATH="$PATH:/usr/local/sbin:/usr/local/bin:/usr/sbin:/sbin:/usr/bin:/bin"; export PATH
have() { command -v "$1" >/dev/null 2>&1; }
# One tool is given at most N seconds, so a hung one can't stall the whole collection.
bounded() { n=$1; shift; if have timeout; then timeout "$n" "$@"; else "$@"; fi; }

os=$(uname -s 2>/dev/null)
echo "@@HOST"
echo "os=$os"
echo "uid=$(id -u 2>/dev/null)"
echo "user=$(id -un 2>/dev/null)"
echo "now=$(date +%s 2>/dev/null)"
echo "hostname=$(hostname 2>/dev/null)"
echo "kernel=$(uname -sr 2>/dev/null)"
if [ -r /etc/os-release ]; then
  echo "distro=$(. /etc/os-release 2>/dev/null; echo "$PRETTY_NAME")"
  echo "family=$(. /etc/os-release 2>/dev/null; echo "$ID $ID_LIKE")"
fi
if [ -s "$HOME/.ssh/authorized_keys" ]; then echo "authkeys=yes"; else echo "authkeys=no"; fi
if [ -d /run/systemd/system ]; then echo "systemd=yes"; else echo "systemd=no"; fi
# This audit is about Linux: say so and stop on anything else.
if [ "$os" != Linux ]; then echo "@@END"; exit 0; fi

# -- SSH daemon ------------------------------------------------------------------------------------------------
# `sshd -T` prints the settings in force, but only for root. Otherwise the files are read, and any that can't be
# are named, because one that can't be read may change what the others say.
echo "@@SSHD"
eff=""
if have sshd; then eff=$(bounded 5 sshd -T 2>/dev/null); fi
if [ -n "$eff" ]; then
  echo "mode=effective"
  printf '%s\n' "$eff" | grep -iE '^(port|permitrootlogin|passwordauthentication|permitemptypasswords|pubkeyauthentication|maxauthtries|x11forwarding|clientaliveinterval|clientalivecountmax|allowusers|allowgroups|ciphers|macs|kexalgorithms) '
elif [ -e /etc/ssh/sshd_config ] || have sshd; then
  echo "mode=files"
  if [ -r /etc/ssh/sshd_config ]; then
    echo "@file /etc/ssh/sshd_config"; grep -vE '^[[:space:]]*(#|$)' /etc/ssh/sshd_config
  else
    echo "@denied /etc/ssh/sshd_config"
  fi
  if [ -d /etc/ssh/sshd_config.d ]; then
    if [ -r /etc/ssh/sshd_config.d ] && [ -x /etc/ssh/sshd_config.d ]; then
      for f in /etc/ssh/sshd_config.d/*.conf; do
        [ -e "$f" ] || continue
        if [ -r "$f" ]; then echo "@file $f"; grep -vE '^[[:space:]]*(#|$)' "$f"; else echo "@denied $f"; fi
      done
    else
      echo "@denied /etc/ssh/sshd_config.d/"
    fi
  fi
else
  echo "mode=none"
fi

# -- listening sockets (the same format hostdetail.sh prints) ---------------------------------------------------
echo "@@PORTS"
if have ss; then
  echo "tool ss"; bounded 10 ss -lntup 2>/dev/null
elif have netstat; then
  echo "tool netstat"; bounded 10 netstat -lntup 2>/dev/null || bounded 10 netstat -lntu 2>/dev/null
else
  echo "tool proc"
  for f in tcp tcp6 udp udp6; do echo "file $f"; cat "/proc/net/$f" 2>/dev/null; done
fi

# -- firewall ---------------------------------------------------------------------------------------------------
echo "@@FIREWALL"
echo "@ufwconf"
grep -E '^ENABLED=' /etc/ufw/ufw.conf 2>/dev/null
grep -E '^DEFAULT_(INPUT|FORWARD)_POLICY=' /etc/default/ufw 2>/dev/null
# These three print rules only to root; for anyone else they print nothing, which the app reads as "not visible".
echo "@ufw"; bounded 5 ufw status verbose 2>/dev/null | head -n 80
echo "@iptables"; bounded 5 iptables -S 2>/dev/null | head -n 80
echo "@nft"; bounded 5 nft list ruleset 2>/dev/null | head -n 120

# -- accounts and privileges -------------------------------------------------------------------------------------
echo "@@PASSWD"; cat /etc/passwd 2>/dev/null
echo "@@SHADOW"
echo "perm=$(stat -c '%a %U %G' /etc/shadow 2>/dev/null)"
if [ -r /etc/shadow ]; then
  echo "readable=yes"
  awk -F: '$2 == "" {print "empty " $1}' /etc/shadow 2>/dev/null
else
  echo "readable=no"
fi
echo "@@UID_MIN"; grep -E '^UID_MIN' /etc/login.defs 2>/dev/null
echo "@@SUDOERS"
if [ -r /etc/sudoers ]; then
  echo "readable=yes"
  { cat /etc/sudoers
    for f in /etc/sudoers.d/*; do
      [ -f "$f" ] || continue
      # sudo ignores names with a dot or ending in ~.
      case "${f##*/}" in *.*|*~) continue;; esac
      cat "$f" 2>/dev/null
    done
  } 2>/dev/null | grep -vE '^[[:space:]]*(#|$)'
else
  echo "readable=no"
  ls /etc/sudoers.d 2>/dev/null | sed 's/^/file /'
fi

echo "@@SUID"
out=$(bounded 20 find /usr /bin /sbin /opt /usr/local -xdev -type f \( -perm -4000 -o -perm -2000 \) 2>/dev/null); rc=$?
printf '%s\n' "$out" | head -n 400
echo "@rc $rc"

# Every directory the system looks in for commands, and the connected account's own.
echo "@@PATHDIRS"
pathdir() {
  [ -d "$1" ] || return 0
  stat -L -c '%a %U %G %n' "$1" 2>/dev/null
  find "$1" -maxdepth 1 ! -type l -perm -0002 2>/dev/null | head -n 5 | sed 's/^/ww /'
}
for d in /usr/local/sbin /usr/local/bin /usr/sbin /usr/bin /sbin /bin; do pathdir "$d"; done
oldifs=$IFS; IFS=:
for d in $PATH; do [ -n "$d" ] && pathdir "$d"; done
IFS=$oldifs

# -- kernel settings (read from /proc/sys, so no sysctl binary or root is needed) ---------------------------------
echo "@@SYSCTL"
for k in net.ipv4.ip_forward net.ipv6.conf.all.forwarding net.ipv4.conf.all.accept_source_route net.ipv4.conf.all.accept_redirects net.ipv4.tcp_syncookies net.ipv4.conf.all.rp_filter fs.protected_hardlinks fs.protected_symlinks kernel.dmesg_restrict kernel.kptr_restrict; do
  p="/proc/sys/$(printf '%s' "$k" | tr '.' '/')"
  if [ -r "$p" ]; then echo "$k=$(cat "$p" 2>/dev/null)"; else echo "$k=?"; fi
done

# What the host does besides serving: a tunnel or a container runtime is a reason to forward packets.
echo "@@NETROLE"
for d in /sys/class/net/*; do [ -e "$d" ] && echo "iface ${d##*/}"; done
echo "@@SERVICES"
if [ -d /run/systemd/system ] && have systemctl; then
  echo "listed=yes"
  systemctl list-units --type=service --state=active --no-legend --plain 2>/dev/null | awk '{print "active " $1}'
else
  echo "listed=no"
fi

# -- containers -----------------------------------------------------------------------------------------------------
echo "@@DOCKER"
if have docker || [ -S /var/run/docker.sock ] || [ -S /run/docker.sock ]; then
  echo "present=yes"
  for s in /var/run/docker.sock /run/docker.sock; do
    [ -S "$s" ] && { stat -L -c 'sock %a %U %G %n' "$s" 2>/dev/null; break; }
  done
  if have docker; then
    echo "secopts $(bounded 8 docker info --format '{{json .SecurityOptions}}' 2>/dev/null)"
    if ids=$(bounded 10 docker ps -q 2>/dev/null); then
      echo "containers=ok"
      # One call for all of them, so many containers cost no more time than a few.
      [ -n "$ids" ] && bounded 20 docker inspect --format 'c|{{.Name}}|{{.Config.Image}}|{{.Config.User}}|{{.HostConfig.Privileged}}|{{json .HostConfig.PortBindings}}|{{range .Mounts}}{{.Source}},{{end}}' $ids 2>/dev/null
    else
      echo "containers=denied"
    fi
  fi
  if [ -r /etc/docker/daemon.json ]; then echo "@daemon"; cat /etc/docker/daemon.json 2>/dev/null; fi
else
  echo "present=no"
fi

# -- patches, logging, brute-force protection ---------------------------------------------------------------------------
echo "@@PATCH"
if have apt-get || [ -d /etc/apt ]; then
  echo "family=apt"
  apt-config dump 2>/dev/null | grep -E '^APT::Periodic::(Update-Package-Lists|Unattended-Upgrade) '
  echo "unattended=$(dpkg-query -W -f='${Status}' unattended-upgrades 2>/dev/null)"
  echo "lists_mtime=$(stat -c %Y /var/lib/apt/lists 2>/dev/null)"
  echo "stamp_mtime=$(stat -c %Y /var/lib/apt/periodic/update-success-stamp 2>/dev/null)"
  up=$(bounded 15 apt list --upgradable 2>/dev/null | tail -n +2)
  echo "upgradable=$(printf '%s\n' "$up" | grep -c .)"
  echo "security=$(printf '%s\n' "$up" | grep -c -- '-security')"
  if [ -f /var/run/reboot-required ]; then echo "reboot=yes"; fi
elif have dnf || have yum; then
  echo "family=rpm"
  if have systemctl; then
    for u in dnf-automatic.timer dnf-automatic-install.timer dnf-automatic-notifyonly.timer yum-cron.service; do
      echo "enabled $u $(systemctl is-enabled "$u" 2>/dev/null)"
    done
  fi
else
  echo "family=other"
fi

echo "@@F2B"
for f in /etc/fail2ban/jail.local /etc/fail2ban/jail.d/*.conf /etc/fail2ban/jail.d/*.local; do
  [ -e "$f" ] || continue
  if [ -r "$f" ]; then echo "@file $f"; grep -vE '^[[:space:]]*(#|;|$)' "$f"; else echo "@denied $f"; fi
done
echo "@@END"
