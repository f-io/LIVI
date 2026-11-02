// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

package dev.fio.livi;

import android.app.Activity;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.net.VpnService;
import android.os.Build;
import android.os.ParcelFileDescriptor;
import android.system.OsConstants;
import android.util.Log;
import java.net.InetAddress;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.TimeUnit;
import java.util.function.BooleanSupplier;

public class LiviVpn extends VpnService {
    private static final String TAG = "livi";
    private static final long CONSENT_WAIT_MS = 120_000;
    private static final long CONSENT_POLL_MS = 500;
    private static final long START_WAIT_S = 10;
    private static final int CONSENT_REQUEST = 1;
    private static final String LOCAL_NETWORK = "android.permission.ACCESS_LOCAL_NETWORK";

    private static volatile CompletableFuture<LiviVpn> running = new CompletableFuture<>();

    @Override
    public void onCreate() {
        super.onCreate();
        running.complete(this);
    }

    @Override
    public void onDestroy() {
        running = new CompletableFuture<>();
        super.onDestroy();
    }

    static int open(Activity activity, String address, int prefix, int mtu) {
        try {
            if (!consented(activity)) {
                Log.w(TAG, "[vpn] no consent for the USB network");
                return -1;
            }
            if (!localNetwork(activity)) {
                Log.w(TAG, "[vpn] no local network access for the USB network");
                return -1;
            }
            activity.startService(new Intent(activity, LiviVpn.class));
            LiviVpn vpn = running.get(START_WAIT_S, TimeUnit.SECONDS);
            ParcelFileDescriptor tun = vpn.new Builder()
                    .setSession("LIVI")
                    .addAddress(InetAddress.getByName(address), prefix)
                    .setMtu(mtu)
                    .allowFamily(OsConstants.AF_INET)
                    .addAllowedApplication(activity.getPackageName())
                    .setMetered(false)
                    .establish();
            if (tun == null) {
                Log.w(TAG, "[vpn] the system did not open the interface");
                return -1;
            }
            return tun.detachFd();
        } catch (Exception e) {
            Log.w(TAG, "[vpn] " + e);
            return -1;
        }
    }

    private static boolean consented(Activity activity) throws InterruptedException {
        Intent consent = prepare(activity);
        if (consent == null) {
            return true;
        }
        activity.runOnUiThread(() -> activity.startActivityForResult(consent, CONSENT_REQUEST));
        return awaited(() -> prepare(activity) == null);
    }

    private static boolean localNetwork(Activity activity) throws InterruptedException {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.CINNAMON_BUN) {
            return true;
        }
        BooleanSupplier granted =
                () -> activity.checkSelfPermission(LOCAL_NETWORK) == PackageManager.PERMISSION_GRANTED;
        if (granted.getAsBoolean()) {
            return true;
        }
        activity.runOnUiThread(() -> activity.requestPermissions(new String[] {LOCAL_NETWORK}, CONSENT_REQUEST));
        return awaited(granted);
    }

    private static boolean awaited(BooleanSupplier done) throws InterruptedException {
        long deadline = System.currentTimeMillis() + CONSENT_WAIT_MS;
        while (!done.getAsBoolean()) {
            if (System.currentTimeMillis() > deadline) {
                return false;
            }
            Thread.sleep(CONSENT_POLL_MS);
        }
        return true;
    }
}
