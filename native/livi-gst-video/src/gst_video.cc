// The gst-host process: it serves a unix socket, keeps the players the main
// process asks for, and feeds them from the CarPlay screen receivers. The
// pipelines themselves live in the livi-video-player crate.
#include <glib.h>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>

#include "cp_host_proto.h"
#include "cp_player.h"
#include "cp_video_codec.h"
#include "cp_video_fanout.h"
#ifdef __linux__
#include <errno.h>
#include <execinfo.h>
#include <fcntl.h>
#include <glib-unix.h>
#include <signal.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>
#include "cp_screen_receiver.h"
#include "cp_video_nal.h"
#endif

#ifdef __linux__
// gst-host: runs the pipeline in this separate process with its own GLib main loop. Reads
// create(1)/data(2)/stop(3) frames from the unix socket the main process serves.
struct LiviHost {
  CpHostFramer* framer;
  GHashTable* players;  // id -> CpPlayer*
  int out_fd;
  GHashTable* receivers;  // id -> HostReceiver*
};

#ifdef __linux__
#define CLUSTER_RECV_ID 0x7a000010u
#define RECV_CACHE_MAX 240
#define CLUSTER_PLANE_MIN 0x7a000011u
#define CLUSTER_PLANE_MAX 0x7a000013u
static bool is_cluster_plane_id(guint32 id) {
  return id >= CLUSTER_PLANE_MIN && id <= CLUSTER_PLANE_MAX;
}

struct HostReceiver {
  CpScreenReceiver* r;
  LiviHost* h;
  guint32 id;        // unique per session-screen (reverse port reply + setActiveFeeder + teardown)
  guint32 plane_id;  // shared role id (main / cluster-recv), carried on reverse config/started
  guint stats_timer;
  bool is_cluster;
  GByteArray* config;  // last config atom, re-forwarded on activation
  CpFanout* fan;       // gating, GOP cache and the window counters
};

static HostReceiver* find_active_feeder(LiviHost* h, guint32 plane_id) {
  GHashTableIter it;
  gpointer k, v;
  g_hash_table_iter_init(&it, h->receivers);
  while (g_hash_table_iter_next(&it, &k, &v)) {
    HostReceiver* hr = (HostReceiver*)v;
    if (cp_fanout_is_active(hr->fan) && hr->plane_id == plane_id) return hr;
  }
  return nullptr;
}

static void livi_write_all(int fd, const guint8* p, gsize n) {
  while (n > 0) {
    ssize_t w = write(fd, p, n);
    if (w < 0) {
      if (errno == EINTR) continue;
      break;
    }
    p += w;
    n -= (gsize)w;
  }
}

static void livi_host_reply(LiviHost* h, guint8 rop, guint32 id, const guint8* rest, guint32 rlen) {
  if (h->out_fd < 0) return;
  guint8 head[9];
  // The body is written straight from the caller's buffer, so only the header
  // is encoded here; its length field counts the body too.
  if (cp_host_reply_encode(rop, id, nullptr, 0, head, sizeof(head)) == 0) return;
  guint32 len = 5 + rlen;
  memcpy(head, &len, 4);
  livi_write_all(h->out_fd, head, sizeof(head));
  if (rlen) livi_write_all(h->out_fd, rest, rlen);
}

static void recv_forward_config(HostReceiver* hr) {
  if (!hr->config || hr->config->len == 0) return;
  livi_host_reply(hr->h, 2, hr->plane_id, hr->config->data, hr->config->len);
}

static void recv_on_config(int codec, const guint8* atom, size_t len, void* user) {
  HostReceiver* hr = (HostReceiver*)user;
  cp_fanout_set_codec(hr->fan, codec);
  if (len == 0) return;  // keepalive config with no record: keep the last real codec_data
  g_byte_array_set_size(hr->config, 0);
  guint8 c = (guint8)codec;
  g_byte_array_append(hr->config, &c, 1);
  g_byte_array_append(hr->config, atom, (guint)len);
  if (cp_fanout_is_active(hr->fan)) recv_forward_config(hr);
}

static bool recv_has_target(HostReceiver* hr) {
  if (hr->is_cluster) {
    for (guint32 id = CLUSTER_PLANE_MIN; id <= CLUSTER_PLANE_MAX; id++)
      if (g_hash_table_lookup(hr->h->players, GUINT_TO_POINTER(id))) return true;
    return false;
  }
  return g_hash_table_lookup(hr->h->players, GUINT_TO_POINTER(hr->plane_id)) != nullptr;
}

static gboolean recv_stats_tick(gpointer user) {
  HostReceiver* hr = (HostReceiver*)user;
  guint64 in = 0, dropped = 0, pushed = 0;
  bool awaiting = false, active = false;
  if (cp_fanout_take_stats(hr->fan, &in, &dropped, &pushed, &awaiting, &active)) {
    g_printerr("[cp_screen] recv 0x%x: in=%llu dropped=%llu pushed=%llu awaiting_kf=%d active=%d\n",
      hr->plane_id, (unsigned long long)in, (unsigned long long)dropped,
      (unsigned long long)pushed, awaiting ? 1 : 0, active ? 1 : 0);
  }
  return TRUE;
}

static void recv_push_targets(HostReceiver* hr, const guint8* nal, size_t len) {
  if (hr->is_cluster) {
    for (guint32 id = CLUSTER_PLANE_MIN; id <= CLUSTER_PLANE_MAX; id++) {
      CpPlayer* p = (CpPlayer*)g_hash_table_lookup(hr->h->players, GUINT_TO_POINTER(id));
      if (p) cp_player_push(p, nal, len);
    }
  } else {
    CpPlayer* p = (CpPlayer*)g_hash_table_lookup(hr->h->players, GUINT_TO_POINTER(hr->plane_id));
    if (p) cp_player_push(p, nal, len);
  }
}

// Feeds a newly created player the current GOP, so it decodes from the last
// keyframe instead of waiting for the next one. The cache is left intact for
// further players on the same stream.
static void recv_prime_player(HostReceiver* hr, CpPlayer* p) {
  if (!p) return;
  size_t n = cp_fanout_cache_len(hr->fan);
  for (size_t i = 0; i < n; i++) {
    const guint8* d = nullptr;
    size_t sz = cp_fanout_cached(hr->fan, i, &d);
    if (sz) cp_player_push(p, d, sz);
  }
}

static void recv_on_frame(const guint8* nal, size_t len, void* user) {
  HostReceiver* hr = (HostReceiver*)user;
  if (cp_fanout_take(hr->fan, nal, len, recv_has_target(hr))) recv_push_targets(hr, nal, len);
}

static void recv_on_started(void* user) {
  HostReceiver* hr = (HostReceiver*)user;
  if (!cp_fanout_is_active(hr->fan)) return;
  livi_host_reply(hr->h, 3, hr->plane_id, nullptr, 0);
}

static void host_receiver_free(HostReceiver* hr) {
  if (!hr) return;
  if (hr->stats_timer) g_source_remove(hr->stats_timer);
  cp_screen_receiver_free(hr->r);
  cp_fanout_free(hr->fan);
  if (hr->config) g_byte_array_free(hr->config, TRUE);
  g_free(hr);
}
#endif

static void livi_host_dispatch(LiviHost* h, guint8 op, guint32 id, const guint8* rest, gsize rlen) {
  gpointer key = GUINT_TO_POINTER(id);
  if (op == 1) {
    // [1B codecLen][codec ascii][codec_data]. codec_data is empty for byte-stream sources.
    char codec[16];
    gsize clen = rlen >= 1 ? rest[0] : 0;
    if (clen > sizeof(codec) - 1) clen = sizeof(codec) - 1;
    if (rlen < 1 + clen) return;
    memcpy(codec, rest + 1, clen);
    codec[clen] = '\0';
    const guint8* codec_data = rlen > 1 + clen ? rest + 1 + clen : nullptr;
    gsize codec_data_len = rlen > 1 + clen ? rlen - 1 - clen : 0;
    CpPlayer* old = (CpPlayer*)g_hash_table_lookup(h->players, key);
    if (old) {
      g_hash_table_remove(h->players, key);
      cp_player_free(old);
    }
    CpPlayer* p = cp_player_new(codec, 0, codec_data, codec_data_len);
    if (!p) {
      g_printerr("livi: create player 0x%x (codec %s) FAILED\n", id, codec);
    }
    if (p) {
      cp_player_start(p);
      g_hash_table_insert(h->players, key, p);
#ifdef __linux__
      guint32 plane_key = is_cluster_plane_id(id) ? CLUSTER_RECV_ID : id;
      HostReceiver* feeder = find_active_feeder(h, plane_key);
      if (feeder) recv_prime_player(feeder, p);
#endif
    }
  } else if (op == 2) {
    cp_player_push((CpPlayer*)g_hash_table_lookup(h->players, key), rest, rlen);
  } else if (op == 3) {
    CpPlayer* p = (CpPlayer*)g_hash_table_lookup(h->players, key);
    if (p) {
      g_hash_table_remove(h->players, key);
      cp_player_free(p);
    }
  } else if (op == 4) {
    CpPlayer* p = (CpPlayer*)g_hash_table_lookup(h->players, key);
    if (p && rlen >= 5 * sizeof(double)) {
      double v[5];
      memcpy(v, rest, sizeof(v));
      cp_player_set_gamma(p, v[0], v[1], v[2], v[3], v[4]);
    }
  } else if (op == 5) {
    // [4B planeId][1B flags: bit0=cluster][32B key]. id (header) = unique receiver id.
#ifdef __linux__
    if (rlen < 37) return;
    HostReceiver* hr = g_new0(HostReceiver, 1);
    hr->h = h;
    hr->id = id;
    memcpy(&hr->plane_id, rest, 4);
    hr->is_cluster = (rest[4] & 1) != 0;
    hr->fan = cp_fanout_new();
    hr->config = g_byte_array_new();
    hr->stats_timer = g_timeout_add_seconds(5, recv_stats_tick, hr);
    CpScreenCallbacks cb;
    cb.on_config = recv_on_config;
    cb.on_frame = recv_on_frame;
    cb.on_started = recv_on_started;
    cb.user = hr;
    uint16_t port = 0;
    hr->r = cp_screen_receiver_new(rest + 5, &cb, &port);
    if (!hr->r) {
      g_byte_array_free(hr->config, TRUE);
      cp_fanout_free(hr->fan);
      g_free(hr);
      return;
    }
    HostReceiver* old = (HostReceiver*)g_hash_table_lookup(h->receivers, key);
    if (old) {
      g_hash_table_remove(h->receivers, key);
      host_receiver_free(old);
    }
    g_hash_table_insert(h->receivers, key, hr);
    guint8 pbuf[2] = {(guint8)(port & 0xff), (guint8)(port >> 8)};
    livi_host_reply(h, 1, id, pbuf, 2);
#endif
  } else if (op == 6) {
#ifdef __linux__
    HostReceiver* hr = (HostReceiver*)g_hash_table_lookup(h->receivers, key);
    if (hr) {
      g_hash_table_remove(h->receivers, key);
      host_receiver_free(hr);
    }
#endif
  } else if (op == 7) {
    // [1B active]. id = receiver id. Make this the exclusive active feeder for its plane.
#ifdef __linux__
    HostReceiver* hr = (HostReceiver*)g_hash_table_lookup(h->receivers, key);
    if (!hr) return;
    bool active = rlen >= 1 && (rest[0] & 1);
    if (active) {
      GHashTableIter it;
      gpointer kk, vv;
      g_hash_table_iter_init(&it, h->receivers);
      while (g_hash_table_iter_next(&it, &kk, &vv)) {
        HostReceiver* o = (HostReceiver*)vv;
        if (o != hr && o->plane_id == hr->plane_id) cp_fanout_set_active(o->fan, false);
      }
      cp_fanout_set_active(hr->fan, true);
      cp_fanout_restart(hr->fan);
      recv_forward_config(hr);
    } else {
      cp_fanout_set_active(hr->fan, false);
    }
#endif
  }
}

static gboolean livi_host_readable(gint fd, GIOCondition cond, gpointer data) {
  LiviHost* h = (LiviHost*)data;
  if (cond & (G_IO_HUP | G_IO_ERR)) exit(0);
  guint8 chunk[65536];
  ssize_t n = read(fd, chunk, sizeof(chunk));
  if (n <= 0) exit(0);
  cp_host_framer_push(h->framer, chunk, (size_t)n);
  guint8 op = 0;
  guint32 id = 0;
  const guint8* rest = nullptr;
  size_t rest_len = 0;
  while (cp_host_framer_next(h->framer, &op, &id, &rest, &rest_len)) {
    livi_host_dispatch(h, op, id, rest, rest_len);
  }
  return G_SOURCE_CONTINUE;
}

// Where to drop the crash backtrace (next to the AppImage); set in Run() before the handler arms.
static char g_crash_log_path[1024] = {0};

static void livi_host_crash(int sig) {
  void* frames[64];
  int n = backtrace(frames, 64);
  const char hdr[] = "\n=== gst-host CRASH backtrace ===\n";
  (void)!write(STDERR_FILENO, hdr, sizeof(hdr) - 1);
  backtrace_symbols_fd(frames, n, STDERR_FILENO);
  if (g_crash_log_path[0]) {
    int cf = open(g_crash_log_path, O_CREAT | O_WRONLY | O_TRUNC, 0644);
    if (cf >= 0) {
      (void)!write(cf, hdr, sizeof(hdr) - 1);
      backtrace_symbols_fd(frames, n, cf);
      close(cf);
    }
  }
  signal(sig, SIG_DFL);
  raise(sig);
}

// Connect to the host socket and run the GLib main loop. The separate process is the libffi
// fix: outside Electron, libwayland binds the system libffi, not Electron's ABI-incompatible
// bundled copy that corrupts wayland marshalling on resize.
static void livi_host_main(const char* sockPath, const char* crashLogPath) {
  g_set_prgname("livi-video");
  cp_gst_ensure_init();
  if (crashLogPath && crashLogPath[0])
    strncpy(g_crash_log_path, crashLogPath, sizeof(g_crash_log_path) - 1);
  signal(SIGSEGV, livi_host_crash);
  signal(SIGABRT, livi_host_crash);

  int fd = socket(AF_UNIX, SOCK_STREAM, 0);
  struct sockaddr_un addr;
  memset(&addr, 0, sizeof(addr));
  addr.sun_family = AF_UNIX;
  strncpy(addr.sun_path, sockPath, sizeof(addr.sun_path) - 1);
  if (fd < 0 || connect(fd, (struct sockaddr*)&addr, sizeof(addr)) != 0) {
    fprintf(stderr, "[gst-host] connect to %s failed\n", sockPath);
    exit(1);
  }

  LiviHost* h = new LiviHost();
  h->framer = cp_host_framer_new();
  h->players = g_hash_table_new(g_direct_hash, g_direct_equal);
  h->out_fd = fd;
  h->receivers = g_hash_table_new(g_direct_hash, g_direct_equal);
  g_unix_fd_add(fd, (GIOCondition)(G_IO_IN | G_IO_HUP | G_IO_ERR), livi_host_readable, h);

  g_main_loop_run(g_main_loop_new(NULL, FALSE));
}

#ifdef LIVI_GST_HOST_STANDALONE
static int livi_probe_stdout() {
  cp_gst_ensure_init();
  const char* codecs[] = {"h264", "h265", "vp9", "av1"};
  std::string out = "{";
  for (int i = 0; i < 4; i++) {
    bool hw = false, sw = false;
    cp_gst_probe(codecs[i], &hw, &sw);
    out += i ? ",\"" : "\"";
    out += codecs[i];
    out += "\":{\"hw\":";
    out += hw ? "true" : "false";
    out += ",\"sw\":";
    out += sw ? "true" : "false";
    out += "}";
  }
  out += "}";
  printf("%s\n", out.c_str());
  fflush(stdout);
  return 0;
}

int main(int argc, char** argv) {
  if (argc > 1 && strcmp(argv[1], "--probe") == 0) return livi_probe_stdout();
  const char* sock = argc > 1 ? argv[1] : "";
  const char* crash = argc > 2 ? argv[2] : "";
  livi_host_main(sock, crash);
  return 0;
}
#else
extern "C" void livi_gst_host_run(const char* sock, const char* crash) {
  livi_host_main(sock ? sock : "", crash ? crash : "");
}
#endif
#endif
