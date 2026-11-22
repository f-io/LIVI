# F133 XFH + AIC8800D40L on SDIO, the kernel resets it through PG10 (wlan_pwrseq).
# 55 MB: no extra 4 MB held free after fragmentation events.
echo 0 > /proc/sys/vm/watermark_boost_factor

board_wifi() {
    if ls /sys/bus/sdio/devices/* >/dev/null 2>&1; then
        grep -q "^aic8800_bsp " /proc/modules || insmod /lib/modules/$(uname -r)/aic8800_bsp.ko 2>&1 | tail -1
        sleep 1
        grep -q "^aic8800_fdrv " /proc/modules || insmod /lib/modules/$(uname -r)/aic8800_fdrv.ko aicwf_dbg_level=1 2>&1 | tail -1
        # wlan0 has 257 TX queues, each with three rings of txqueuelen entries: 1000 hold 6 MB,
        # 32 fit in 256 bytes. The driver queues per station itself.
        ifconfig wlan0 txqueuelen 32
    else
        echo "[livi] no SDIO card, WiFi skipped"
    fi
}

# LIVI keeps its settings and pairings on the NOR's data partition, jffs2 formats it on the first mount.
board_start() {
    # Swap on zram, room for 16 MB of process memory that sits idle. Swapping it costs only CPU.
    if [ -e /sys/block/zram0 ]; then
        echo 16M > /sys/block/zram0/disksize && busybox mkswap /dev/zram0 >/dev/null && busybox swapon /dev/zram0
        echo 100 > /proc/sys/vm/swappiness
    fi
    # Wired phones need the USB-A port as host
    for m in /sys/devices/platform/soc/4100000.usb/musb-hdrc.*/mode; do echo host > $m; done
    home=/mnt/data
    mount -t jffs2 mtd:data $home || { home=/tmp/home; mkdir -p $home; echo "[livi] no data partition, settings stay in RAM"; }
    # 512 KB of data partition: state files and logs stay in RAM, else they fill it and pairings are not saved.
    for d in state log; do
        rm -rf $home/.config/LIVI/$d
        mkdir -p /tmp/livi/$d $home/.config/LIVI
        ln -s /tmp/livi/$d $home/.config/LIVI/$d
    done
    # core applies these at load and on every change, the config file keeps only what the user chose.
    # Hidden settings keep the board's value: the codec is the only audio device, 48 kHz is its rate.
    export LIVI_BOARD_DEFAULTS='{"uiZoomPercent":160,"wifiInterface":"wlan0","wifiDedicatedInterface":true,"btAdapter":"module"}'
    export LIVI_BOARD_PINNED='{"projectionFps":30,"clusterFps":30,"samplingFrequency":1,"huVolumeLinkSystem":true,"audioOutputDevice":"","audioInputDevice":""}'
    export LIVI_BOARD_HIDDEN=screenSettings,dashboard.cluster,dashboard.aux,media.cluster,media.aux,camera.cluster,camera.aux,projectionWidth,projectionHeight,projectionFps,clusterWidth,clusterHeight,clusterFps,softwareUpdate,audioOutputDevice,audioInputDevice,samplingFrequency,huVolumeLinkSystem
    export SLINT_KMS_ROTATION=270 SLINT_KMS_FORMAT=AR24 LIVI_UART_MODULE=/dev/ttyS1 LIVI_MFI_ON_BOARD=1
    export LIVI_LOCKDOWN_STORE=$home/.config/LIVI/lockdown
    # One decoder at a time: its buffers stay reserved and pass from one stream to the next.
    export LIVI_VIDEO_DECODERS=1
    export LIVI_CODECS="h264=hw h265=hw"
    # Mirroring offers H.264 only to save CMA: a landscape iPhone screen needs about 24 MB of
    # decoder buffers in HEVC, 15 MB in H.264. LIVI_MIRROR_HEVC=1 offers HEVC here too.
    export LIVI_MIRROR_HEVC=0
}

# The rootfs is up: boot0's rtc[3] mark for this boot is cleared, the next one boots normally.
devmem 0x0709010c 32 0
