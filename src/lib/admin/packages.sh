# Lists a host's installed packages and the updates waiting, with the package manager it found. Read-only: it changes
# nothing and installs nothing. Update lists come from what the host already has cached (apt's lists, dnf's metadata),
# so they are as fresh as the host's last refresh, which needs root and the network. Output is sections `@@NAME`.
LC_ALL=C; export LC_ALL
PATH="$PATH:/usr/local/bin:/usr/sbin:/sbin"; export PATH
have() { command -v "$1" >/dev/null 2>&1; }
echo "@@NOW"; date +%s 2>/dev/null
if have dpkg-query && have apt-get; then m=apt
elif have dnf; then m=dnf
elif have yum; then m=yum
elif have zypper; then m=zypper
elif have pacman; then m=pacman
elif have apk; then m=apk
else m=none; fi
echo "@@MANAGER"; echo "$m"
case $m in
  apt)
    echo "@@INSTALLED"; dpkg-query -W -f='${Package}\t${Version}\t${Architecture}\t${db:Status-Abbrev}\n' 2>/dev/null | head -n 20000
    echo "@@UPDATES"; apt list --upgradable 2>/dev/null | head -n 5000
    echo "@@HELD"; apt-mark showhold 2>/dev/null | head -n 500
    echo "@@STATE"
    if [ -f /var/run/reboot-required ]; then echo "reboot=yes"; else echo "reboot=no"; fi
    for f in /var/lib/apt/periodic/update-success-stamp /var/cache/apt/pkgcache.bin /var/lib/apt/lists; do
      if [ -e "$f" ]; then echo "cache=$(stat -c %Y "$f" 2>/dev/null)"; break; fi
    done
    ;;
  dnf|yum)
    echo "@@INSTALLED"; rpm -qa --qf '%{NAME}\t%{VERSION}-%{RELEASE}\t%{ARCH}\n' 2>/dev/null | head -n 20000
    echo "@@UPDATES"; "$m" -q -C check-update 2>/dev/null | head -n 5000
    echo "@@STATE"
    ;;
  zypper)
    echo "@@INSTALLED"; rpm -qa --qf '%{NAME}\t%{VERSION}-%{RELEASE}\t%{ARCH}\n' 2>/dev/null | head -n 20000
    echo "@@UPDATES"; zypper -q --no-refresh lu 2>/dev/null | head -n 5000
    echo "@@STATE"
    ;;
  pacman)
    echo "@@INSTALLED"; pacman -Q 2>/dev/null | head -n 20000
    echo "@@UPDATES"; pacman -Qu 2>/dev/null | head -n 5000
    echo "@@STATE"
    ;;
  apk)
    echo "@@INSTALLED"; apk list -I 2>/dev/null | head -n 20000
    echo "@@UPDATES"; apk version -l '<' 2>/dev/null | head -n 5000
    echo "@@STATE"
    ;;
esac
echo "@@END"
