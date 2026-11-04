// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

package dev.fio.livi;

import android.Manifest;
import android.annotation.SuppressLint;
import android.app.Activity;
import android.bluetooth.BluetoothAdapter;
import android.bluetooth.BluetoothManager;
import android.bluetooth.BluetoothServerSocket;
import android.bluetooth.BluetoothSocket;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.os.Build;
import android.os.ParcelFileDescriptor;
import android.os.ParcelUuid;
import android.provider.Settings;
import android.system.ErrnoException;
import android.system.Os;
import android.system.OsConstants;
import android.util.Log;
import java.io.Closeable;
import java.io.FileDescriptor;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.Locale;
import java.util.Map;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.atomic.AtomicBoolean;

@SuppressLint("MissingPermission")
final class LiviBluetooth {
    private static final String TAG = "livi";
    private static final String NAME = "LIVI";
    private static final UUID IAP = UUID.fromString("00000000-deca-fade-deca-deafdecacaff");
    private static final UUID CARPLAY = UUID.fromString("ec884348-cd41-40a2-9727-575d50bf1fd3");
    private static final UUID HANDS_FREE = UUID.fromString("0000111e-0000-1000-8000-00805f9b34fb");
    private static final UUID AUDIO_GATEWAY = UUID.fromString("0000111f-0000-1000-8000-00805f9b34fb");
    private static final String UNKNOWN_ADDRESS = "02:00:00:00:00:00";
    private static final UUID ANDROID_AUTO = UUID.fromString("4de17a00-52cb-11e6-bdf4-0800200c9a66");
    private static final int CLOSED = -1;
    private static final int IAP_LINK = 0;
    private static final int HFP_LINK = 1;
    private static final int AA_LINK = 2;
    private static final int BUFFER = 4096;
    private static final int DISCOVERABLE_S = 3600;

    static final String[] PERMISSIONS = Build.VERSION.SDK_INT >= Build.VERSION_CODES.S
            ? new String[] {
                Manifest.permission.BLUETOOTH_CONNECT,
                Manifest.permission.BLUETOOTH_ADVERTISE,
                Manifest.permission.BLUETOOTH_SCAN
            }
            : new String[0];

    private static final Map<String, BluetoothSocket> LINKS = new ConcurrentHashMap<>();

    private static boolean listening;
    private static boolean handsFree;
    private static BluetoothAdapter adapter;

    private LiviBluetooth() {}

    private static native void phone(int fd, String peer, int service);

    static synchronized boolean listen(Activity activity, String name, boolean carplay, boolean androidAuto) {
        if (listening) {
            return true;
        }
        try {
            if (!LiviVpn.awaited(() -> permitted(activity))) {
                Log.w(TAG, "[bt] no Bluetooth permission");
                return false;
            }
            BluetoothManager manager = activity.getSystemService(BluetoothManager.class);
            BluetoothAdapter adapter = manager == null ? null : manager.getAdapter();
            if (adapter == null) {
                Log.w(TAG, "[bt] no Bluetooth adapter");
                return false;
            }
            if (!enabled(activity, adapter)) {
                Log.w(TAG, "[bt] Bluetooth stayed off");
                return false;
            }
            if (!name.equals(adapter.getName()) && !adapter.setName(name)) {
                Log.w(TAG, "[bt] Android kept the name " + adapter.getName());
            }
            LiviBluetooth.adapter = adapter;
            if (carplay) {
                serve(adapter.listenUsingRfcommWithServiceRecord(NAME, IAP), IAP_LINK);
                serve(adapter.listenUsingRfcommWithServiceRecord(NAME, CARPLAY), CLOSED);
            }
            handsFree = offersHandsFree(adapter);
            if (androidAuto && handsFree) {
                Log.i(TAG, "[bt] Android's own Hands-Free takes the calls");
            } else if (androidAuto) {
                serve(adapter.listenUsingRfcommWithServiceRecord("Hands-Free Unit", HANDS_FREE), HFP_LINK);
            }
            if (androidAuto) {
                serve(adapter.listenUsingRfcommWithServiceRecord("AndroidAuto", ANDROID_AUTO), AA_LINK);
            }
            if (adapter.getScanMode() != BluetoothAdapter.SCAN_MODE_CONNECTABLE_DISCOVERABLE) {
                activity.runOnUiThread(() -> activity.startActivity(
                        new Intent(BluetoothAdapter.ACTION_REQUEST_DISCOVERABLE)
                                .putExtra(BluetoothAdapter.EXTRA_DISCOVERABLE_DURATION, DISCOVERABLE_S)));
            }
            listening = true;
            return true;
        } catch (Exception e) {
            Log.w(TAG, "[bt] " + e);
            return false;
        }
    }

    static boolean drop(String peer) {
        String prefix = peer.toUpperCase(Locale.ROOT) + "/";
        boolean dropped = false;
        for (String link : LINKS.keySet()) {
            if (link.startsWith(prefix)) {
                BluetoothSocket socket = LINKS.remove(link);
                if (socket != null) {
                    close(socket);
                    dropped = true;
                }
            }
        }
        return dropped;
    }

    static String page(String peer, String uuid) {
        BluetoothAdapter bt = adapter;
        if (bt == null) {
            return "Bluetooth is not listening";
        }
        BluetoothSocket socket = null;
        try {
            UUID service = UUID.fromString(uuid);
            int kind = AUDIO_GATEWAY.equals(service) ? HFP_LINK : IAP_LINK;
            if (kind == HFP_LINK && handsFree) {
                return "Android's own Hands-Free connects this phone";
            }
            if (LINKS.containsKey(link(peer, kind))) {
                return null;
            }
            socket = bt.getRemoteDevice(peer).createRfcommSocketToServiceRecord(service);
            socket.connect();
            bridge(socket, peer, kind);
            return null;
        } catch (IOException | IllegalArgumentException e) {
            if (socket != null) {
                close(socket);
            }
            return e.toString();
        }
    }

    static String address(Context context) {
        BluetoothManager manager = context.getSystemService(BluetoothManager.class);
        BluetoothAdapter bt = manager == null ? null : manager.getAdapter();
        try {
            String own = bt == null ? null : bt.getAddress();
            if (own != null && !UNKNOWN_ADDRESS.equals(own)) {
                return own;
            }
        } catch (SecurityException e) {
            Log.w(TAG, "[bt] the adapter keeps its address: " + e);
        }
        try {
            String setting = Settings.Secure.getString(context.getContentResolver(), "bluetooth_address");
            return setting == null ? "" : setting;
        } catch (SecurityException e) {
            Log.w(TAG, "[bt] the settings keep the address: " + e);
            return "";
        }
    }

    private static boolean offersHandsFree(BluetoothAdapter adapter) {
        try {
            Object uuids = adapter.getClass().getMethod("getUuids").invoke(adapter);
            if (uuids instanceof ParcelUuid[]) {
                for (ParcelUuid uuid : (ParcelUuid[]) uuids) {
                    if (HANDS_FREE.equals(uuid.getUuid())) {
                        return true;
                    }
                }
            }
        } catch (ReflectiveOperationException | RuntimeException e) {
            Log.w(TAG, "[bt] could not read the adapter's services: " + e);
        }
        return false;
    }

    private static boolean permitted(Activity activity) {
        for (String permission : PERMISSIONS) {
            if (activity.checkSelfPermission(permission) != PackageManager.PERMISSION_GRANTED) {
                return false;
            }
        }
        return true;
    }

    private static boolean enabled(Activity activity, BluetoothAdapter adapter) throws InterruptedException {
        if (adapter.isEnabled()) {
            return true;
        }
        activity.runOnUiThread(() -> activity.startActivity(new Intent(BluetoothAdapter.ACTION_REQUEST_ENABLE)));
        return LiviVpn.awaited(adapter::isEnabled);
    }

    private static String link(String peer, int kind) {
        return peer + "/" + kind;
    }

    private static void serve(BluetoothServerSocket server, int kind) {
        Thread accepting = new Thread(() -> {
            while (true) {
                BluetoothSocket socket;
                try {
                    socket = server.accept();
                } catch (IOException e) {
                    Log.w(TAG, "[bt] stopped listening: " + e);
                    return;
                }
                String peer = socket.getRemoteDevice().getAddress();
                if (kind == CLOSED) {
                    Log.i(TAG, "[bt] " + peer + " opened the CarPlay record, closing it");
                    close(socket);
                    continue;
                }
                try {
                    bridge(socket, peer, kind);
                } catch (IOException e) {
                    Log.w(TAG, "[bt] " + peer + ": " + e);
                    close(socket);
                }
            }
        }, "bt-accept");
        accepting.setDaemon(true);
        accepting.start();
    }

    private static void bridge(BluetoothSocket socket, String peer, int kind) throws IOException {
        String link = link(peer, kind);
        ParcelFileDescriptor[] pair = ParcelFileDescriptor.createSocketPair();
        ParcelFileDescriptor ours = pair[0];
        FileDescriptor fd = ours.getFileDescriptor();
        AtomicBoolean ended = new AtomicBoolean();
        Runnable end = () -> {
            if (ended.compareAndSet(false, true)) {
                LINKS.remove(link, socket);
                close(socket);
                try {
                    Os.shutdown(fd, OsConstants.SHUT_RDWR);
                } catch (ErrnoException ignored) {
                }
                close(ours);
            }
        };
        LINKS.put(link, socket);
        pump(socket.getInputStream(), new FileOutputStream(fd), end, "bt-in");
        pump(new FileInputStream(fd), socket.getOutputStream(), end, "bt-out");
        phone(pair[1].detachFd(), peer, kind);
    }

    private static void pump(InputStream in, OutputStream out, Runnable end, String name) {
        Thread pumping = new Thread(() -> {
            byte[] buffer = new byte[BUFFER];
            try {
                int n;
                while ((n = in.read(buffer)) > 0) {
                    out.write(buffer, 0, n);
                    out.flush();
                }
            } catch (IOException ignored) {
            } finally {
                end.run();
            }
        }, name);
        pumping.setDaemon(true);
        pumping.start();
    }

    private static void close(Closeable closeable) {
        try {
            closeable.close();
        } catch (IOException ignored) {
        }
    }
}
