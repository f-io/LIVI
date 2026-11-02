// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

package dev.fio.livi;

import android.Manifest;
import android.app.NativeActivity;
import android.app.PendingIntent;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.content.pm.PackageInstaller;
import android.graphics.PixelFormat;
import android.graphics.Rect;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.FileUtils;
import android.util.DisplayMetrics;
import android.util.Log;
import android.util.SparseArray;
import android.view.Display;
import android.view.KeyEvent;
import android.view.Surface;
import android.view.SurfaceControl;
import android.view.SurfaceHolder;
import android.view.SurfaceView;
import android.view.View;
import android.view.ViewGroup;
import android.view.WindowInsets;
import android.view.WindowInsetsController;
import android.widget.FrameLayout;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.Objects;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.atomic.AtomicInteger;
import org.freedesktop.gstreamer.GStreamer;

public class LiviActivity extends NativeActivity {
    private static final float MM_PER_INCH = 25.4f;
    private static final int PERMISSIONS_REQUEST = 2;
    private static final int PICK_REQUEST = 3;
    private static final String INSTALLED = "dev.fio.livi.INSTALLED";

    private final SparseArray<SurfaceView> planes = new SparseArray<>();
    private final AtomicInteger lastPlane = new AtomicInteger();
    private FrameLayout stage;

    static {
        System.loadLibrary("gstreamer_android");
        System.loadLibrary("livi");
    }

    private static native void key(int scancode, boolean down, boolean repeat);

    private static native void surface(int plane, Surface surface);

    private static native void screen(int widthPx, int heightPx, float widthMm, float heightMm);

    private static native void picked(String path);

    @Override
    protected void onCreate(Bundle state) {
        try {
            GStreamer.init(this);
        } catch (Exception e) {
            Log.w("livi", "[gst] " + e);
        }
        Display display = Build.VERSION.SDK_INT >= Build.VERSION_CODES.R
                ? getDisplay()
                : getWindowManager().getDefaultDisplay();
        Display.Mode mode = display.getMode();
        DisplayMetrics metrics = getResources().getDisplayMetrics();
        int width = mode.getPhysicalWidth();
        int height = mode.getPhysicalHeight();
        screen(width, height, width / metrics.xdpi * MM_PER_INCH, height / metrics.ydpi * MM_PER_INCH);
        super.onCreate(state);
        getWindow().setFormat(PixelFormat.TRANSLUCENT);
        requestPermissions(new String[] {Manifest.permission.RECORD_AUDIO}, PERMISSIONS_REQUEST);
        stage = new FrameLayout(this);
        stage.addOnLayoutChangeListener((v, l, t, r, b, ol, ot, or, ob) -> {
            for (int i = 0; i < planes.size(); i++) {
                place(planes.valueAt(i));
            }
        });
        addContentView(stage, new ViewGroup.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
    }

    private int plane(int rank) {
        int id = lastPlane.incrementAndGet();
        runOnUiThread(() -> {
            SurfaceView view = new SurfaceView(this);
            view.setTag(new double[] {rank, 0, 0, 0, 0, 0, 0});
            view.setZOrderMediaOverlay(rank > 0);
            view.getHolder().addCallback(new SurfaceHolder.Callback() {
                @Override
                public void surfaceCreated(SurfaceHolder holder) {
                    reveal(view);
                }

                @Override
                public void surfaceChanged(SurfaceHolder holder, int format, int width, int height) {
                    surface(id, holder.getSurface());
                }

                @Override
                public void surfaceDestroyed(SurfaceHolder holder) {
                    surface(id, null);
                }
            });
            int at = 0;
            while (at < stage.getChildCount() && ((double[]) stage.getChildAt(at).getTag())[0] <= rank) {
                at++;
            }
            stage.addView(view, at, new FrameLayout.LayoutParams(
                    ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
            planes.put(id, view);
        });
        return id;
    }

    private void planeShown(int id, boolean shown) {
        runOnUiThread(() -> {
            SurfaceView view = planes.get(id);
            if (view != null) {
                view.setAlpha(shown ? 1f : 0f);
                reveal(view);
            }
        });
    }

    private static void reveal(SurfaceView view) {
        SurfaceControl control = view.getSurfaceControl();
        if (control != null && control.isValid()) {
            new SurfaceControl.Transaction().setAlpha(control, view.getAlpha()).apply();
        }
    }

    private void planePlace(int id, double cropL, double cropT, double visW, double visH, double tierW, double tierH) {
        runOnUiThread(() -> {
            SurfaceView view = planes.get(id);
            if (view == null) {
                return;
            }
            double[] at = (double[]) view.getTag();
            at[1] = cropL;
            at[2] = cropT;
            at[3] = visW;
            at[4] = visH;
            at[5] = tierW;
            at[6] = tierH;
            place(view);
        });
    }

    private void planeGone(int id) {
        runOnUiThread(() -> {
            SurfaceView view = planes.get(id);
            if (view != null) {
                planes.remove(id);
                stage.removeView(view);
            }
        });
    }

    private void place(SurfaceView view) {
        double[] at = (double[]) view.getTag();
        double ww = stage.getWidth();
        double wh = stage.getHeight();
        FrameLayout.LayoutParams p = (FrameLayout.LayoutParams) view.getLayoutParams();
        int width = ViewGroup.LayoutParams.MATCH_PARENT;
        int height = ViewGroup.LayoutParams.MATCH_PARENT;
        int left = 0;
        int top = 0;
        Rect clip = null;
        if (ww > 0 && wh > 0 && at[3] > 0 && at[4] > 0 && at[5] > 0 && at[6] > 0) {
            double scale = Math.min(ww / at[3], wh / at[4]);
            width = (int) Math.round(at[5] * scale);
            height = (int) Math.round(at[6] * scale);
            left = (int) Math.round((ww - at[3] * scale) / 2 - at[1] * scale);
            top = (int) Math.round((wh - at[4] * scale) / 2 - at[2] * scale);
            clip = new Rect(
                    (int) Math.round(at[1] * scale),
                    (int) Math.round(at[2] * scale),
                    (int) Math.round((at[1] + at[3]) * scale),
                    (int) Math.round((at[2] + at[4]) * scale));
        }
        if (!Objects.equals(clip, view.getClipBounds())) {
            view.setClipBounds(clip);
        }
        if (p.width != width || p.height != height || p.leftMargin != left || p.topMargin != top) {
            p.width = width;
            p.height = height;
            p.leftMargin = left;
            p.topMargin = top;
            view.setLayoutParams(p);
        }
    }

    private void kiosk(boolean on) {
        runOnUiThread(() -> {
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) {
                getWindow().getDecorView().setSystemUiVisibility(on
                        ? View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
                                | View.SYSTEM_UI_FLAG_FULLSCREEN
                                | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                        : 0);
                return;
            }
            WindowInsetsController bars = getWindow().getInsetsController();
            bars.setSystemBarsBehavior(WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
            if (on) {
                bars.hide(WindowInsets.Type.systemBars());
            } else {
                bars.show(WindowInsets.Type.systemBars());
            }
        });
    }

    private void exit(boolean restart) {
        runOnUiThread(() -> {
            if (restart) {
                startActivity(Intent.makeRestartActivityTask(getComponentName()));
            }
            finishAndRemoveTask();
            Runtime.getRuntime().exit(0);
        });
    }

    private String install(String path) {
        CompletableFuture<String> result = new CompletableFuture<>();
        BroadcastReceiver receiver = new BroadcastReceiver() {
            @Override
            public void onReceive(Context context, Intent intent) {
                int status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE);
                if (status == PackageInstaller.STATUS_PENDING_USER_ACTION) {
                    startActivity(Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU
                            ? intent.getParcelableExtra(Intent.EXTRA_INTENT, Intent.class)
                            : intent.getParcelableExtra(Intent.EXTRA_INTENT));
                } else if (status == PackageInstaller.STATUS_SUCCESS) {
                    result.complete(null);
                } else {
                    String message = intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE);
                    result.complete(message != null ? message : "install failed with status " + status);
                }
            }
        };
        registerReceiver(receiver, new IntentFilter(INSTALLED), RECEIVER_NOT_EXPORTED);
        try {
            PackageInstaller installer = getPackageManager().getPackageInstaller();
            PackageInstaller.SessionParams params =
                    new PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL);
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                params.setRequireUserAction(PackageInstaller.SessionParams.USER_ACTION_NOT_REQUIRED);
            }
            int id = installer.createSession(params);
            try (PackageInstaller.Session session = installer.openSession(id)) {
                File apk = new File(path);
                try (InputStream in = new FileInputStream(apk);
                        OutputStream out = session.openWrite("livi.apk", 0, apk.length())) {
                    FileUtils.copy(in, out);
                    session.fsync(out);
                }
                Intent done = new Intent(INSTALLED).setPackage(getPackageName());
                session.commit(PendingIntent.getBroadcast(
                                this, id, done, PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_MUTABLE)
                        .getIntentSender());
            }
            return result.get();
        } catch (IOException | ExecutionException e) {
            return e.toString();
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            return e.toString();
        } finally {
            unregisterReceiver(receiver);
        }
    }

    private void pick() {
        runOnUiThread(() -> startActivityForResult(
                new Intent(Intent.ACTION_OPEN_DOCUMENT)
                        .addCategory(Intent.CATEGORY_OPENABLE)
                        .setType("*/*"),
                PICK_REQUEST));
    }

    @Override
    protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (request == PICK_REQUEST && result == RESULT_OK && data != null && data.getData() != null) {
            Uri uri = data.getData();
            new Thread(() -> copy(uri)).start();
        }
    }

    private void copy(Uri uri) {
        File picked = new File(getCacheDir(), "picked");
        try (InputStream in = getContentResolver().openInputStream(uri);
                OutputStream out = new FileOutputStream(picked)) {
            if (in == null) {
                throw new IOException("nothing to read at " + uri);
            }
            FileUtils.copy(in, out);
        } catch (IOException e) {
            Log.w("livi", "[pick] " + e);
            return;
        }
        picked(picked.getPath());
    }

    private int tun(String address, int prefix, int mtu) {
        return LiviVpn.open(this, address, prefix, mtu);
    }

    private static boolean system(int keyCode) {
        return keyCode == KeyEvent.KEYCODE_VOLUME_UP
                || keyCode == KeyEvent.KEYCODE_VOLUME_DOWN
                || keyCode == KeyEvent.KEYCODE_VOLUME_MUTE;
    }

    @Override
    public boolean dispatchKeyEvent(KeyEvent event) {
        int action = event.getAction();
        if (system(event.getKeyCode())
                || (action != KeyEvent.ACTION_DOWN && action != KeyEvent.ACTION_UP)) {
            return super.dispatchKeyEvent(event);
        }
        key(event.getScanCode(), action == KeyEvent.ACTION_DOWN, event.getRepeatCount() > 0);
        return true;
    }
}
