#ifndef LIVI_GST_HOST_STANDALONE
#include <node_api.h>
#endif
#include <gst/gst.h>
#include <gst/app/gstappsrc.h>
#include <gst/base/gstbasesink.h>
#include <gst/video/videooverlay.h>
#include <gst/video/video.h>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <initializer_list>
#include <string>
#ifdef __linux__
#include <execinfo.h>
#include <fcntl.h>
#include <glib-unix.h>
#include <signal.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>
#endif

#if defined(__APPLE__) || defined(_WIN32)
extern "C" guintptr livi_attach_view(guintptr parent, void** outView);
extern "C" void livi_remove_view(void* view);
extern "C" void livi_set_view_hidden(void* view, bool hidden);
extern "C" void livi_set_content_region(void* view, void* sink, double cropL,
    double cropT, double visW, double visH, double tierW, double tierH);
extern "C" void livi_set_backdrop(guintptr parent, double r, double g, double b);
#else
[[maybe_unused]] static guintptr livi_attach_view(guintptr parent, void** outView) {
  *outView = nullptr;
  return parent;
}
[[maybe_unused]] static void livi_remove_view(void*) {}
[[maybe_unused]] static void livi_set_view_hidden(void*, bool) {}
[[maybe_unused]] static void livi_set_content_region(void*, void*, double, double, double, double,
    double, double) {}
[[maybe_unused]] static void livi_set_backdrop(guintptr, double, double, double) {}
#endif

struct Player {
  GstElement* pipeline = nullptr;
  GstElement* appsrc = nullptr;
  GstElement* sink = nullptr;
  GstElement* glshader = nullptr;
  void* view = nullptr;
};

static void ensure_init() {
  static bool done = false;
  if (!done) {
    gst_init(NULL, NULL);
    if (const char* dbg = getenv("LIVI_GST_DEBUG")) {
      gst_debug_set_threshold_from_string(
        (dbg[0] == '1' && dbg[1] == '\0')
          ? "v4l2codecs-decoder:6,v4l2codecs-h265dec:6,waylandsink:5,wl_dmabuf:6"
          : dbg,
        FALSE);
    }
    done = true;
  }
}

static GstBusSyncReply bus_sync(GstBus*, GstMessage* msg, gpointer) {
  GstMessageType t = GST_MESSAGE_TYPE(msg);
  if (t == GST_MESSAGE_ERROR) {
    GError* e = nullptr; gchar* d = nullptr;
    gst_message_parse_error(msg, &e, &d);
    fprintf(stderr, "[gst_video] ERROR from %s: %s | %s\n",
      GST_OBJECT_NAME(msg->src), e ? e->message : "?", d ? d : "");
    if (e) g_error_free(e);
    g_free(d);
  } else if (t == GST_MESSAGE_WARNING) {
    GError* e = nullptr; gchar* d = nullptr;
    gst_message_parse_warning(msg, &e, &d);
    fprintf(stderr, "[gst_video] WARN from %s: %s | %s\n",
      GST_OBJECT_NAME(msg->src), e ? e->message : "?", d ? d : "");
    if (e) g_error_free(e);
    g_free(d);
  }
  return GST_BUS_PASS;
}

// Force every base sink in the pipeline to render unsynced (live, drop-late)
static void force_sinks_realtime(GstElement* pipeline) {
  GstIterator* it = gst_bin_iterate_recurse(GST_BIN(pipeline));
  GValue item = G_VALUE_INIT;
  gboolean done = FALSE;
  while (!done) {
    switch (gst_iterator_next(it, &item)) {
      case GST_ITERATOR_OK: {
        GstElement* el = GST_ELEMENT(g_value_get_object(&item));
        if (GST_IS_BASE_SINK(el)) {
          g_object_set(el, "sync", FALSE, "qos", FALSE, "max-lateness", (gint64)0, NULL);
        }
        g_value_reset(&item);
        break;
      }
      case GST_ITERATOR_RESYNC:
        gst_iterator_resync(it);
        break;
      case GST_ITERATOR_ERROR:
      case GST_ITERATOR_DONE:
        done = TRUE;
        break;
    }
  }
  g_value_unset(&item);
  gst_iterator_free(it);
}

// Rewrite a colorimetry the Pi 4 stateful v4l2 decoder rejects to one it accepts.
static const char* kBadColorimetry = "1:4:5:1";
static const char* kGoodColorimetry = "1:4:7:1";

static const char* caps_colorimetry(GstCaps* caps) {
  if (!caps || gst_caps_get_size(caps) == 0) return nullptr;
  return gst_structure_get_string(gst_caps_get_structure(caps, 0), "colorimetry");
}

static GstPadProbeReturn colorimetry_query_probe(GstPad* pad, GstPadProbeInfo* info, gpointer) {
  GstQuery* q = GST_PAD_PROBE_INFO_QUERY(info);
  if (!q) return GST_PAD_PROBE_OK;
  if (GST_QUERY_TYPE(q) == GST_QUERY_CAPS) {
    GstCaps* filter = nullptr;
    gst_query_parse_caps(q, &filter);
    const char* col = caps_colorimetry(filter);
    if (!col || strcmp(col, kBadColorimetry) != 0) return GST_PAD_PROBE_OK;
    GstCaps* tmpl = gst_pad_get_pad_template_caps(pad);
    GstCaps* res = gst_caps_intersect(tmpl, filter);
    gst_caps_unref(tmpl);
    gst_query_set_caps_result(q, res);
    gst_caps_unref(res);
    return GST_PAD_PROBE_HANDLED;
  }
  if (GST_QUERY_TYPE(q) == GST_QUERY_ACCEPT_CAPS) {
    GstCaps* caps = nullptr;
    gst_query_parse_accept_caps(q, &caps);
    const char* col = caps_colorimetry(caps);
    if (!col || strcmp(col, kBadColorimetry) != 0) return GST_PAD_PROBE_OK;
    gst_query_set_accept_caps_result(q, TRUE);
    return GST_PAD_PROBE_HANDLED;
  }
  return GST_PAD_PROBE_OK;
}

// Rewrite kBadColorimetry to kGoodColorimetry on the caps event the decoder sees. Metadata only.
static GstPadProbeReturn colorimetry_fixup_probe(GstPad*, GstPadProbeInfo* info, gpointer) {
  GstEvent* ev = GST_PAD_PROBE_INFO_EVENT(info);
  if (!ev || GST_EVENT_TYPE(ev) != GST_EVENT_CAPS) return GST_PAD_PROBE_OK;
  GstCaps* caps = nullptr;
  gst_event_parse_caps(ev, &caps);
  const char* col = caps_colorimetry(caps);
  if (!col || strcmp(col, kBadColorimetry) != 0) return GST_PAD_PROBE_OK;
  GstCaps* nc = gst_caps_copy(caps);
  gst_caps_set_simple(nc, "colorimetry", G_TYPE_STRING, kGoodColorimetry, NULL);
  gst_event_unref(ev);
  GST_PAD_PROBE_INFO_DATA(info) = gst_event_new_caps(nc);
  gst_caps_unref(nc);
  fprintf(stderr, "[gst_video] colorimetry %s -> %s (pi4 v4l2 decoder)\n",
    kBadColorimetry, kGoodColorimetry);
  return GST_PAD_PROBE_OK;
}

static void remove_video_view(Player* p) {
  if (p->view) {
    livi_remove_view(p->view);
    p->view = nullptr;
  }
}

static const char* parser_for(const std::string& c) {
  if (c == "h265") return "h265parse";
  if (c == "vp9") return "vp9parse";
  if (c == "av1") return "av1parse";
  return "h264parse";
}

// First decoder in the list whose factory is registered, NULL when none of them is.
static const char* pick_decoder(std::initializer_list<const char*> cands) {
  for (const char* c : cands) {
    GstElementFactory* f = gst_element_factory_find(c);
    if (f) {
      gst_object_unref(f);
      return c;
    }
  }
  return nullptr;
}

// Software decoders (everything else, vtdec/v4l2*/va*/d3d11*, is HW)
static bool is_hw_decoder(const char* name) {
  if (!name || !*name) return false;
  if (strncmp(name, "avdec_", 6) == 0) return false;
  if (strcmp(name, "vp9dec") == 0 || strcmp(name, "vp8dec") == 0) return false;
  if (strcmp(name, "dav1ddec") == 0 || strcmp(name, "openh264dec") == 0) return false;
  return true;
}

// Primary software decoder per codec, used to report SW availability and by LIVI_GST_SWDEC
static const char* sw_decoder_for(const std::string& c) {
  if (c == "h265") return "avdec_h265";
  if (c == "vp9") return "vp9dec";
  if (c == "av1") return "dav1ddec";
  return "avdec_h264";
}

#ifndef LIVI_GST_HOST_STANDALONE
static bool factory_exists(const char* name) {
  GstElementFactory* f = name && *name ? gst_element_factory_find(name) : nullptr;
  if (f) {
    gst_object_unref(f);
    return true;
  }
  return false;
}
#endif

// Best available decoder per codec, HW-first then software fallback
static const char* decoder_for(const std::string& c) {
  if (getenv("LIVI_GST_SWDEC")) {
    return pick_decoder({sw_decoder_for(c)});
  }
#ifdef __APPLE__
  // HEVC on macOS uses avdec_h265, not vtdec: vtdec ignores sps_max_num_reorder_pics and adds
  // output latency. Revert to vtdec once the GStreamer bug is fixed.
  // https://gitlab.freedesktop.org/gstreamer/gstreamer/-/work_items/5133
  if (c == "h265") return pick_decoder({"avdec_h265", "vtdec"});
  if (c == "vp9") return pick_decoder({"vp9dec"});
  if (c == "av1") return pick_decoder({"dav1ddec"});
  return pick_decoder({"vtdec", "avdec_h264"});
#elif defined(_WIN32)
  if (c == "h265") return pick_decoder({"d3d11h265dec", "avdec_h265"});
  if (c == "vp9") return pick_decoder({"d3d11vp9dec", "vp9dec"});
  if (c == "av1") return pick_decoder({"d3d11av1dec", "dav1ddec"});
  return pick_decoder({"d3d11h264dec", "avdec_h264"});
#else
  if (c == "h265") return pick_decoder({"v4l2slh265dec", "v4l2h265dec", "vah265dec", "avdec_h265"});
  if (c == "vp9") return pick_decoder({"v4l2slvp9dec", "v4l2vp9dec", "vavp9dec", "vp9dec"});
  if (c == "av1") return pick_decoder({"vaav1dec", "dav1ddec"});
  return pick_decoder({"v4l2slh264dec", "v4l2h264dec", "vah264dec", "avdec_h264"});
#endif
}

// Sink chain per platform. Linux presents the decoded dmabuf to livi-compositor via
// waylandsink. mac/Windows render into the window surface directly.
static std::string sink_chain() {
#ifdef __APPLE__
  // force-aspect-ratio=false: the clip view enforces AR, glimagesink must fill (no black bars).
  return "glimagesink name=sink sync=false qos=false force-aspect-ratio=false";
#elif defined(_WIN32)
  return "d3d11videosink name=sink sync=false qos=false force-aspect-ratio=false";
#else
  // waylandsink hands the decoded dmabuf to livi-compositor zero-copy. LIVI_GST_SINK overrides.
  const char* sink_env = getenv("LIVI_GST_SINK");
  return std::string(sink_env && *sink_env ? sink_env : "waylandsink") +
    " name=sink sync=false";
#endif
}

static std::string caps_for(const std::string& c) {
  if (c == "h265") return "video/x-h265,stream-format=byte-stream";
  if (c == "vp9") return "video/x-vp9";
  if (c == "av1") return "video/x-av1";
  return "video/x-h264,stream-format=byte-stream";
}

// CarPlay ships the parameter sets once as an hvcC/avcC record and every frame as
// length-prefixed NALs, which is the hvc1/avc stream format.
static void set_length_prefixed_caps(GstElement* appsrc, const std::string& codec,
                                     const guint8* codec_data, gsize codec_data_len) {
  const char* media = codec == "h265" ? "video/x-h265" : "video/x-h264";
  const char* stream_format = codec == "h265" ? "hvc1" : "avc";
  GstCaps* caps = gst_caps_new_simple(media, "stream-format", G_TYPE_STRING, stream_format,
                                      "alignment", G_TYPE_STRING, "au", NULL);
  GstBuffer* cd = gst_buffer_new_memdup(codec_data, codec_data_len);
  gst_caps_set_simple(caps, "codec_data", GST_TYPE_BUFFER, cd, NULL);
  gst_buffer_unref(cd);
  g_object_set(appsrc, "caps", caps, NULL);
  gst_caps_unref(caps);
}

#ifndef LIVI_GST_HOST_STANDALONE
static std::string get_string_arg(napi_env env, napi_value v) {
  size_t len = 0;
  napi_get_value_string_utf8(env, v, NULL, 0, &len);
  std::string s(len, '\0');
  napi_get_value_string_utf8(env, v, &s[0], len + 1, &len);
  return s;
}

static napi_value Version(napi_env env, napi_callback_info info) {
  ensure_init();
  gchar* v = gst_version_string();
  napi_value result;
  napi_create_string_utf8(env, v, NAPI_AUTO_LENGTH, &result);
  g_free(v);
  return result;
}

// Returns { codec: {hw, sw} } per codec, hw/sw = a hardware/software decoder exists.
static napi_value ProbeCodecs(napi_env env, napi_callback_info info) {
  ensure_init();
  napi_value obj;
  napi_create_object(env, &obj);
  const char* codecs[] = {"h264", "h265", "vp9", "av1"};
  for (const char* c : codecs) {
    const char* dec = decoder_for(c);
    bool hw = factory_exists(dec) && is_hw_decoder(dec);
    bool sw = factory_exists(sw_decoder_for(c));

    napi_value entry, b;
    napi_create_object(env, &entry);
    napi_get_boolean(env, hw, &b);
    napi_set_named_property(env, entry, "hw", b);
    napi_get_boolean(env, sw, &b);
    napi_set_named_property(env, entry, "sw", b);
    napi_set_named_property(env, obj, c, entry);
  }
  return obj;
}
#endif

static void livi_free_player(Player* p) {
  if (!p) return;
  if (p->pipeline) {
    gst_element_set_state(p->pipeline, GST_STATE_NULL);
    // Wait for NULL to complete: the stateless v4l2 decoder frees its kernel buffers only
    // when the transition finishes, and unreffing before that leaks a decoder instance per
    // teardown, so rapid session switching exhausts the pool and planes go black. Bounded so
    // a stuck transition cannot deadlock the teardown.
    gst_element_get_state(p->pipeline, NULL, NULL, 2 * GST_SECOND);
    if (p->appsrc) gst_object_unref(p->appsrc);
    if (p->sink) gst_object_unref(p->sink);
    if (p->glshader) gst_object_unref(p->glshader);
    gst_object_unref(p->pipeline);
  }
  remove_video_view(p);
  delete p;
}

static const char* CAL_FRAGMENT =
  "#version 100\n"
  "precision highp float;\n"
  "varying vec2 v_texcoord;\n"
  "uniform sampler2D tex;\n"
  "uniform float u_gamma;\n"
  "uniform float u_contrast;\n"
  "uniform float u_gain_r;\n"
  "uniform float u_gain_g;\n"
  "uniform float u_gain_b;\n"
  "void main() {\n"
  "  vec3 c = texture2D(tex, v_texcoord).rgb;\n"
  "  c = pow(c, vec3(1.0 / u_gamma));\n"
  "  c = (c - 0.5) * u_contrast + 0.5;\n"
  "  c = c * vec3(u_gain_r, u_gain_g, u_gain_b);\n"
  "  gl_FragColor = vec4(clamp(c, 0.0, 1.0), 1.0);\n"
  "}\n";

static void livi_set_gamma_player(Player* p, double gamma, double contrast, double gain_r,
                                  double gain_g, double gain_b) {
  if (!p || !p->glshader) return;
  GstStructure* u = gst_structure_new(
    "uniforms", "u_gamma", G_TYPE_FLOAT, (float)(gamma > 0.0 ? gamma : 1.0), "u_contrast",
    G_TYPE_FLOAT, (float)contrast, "u_gain_r", G_TYPE_FLOAT, (float)gain_r, "u_gain_g",
    G_TYPE_FLOAT, (float)gain_g, "u_gain_b", G_TYPE_FLOAT, (float)gain_b, NULL);
  g_object_set(p->glshader, "uniforms", u, NULL);
  gst_structure_free(u);
}

#ifndef LIVI_GST_HOST_STANDALONE
static void player_finalize(napi_env env, void* data, void* hint) {
  (void)env;
  (void)hint;
  livi_free_player(static_cast<Player*>(data));
}
#endif

// Logs the decoder's negotiated output caps (format, resolution, memory) once per caps change.
static GstPadProbeReturn decoded_caps_log_probe(GstPad*, GstPadProbeInfo* info, gpointer) {
  GstEvent* ev = GST_PAD_PROBE_INFO_EVENT(info);
  if (!ev || GST_EVENT_TYPE(ev) != GST_EVENT_CAPS) return GST_PAD_PROBE_OK;
  GstCaps* caps = nullptr;
  gst_event_parse_caps(ev, &caps);
  if (caps && gst_caps_get_size(caps) > 0) {
    GstStructure* s = gst_caps_get_structure(caps, 0);
    const char* fmt = gst_structure_get_string(s, "format");
    const char* drm = gst_structure_get_string(s, "drm-format");
    int w = 0, h = 0;
    gst_structure_get_int(s, "width", &w);
    gst_structure_get_int(s, "height", &h);
    GstCapsFeatures* feat = gst_caps_get_features(caps, 0);
    gchar* fs = feat ? gst_caps_features_to_string(feat) : nullptr;
    fprintf(stderr, "[gst_video] decoded format=%s%s%s %dx%d mem=%s\n",
      fmt ? fmt : "?", drm ? " drm=" : "", drm ? drm : "", w, h,
      fs && *fs ? fs : "SystemMemory");
    if (fs) g_free(fs);
  }
  return GST_PAD_PROBE_OK;
}

// Build the decode + waylandsink pipeline for a codec. handle is the native window for the
// mac/Windows overlay, unused on Linux. Returns NULL on parse failure.
static Player* livi_create_player(const std::string& codec, guintptr handle,
                                  const guint8* codec_data = nullptr, gsize codec_data_len = 0) {
  // Two queues: before the decoder non-leaky (a stateless HW decoder needs every
  // frame for its reference chain), after the decoder leaky=downstream.
  const char* decoder = decoder_for(codec);
  if (!decoder) {
    fprintf(stderr,
      "[gst_video] no decoder registered for %s. Install the GStreamer plugin providing it, "
      "software decoding needs gstreamer1.0-libav (%s).\n",
      codec.c_str(), sw_decoder_for(codec));
    return nullptr;
  }

  std::string presink;
#if !defined(__APPLE__) && !defined(_WIN32)
  if (!is_hw_decoder(decoder)) presink = "videoconvert ! ";
#endif

  std::string cal;
#if defined(__APPLE__)
  cal = "glupload ! glcolorconvert ! glshader name=cal ! ";
#endif

  auto make_desc = [&](const std::string& calc) -> std::string {
    return "appsrc name=src is-live=true do-timestamp=true format=time"
      " min-latency=0 max-latency=0 caps=" +
      caps_for(codec) + " ! " + parser_for(codec) +
      " ! queue max-size-buffers=0 max-size-bytes=0 max-size-time=2000000000" +
      " ! " + std::string(decoder) + " name=dec" +
      " ! queue max-size-buffers=2 max-size-bytes=0 max-size-time=0 leaky=downstream" +
      " ! " + presink + calc + sink_chain();
  };

  std::string desc = make_desc(cal);
  fprintf(stderr, "[gst_video] codec=%s decoder=%s (%s) | %s\n",
    codec.c_str(), decoder, is_hw_decoder(decoder) ? "hw" : "sw", desc.c_str());

  GError* err = nullptr;
  GstElement* pipeline = gst_parse_launch(desc.c_str(), &err);
  if ((!pipeline || err) && !cal.empty()) {
    fprintf(stderr, "[gst_video] calibration pass failed (%s), retrying without it\n",
      err ? err->message : "unknown");
    if (err) { g_error_free(err); err = nullptr; }
    if (pipeline) { gst_object_unref(pipeline); pipeline = nullptr; }
    desc = make_desc("");
    pipeline = gst_parse_launch(desc.c_str(), &err);
  }
  if (!pipeline || err) {
    fprintf(stderr, "[gst_video] pipeline parse FAILED: %s\n",
      err ? err->message : "unknown");
    if (err) g_error_free(err);
    if (pipeline) gst_object_unref(pipeline);
    return nullptr;
  }

  Player* p = new Player();
  p->pipeline = pipeline;
  p->appsrc = gst_bin_get_by_name(GST_BIN(pipeline), "src");
  if (p->appsrc && codec_data && codec_data_len > 0 && (codec == "h265" || codec == "h264")) {
    set_length_prefixed_caps(p->appsrc, codec, codec_data, codec_data_len);
  }
  p->sink = gst_bin_get_by_name(GST_BIN(pipeline), "sink");
  p->glshader = gst_bin_get_by_name(GST_BIN(pipeline), "cal");
  if (p->glshader) {
    g_object_set(p->glshader, "fragment", CAL_FRAGMENT, NULL);
    livi_set_gamma_player(p, 1.0, 1.0, 1.0, 1.0, 1.0);
  }

  force_sinks_realtime(pipeline);

  GstElement* dec = gst_bin_get_by_name(GST_BIN(pipeline), "dec");
  if (dec) {
    GstPad* dsrc = gst_element_get_static_pad(dec, "src");
    if (dsrc) {
      gst_pad_add_probe(dsrc, GST_PAD_PROBE_TYPE_EVENT_DOWNSTREAM, decoded_caps_log_probe, NULL,
        NULL);
      gst_object_unref(dsrc);
    }
    GstPad* dsp = gst_element_get_static_pad(dec, "sink");
    if (dsp) {
      // Only the Pi 4 stateful v4l2 decoders reject 1:4:5:1, the Pi 5 stateless ones accept it.
      if (!strcmp(decoder, "v4l2h264dec") || !strcmp(decoder, "v4l2h265dec")) {
        gst_pad_add_probe(dsp, GST_PAD_PROBE_TYPE_QUERY_DOWNSTREAM, colorimetry_query_probe, NULL,
          NULL);
        gst_pad_add_probe(dsp, GST_PAD_PROBE_TYPE_EVENT_DOWNSTREAM, colorimetry_fixup_probe, NULL,
          NULL);
      }
      gst_object_unref(dsp);
    }
    gst_object_unref(dec);
  }

  GstBus* bus = gst_element_get_bus(pipeline);
  gst_bus_set_sync_handler(bus, bus_sync, NULL, NULL);
  gst_object_unref(bus);

#ifndef __linux__
  guintptr overlay = handle ? livi_attach_view(handle, &p->view) : handle;
  if (p->sink && GST_IS_VIDEO_OVERLAY(p->sink) && overlay) {
    gst_video_overlay_set_window_handle(GST_VIDEO_OVERLAY(p->sink), overlay);
  }
#else
  (void)handle;
#endif

  return p;
}

#ifndef LIVI_GST_HOST_STANDALONE
static napi_value CreatePlayer(napi_env env, napi_callback_info info) {
  ensure_init();

  size_t argc = 3;
  napi_value argv[3];
  napi_get_cb_info(env, info, &argc, argv, NULL, NULL);

  std::string codec = argc >= 1 ? get_string_arg(env, argv[0]) : "h264";

  guintptr handle = 0;
  if (argc >= 2) {
    void* data = nullptr;
    size_t len = 0;
    if (napi_get_buffer_info(env, argv[1], &data, &len) == napi_ok && data && len >= sizeof(void*)) {
      memcpy(&handle, data, sizeof(void*));
    }
  }

  const guint8* codec_data = nullptr;
  gsize codec_data_len = 0;
  if (argc >= 3) {
    void* data = nullptr;
    size_t len = 0;
    if (napi_get_buffer_info(env, argv[2], &data, &len) == napi_ok && data && len > 0) {
      codec_data = static_cast<const guint8*>(data);
      codec_data_len = len;
    }
  }

  Player* p = livi_create_player(codec, handle, codec_data, codec_data_len);
  if (!p) {
    napi_value n;
    napi_get_null(env, &n);
    return n;
  }
  napi_value ext;
  napi_create_external(env, p, player_finalize, NULL, &ext);
  return ext;
}

static Player* unwrap(napi_env env, napi_value v) {
  void* data = nullptr;
  napi_get_value_external(env, v, &data);
  return static_cast<Player*>(data);
}

static napi_value Start(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1];
  napi_get_cb_info(env, info, &argc, argv, NULL, NULL);
  Player* p = argc >= 1 ? unwrap(env, argv[0]) : nullptr;
  if (p && p->pipeline) gst_element_set_state(p->pipeline, GST_STATE_PLAYING);
  napi_value undef;
  napi_get_undefined(env, &undef);
  return undef;
}
#endif

static void livi_push_player(Player* p, const void* data, size_t len) {
  if (!p || !p->appsrc || !data || len == 0) return;
  GstBuffer* buf = gst_buffer_new_memdup(data, len);
  gst_app_src_push_buffer(GST_APP_SRC(p->appsrc), buf);
}

#ifndef LIVI_GST_HOST_STANDALONE
static napi_value PushBuffer(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2];
  napi_get_cb_info(env, info, &argc, argv, NULL, NULL);
  Player* p = argc >= 1 ? unwrap(env, argv[0]) : nullptr;

  void* data = nullptr;
  size_t len = 0;
  bool ok = p && p->appsrc && argc >= 2 &&
    napi_get_buffer_info(env, argv[1], &data, &len) == napi_ok && data && len > 0;
  if (ok) livi_push_player(p, data, len);

  napi_value result;
  napi_get_boolean(env, ok, &result);
  return result;
}

// Show/hide the video view (UI navigation in/out)
static napi_value SetVisible(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2];
  napi_get_cb_info(env, info, &argc, argv, NULL, NULL);
  Player* p = argc >= 1 ? unwrap(env, argv[0]) : nullptr;
  bool visible = true;
  if (argc >= 2) napi_get_value_bool(env, argv[1], &visible);
  if (p) livi_set_view_hidden(p->view, !visible);
  napi_value undef;
  napi_get_undefined(env, &undef);
  return undef;
}

static napi_value Stop(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1];
  napi_get_cb_info(env, info, &argc, argv, NULL, NULL);
  Player* p = argc >= 1 ? unwrap(env, argv[0]) : nullptr;
  if (p && p->pipeline) gst_element_set_state(p->pipeline, GST_STATE_NULL);
  if (p) remove_video_view(p);
  napi_value undef;
  napi_get_undefined(env, &undef);
  return undef;
}

static napi_value SetContentRegion(napi_env env, napi_callback_info info) {
  size_t argc = 7;
  napi_value argv[7];
  napi_get_cb_info(env, info, &argc, argv, NULL, NULL);
  Player* p = argc >= 1 ? unwrap(env, argv[0]) : nullptr;
  if (p && p->view) {
    auto d = [&](size_t idx) -> double {
      double v = 0;
      if (argc > idx) napi_get_value_double(env, argv[idx], &v);
      return v;
    };
    livi_set_content_region(p->view, p->sink, d(1), d(2), d(3), d(4), d(5), d(6));
  }
  napi_value undef;
  napi_get_undefined(env, &undef);
  return undef;
}

// Paint the window content view with the theme colour where the UI is transparent and no video covers.
static napi_value SetBackdrop(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4];
  napi_get_cb_info(env, info, &argc, argv, NULL, NULL);
  guintptr handle = 0;
  if (argc >= 1) {
    void* data = nullptr;
    size_t len = 0;
    if (napi_get_buffer_info(env, argv[0], &data, &len) == napi_ok && data &&
        len >= sizeof(void*)) {
      memcpy(&handle, data, sizeof(void*));
    }
  }
  auto d = [&](size_t idx) -> double {
    double v = 0;
    if (argc > idx) napi_get_value_double(env, argv[idx], &v);
    return v;
  };
  if (handle) livi_set_backdrop(handle, d(1), d(2), d(3));
  napi_value undef;
  napi_get_undefined(env, &undef);
  return undef;
}

static napi_value SetGamma(napi_env env, napi_callback_info info) {
  size_t argc = 6;
  napi_value argv[6];
  napi_get_cb_info(env, info, &argc, argv, NULL, NULL);
  Player* p = argc >= 1 ? unwrap(env, argv[0]) : nullptr;
  auto d = [&](size_t idx) -> double {
    double v = 1.0;
    if (argc > idx) napi_get_value_double(env, argv[idx], &v);
    return v;
  };
  if (p) livi_set_gamma_player(p, d(1), d(2), d(3), d(4), d(5));
  napi_value undef;
  napi_get_undefined(env, &undef);
  return undef;
}
#endif

#ifdef __linux__
// gst-host: runs the pipeline in this separate process with its own GLib main loop. Reads
// create(1)/data(2)/stop(3) frames from the unix socket the main process serves.
struct LiviHost {
  GByteArray* buf;
  GHashTable* players;  // id -> Player*
};

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
    Player* old = (Player*)g_hash_table_lookup(h->players, key);
    if (old) {
      g_hash_table_remove(h->players, key);
      livi_free_player(old);
    }
    Player* p = livi_create_player(codec, 0, codec_data, codec_data_len);
    if (p) {
      gst_element_set_state(p->pipeline, GST_STATE_PLAYING);
      g_hash_table_insert(h->players, key, p);
    }
  } else if (op == 2) {
    livi_push_player((Player*)g_hash_table_lookup(h->players, key), rest, rlen);
  } else if (op == 3) {
    Player* p = (Player*)g_hash_table_lookup(h->players, key);
    if (p) {
      g_hash_table_remove(h->players, key);
      livi_free_player(p);
    }
  } else if (op == 4) {
    Player* p = (Player*)g_hash_table_lookup(h->players, key);
    if (p && rlen >= 5 * sizeof(double)) {
      double v[5];
      memcpy(v, rest, sizeof(v));
      livi_set_gamma_player(p, v[0], v[1], v[2], v[3], v[4]);
    }
  }
}

static gboolean livi_host_readable(gint fd, GIOCondition cond, gpointer data) {
  LiviHost* h = (LiviHost*)data;
  if (cond & (G_IO_HUP | G_IO_ERR)) exit(0);
  guint8 chunk[65536];
  ssize_t n = read(fd, chunk, sizeof(chunk));
  if (n <= 0) exit(0);
  g_byte_array_append(h->buf, chunk, (guint)n);
  while (h->buf->len >= 4) {
    guint32 len;
    memcpy(&len, h->buf->data, 4);
    if (h->buf->len < 4 + len) break;
    if (len >= 5) {
      guint8* payload = h->buf->data + 4;
      guint32 id;
      memcpy(&id, payload + 1, 4);
      livi_host_dispatch(h, payload[0], id, payload + 5, len - 5);
    }
    g_byte_array_remove_range(h->buf, 0, 4 + len);
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
  ensure_init();
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
  h->buf = g_byte_array_new();
  h->players = g_hash_table_new(g_direct_hash, g_direct_equal);
  g_unix_fd_add(fd, (GIOCondition)(G_IO_IN | G_IO_HUP | G_IO_ERR), livi_host_readable, h);

  g_main_loop_run(g_main_loop_new(NULL, FALSE));
}

#ifdef LIVI_GST_HOST_STANDALONE
static int livi_probe_stdout() {
  ensure_init();
  const char* codecs[] = {"h264", "h265", "vp9", "av1"};
  std::string out = "{";
  for (int i = 0; i < 4; i++) {
    const char* dec = decoder_for(codecs[i]);
    bool hw = dec && is_hw_decoder(dec);
    bool sw = pick_decoder({sw_decoder_for(codecs[i])}) != nullptr;
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
static napi_value Run(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2];
  napi_get_cb_info(env, info, &argc, argv, NULL, NULL);
  std::string sockPath = argc >= 1 ? get_string_arg(env, argv[0]) : "";
  std::string crashPath = argc >= 2 ? get_string_arg(env, argv[1]) : "";
  livi_host_main(sockPath.c_str(), crashPath.c_str());
  napi_value undef;
  napi_get_undefined(env, &undef);
  return undef;
}
#endif
#endif

#ifndef LIVI_GST_HOST_STANDALONE
static napi_value Init(napi_env env, napi_value exports) {
  napi_value fn;
  napi_create_function(env, "version", NAPI_AUTO_LENGTH, Version, NULL, &fn);
  napi_set_named_property(env, exports, "version", fn);
  napi_create_function(env, "probeCodecs", NAPI_AUTO_LENGTH, ProbeCodecs, NULL, &fn);
  napi_set_named_property(env, exports, "probeCodecs", fn);
  napi_create_function(env, "createPlayer", NAPI_AUTO_LENGTH, CreatePlayer, NULL, &fn);
  napi_set_named_property(env, exports, "createPlayer", fn);
  napi_create_function(env, "start", NAPI_AUTO_LENGTH, Start, NULL, &fn);
  napi_set_named_property(env, exports, "start", fn);
  napi_create_function(env, "pushBuffer", NAPI_AUTO_LENGTH, PushBuffer, NULL, &fn);
  napi_set_named_property(env, exports, "pushBuffer", fn);
  napi_create_function(env, "setVisible", NAPI_AUTO_LENGTH, SetVisible, NULL, &fn);
  napi_set_named_property(env, exports, "setVisible", fn);
  napi_create_function(env, "setContentRegion", NAPI_AUTO_LENGTH, SetContentRegion, NULL, &fn);
  napi_set_named_property(env, exports, "setContentRegion", fn);
  napi_create_function(env, "setBackdrop", NAPI_AUTO_LENGTH, SetBackdrop, NULL, &fn);
  napi_set_named_property(env, exports, "setBackdrop", fn);
  napi_create_function(env, "setGamma", NAPI_AUTO_LENGTH, SetGamma, NULL, &fn);
  napi_set_named_property(env, exports, "setGamma", fn);
  napi_create_function(env, "stop", NAPI_AUTO_LENGTH, Stop, NULL, &fn);
  napi_set_named_property(env, exports, "stop", fn);
#ifdef __linux__
  napi_create_function(env, "run", NAPI_AUTO_LENGTH, Run, NULL, &fn);
  napi_set_named_property(env, exports, "run", fn);
#endif
  return exports;
}

NAPI_MODULE(NODE_GYP_MODULE_NAME, Init)
#endif
