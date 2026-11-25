#!/bin/sh
# End-to-end test against a fake head unit in Docker.
# Usage (from the repository root): sh tests/car/run-e2e.sh
set -eu

V=0.12.0
PORT=${E2E_PORT:-2222}
NAME=entrynav2-fake-car
DIR=$(cd "$(dirname "$0")" && pwd)
WORK=$PWD/target/e2e

rm -rf "$WORK"
T=$WORK/entrynav2-android-auto-$V
mkdir -p "$T/opt/aa" "$T/opt/hmi/data/ro/Plugins/EntryAad"
printf 'ELF\000entry-aad %s\000' "$V" > "$T/opt/aa/entry-aad"
cat > "$T/opt/aa/aa-supervisor" <<'EOF'
#!/bin/sh
[ "$1" = --compat ] && { echo '@ok|screen|800x480'; echo '@ok|bluetooth|found'; }
exit 0
EOF
echo lib > "$T/opt/hmi/data/ro/Plugins/EntryAad/libEntryAad.so"
chmod 755 "$T/opt/aa/entry-aad" "$T/opt/aa/aa-supervisor" "$T/opt/hmi/data/ro/Plugins/EntryAad/libEntryAad.so"
(cd "$T" && md5sum opt/aa/entry-aad opt/aa/aa-supervisor opt/hmi/data/ro/Plugins/EntryAad/libEntryAad.so > MD5SUMS)
(cd "$WORK" && tar -cf "entrynav2-android-auto-$V-armhf.tar" "entrynav2-android-auto-$V")

docker rm -f "$NAME" >/dev/null 2>&1 || true
docker build -q -t "$NAME" "$DIR" >/dev/null
docker run -d --init --name "$NAME" -p "127.0.0.1:$PORT:22" "$NAME" >/dev/null
trap 'docker rm -f "$NAME" >/dev/null 2>&1 || true' EXIT

i=0
until docker exec "$NAME" sh -c 'nc -z 127.0.0.1 22' >/dev/null 2>&1; do
    i=$((i + 1))
    [ "$i" -lt 30 ] || { docker logs "$NAME"; exit 1; }
    sleep 1
done

E2E_HOST=127.0.0.1 E2E_PORT=$PORT E2E_PASSWORD=sim E2E_VERSION=$V \
E2E_ARCHIVE="$WORK/entrynav2-android-auto-$V-armhf.tar" \
    cargo test --locked e2e -- --ignored --nocapture
