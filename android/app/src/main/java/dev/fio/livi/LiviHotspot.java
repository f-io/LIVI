// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

package dev.fio.livi;

import android.Manifest;
import android.annotation.SuppressLint;
import android.app.Activity;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.content.pm.PackageManager;
import android.net.wifi.WifiManager;
import android.net.wifi.p2p.WifiP2pConfig;
import android.net.wifi.p2p.WifiP2pGroup;
import android.net.wifi.p2p.WifiP2pInfo;
import android.net.wifi.p2p.WifiP2pManager;
import android.os.Build;
import android.os.HandlerThread;
import android.provider.Settings;
import android.util.Log;
import java.net.NetworkInterface;
import java.net.SocketException;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.TimeUnit;
import java.util.function.Consumer;

@SuppressLint("MissingPermission")
final class LiviHotspot {
    private static final String TAG = "livi";
    private static final String PREFIX = "DIRECT-";
    private static final int DONE = -1;
    private static final int TRIES = 5;
    private static final long CALL_WAIT_S = 10;
    private static final long GROUP_WAIT_MS = 10_000;
    private static final long POLL_MS = 250;

    static final String[] PERMISSIONS = Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU
            ? new String[] {Manifest.permission.NEARBY_WIFI_DEVICES}
            : new String[] {Manifest.permission.ACCESS_COARSE_LOCATION, Manifest.permission.ACCESS_FINE_LOCATION};

    private static WifiP2pManager manager;
    private static WifiP2pManager.Channel channel;
    private static volatile String[] granted;

    private LiviHotspot() {}

    private static native void stopped();

    static synchronized String[] start(Activity activity, String name, String passphrase, int frequency) {
        if (granted != null) {
            return granted;
        }
        try {
            if (!LiviVpn.awaited(() -> permitted(activity))) {
                Log.w(TAG, "[ap] no permission for nearby Wi-Fi devices");
                return null;
            }
            if (!enabled(activity, activity.getSystemService(WifiManager.class))) {
                Log.w(TAG, "[ap] Wi-Fi stayed off");
                return null;
            }
            if (manager == null) {
                manager = activity.getSystemService(WifiP2pManager.class);
                if (manager == null) {
                    Log.w(TAG, "[ap] this device has no Wi-Fi Direct");
                    return null;
                }
                HandlerThread thread = new HandlerThread("livi-p2p");
                thread.start();
                channel = manager.initialize(activity, thread.getLooper(), LiviHotspot::gone);
                watch(activity);
            }
            call(done -> manager.removeGroup(channel, done));
            String network = PREFIX + name;
            int refused = create(new WifiP2pConfig.Builder()
                    .setNetworkName(network)
                    .setPassphrase(passphrase)
                    .setGroupOperatingFrequency(frequency)
                    .build());
            if (refused != DONE) {
                Log.w(TAG, "[ap] " + network + " at " + frequency + " MHz refused with reason " + refused
                        + ", trying the band");
                refused = create(new WifiP2pConfig.Builder()
                        .setNetworkName(network)
                        .setPassphrase(passphrase)
                        .setGroupOperatingBand(frequency >= 5000
                                ? WifiP2pConfig.GROUP_OWNER_BAND_5GHZ
                                : WifiP2pConfig.GROUP_OWNER_BAND_2GHZ)
                        .build());
            }
            if (refused != DONE) {
                Log.w(TAG, "[ap] " + network + " refused with reason " + refused);
                return null;
            }
            WifiP2pGroup group = formed();
            if (group == null) {
                Log.w(TAG, "[ap] " + network + " did not come up");
                return null;
            }
            String iface = group.getInterface();
            if (!addressed(iface)) {
                Log.w(TAG, "[ap] " + group.getNetworkName() + " is up, but " + iface + " has no address");
                return null;
            }
            Log.i(TAG, "[ap] " + group.getNetworkName() + " is up on " + iface + " at "
                    + group.getFrequency() + " MHz");
            granted = new String[] {
                iface, group.getNetworkName(), group.getPassphrase(), Integer.toString(group.getFrequency())
            };
            return granted;
        } catch (Exception e) {
            Log.w(TAG, "[ap] " + e);
            return null;
        }
    }

    private static void gone() {
        String[] was = granted;
        if (was == null) {
            return;
        }
        granted = null;
        Log.w(TAG, "[ap] " + was[1] + " stopped");
        stopped();
    }

    private static void watch(Activity activity) {
        BroadcastReceiver receiver = new BroadcastReceiver() {
            @Override
            public void onReceive(Context context, Intent intent) {
                WifiP2pInfo info = Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU
                        ? intent.getParcelableExtra(WifiP2pManager.EXTRA_WIFI_P2P_INFO, WifiP2pInfo.class)
                        : intent.getParcelableExtra(WifiP2pManager.EXTRA_WIFI_P2P_INFO);
                if (info != null && !info.groupFormed) {
                    manager.requestGroupInfo(channel, group -> {
                        if (group == null) {
                            gone();
                        }
                    });
                }
            }
        };
        IntentFilter filter = new IntentFilter(WifiP2pManager.WIFI_P2P_CONNECTION_CHANGED_ACTION);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            activity.registerReceiver(receiver, filter, Context.RECEIVER_NOT_EXPORTED);
        } else {
            activity.registerReceiver(receiver, filter);
        }
    }

    private static int create(WifiP2pConfig config) throws Exception {
        int refused = DONE;
        for (int i = 0; i < TRIES; i++) {
            refused = call(done -> manager.createGroup(channel, config, done));
            if (refused != WifiP2pManager.BUSY) {
                return refused;
            }
            Thread.sleep(GROUP_WAIT_MS / TRIES);
        }
        return refused;
    }

    private static int call(Consumer<WifiP2pManager.ActionListener> action) throws Exception {
        CompletableFuture<Integer> result = new CompletableFuture<>();
        action.accept(new WifiP2pManager.ActionListener() {
            @Override
            public void onSuccess() {
                result.complete(DONE);
            }

            @Override
            public void onFailure(int reason) {
                result.complete(reason);
            }
        });
        return result.get(CALL_WAIT_S, TimeUnit.SECONDS);
    }

    private static WifiP2pGroup formed() throws Exception {
        long deadline = System.currentTimeMillis() + GROUP_WAIT_MS;
        while (System.currentTimeMillis() < deadline) {
            CompletableFuture<WifiP2pGroup> info = new CompletableFuture<>();
            manager.requestGroupInfo(channel, info::complete);
            WifiP2pGroup group = info.get(CALL_WAIT_S, TimeUnit.SECONDS);
            if (group != null && group.isGroupOwner() && group.getInterface() != null && group.getFrequency() > 0) {
                return group;
            }
            Thread.sleep(POLL_MS);
        }
        return null;
    }

    private static boolean addressed(String iface) throws InterruptedException {
        long deadline = System.currentTimeMillis() + GROUP_WAIT_MS;
        while (System.currentTimeMillis() < deadline) {
            try {
                NetworkInterface net = NetworkInterface.getByName(iface);
                if (net != null && net.getInetAddresses().hasMoreElements()) {
                    return true;
                }
            } catch (SocketException e) {
                Log.w(TAG, "[ap] " + e);
            }
            Thread.sleep(POLL_MS);
        }
        return false;
    }

    private static boolean enabled(Activity activity, WifiManager wifi) throws InterruptedException {
        if (wifi.isWifiEnabled()) {
            return true;
        }
        activity.runOnUiThread(() -> activity.startActivity(new Intent(Settings.Panel.ACTION_WIFI)));
        return LiviVpn.awaited(wifi::isWifiEnabled);
    }

    private static boolean permitted(Activity activity) {
        for (String permission : PERMISSIONS) {
            if (activity.checkSelfPermission(permission) != PackageManager.PERMISSION_GRANTED) {
                return false;
            }
        }
        return true;
    }
}
