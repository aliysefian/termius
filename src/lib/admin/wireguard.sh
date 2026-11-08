# Lists a host's WireGuard interfaces and their peers. Read-only, and it never prints a key that is secret: the interface's
# private key and each peer's preshared key are blanked here, on the host, before anything leaves it. Public keys, endpoints,
# allowed addresses, handshakes and byte counts are what is left. Peers need root (`wg` refuses an ordinary user); without
# it only the interfaces are listed. Output is sections `@@NAME`.
LC_ALL=C; export LC_ALL
PATH="$PATH:/usr/local/bin:/usr/sbin:/sbin"; export PATH
have() { command -v "$1" >/dev/null 2>&1; }
echo "@@NOW"; date +%s 2>/dev/null
echo "@@TOOLS"
for t in wg wg-quick ip; do have "$t" && echo "$t"; done
echo "@@LINKS"
if have ip; then ip -o link show type wireguard 2>/dev/null; fi
echo "@@ADDRS"
if have ip; then
  ip -o link show type wireguard 2>/dev/null | awk -F': ' '{ print $2 }' | while read -r n; do ip -o addr show dev "$n" 2>/dev/null; done
fi
echo "@@DUMP"
if have wg; then
  wg show all dump 2>&1 | awk -F'\t' 'BEGIN { OFS = "\t" } NF == 5 { $2 = "(hidden)" } NF == 9 { $3 = "(hidden)" } { print }' | head -n 2000
fi
echo "@@SESSION"
echo "$SSH_CONNECTION"
c=${SSH_CONNECTION%% *}
if [ -n "$c" ] && have ip; then ip route get "$c" 2>/dev/null | head -n 1; fi
echo "@@CONFS"
if [ -d /etc/wireguard ]; then ls /etc/wireguard 2>&1 | head -n 50; else echo "absent"; fi
echo "@@END"
