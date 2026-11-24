#!/bin/sh
# Runs the scripts sent to the car in a sandbox with stub commands.
# Usage: sh tests/scripts/sim.sh <scripts dir> <work dir> (absolute paths,
# after `SCRIPT_DUMP_DIR=... cargo test dump_scripts -- --ignored`). Needs GNU tar.
# C:/x -> /c/x: no colon in PATH.
SCRIPTS=$(echo "$1" | sed 's#^\([A-Za-z]\):#/\L\1#')
WORK=$(echo "$2" | sed 's#^\([A-Za-z]\):#/\L\1#')
V=0.12.0
PASS=0; FAILN=0

ok()   { PASS=$((PASS+1)); echo "  PASS: $1"; }
bad()  { FAILN=$((FAILN+1)); echo "  FAIL: $1"; [ -n "$SHOWN" ] || { tail -n 8 "$WORK/out.txt" | sed 's/^/      | /'; SHOWN=1; }; }
check(){ if eval "$2"; then ok "$1"; else bad "$1"; fi; }

setup() {
    rm -rf "$WORK"; mkdir -p "$WORK/root/tmp" "$WORK/bin"
    R=$WORK/root
    A=$R/tmp/entrynav2-android-auto-$V
    mkdir -p "$A/opt/aa" "$A/opt/hmi/data/ro/Plugins/EntryAad"
    echo bin1 > "$A/opt/aa/entry-aad"; echo bin2 > "$A/opt/aa/aa-supervisor"
    echo lib > "$A/opt/hmi/data/ro/Plugins/EntryAad/libEntryAad.so"
    (cd "$A" && md5sum opt/aa/entry-aad opt/aa/aa-supervisor opt/hmi/data/ro/Plugins/EntryAad/libEntryAad.so > MD5SUMS)
    # Stubs
    cat > "$WORK/bin/mount" <<'EOF'
#!/bin/sh
echo "$*" >> "$SIM_LOG/mount.log"
EOF
    # Fake processes: one "name pid" line per process in $SIM_LOG/procs.
    cat > "$WORK/bin/pidof" <<'EOF'
#!/bin/sh
[ "$SIM_RUNNING" = 1 ] && { echo 99; exit 0; }
found=1
for n; do
    p=$(grep "^$n " "$SIM_LOG/procs" 2>/dev/null | cut -d' ' -f2)
    [ -n "$p" ] && { echo "$p"; found=0; }
done
exit $found
EOF
    # kill: TERM to the supervisor ends both when it obeys; KILL ends the target.
    cat > "$WORK/bin/kill" <<'EOF'
#!/bin/sh
sig=$1; shift
# "Is it alive?" is answered for real (lock owner checks).
if [ "$sig" = -0 ]; then kill -0 "$@" 2>/dev/null; exit $?; fi
echo "kill $sig $*" >> "$SIM_LOG/kill.log"
for pid; do
    name=$(grep " $pid\$" "$SIM_LOG/procs" | cut -d' ' -f1)
    case "$sig:$name" in
        -KILL:*) sed -i "/ $pid\$/d" "$SIM_LOG/procs" ;;
        -TERM:aa-supervisor) [ "$SIM_OBEYS" = 1 ] && : > "$SIM_LOG/procs" ;;
        -TERM:entry-aad) [ "$SIM_OBEYS" = 1 ] && sed -i "/ $pid\$/d" "$SIM_LOG/procs" ;;
    esac
done
exit 0
EOF
    # sleep: instant; an obedient supervisor reacts to the STOP file.
    cat > "$WORK/bin/sleep" <<'EOF'
#!/bin/sh
if [ "$SIM_OBEYS" = 1 ] && [ -f "$SIM_ROOT/tmp/entry-aa-service/STOP" ]; then
    : > "$SIM_LOG/procs"; rm -f "$SIM_ROOT/tmp/entry-aa-service/STOP"
fi
exit 0
EOF
    cat > "$WORK/bin/df" <<'EOF'
#!/bin/sh
echo "Filesystem 1K-blocks Used Available Use% Mounted"
echo "rootfs 500000 100000 ${SIM_FREE:-200000} 20% /"
EOF
    REALCP=$(command -v cp)
    cat > "$WORK/bin/cp" <<EOF
#!/bin/sh
for last; do :; done
case "\$last" in */opt/aa/) [ "\$SIM_FAIL_CP" = 1 ] && { echo "cp: simulated write error"; exit 1; } ;; esac
exec "$REALCP" "\$@"
EOF
    chmod +x "$WORK/bin/"*
    export SIM_LOG=$WORK SIM_ROOT=$R; : > "$WORK/mount.log"; : > "$WORK/procs"; : > "$WORK/kill.log"
}

# Rewrites absolute paths of a script into the sandbox root.
sandbox() {
    sed -e "s#/opt/aa#$R/opt/aa#g" -e "s#/opt/hmi#$R/opt/hmi#g" \
        -e "s#/tmp/#$R/tmp/#g" -e "s#/var/local#$R/var/local#g" \
        -e "s#^cd /\$#cd $R#" -e "s#^cd /tmp #cd $R/tmp #" \
        -e "s#kill -#$WORK/bin/kill -#g" "$1" > "$WORK/run.sh"
}

run() { (PATH="$WORK/bin:$PATH"; export PATH; sh "$WORK/run.sh") > "$WORK/out.txt" 2>&1; echo $?; }

P=$WORK/root/opt/hmi/data/ro/Plugins

echo "1. Fresh install"
setup; sandbox "$SCRIPTS/install.sh"; rc=$(run)
check "exit code 0" '[ "$rc" = 0 ]'
check "@@INSTALL_OK printed" 'grep -q "^@@INSTALL_OK" "$WORK/out.txt"'
check "binary copied" '[ -f "$R/opt/aa/entry-aad" ]'
check "plugin copied" '[ -f "$P/EntryAad/libEntryAad.so" ]'
check "markers written" '[ -f "$R/opt/aa/.installed-by-entryaad" ] && [ -f "$P/EntryAad/.installed-by-entryaad" ]'
check "version file" '[ "$(cat "$R/opt/aa/.installer-version")" = "$V" ]'
check "remounted rw then ro" '[ "$(cat "$WORK/mount.log" | tr "\n" ";")" = "-o remount,rw /;-o remount,ro /;" ]'
check "log file removed" '! ls "$R"/tmp/entrynav2-installer-*.log >/dev/null 2>&1 && ! ls /tmp/entrynav2-installer-install.log >/dev/null 2>&1'

echo "2. Update over an existing install"
SAVED=$R
sandbox "$SCRIPTS/install.sh"; echo old > "$R/opt/aa/old-file"; rc=$(run)
check "exit code 0" '[ "$rc" = 0 ]'
check "old files removed" '[ ! -f "$R/opt/aa/old-file" ]'

echo "3. Android Auto running"
setup; sandbox "$SCRIPTS/install.sh"; rc=$(SIM_RUNNING=1 run)
check "refused" '[ "$rc" != 0 ] && grep -q "@@ERR|Android Auto is running" "$WORK/out.txt"'
check "nothing written" '[ ! -d "$R/opt" ]'
check "never remounted rw" '! grep -q rw "$WORK/mount.log"'

echo "4. Foreign /opt/aa folder"
setup; mkdir -p "$R/opt/aa"; echo x > "$R/opt/aa/foreign"; sandbox "$SCRIPTS/install.sh"; rc=$(run)
check "refused" '[ "$rc" != 0 ] && grep -q "not created by EntryNav2" "$WORK/out.txt"'
check "foreign file untouched" '[ -f "$R/opt/aa/foreign" ]'
check "never remounted rw" '! grep -q rw "$WORK/mount.log"'

echo "5. Not enough free space"
setup; sandbox "$SCRIPTS/install.sh"; rc=$(SIM_FREE=1000 run)
check "refused" '[ "$rc" != 0 ] && grep -q "not enough free space" "$WORK/out.txt"'
check "never remounted rw" '! grep -q rw "$WORK/mount.log"'

echo "6. Archive changed since the check"
setup; echo tampered > "$A/opt/aa/entry-aad"; sandbox "$SCRIPTS/install.sh"; rc=$(run)
check "refused" '[ "$rc" != 0 ] && grep -q "no longer matches" "$WORK/out.txt"'
check "never remounted rw" '! grep -q rw "$WORK/mount.log"'

echo "7. Copy fails half-way"
setup; sandbox "$SCRIPTS/install.sh"; rc=$(SIM_FAIL_CP=1 run)
check "failed" '[ "$rc" != 0 ] && ! grep -q "^@@INSTALL_OK" "$WORK/out.txt"'
check "partial plugin removed" '[ ! -e "$P/EntryAad" ]'
check "partial /opt/aa removed" '[ ! -e "$R/opt/aa" ]'
check "Plugins folder left in place" '[ -d "$P" ]'
check "remounted read-only" 'tail -n 1 "$WORK/mount.log" | grep -q "remount,ro"'

echo "8. Archive gone from /tmp (head unit restarted)"
setup; rm -rf "$A"; sandbox "$SCRIPTS/install.sh"; rc=$(run)
check "refused" '[ "$rc" != 0 ] && grep -q "no longer in /tmp" "$WORK/out.txt"'

echo "9. Uninstall after install"
setup; sandbox "$SCRIPTS/install.sh"; run >/dev/null; mkdir -p "$R/var/local/data/AA"; : > "$WORK/mount.log"
sandbox "$SCRIPTS/uninstall-keep.sh"; rc=$(run)
check "exit code 0" '[ "$rc" = 0 ] && grep -q "^@@UNINSTALL_OK" "$WORK/out.txt"'
check "folders removed" '[ ! -e "$R/opt/aa" ] && [ ! -e "$P/EntryAad" ]'
check "Plugins folder left in place" '[ -d "$P" ]'
check "settings kept" '[ -d "$R/var/local/data/AA" ]'
check "remounted rw then ro" '[ "$(cat "$WORK/mount.log" | tr "\n" ";")" = "-o remount,rw /;-o remount,ro /;" ]'

echo "10. Uninstall refuses foreign folders"
setup; mkdir -p "$R/opt/aa"; echo x > "$R/opt/aa/foreign"; sandbox "$SCRIPTS/uninstall-wipe.sh"; rc=$(run)
check "refused" '[ "$rc" != 0 ] && [ -f "$R/opt/aa/foreign" ]'
check "never remounted rw" '! grep -q rw "$WORK/mount.log"'

echo "11. Uninstall refuses while running"
setup; sandbox "$SCRIPTS/install.sh"; run >/dev/null; : > "$WORK/mount.log"
sandbox "$SCRIPTS/uninstall-wipe.sh"; rc=$(SIM_RUNNING=1 run)
check "refused" '[ "$rc" != 0 ] && [ -d "$R/opt/aa" ]'

echo "12. Plugins folder that existed before is kept"
setup; mkdir -p "$P"; sandbox "$SCRIPTS/install.sh"; run >/dev/null
sandbox "$SCRIPTS/uninstall-keep.sh"; run >/dev/null
check "Plugins dir kept after uninstall" '[ -d "$P" ] && [ ! -e "$P/EntryAad" ]'

echo "13. Stop: supervisor running, obeys the STOP file"
setup; mkdir -p "$R/tmp/entry-aa-service"; printf 'aa-supervisor 100\nentry-aad 101\n' > "$WORK/procs"
sandbox "$SCRIPTS/stop.sh"; rc=$(SIM_OBEYS=1 run)
check "exit code 0" '[ "$rc" = 0 ] && grep -q "^@@STOP_OK|stopped" "$WORK/out.txt"'
check "used the STOP file" 'grep -q "STOP file" "$WORK/out.txt"'
check "no signal sent" '[ ! -s "$WORK/kill.log" ]'
check "nothing running" '[ ! -s "$WORK/procs" ]'

echo "14. Stop: supervisor never stops -> error, never SIGKILLed"
setup; mkdir -p "$R/tmp/entry-aa-service"; printf 'aa-supervisor 100\nentry-aad 101\n' > "$WORK/procs"
sandbox "$SCRIPTS/stop.sh"; rc=$(SIM_OBEYS=0 run)
check "error reported" '[ "$rc" != 0 ] && grep -q "@@ERR|Android Auto is still running" "$WORK/out.txt"'
check "supervisor not killed" '! grep -q "KILL 100" "$WORK/kill.log" && grep -q "^aa-supervisor" "$WORK/procs"'

echo "15. Stop: no service folder -> SIGTERM to the supervisor"
setup; printf 'aa-supervisor 100\n' > "$WORK/procs"
sandbox "$SCRIPTS/stop.sh"; rc=$(SIM_OBEYS=1 run)
check "exit code 0" '[ "$rc" = 0 ]'
check "SIGTERM sent to the supervisor" 'grep -q "kill -TERM 100" "$WORK/kill.log"'

echo "16. Stop: lone entry-aad that ignores SIGTERM -> SIGKILL after 20 s"
setup; printf 'entry-aad 101\n' > "$WORK/procs"
sandbox "$SCRIPTS/stop.sh"; rc=$(SIM_OBEYS=0 run)
check "exit code 0" '[ "$rc" = 0 ] && grep -q "^@@STOP_OK|stopped" "$WORK/out.txt"'
check "TERM then KILL" 'grep -q "kill -TERM 101" "$WORK/kill.log" && grep -q "kill -KILL 101" "$WORK/kill.log"'

echo "17. Stop: not running"
setup; sandbox "$SCRIPTS/stop.sh"; rc=$(run)
check "nothing to do" '[ "$rc" = 0 ] && grep -q "^@@STOP_OK|not running" "$WORK/out.txt"'

echo "18. Stop: Bluetooth restore not confirmed -> warning"
setup; mkdir -p "$R/tmp/entry-aa-service"; : > "$R/tmp/entry-aa-service/bluetooth-retour.txt"; printf 'aa-supervisor 100\n' > "$WORK/procs"
sandbox "$SCRIPTS/stop.sh"; rc=$(SIM_OBEYS=1 run)
check "warning shown" 'grep -q "@@WARN|The Bluetooth restore was not confirmed" "$WORK/out.txt"'

echo "19. USB mode keeps an existing port name"
setup; mkdir -p "$R/var/local/data/AA"; echo 1-1 > "$R/var/local/data/AA/usb"
sandbox "$SCRIPTS/usb.sh"; rc=$(run)
check "kept" '[ "$rc" = 0 ] && [ "$(cat "$R/var/local/data/AA/usb")" = 1-1 ]'
setup; sandbox "$SCRIPTS/usb.sh"; rc=$(run)
check "created" '[ "$rc" = 0 ] && [ -f "$R/var/local/data/AA/usb" ]'

echo "20. Check refuses an archive without the programs"
setup; rm "$A/opt/aa/aa-supervisor"; ( cd "$R/tmp" && tar -cf "entrynav2-android-auto-$V-armhf.tar" "entrynav2-android-auto-$V" )
sandbox "$SCRIPTS/check.sh"; rc=$(run)
check "refused before md5" '[ "$rc" != 0 ] && grep -q "@@ERR|opt/aa/aa-supervisor is missing" "$WORK/out.txt"'

echo "21. Install removes the journal of an interrupted USB-stick update"
setup; mkdir -p "$R/var/local/data/AA"; : > "$R/var/local/data/AA/update-pending"
sandbox "$SCRIPTS/install.sh"; rc=$(run)
check "installed" '[ "$rc" = 0 ]'
check "update-pending removed" '[ ! -e "$R/var/local/data/AA/update-pending" ]'
check "lock released" '[ ! -e "$R/tmp/entrynav2-installer.lock" ]'

echo "22. Another installer holds the lock (live process)"
setup; mkdir -p "$R/tmp/entrynav2-installer.lock"; echo $$ > "$R/tmp/entrynav2-installer.lock/pid"
sandbox "$SCRIPTS/install.sh"; rc=$(run)
check "refused" '[ "$rc" != 0 ] && grep -q "another installer is changing this car" "$WORK/out.txt"'
check "never remounted rw" '! grep -q rw "$WORK/mount.log"'
check "the other lock is kept" '[ "$(cat "$R/tmp/entrynav2-installer.lock/pid")" = "$$" ]'

echo "23. A lock left by a dead process is taken over"
setup; mkdir -p "$R/tmp/entrynav2-installer.lock"; echo 999999 > "$R/tmp/entrynav2-installer.lock/pid"
sandbox "$SCRIPTS/install.sh"; rc=$(run)
check "installed" '[ "$rc" = 0 ] && grep -q "^@@INSTALL_OK" "$WORK/out.txt"'
check "lock released" '[ ! -e "$R/tmp/entrynav2-installer.lock" ]'

echo "24. Check refuses a path escaping the archive folder"
setup; T=entrynav2-android-auto-$V; mkdir -p "$R/tmp/x"; echo evil > "$R/tmp/x/evil"
( cd "$R/tmp" && tar -cf "$T-armhf.tar" -P --transform "s,^x/evil,$T/../../evil," x/evil "$T" )
rm -rf "$R/tmp/x" "$R/tmp/$T"
sandbox "$SCRIPTS/check.sh"; rc=$(run)
check "refused" '[ "$rc" != 0 ] && grep -q "@@ERR|unsafe path in the archive" "$WORK/out.txt"'
check "nothing extracted" '[ ! -e "$R/tmp/$T" ] && [ ! -e "$R/evil" ] && [ ! -e "$WORK/evil" ]'

echo "25. Check refuses links in the archive"
setup; T=entrynav2-android-auto-$V; ln "$A/opt/aa/entry-aad" "$A/opt/aa/hardlink"
( cd "$R/tmp" && tar -cf "$T-armhf.tar" "$T" ); rm -rf "$R/tmp/$T"
sandbox "$SCRIPTS/check.sh"; rc=$(run)
check "refused" '[ "$rc" != 0 ] && grep -q "@@ERR|the archive contains links" "$WORK/out.txt"'
check "nothing extracted" '[ ! -e "$R/tmp/$T" ]'

echo "26. Check flags files outside the two known folders"
setup; T=entrynav2-android-auto-$V; mkdir -p "$A/etc/init.d"; echo x > "$A/etc/init.d/aa"
( cd "$A" && md5sum etc/init.d/aa >> MD5SUMS ); ( cd "$R/tmp" && tar -cf "$T-armhf.tar" "$T" ); rm -rf "$R/tmp/$T"
sandbox "$SCRIPTS/check.sh"; run >/dev/null
check "unsupported reported" 'grep -q "^@@UNSUPPORTED|etc/init.d/aa" "$WORK/out.txt"'

echo "27. Earlier checks of other versions are removed from /tmp"
setup; mkdir -p "$R/tmp/entrynav2-android-auto-0.11.0"; : > "$R/tmp/entrynav2-android-auto-0.11.0-armhf.tar"
sandbox "$SCRIPTS/clean-others.sh"; rc=$(run)
check "other version removed" '[ ! -e "$R/tmp/entrynav2-android-auto-0.11.0" ] && [ ! -e "$R/tmp/entrynav2-android-auto-0.11.0-armhf.tar" ]'
check "current version kept" '[ -d "$A" ]'

echo "28. Status lists the archives waiting in /tmp"
setup; sandbox "$SCRIPTS/info.sh"; run >/dev/null
check "staged reported" 'grep -q "^@@STAGED|$V" "$WORK/out.txt"'

echo "29. Status reads the version compiled into the program"
setup; mkdir -p "$R/opt/aa"; : > "$R/opt/aa/.installed-by-entryaad"
printf 'ELF...\0000.13.0-rc.10\000...' > "$R/opt/aa/entry-aad"
sandbox "$SCRIPTS/info.sh"; run >/dev/null
check "installed by hand" 'grep -q "^@@APPVER|?" "$WORK/out.txt"'
check "rc.10 found" 'grep -q "^@@BINVER|0.13.0-rc.10" "$WORK/out.txt"'
check "0.12.0 not found" '! grep -q "^@@BINVER|0.12.0" "$WORK/out.txt"'

echo
echo "RESULT: $PASS passed, $FAILN failed"
[ "$FAILN" = 0 ]
