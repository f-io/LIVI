# busybox defconfig changes every board's userspace build applies (sed -f on .config): the flash
# tools the updaters use go in, tc goes out (it does not build against current kernel headers), and
# so does the console/VT tooling a headless dongle has no use for.
s|^# CONFIG_FLASHCP is not set|CONFIG_FLASHCP=y|
s|^# CONFIG_FLASH_ERASEALL is not set|CONFIG_FLASH_ERASEALL=y|
s|^CONFIG_TC=y|# CONFIG_TC is not set|
s|^CONFIG_FEATURE_TC.*|# &|
s|^CONFIG_KBD_MODE=y|# CONFIG_KBD_MODE is not set|
s|^CONFIG_LOADFONT=y|# CONFIG_LOADFONT is not set|
s|^CONFIG_LOADKMAP=y|# CONFIG_LOADKMAP is not set|
s|^CONFIG_OPENVT=y|# CONFIG_OPENVT is not set|
s|^CONFIG_SETCONSOLE=y|# CONFIG_SETCONSOLE is not set|
s|^CONFIG_SETFONT=y|# CONFIG_SETFONT is not set|
s|^CONFIG_SHOWKEY=y|# CONFIG_SHOWKEY is not set|
s|^CONFIG_CHVT=y|# CONFIG_CHVT is not set|
s|^CONFIG_DEALLOCVT=y|# CONFIG_DEALLOCVT is not set|
s|^CONFIG_RESET=y|# CONFIG_RESET is not set|
s|^CONFIG_FGCONSOLE=y|# CONFIG_FGCONSOLE is not set|
s|^CONFIG_HWCLOCK=y|# CONFIG_HWCLOCK is not set|
s|^CONFIG_RTCWAKE=y|# CONFIG_RTCWAKE is not set|
