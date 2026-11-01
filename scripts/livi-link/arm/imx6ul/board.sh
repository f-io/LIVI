# i.MX6ULL with the Wi-Fi/Bluetooth module RADIO names.
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
BOARD=imx6ul
RADIO=${RADIO:-iw416}
case $RADIO in
  iw416 | rtl8822cs | rtl8822bs | bcm4354) ;;
  *) echo "RADIO=$RADIO: this board has builds for iw416, rtl8822cs, rtl8822bs and bcm4354" >&2; exit 2 ;;
esac
: "${TOP:=$HOME/LocalDev/imx6ul-$RADIO-kernel}"
source "$HERE/../common.sh"

# The vendor U-Boot reads a fixed number of kernel bytes, too few for the full busybox next to the
# kernel, hostapd and the Wi-Fi firmware, so the initramfs gets a busybox with only what the two
# ways in (USB NCM, the Wi-Fi AP), the switch to the rootfs and the provisioner use.
RESCUE_APPLETS="
  LFS BUSYBOX ASH SH_IS_ASH ASH_JOB_CONTROL ASH_ECHO ASH_PRINTF ASH_TEST ASH_CMDCMD ASH_OPTIMIZE_FOR_SIZE
  FEATURE_SH_MATH FEATURE_EDITING FEATURE_TAB_COMPLETION FEATURE_INSTALLER FEATURE_DEVPTS
  CTTYHACK SETSID STTY SWITCH_ROOT
  CAT CHMOD CP CUT DATE DD DF DIRNAME BASENAME DU ECHO ENV FALSE TRUE HEAD TAIL LN LS MKDIR MV
  OD PRINTF RM SEQ SLEEP SYNC TEST TEST1 TEST2 TOUCH TR UNAME UPTIME WC MD5SUM SHA256SUM
  GREP SED AWK HEXDUMP XXD VI
  PS KILL KILLALL PKILL PGREP PIDOF FREE
  MOUNT UMOUNT DMESG FLASHCP FLASH_ERASEALL DEVMEM HALT REBOOT POWEROFF RX
  IFCONFIG FEATURE_IFCONFIG_STATUS BRCTL FEATURE_BRCTL_FANCY FEATURE_BRCTL_SHOW NC NC_SERVER TELNETD
  FEATURE_TELNETD_STANDALONE UDHCPD PING
"

# The IW416 build came first and keeps the names dongles in the field update from.
case $RADIO in
  iw416) NAME=livi-link-imx6ull ;;
  *) NAME=livi-link-imx6ull-$RADIO ;;
esac
KERNEL_IMG=$OUT/$NAME.zimg
# How many bytes the vendor U-Boot reads on the units we know. The provisioner checks each
# dongle's own value before it writes anything.
KERNEL_MAX=$((0x302fd8))
BUSYBOX=$USERSPACE/bin/busybox
HOSTAPD=$USERSPACE/usr/sbin/hostapd
LIVID=$OUT/livid
ROOTFS_IMG=$OUT/$NAME-rootfs.bin
ROOTFS_SIZE=$((0xc60000))  # "rootfs" partition in imx6ull.dts
# The MTD index in imx6ull.dts: 2 = kernel (the provisioner stages it for the vendor U-Boot), 3 = rootfs.
BUNDLE=$OUT/$NAME.lfwb
BUNDLE_IMAGES=("2:$KERNEL_IMG" "3:$ROOTFS_IMG")

# Firmware as "<path in linux-firmware or URL> <sha256> [<tag> [<path under /lib/firmware>]]", tag "-"
# for none. The Wi-Fi driver is built into the kernel and loads its part from the initramfs, the
# Bluetooth modules theirs from the rootfs.
LINUX_FIRMWARE=https://git.kernel.org/pub/scm/linux/kernel/git/firmware/linux-firmware.git/plain
ARMBIAN_FIRMWARE=https://raw.githubusercontent.com/armbian/firmware/2e568bc338825be5b36eb888aacf473bb8ffd472
case $RADIO in
  iw416)
    # One combo firmware runs both halves: mwifiex loads it, btnxpuart only attaches. It is in no
    # tagged tree under this path, so pinned by its hash alone.
    WIFI_FW=("mrvl/sdiouartiw416_combo_v0.bin afcca1b8c240a97b9c452ef8023a829c1942127072907efe34c25bfabb2ba7f9")
    BT_FW=()
    ;;
  rtl8822cs)
    WIFI_FW=("rtw88/rtw8822c_fw.bin 3deecb31210986d98cdbfb000391e08d602a6eee4ffc883969faa2b907ab03ba 20260916")
    BT_FW=(
      "rtl_bt/rtl8822cs_fw.bin 42db5218c54b0638e1bafdbc7d0986172288e55a9e1c879080d41206bb87dfa2 20260916"
      "rtl_bt/rtl8822cs_config.bin dbdc0a455f628337509afbdb6e1fb42e1622e3e876fc2aa962cc1b754709131c 20260916"
    )
    ;;
  rtl8822bs)
    WIFI_FW=("rtw88/rtw8822b_fw.bin a72da690597bfa99d8eb6fc2ab090d18d8ad92ac2befd35db1c9e3662d8d8418 20260916")
    # btrtl knows the 8822B over USB only, its Bluetooth half over UART stays off for now.
    BT_FW=()
    ;;
  bcm4354)
    # brcmfmac asks for the brcm/ names, linux-firmware has the files under cypress/ and links them.
    # The board's NVRAM is the AP6354 one, the module these dongles carry.
    WIFI_FW=(
      "cypress/cyfmac4354-sdio.bin f6b4164c50c4406573f3671d50c2d19b370ed079035a7d76a38535e244ae41d8 20260916 brcm/brcmfmac4354-sdio.bin"
      "cypress/cyfmac4354-sdio.clm_blob a72cbbaea17415d99be5c747d719cb22e6787471d047cf72fd66f9e07ab12dbd 20260916 brcm/brcmfmac4354-sdio.clm_blob"
      "$ARMBIAN_FIRMWARE/rkwifi/nvram_ap6354.txt 4a30289f4733db12fdc88c7c885dfb1dc3a695c5361aa7c79f91d731a5f32804 - brcm/brcmfmac4354-sdio.txt"
    )
    BT_FW=()
    ;;
esac
FIRMWARE=$TOP/firmware
# cfg80211 allows no access point on 5 GHz without a regulatory database.
REGDB_VER=2026.09.03
REGDB_SHA=b22e0901227b820cd1c280abe681a15b773a5103a5e10dc442e94ebb34cbf58d
REGDB=$TOP/wireless-regdb-$REGDB_VER/regulatory.db

fetch_firmware() {
  local entry path sha tag as url
  for entry in "${WIFI_FW[@]}" "${BT_FW[@]}"; do
    read -r path sha tag as <<< "$entry"
    [[ $tag == - ]] && tag=
    as=${as:-$path}
    echo "$sha  $FIRMWARE/$as" | sha256sum -c --status 2>/dev/null && continue
    case $path in
      https://*) url=$path ;;
      *) url=$LINUX_FIRMWARE/$path${tag:+?h=$tag} ;;
    esac
    log "fetch $as"
    mkdir -p "$(dirname "$FIRMWARE/$as")"
    curl -sSfL "$url" -o "$FIRMWARE/$as"
    echo "$sha  $FIRMWARE/$as" | sha256sum -c - || { log "$as changed upstream, check it and update its hash"; exit 3; }
  done
  if [[ ! -f $REGDB ]]; then
    log "fetch wireless-regdb $REGDB_VER"
    curl -sSL "https://cdn.kernel.org/pub/software/network/wireless-regdb/wireless-regdb-$REGDB_VER.tar.xz" \
      -o "$TOP/wireless-regdb-$REGDB_VER.tar.xz"
    echo "$REGDB_SHA  $TOP/wireless-regdb-$REGDB_VER.tar.xz" | sha256sum -c -
    tar -xJf "$TOP/wireless-regdb-$REGDB_VER.tar.xz" -C "$TOP"
  fi
}

# gen_init_cpio lines for the Wi-Fi firmware and the directories it sits in.
wifi_firmware_initramfs() {
  local entry path sha tag as sub part dir made=" "
  for entry in "${WIFI_FW[@]}"; do
    read -r path sha tag as <<< "$entry"
    path=${as:-$path}
    sub=$(dirname "$path")
    dir=/lib/firmware
    if [[ $sub != . ]]; then
      for part in ${sub//\// }; do
        dir=$dir/$part
        [[ $made == *" $dir "* ]] || { echo "dir $dir 0755 0 0"; made+="$dir "; }
      done
    fi
    echo "file /lib/firmware/$path $FIRMWARE/$path 0644 0 0"
  done
}

rootfs_payload() {
  local work=$1 entry path sha tag as
  need "$OUT/modules/load" "run build.sh first"
  fetch_firmware
  log "$RADIO firmware + regulatory.db"
  for entry in "${WIFI_FW[@]}" "${BT_FW[@]}"; do
    read -r path sha tag as <<< "$entry"
    path=${as:-$path}
    mkdir -p "$(dirname "$work/lib/firmware/$path")"
    cp "$FIRMWARE/$path" "$work/lib/firmware/$path"
  done
  cp "$REGDB" "$work/lib/firmware/regulatory.db"

  log "kernel modules (Bluetooth and the crypto it selects)"
  mkdir -p "$work/lib/modules/$(krelease)"
  cp "$OUT/modules"/* "$work/lib/modules/$(krelease)/"
}
