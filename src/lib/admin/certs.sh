# Finds TLS certificates on a host and reads their public details: who they are for, who issued them, when they end, and
# what renews them. Read-only. It reads certificate files only (never a private key), and what it cannot open it lists as
# unreadable so a person knows to look with more rights. Output is sections `@@NAME`.
LC_ALL=C; export LC_ALL
PATH="$PATH:/usr/local/bin:/usr/sbin:/sbin"; export PATH
have() { command -v "$1" >/dev/null 2>&1; }
echo "@@NOW"; date +%s 2>/dev/null
echo "@@TOOLS"
for t in certbot openssl; do have "$t" && echo "$t"; done
[ -x "$HOME/.acme.sh/acme.sh" ] && echo "acme.sh"
[ -x /root/.acme.sh/acme.sh ] && echo "acme.sh-root"
echo "@@LETSENCRYPT"
if [ -d /etc/letsencrypt ]; then
  if [ -r /etc/letsencrypt/live ]; then echo "readable"; else echo "unreadable"; fi
else
  echo "absent"
fi
echo "@@RENEWAL"
for f in /etc/letsencrypt/renewal/*.conf; do
  [ -r "$f" ] || continue
  echo "#FILE ${f##*/}"
  grep -E '^(cert|authenticator|installer|server|renew_before_expiry) *=' "$f" 2>/dev/null
done
list() {
  for f in /etc/letsencrypt/live/*/cert.pem; do [ -e "$f" ] && echo "$f|certbot"; done
  for f in "$HOME"/.acme.sh/*/*.cer "$HOME"/.acme.sh/*_ecc/*.cer /root/.acme.sh/*/*.cer /root/.acme.sh/*_ecc/*.cer; do
    case $f in */fullchain.cer|*/ca.cer) continue;; esac
    [ -e "$f" ] && echo "$f|acme.sh"
  done
  grep -rhE '^[[:space:]]*ssl_certificate[[:space:]]' /etc/nginx 2>/dev/null | sed -E 's/^[[:space:]]*ssl_certificate[[:space:]]+//; s/[[:space:];].*$//' | while read -r f; do echo "$f|nginx"; done
  grep -rhE '^[[:space:]]*SSLCertificateFile[[:space:]]' /etc/apache2 /etc/httpd 2>/dev/null | awk '{print $2 "|apache"}'
}
echo "@@CERTS"
list 2>/dev/null | grep -v '\$' | awk -F'|' '!seen[$1]++' | head -n 100 | while IFS='|' read -r f src; do
  [ -n "$f" ] || continue
  if [ -r "$f" ] && have openssl; then
    echo "#CERT $f|$src"
    openssl x509 -in "$f" -noout -subject -issuer -dates -fingerprint -sha256 -text 2>/dev/null | awk '/^subject=/ || /^issuer=/ || /^notBefore=/ || /^notAfter=/ || /Fingerprint=/ { print } /Subject Alternative Name/ { getline; print "SAN=" $0 }'
  else
    echo "#UNREADABLE $f|$src"
  fi
done
echo "@@END"
