/*
 * livi-compositor — a nested wlroots compositor for LIVI (based on tinywl, 0.20).
 *
 * One screen per role (LIVI_SCREENS), each a nested output with a transparent Electron
 * UI on top and tagged GStreamer waylandsink video planes below, composited zero-copy.
 * The host drives video placement/crop/visibility over a control socket.
 */
#include <assert.h>
#include <cairo/cairo.h>
#include <drm_fourcc.h>
#include <errno.h>
#include <fcntl.h>
#include <getopt.h>
#include <math.h>
#include <signal.h>
#include <stdbool.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>
#include <wayland-server-core.h>
#include <wlr/backend.h>
#include <wlr/interfaces/wlr_buffer.h>
#include <wlr/backend/multi.h>
#include <wlr/backend/wayland.h>
#include <drm_fourcc.h>
#include <wlr/render/allocator.h>
#include <wlr/render/wlr_renderer.h>
#include <wlr/types/wlr_buffer.h>
#include <wlr/types/wlr_cursor.h>
#include <wlr/types/wlr_compositor.h>
#include <wlr/types/wlr_data_device.h>
#include <wlr/types/wlr_input_device.h>
#include <wlr/types/wlr_keyboard.h>
#include <wlr/types/wlr_output.h>
#include <wlr/types/wlr_output_layout.h>
#include <wlr/types/wlr_pointer.h>
#include <wlr/types/wlr_scene.h>
#include <wlr/types/wlr_seat.h>
#include <wlr/types/wlr_subcompositor.h>
#include <wlr/types/wlr_touch.h>
#include <wlr/types/wlr_viewporter.h>
#include <wlr/types/wlr_xcursor_manager.h>
#include <wlr/types/wlr_xdg_decoration_v1.h>
#include <wlr/types/wlr_xdg_shell.h>
#include <wlr/util/addon.h>
#include <wlr/util/log.h>
#include <wlr/render/gles2.h>
#include <wlr/render/egl.h>
#include <wlr/render/swapchain.h>
#include <wlr/render/dmabuf.h>
#include <wlr/render/drm_format_set.h>
#include <wlr/render/wlr_renderer.h>
#include <render/wlr_renderer.h>
#include <GLES2/gl2.h>
#include <GLES2/gl2ext.h>
#include <EGL/egl.h>
#include <EGL/eglext.h>
#include <xkbcommon/xkbcommon.h>

enum tinywl_cursor_mode {
	TINYWL_CURSOR_PASSTHROUGH,
	TINYWL_CURSOR_MOVE,
	TINYWL_CURSOR_RESIZE,
};

// Each screen role gets its own non-overlapping x-slot in the scene
#define LIVI_SCREEN_X_SLOT 100000

#define LIVI_TITLEBAR_H 32
#define LIVI_BTN_W 32
#define LIVI_BTN_GAP 2
#define LIVI_RESIZE_BORDER 8

// Per-tag video config, cached until its tagged toplevel appears.
#define LIVI_MAX_VIDEO_CFGS 16
struct livi_video_cfg {
	bool valid;
	char tag[64];
	char screen[32];
	bool has_crop;
	double crop_l, crop_t, vis_w, vis_h, tier_w, tier_h;
	bool has_visible;
	bool visible;
};

struct tinywl_server {
	struct wl_display *wl_display;
	struct wlr_backend *backend;
	struct wlr_backend *wl_backend;
	struct wlr_renderer *renderer;
	struct wlr_allocator *allocator;
	struct wlr_scene *scene;
	struct wlr_scene_output_layout *scene_layout;
	// fixed z-order layers (bottom -> top): backdrop, video planes, UI, decoration, overlay
	struct wlr_scene_tree *layer_bg;
	struct wlr_scene_tree *layer_video;
	struct wlr_scene_tree *layer_ui;
	struct wlr_scene_tree *layer_deco;
	struct wlr_scene_tree *layer_overlay;

	struct wlr_xdg_shell *xdg_shell;
	struct wl_listener new_xdg_toplevel;
	struct wl_listener new_toplevel_decoration;
	struct wl_listener new_xdg_popup;
	struct wl_list toplevels;

	struct wlr_cursor *cursor;
	struct wlr_xcursor_manager *cursor_mgr;
	struct wl_listener cursor_motion;
	struct wl_listener cursor_motion_absolute;
	struct wl_listener cursor_button;
	struct wl_listener cursor_axis;
	struct wl_listener cursor_frame;
	struct wl_listener touch_down;
	struct wl_listener touch_up;
	struct wl_listener touch_motion;
	struct wl_listener touch_frame;
	bool has_touch;

	struct wlr_seat *seat;
	struct wl_listener new_input;
	struct wl_listener request_cursor;
	struct wl_listener pointer_focus_change;
	struct wl_listener request_set_selection;
	struct wl_list keyboards;
	enum tinywl_cursor_mode cursor_mode;
	struct tinywl_toplevel *grabbed_toplevel;
	double grab_x, grab_y;
	struct wlr_box grab_geobox;
	uint32_t resize_edges;

	struct wlr_output_layout *output_layout;
	struct wl_list outputs;
	struct wl_listener new_output;

	struct livi_screen *screens;
	int n_screens;
	struct livi_screen *pending_screen;   // next new output binds here (NULL -> main)
	char pending_video_tags[LIVI_MAX_VIDEO_CFGS][64];
	int n_pending_video_tags;
	struct wl_list videos;
	struct livi_video_cfg video_cfgs[LIVI_MAX_VIDEO_CFGS];
	int ctrl_fd;
	int ctrl_client_fd;   // current control-socket client, for the bound-tag ack channel

	// Display calibration state for the full-output shader pass. cal_active gates it.
	bool cal_active;
	float cal_gamma, cal_contrast, cal_gain[3];
	GLuint cal_prog;
	GLint cal_loc_gamma, cal_loc_contrast, cal_loc_gain, cal_loc_tex;
	bool cal_prog_failed;

	char *startup_cmd;
	const char *ui_socket;   // WAYLAND_DISPLAY the inner UI connects to (set per-child only)
	pid_t startup_pid;
	bool full_restart;   // on shutdown, re-exec the whole compositor instead of exiting
	struct wl_event_source *restart_timer;   // fallback if the inner UI doesn't exit on SIGTERM
	char **argv;         // saved for the re-exec
};

struct tinywl_output {
	struct wl_list link;
	struct tinywl_server *server;
	struct wlr_output *wlr_output;
	struct livi_screen *screen;
	struct wl_listener frame;
	struct wl_listener request_state;
	struct wl_listener destroy;
	// full-output calibration pass: scene composites into cal_inter, gamma shader into cal_final.
	struct wlr_swapchain *cal_inter, *cal_final;
	int cal_ow, cal_oh;
};

struct tinywl_toplevel {
	struct wl_list link;
	struct tinywl_server *server;
	struct livi_screen *screen;
	struct wlr_xdg_toplevel *xdg_toplevel;
	struct wlr_scene_tree *scene_tree;
	struct wlr_xdg_toplevel_decoration_v1 *decoration;  // forced server-side on initial commit
	bool is_video;
	bool is_dialog;   // modal dialog: lives in layer_overlay, kept centered
	// video plane: tag (claim) + crop region, placed by apply_video_layout
	char tag[64];
	bool has_crop;
	double crop_l, crop_t, vis_w, vis_h, tier_w, tier_h;
	struct wl_list video_link;   // in server->videos
	struct wl_listener map;
	struct wl_listener unmap;
	struct wl_listener commit;
	struct wl_listener destroy;
	struct wl_listener request_move;
	struct wl_listener request_resize;
	struct wl_listener request_maximize;
	struct wl_listener request_fullscreen;
};

struct tinywl_popup {
	struct wlr_xdg_popup *xdg_popup;
	struct wl_listener commit;
	struct wl_listener destroy;
};

struct tinywl_keyboard {
	struct wl_list link;
	struct tinywl_server *server;
	struct wlr_keyboard *wlr_keyboard;

	struct wl_listener modifiers;
	struct wl_listener key;
	struct wl_listener destroy;
};

// One output + its UI plane + backdrop. Video planes are tagged, looked up separately.
struct livi_screen {
	char role[32];
	struct wlr_output *wlr_output;
	int32_t x;                       // layout x-offset in the scene
	int32_t width, height;
	int32_t req_width, req_height;   // host-requested output size (0 -> LIVI_OUTPUT_SIZE)

	struct tinywl_toplevel *ui;

	struct wlr_scene_rect *backdrop;
	float backdrop_color[4];
	bool has_backdrop_color;

	struct wlr_scene_buffer *titlebar;
	struct wlr_scene_buffer *title;
	struct wlr_scene_buffer *btn_min;
	struct wlr_scene_buffer *btn_fs;
	struct wlr_scene_buffer *btn_close;
	int titlebar_w;
	bool fullscreen;
};

static int screen_top_inset(const struct livi_screen *s) {
	return s->fullscreen ? 0 : LIVI_TITLEBAR_H;
}

static struct livi_screen *screen_by_role(struct tinywl_server *server, const char *role) {
	for (int i = 0; i < server->n_screens; i++) {
		if (strcmp(server->screens[i].role, role) == 0) {
			return &server->screens[i];
		}
	}
	return NULL;
}

static struct livi_screen *screen_for_output_name(struct tinywl_server *server,
		const char *name) {
	if (name == NULL) {
		return NULL;
	}
	struct tinywl_output *o;
	wl_list_for_each(o, &server->outputs, link) {
		if (o->wlr_output->name && strcmp(o->wlr_output->name, name) == 0) {
			return o->screen;
		}
	}
	return NULL;
}

static void ctrl_send(struct tinywl_server *server, const char *line);

static struct tinywl_toplevel *find_video_by_tag(struct tinywl_server *server,
		const char *tag) {
	struct tinywl_toplevel *t;
	wl_list_for_each(t, &server->videos, video_link) {
		if (strcmp(t->tag, tag) == 0) {
			return t;
		}
	}
	return NULL;
}

static void apply_video_layout(struct tinywl_toplevel *video);
static void apply_ui_layout(struct livi_screen *s);
static void livi_toggle_fullscreen(struct livi_screen *s);

static struct livi_video_cfg *cfg_for_tag(struct tinywl_server *server, const char *tag,
		bool create) {
	for (int i = 0; i < LIVI_MAX_VIDEO_CFGS; i++) {
		if (server->video_cfgs[i].valid && strcmp(server->video_cfgs[i].tag, tag) == 0) {
			return &server->video_cfgs[i];
		}
	}
	if (!create) {
		return NULL;
	}
	for (int i = 0; i < LIVI_MAX_VIDEO_CFGS; i++) {
		if (!server->video_cfgs[i].valid) {
			struct livi_video_cfg *c = &server->video_cfgs[i];
			memset(c, 0, sizeof(*c));
			snprintf(c->tag, sizeof(c->tag), "%s", tag);
			c->valid = true;
			return c;
		}
	}
	return NULL;
}

static void apply_cfg_to_video(struct tinywl_server *server, struct livi_video_cfg *cfg,
		struct tinywl_toplevel *v) {
	if (cfg->screen[0]) {
		struct livi_screen *s = screen_by_role(server, cfg->screen);
		if (s) {
			v->screen = s;
		}
		v->has_crop = cfg->has_crop;
		v->crop_l = cfg->crop_l;
		v->crop_t = cfg->crop_t;
		v->vis_w = cfg->vis_w;
		v->vis_h = cfg->vis_h;
		v->tier_w = cfg->tier_w;
		v->tier_h = cfg->tier_h;
		apply_video_layout(v);
	}
	if (cfg->has_visible) {
		wlr_scene_node_set_enabled(&v->scene_tree->node, cfg->visible);
	}
}

static void focus_toplevel(struct tinywl_toplevel *toplevel) {
	if (toplevel == NULL) {
		return;
	}
	if (toplevel->is_video) {
		return;
	}
	struct tinywl_server *server = toplevel->server;
	struct wlr_seat *seat = server->seat;
	struct wlr_surface *prev_surface = seat->keyboard_state.focused_surface;
	struct wlr_surface *surface = toplevel->xdg_toplevel->base->surface;
	if (prev_surface == surface) {
		return;
	}
	if (prev_surface) {
		struct wlr_xdg_toplevel *prev_toplevel =
			wlr_xdg_toplevel_try_from_wlr_surface(prev_surface);
		if (prev_toplevel != NULL) {
			wlr_xdg_toplevel_set_activated(prev_toplevel, false);
		}
	}
	struct wlr_keyboard *keyboard = wlr_seat_get_keyboard(seat);
	wlr_scene_node_raise_to_top(&toplevel->scene_tree->node);
	wl_list_remove(&toplevel->link);
	wl_list_insert(&server->toplevels, &toplevel->link);
	wlr_xdg_toplevel_set_activated(toplevel->xdg_toplevel, true);
	if (keyboard != NULL) {
		wlr_seat_keyboard_notify_enter(seat, surface,
			keyboard->keycodes, keyboard->num_keycodes, &keyboard->modifiers);
	}
}

static void keyboard_handle_modifiers(
		struct wl_listener *listener, void *data) {
	struct tinywl_keyboard *keyboard =
		wl_container_of(listener, keyboard, modifiers);
	wlr_seat_set_keyboard(keyboard->server->seat, keyboard->wlr_keyboard);
	wlr_seat_keyboard_notify_modifiers(keyboard->server->seat,
		&keyboard->wlr_keyboard->modifiers);
}

static bool handle_keybinding(struct tinywl_server *server, xkb_keysym_t sym) {
	/* Alt is assumed held. Esc quits the compositor. */
	switch (sym) {
	case XKB_KEY_Escape:
		wl_display_terminate(server->wl_display);
		break;
	case XKB_KEY_F1:
		if (wl_list_length(&server->toplevels) < 2) {
			break;
		}
		struct tinywl_toplevel *next_toplevel =
			wl_container_of(server->toplevels.prev, next_toplevel, link);
		focus_toplevel(next_toplevel);
		break;
	case XKB_KEY_F11:
		// Toggle fullscreen on the main screen. The titlebar button only enters fullscreen
		// (it is hidden once fullscreen), so this is how you leave it from the keyboard.
		if (server->n_screens > 0) {
			livi_toggle_fullscreen(&server->screens[0]);
		}
		break;
	default:
		return false;
	}
	return true;
}

static void keyboard_handle_key(
		struct wl_listener *listener, void *data) {
	struct tinywl_keyboard *keyboard =
		wl_container_of(listener, keyboard, key);
	struct tinywl_server *server = keyboard->server;
	struct wlr_keyboard_key_event *event = data;
	struct wlr_seat *seat = server->seat;

	uint32_t keycode = event->keycode + 8;
	const xkb_keysym_t *syms;
	int nsyms = xkb_state_key_get_syms(
			keyboard->wlr_keyboard->xkb_state, keycode, &syms);

	bool handled = false;
	uint32_t modifiers = wlr_keyboard_get_modifiers(keyboard->wlr_keyboard);
	if ((modifiers & WLR_MODIFIER_ALT) &&
			event->state == WL_KEYBOARD_KEY_STATE_PRESSED) {
		for (int i = 0; i < nsyms; i++) {
			handled = handle_keybinding(server, syms[i]);
		}
	}

	if (!handled) {
		wlr_seat_set_keyboard(seat, keyboard->wlr_keyboard);
		wlr_seat_keyboard_notify_key(seat, event->time_msec,
			event->keycode, event->state);
	}
}

static void keyboard_handle_destroy(struct wl_listener *listener, void *data) {
	struct tinywl_keyboard *keyboard =
		wl_container_of(listener, keyboard, destroy);
	wl_list_remove(&keyboard->modifiers.link);
	wl_list_remove(&keyboard->key.link);
	wl_list_remove(&keyboard->destroy.link);
	wl_list_remove(&keyboard->link);
	free(keyboard);
}

static void server_new_keyboard(struct tinywl_server *server,
		struct wlr_input_device *device) {
	struct wlr_keyboard *wlr_keyboard = wlr_keyboard_from_input_device(device);

	struct tinywl_keyboard *keyboard = calloc(1, sizeof(*keyboard));
	keyboard->server = server;
	keyboard->wlr_keyboard = wlr_keyboard;

	struct xkb_context *context = xkb_context_new(XKB_CONTEXT_NO_FLAGS);
	struct xkb_keymap *keymap = xkb_keymap_new_from_names(context, NULL,
		XKB_KEYMAP_COMPILE_NO_FLAGS);

	wlr_keyboard_set_keymap(wlr_keyboard, keymap);
	xkb_keymap_unref(keymap);
	xkb_context_unref(context);
	wlr_keyboard_set_repeat_info(wlr_keyboard, 25, 600);

	keyboard->modifiers.notify = keyboard_handle_modifiers;
	wl_signal_add(&wlr_keyboard->events.modifiers, &keyboard->modifiers);
	keyboard->key.notify = keyboard_handle_key;
	wl_signal_add(&wlr_keyboard->events.key, &keyboard->key);
	keyboard->destroy.notify = keyboard_handle_destroy;
	wl_signal_add(&device->events.destroy, &keyboard->destroy);

	wlr_seat_set_keyboard(server->seat, keyboard->wlr_keyboard);

	wl_list_insert(&server->keyboards, &keyboard->link);
}

static void server_new_pointer(struct tinywl_server *server,
		struct wlr_input_device *device) {
	wlr_cursor_attach_input_device(server->cursor, device);
	// each nested output has its own pointer, pin it to that output's region
	struct wlr_pointer *pointer = wlr_pointer_from_input_device(device);
	struct livi_screen *s = screen_for_output_name(server, pointer->output_name);
	if (s != NULL && s->wlr_output != NULL) {
		wlr_cursor_map_input_to_output(server->cursor, device, s->wlr_output);
	}
}

static void server_new_input(struct wl_listener *listener, void *data) {
	struct tinywl_server *server =
		wl_container_of(listener, server, new_input);
	struct wlr_input_device *device = data;
	switch (device->type) {
	case WLR_INPUT_DEVICE_KEYBOARD:
		server_new_keyboard(server, device);
		break;
	case WLR_INPUT_DEVICE_POINTER:
		server_new_pointer(server, device);
		break;
	case WLR_INPUT_DEVICE_TOUCH:
		/* LIVI: the head unit is a touchscreen, route touch through the cursor */
		wlr_cursor_attach_input_device(server->cursor, device);
		server->has_touch = true;
		break;
	default:
		break;
	}
	uint32_t caps = WL_SEAT_CAPABILITY_POINTER;
	if (!wl_list_empty(&server->keyboards)) {
		caps |= WL_SEAT_CAPABILITY_KEYBOARD;
	}
	if (server->has_touch) {
		caps |= WL_SEAT_CAPABILITY_TOUCH;
	}
	wlr_seat_set_capabilities(server->seat, caps);
}

static void seat_request_cursor(struct wl_listener *listener, void *data) {
	struct tinywl_server *server = wl_container_of(
			listener, server, request_cursor);
	struct wlr_seat_pointer_request_set_cursor_event *event = data;
	struct wlr_seat_client *focused_client =
		server->seat->pointer_state.focused_client;
	if (focused_client == event->seat_client) {
		wlr_cursor_set_surface(server->cursor, event->surface,
				event->hotspot_x, event->hotspot_y);
	}
}

static void seat_pointer_focus_change(struct wl_listener *listener, void *data) {
	struct tinywl_server *server = wl_container_of(
			listener, server, pointer_focus_change);
	struct wlr_seat_pointer_focus_change_event *event = data;
	if (event->new_surface == NULL) {
		wlr_cursor_set_xcursor(server->cursor, server->cursor_mgr, "default");
	}
}

static void seat_request_set_selection(struct wl_listener *listener, void *data) {
	struct tinywl_server *server = wl_container_of(
			listener, server, request_set_selection);
	struct wlr_seat_request_set_selection_event *event = data;
	wlr_seat_set_selection(server->seat, event->source, event->serial);
}

static struct tinywl_toplevel *desktop_toplevel_at(
		struct tinywl_server *server, double lx, double ly,
		struct wlr_surface **surface, double *sx, double *sy) {
	struct wlr_scene_node *node = wlr_scene_node_at(
		&server->scene->tree.node, lx, ly, sx, sy);
	if (node == NULL || node->type != WLR_SCENE_NODE_BUFFER) {
		return NULL;
	}
	struct wlr_scene_buffer *scene_buffer = wlr_scene_buffer_from_node(node);
	struct wlr_scene_surface *scene_surface =
		wlr_scene_surface_try_from_buffer(scene_buffer);
	if (!scene_surface) {
		return NULL;
	}

	*surface = scene_surface->surface;
	struct wlr_scene_tree *tree = node->parent;
	while (tree != NULL && tree->node.data == NULL) {
		tree = tree->node.parent;
	}
	return tree->node.data;
}

static void reset_cursor_mode(struct tinywl_server *server) {
	server->cursor_mode = TINYWL_CURSOR_PASSTHROUGH;
	server->grabbed_toplevel = NULL;
}

static void process_cursor_move(struct tinywl_server *server) {
	struct tinywl_toplevel *toplevel = server->grabbed_toplevel;
	wlr_scene_node_set_position(&toplevel->scene_tree->node,
		server->cursor->x - server->grab_x,
		server->cursor->y - server->grab_y);
}

static void process_cursor_resize(struct tinywl_server *server) {
	struct tinywl_toplevel *toplevel = server->grabbed_toplevel;
	double border_x = server->cursor->x - server->grab_x;
	double border_y = server->cursor->y - server->grab_y;
	int new_left = server->grab_geobox.x;
	int new_right = server->grab_geobox.x + server->grab_geobox.width;
	int new_top = server->grab_geobox.y;
	int new_bottom = server->grab_geobox.y + server->grab_geobox.height;

	if (server->resize_edges & WLR_EDGE_TOP) {
		new_top = border_y;
		if (new_top >= new_bottom) {
			new_top = new_bottom - 1;
		}
	} else if (server->resize_edges & WLR_EDGE_BOTTOM) {
		new_bottom = border_y;
		if (new_bottom <= new_top) {
			new_bottom = new_top + 1;
		}
	}
	if (server->resize_edges & WLR_EDGE_LEFT) {
		new_left = border_x;
		if (new_left >= new_right) {
			new_left = new_right - 1;
		}
	} else if (server->resize_edges & WLR_EDGE_RIGHT) {
		new_right = border_x;
		if (new_right <= new_left) {
			new_right = new_left + 1;
		}
	}

	struct wlr_box *geo_box = &toplevel->xdg_toplevel->base->geometry;
	wlr_scene_node_set_position(&toplevel->scene_tree->node,
		new_left - geo_box->x, new_top - geo_box->y);

	int new_width = new_right - new_left;
	int new_height = new_bottom - new_top;
	wlr_xdg_toplevel_set_size(toplevel->xdg_toplevel, new_width, new_height);
}

// Hit-test the compositor decoration. Returns the screen the point is over (NULL if none) and,
// for a resize border, the edge bitmask. Buttons and the titlebar live in the top bar, resize
// borders run along the left, right and bottom edges. Coords are layout space.
enum livi_deco_hit {
	LIVI_DECO_NONE, LIVI_DECO_MIN, LIVI_DECO_FS, LIVI_DECO_CLOSE, LIVI_DECO_MOVE, LIVI_DECO_RESIZE
};

static enum livi_deco_hit deco_hit_test(struct tinywl_server *server, double lx, double ly,
		struct livi_screen **out, uint32_t *out_edges) {
	*out = NULL;
	*out_edges = 0;
	for (int i = 0; i < server->n_screens; i++) {
		struct livi_screen *s = &server->screens[i];
		if (s->fullscreen || s->width <= 0 || s->height <= 0) {
			continue;
		}
		if (lx < s->x || lx >= s->x + s->width || ly < 0 || ly >= s->height) {
			continue;
		}
		double lxw = lx - s->x;
		uint32_t edges = 0;
		if (ly >= s->height - LIVI_RESIZE_BORDER) edges |= WLR_EDGE_BOTTOM;
		if (lxw < LIVI_RESIZE_BORDER) edges |= WLR_EDGE_LEFT;
		if (lxw >= s->width - LIVI_RESIZE_BORDER) edges |= WLR_EDGE_RIGHT;

		if (ly < LIVI_TITLEBAR_H) {
			// titlebar: buttons first (full-height touch slots), then resize borders, else move
			int slot = LIVI_BTN_W + LIVI_BTN_GAP;
			int close_x = s->x + s->width - 1 * slot;
			int fs_x = s->x + s->width - 2 * slot;
			int min_x = s->x + s->width - 3 * slot;
			if (lx >= close_x && lx < close_x + LIVI_BTN_W) { *out = s; return LIVI_DECO_CLOSE; }
			if (lx >= fs_x && lx < fs_x + LIVI_BTN_W) { *out = s; return LIVI_DECO_FS; }
			if (lx >= min_x && lx < min_x + LIVI_BTN_W) { *out = s; return LIVI_DECO_MIN; }
			if (edges != 0) { *out = s; *out_edges = edges; return LIVI_DECO_RESIZE; }
			*out = s;
			return LIVI_DECO_MOVE;
		}
		if (edges != 0) { *out = s; *out_edges = edges; return LIVI_DECO_RESIZE; }
		return LIVI_DECO_NONE;
	}
	return LIVI_DECO_NONE;
}

static const char *resize_cursor_name(uint32_t edges) {
	bool b = (edges & WLR_EDGE_BOTTOM) != 0;
	bool l = (edges & WLR_EDGE_LEFT) != 0;
	bool r = (edges & WLR_EDGE_RIGHT) != 0;
	if (b && l) return "sw-resize";
	if (b && r) return "se-resize";
	if (b) return "s-resize";
	if (l) return "w-resize";
	if (r) return "e-resize";
	return "default";
}

static void process_cursor_motion(struct tinywl_server *server, uint32_t time) {
	if (server->cursor_mode == TINYWL_CURSOR_MOVE) {
		process_cursor_move(server);
		return;
	} else if (server->cursor_mode == TINYWL_CURSOR_RESIZE) {
		process_cursor_resize(server);
		return;
	}

	struct livi_screen *ds = NULL;
	uint32_t dedges = 0;
	enum livi_deco_hit dh = deco_hit_test(server, server->cursor->x, server->cursor->y,
			&ds, &dedges);
	if (ds != NULL) {
		wlr_cursor_set_xcursor(server->cursor, server->cursor_mgr,
			dh == LIVI_DECO_RESIZE ? resize_cursor_name(dedges) : "default");
		wlr_seat_pointer_clear_focus(server->seat);
		return;
	}

	double sx, sy;
	struct wlr_seat *seat = server->seat;
	struct wlr_surface *surface = NULL;
	struct tinywl_toplevel *toplevel = desktop_toplevel_at(server,
			server->cursor->x, server->cursor->y, &surface, &sx, &sy);
	if (!toplevel) {
		wlr_cursor_set_xcursor(server->cursor, server->cursor_mgr, "default");
	}
	if (surface) {
		wlr_seat_pointer_notify_enter(seat, surface, sx, sy);
		wlr_seat_pointer_notify_motion(seat, time, sx, sy);
	} else {
		wlr_seat_pointer_clear_focus(seat);
	}
}

static void server_cursor_motion(struct wl_listener *listener, void *data) {
	struct tinywl_server *server =
		wl_container_of(listener, server, cursor_motion);
	struct wlr_pointer_motion_event *event = data;
	wlr_cursor_move(server->cursor, &event->pointer->base,
			event->delta_x, event->delta_y);
	process_cursor_motion(server, event->time_msec);
}

static void server_cursor_motion_absolute(
		struct wl_listener *listener, void *data) {
	struct tinywl_server *server =
		wl_container_of(listener, server, cursor_motion_absolute);
	struct wlr_pointer_motion_absolute_event *event = data;
	wlr_cursor_warp_absolute(server->cursor, &event->pointer->base, event->x,
		event->y);
	process_cursor_motion(server, event->time_msec);
}

static void server_cursor_button(struct wl_listener *listener, void *data) {
	struct tinywl_server *server =
		wl_container_of(listener, server, cursor_button);
	struct wlr_pointer_button_event *event = data;

	// Clicks on our decoration are consumed here and never forwarded to a surface. Move and
	// resize are handed to the host compositor, which performs the interactive grab.
	struct livi_screen *deco_screen = NULL;
	uint32_t deco_edges = 0;
	enum livi_deco_hit hit = deco_hit_test(server,
			server->cursor->x, server->cursor->y, &deco_screen, &deco_edges);
	if (deco_screen != NULL) {
		if (event->state == WL_POINTER_BUTTON_STATE_PRESSED) {
			struct wlr_output *o = deco_screen->wlr_output;
			bool is_wl = o != NULL && wlr_output_is_wl(o);
			switch (hit) {
			case LIVI_DECO_CLOSE:
				if (deco_screen->ui != NULL) {
					wlr_xdg_toplevel_send_close(deco_screen->ui->xdg_toplevel);
				}
				break;
			case LIVI_DECO_MIN:
				if (is_wl) wlr_wl_output_set_minimized(o);
				break;
			case LIVI_DECO_FS:
				livi_toggle_fullscreen(deco_screen);
				break;
			case LIVI_DECO_MOVE:
				if (is_wl) wlr_wl_output_begin_move(o);
				break;
			case LIVI_DECO_RESIZE:
				if (is_wl) wlr_wl_output_begin_resize(o, deco_edges);
				break;
			default:
				break;
			}
		}
		return;
	}

	wlr_seat_pointer_notify_button(server->seat,
			event->time_msec, event->button, event->state);
	if (event->state == WL_POINTER_BUTTON_STATE_RELEASED) {
		reset_cursor_mode(server);
	} else {
		double sx, sy;
		struct wlr_surface *surface = NULL;
		struct tinywl_toplevel *toplevel = desktop_toplevel_at(server,
				server->cursor->x, server->cursor->y, &surface, &sx, &sy);
		focus_toplevel(toplevel);
	}
}

static void server_cursor_axis(struct wl_listener *listener, void *data) {
	struct tinywl_server *server =
		wl_container_of(listener, server, cursor_axis);
	struct wlr_pointer_axis_event *event = data;
	wlr_seat_pointer_notify_axis(server->seat,
			event->time_msec, event->orientation, event->delta,
			event->delta_discrete, event->source, event->relative_direction);
}

static void server_cursor_frame(struct wl_listener *listener, void *data) {
	struct tinywl_server *server =
		wl_container_of(listener, server, cursor_frame);
	wlr_seat_pointer_notify_frame(server->seat);
}

/* LIVI: touch. Event coords are [0,1] over the output the touch came from, map to that
 * output's screen and scale by its size to find the surface under the touch point. */
static void server_touch_down(struct wl_listener *listener, void *data) {
	struct tinywl_server *server = wl_container_of(listener, server, touch_down);
	struct wlr_touch_down_event *event = data;

	struct livi_screen *ts = screen_for_output_name(server, event->touch->output_name);
	if (ts == NULL) {
		ts = server->n_screens > 0 ? &server->screens[0] : NULL;
	}
	double lx = ts ? ts->x + event->x * ts->width : 0;
	double ly = ts ? event->y * ts->height : 0;
	double sx, sy;
	struct wlr_surface *surface = NULL;
	desktop_toplevel_at(server, lx, ly, &surface, &sx, &sy);
	if (surface) {
		wlr_seat_touch_notify_down(server->seat, surface,
			event->time_msec, event->touch_id, sx, sy);
	}
}

static void server_touch_motion(struct wl_listener *listener, void *data) {
	struct tinywl_server *server = wl_container_of(listener, server, touch_motion);
	struct wlr_touch_motion_event *event = data;

	struct livi_screen *ts = screen_for_output_name(server, event->touch->output_name);
	if (ts == NULL) {
		ts = server->n_screens > 0 ? &server->screens[0] : NULL;
	}
	double lx = ts ? ts->x + event->x * ts->width : 0;
	double ly = ts ? event->y * ts->height : 0;
	double sx, sy;
	struct wlr_surface *surface = NULL;
	desktop_toplevel_at(server, lx, ly, &surface, &sx, &sy);
	if (surface) {
		wlr_seat_touch_notify_motion(server->seat,
			event->time_msec, event->touch_id, sx, sy);
	}
}

static void server_touch_up(struct wl_listener *listener, void *data) {
	struct tinywl_server *server = wl_container_of(listener, server, touch_up);
	struct wlr_touch_up_event *event = data;
	wlr_seat_touch_notify_up(server->seat, event->time_msec, event->touch_id);
}

static void server_touch_frame(struct wl_listener *listener, void *data) {
	struct tinywl_server *server = wl_container_of(listener, server, touch_frame);
	wlr_seat_touch_notify_frame(server->seat);
}

static bool cal_full_output(struct tinywl_output *output);

static void output_frame(struct wl_listener *listener, void *data) {
	struct tinywl_output *output = wl_container_of(listener, output, frame);
	struct tinywl_server *server = output->server;
	struct wlr_scene_output *scene_output = wlr_scene_get_scene_output(
		server->scene, output->wlr_output);

	if (!server->cal_active || !cal_full_output(output)) {
		wlr_scene_output_commit(scene_output, NULL);
	}

	struct timespec now;
	clock_gettime(CLOCK_MONOTONIC, &now);
	wlr_scene_output_send_frame_done(scene_output, &now);
}

// Size+position a video plane so its content region fills the screen, margins
// overflowing off the output edge (the scene clips them). Zero-copy.
static void apply_video_layout(struct tinywl_toplevel *video) {
	struct livi_screen *s = video->screen;
	if (s == NULL || !video->xdg_toplevel->base->initialized) {
		return;
	}
	int top = screen_top_inset(s);
	int ow = s->width, oh = s->height - top;
	if (ow <= 0 || oh <= 0) {
		return;
	}
	if (!video->has_crop || video->vis_w <= 0 || video->vis_h <= 0 ||
			video->tier_w <= 0 || video->tier_h <= 0) {
		wlr_xdg_toplevel_set_size(video->xdg_toplevel, ow, oh);
		wlr_scene_node_set_position(&video->scene_tree->node, s->x, top);
		return;
	}
	/* contain the content into the output (uniform scale, bars only on AR mismatch) */
	double scx = (double)ow / video->vis_w;
	double scy = (double)oh / video->vis_h;
	double scale = scx < scy ? scx : scy;
	double off_x = (ow - video->vis_w * scale) / 2.0;
	double off_y = (oh - video->vis_h * scale) / 2.0;
	int tw = (int)lround(video->tier_w * scale);
	int th = (int)lround(video->tier_h * scale);
	int px = (int)lround(s->x + off_x - video->crop_l * scale);
	int py = (int)lround(top + off_y - video->crop_t * scale);
	wlr_xdg_toplevel_set_size(video->xdg_toplevel, tw, th);
	wlr_scene_node_set_position(&video->scene_tree->node, px, py);
}

static const char CAL_VERT_SRC[] =
	"attribute vec2 pos;\n"
	"varying vec2 v_uv;\n"
	"void main() {\n"
	"  v_uv = vec2(pos.x * 0.5 + 0.5, pos.y * 0.5 + 0.5);\n"
	"  gl_Position = vec4(pos, 0.0, 1.0);\n"
	"}\n";

static const char CAL_FRAG_SRC[] =
	"precision highp float;\n"
	"varying vec2 v_uv;\n"
	"uniform sampler2D tex;\n"
	"uniform float u_gamma;\n"
	"uniform float u_contrast;\n"
	"uniform vec3 u_gain;\n"
	"void main() {\n"
	"  vec3 c = texture2D(tex, v_uv).rgb;\n"
	"  c = pow(c, vec3(1.0 / u_gamma));\n"
	"  c = (c - 0.5) * u_contrast + 0.5;\n"
	"  c = clamp(c * u_gain, 0.0, 1.0);\n"
	"  gl_FragColor = vec4(c, 1.0);\n"
	"}\n";

static GLuint cal_compile(GLenum type, const char *src) {
	GLuint sh = glCreateShader(type);
	glShaderSource(sh, 1, &src, NULL);
	glCompileShader(sh);
	GLint ok = GL_FALSE;
	glGetShaderiv(sh, GL_COMPILE_STATUS, &ok);
	if (!ok) {
		char log[512];
		glGetShaderInfoLog(sh, sizeof(log), NULL, log);
		wlr_log(WLR_ERROR, "livi cal: shader compile failed: %s", log);
		glDeleteShader(sh);
		return 0;
	}
	return sh;
}

// Compile the calibration program. EGL context must be current.
static bool cal_ensure_program(struct tinywl_server *server) {
	if (server->cal_prog) return true;
	if (server->cal_prog_failed) return false;
	GLuint vs = cal_compile(GL_VERTEX_SHADER, CAL_VERT_SRC);
	GLuint fs = cal_compile(GL_FRAGMENT_SHADER, CAL_FRAG_SRC);
	if (!vs || !fs) {
		if (vs) glDeleteShader(vs);
		if (fs) glDeleteShader(fs);
		server->cal_prog_failed = true;
		return false;
	}
	GLuint prog = glCreateProgram();
	glAttachShader(prog, vs);
	glAttachShader(prog, fs);
	glBindAttribLocation(prog, 0, "pos");
	glLinkProgram(prog);
	glDeleteShader(vs);
	glDeleteShader(fs);
	GLint ok = GL_FALSE;
	glGetProgramiv(prog, GL_LINK_STATUS, &ok);
	if (!ok) {
		char log[512];
		glGetProgramInfoLog(prog, sizeof(log), NULL, log);
		wlr_log(WLR_ERROR, "livi cal: program link failed: %s", log);
		glDeleteProgram(prog);
		server->cal_prog_failed = true;
		return false;
	}
	server->cal_prog = prog;
	server->cal_loc_gamma = glGetUniformLocation(prog, "u_gamma");
	server->cal_loc_contrast = glGetUniformLocation(prog, "u_contrast");
	server->cal_loc_gain = glGetUniformLocation(prog, "u_gain");
	server->cal_loc_tex = glGetUniformLocation(prog, "tex");
	return true;
}

static PFNEGLCREATEIMAGEKHRPROC p_eglCreateImageKHR;
static PFNEGLDESTROYIMAGEKHRPROC p_eglDestroyImageKHR;
static PFNGLEGLIMAGETARGETTEXTURE2DOESPROC p_glEGLImageTargetTexture2DOES;

static bool cal_load_egl_ext(void) {
	if (p_eglCreateImageKHR) {
		return true;
	}
	p_eglCreateImageKHR = (PFNEGLCREATEIMAGEKHRPROC)eglGetProcAddress("eglCreateImageKHR");
	p_eglDestroyImageKHR = (PFNEGLDESTROYIMAGEKHRPROC)eglGetProcAddress("eglDestroyImageKHR");
	p_glEGLImageTargetTexture2DOES =
		(PFNGLEGLIMAGETARGETTEXTURE2DOESPROC)eglGetProcAddress("glEGLImageTargetTexture2DOES");
	return p_eglCreateImageKHR && p_eglDestroyImageKHR && p_glEGLImageTargetTexture2DOES;
}

// EGLImage + GL texture + FBO for one swapchain buffer, cached on it via a wlr_addon.
struct cal_target {
	struct wlr_addon addon;
	struct tinywl_server *server;
	EGLImageKHR image;
	GLuint tex;
	GLuint fbo;
};

static void cal_target_destroy(struct wlr_addon *addon) {
	struct cal_target *t = wl_container_of(addon, t, addon);
	struct wlr_egl *egl = wlr_gles2_renderer_get_egl(t->server->renderer);
	EGLDisplay dpy = wlr_egl_get_display(egl);
	eglMakeCurrent(dpy, EGL_NO_SURFACE, EGL_NO_SURFACE, wlr_egl_get_context(egl));
	glDeleteFramebuffers(1, &t->fbo);
	glDeleteTextures(1, &t->tex);
	p_eglDestroyImageKHR(dpy, t->image);
	wlr_addon_finish(&t->addon);
	free(t);
}

static const struct wlr_addon_interface cal_target_impl = {
	.name = "livi_cal_target",
	.destroy = cal_target_destroy,
};

// Get or build the FBO that renders into `buf`. EGL context must be current.
static struct cal_target *cal_target_get(struct tinywl_server *server, struct wlr_buffer *buf) {
	struct wlr_addon *existing = wlr_addon_find(&buf->addons, server, &cal_target_impl);
	if (existing) {
		struct cal_target *t = wl_container_of(existing, t, addon);
		return t;
	}
	if (!cal_load_egl_ext()) {
		return NULL;
	}
	struct wlr_dmabuf_attributes attribs;
	if (!wlr_buffer_get_dmabuf(buf, &attribs)) {
		return NULL;
	}
	EGLDisplay dpy = wlr_egl_get_display(wlr_gles2_renderer_get_egl(server->renderer));
	EGLint a[50];
	int i = 0;
	a[i++] = EGL_WIDTH; a[i++] = attribs.width;
	a[i++] = EGL_HEIGHT; a[i++] = attribs.height;
	a[i++] = EGL_LINUX_DRM_FOURCC_EXT; a[i++] = (EGLint)attribs.format;
	a[i++] = EGL_DMA_BUF_PLANE0_FD_EXT; a[i++] = attribs.fd[0];
	a[i++] = EGL_DMA_BUF_PLANE0_OFFSET_EXT; a[i++] = (EGLint)attribs.offset[0];
	a[i++] = EGL_DMA_BUF_PLANE0_PITCH_EXT; a[i++] = (EGLint)attribs.stride[0];
	if (attribs.modifier != DRM_FORMAT_MOD_INVALID) {
		a[i++] = EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT;
		a[i++] = (EGLint)(attribs.modifier & 0xFFFFFFFF);
		a[i++] = EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT;
		a[i++] = (EGLint)(attribs.modifier >> 32);
	}
	a[i++] = EGL_NONE;
	EGLImageKHR img = p_eglCreateImageKHR(dpy, EGL_NO_CONTEXT, EGL_LINUX_DMA_BUF_EXT, NULL, a);
	if (img == EGL_NO_IMAGE_KHR) {
		wlr_log(WLR_ERROR, "livi cal: eglCreateImageKHR for target failed");
		return NULL;
	}
	GLuint tex;
	glGenTextures(1, &tex);
	glBindTexture(GL_TEXTURE_2D, tex);
	p_glEGLImageTargetTexture2DOES(GL_TEXTURE_2D, img);
	GLuint fbo;
	glGenFramebuffers(1, &fbo);
	glBindFramebuffer(GL_FRAMEBUFFER, fbo);
	glFramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, tex, 0);
	GLenum st = glCheckFramebufferStatus(GL_FRAMEBUFFER);
	glBindFramebuffer(GL_FRAMEBUFFER, 0);
	glBindTexture(GL_TEXTURE_2D, 0);
	if (st != GL_FRAMEBUFFER_COMPLETE) {
		wlr_log(WLR_ERROR, "livi cal: target FBO incomplete (0x%x)", st);
		glDeleteFramebuffers(1, &fbo);
		glDeleteTextures(1, &tex);
		p_eglDestroyImageKHR(dpy, img);
		return NULL;
	}
	struct cal_target *t = calloc(1, sizeof(*t));
	t->server = server;
	t->image = img;
	t->tex = tex;
	t->fbo = fbo;
	wlr_addon_init(&t->addon, &buf->addons, server, &cal_target_impl);
	return t;
}

// Composite the whole scene into an intermediate buffer, run the gamma shader over it, commit
// the result. Covers UI + video uniformly, GPU->GPU, no readback. Returns false on any setup
// failure so the caller falls back to a normal commit and never blacks out.
static bool cal_full_output(struct tinywl_output *output) {
	struct tinywl_server *server = output->server;
	struct wlr_scene_output *scene_output =
		wlr_scene_get_scene_output(server->scene, output->wlr_output);
	if (scene_output == NULL) {
		return false;
	}
	int ow = output->wlr_output->width, oh = output->wlr_output->height;
	if (ow <= 0 || oh <= 0) {
		return false;
	}
	struct wlr_egl *egl = wlr_gles2_renderer_get_egl(server->renderer);
	EGLDisplay dpy = wlr_egl_get_display(egl);
	eglMakeCurrent(dpy, EGL_NO_SURFACE, EGL_NO_SURFACE, wlr_egl_get_context(egl));
	if (!cal_ensure_program(server)) {
		return false;
	}
	const struct wlr_drm_format_set *fmts = wlr_renderer_get_render_formats(server->renderer);
	const struct wlr_drm_format *fmt =
		fmts ? wlr_drm_format_set_get(fmts, DRM_FORMAT_XRGB8888) : NULL;
	if (fmt == NULL) {
		return false;
	}
	if (!output->cal_inter || output->cal_ow != ow || output->cal_oh != oh) {
		if (output->cal_inter) wlr_swapchain_destroy(output->cal_inter);
		if (output->cal_final) wlr_swapchain_destroy(output->cal_final);
		output->cal_inter = wlr_swapchain_create(server->allocator, ow, oh, fmt);
		output->cal_final = wlr_swapchain_create(server->allocator, ow, oh, fmt);
		output->cal_ow = ow;
		output->cal_oh = oh;
	}
	if (!output->cal_inter || !output->cal_final) {
		return false;
	}

	struct wlr_output_state state;
	wlr_output_state_init(&state);
	struct wlr_scene_output_state_options opts = { .swapchain = output->cal_inter };
	if (!wlr_scene_output_build_state(scene_output, &state, &opts)
			|| !(state.committed & WLR_OUTPUT_STATE_BUFFER) || state.buffer == NULL) {
		wlr_output_state_finish(&state);
		return false;
	}

	struct wlr_buffer *dst = wlr_swapchain_acquire(output->cal_final);
	struct cal_target *t = dst ? cal_target_get(server, dst) : NULL;
	struct wlr_texture *src = wlr_texture_from_buffer(server->renderer, state.buffer);
	if (!t || !src) {
		if (src) wlr_texture_destroy(src);
		if (dst) wlr_buffer_unlock(dst);
		wlr_output_state_finish(&state);
		return false;
	}
	struct wlr_gles2_texture_attribs sa;
	wlr_gles2_texture_get_attribs(src, &sa);

	eglMakeCurrent(dpy, EGL_NO_SURFACE, EGL_NO_SURFACE, wlr_egl_get_context(egl));
	glBindFramebuffer(GL_FRAMEBUFFER, t->fbo);
	glViewport(0, 0, ow, oh);
	glDisable(GL_BLEND);
	glUseProgram(server->cal_prog);
	glUniform1f(server->cal_loc_gamma, server->cal_gamma);
	glUniform1f(server->cal_loc_contrast, server->cal_contrast);
	glUniform3f(server->cal_loc_gain, server->cal_gain[0], server->cal_gain[1], server->cal_gain[2]);
	glActiveTexture(GL_TEXTURE0);
	glBindTexture(sa.target, sa.tex);
	glTexParameteri(sa.target, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
	glTexParameteri(sa.target, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
	glUniform1i(server->cal_loc_tex, 0);
	static const GLfloat quad[] = { -1, -1, 1, -1, -1, 1, 1, 1 };
	glEnableVertexAttribArray(0);
	glVertexAttribPointer(0, 2, GL_FLOAT, GL_FALSE, 0, quad);
	glDrawArrays(GL_TRIANGLE_STRIP, 0, 4);
	glDisableVertexAttribArray(0);
	glBindTexture(sa.target, 0);
	glUseProgram(0);
	glBindFramebuffer(GL_FRAMEBUFFER, 0);
	glFlush();
	wlr_texture_destroy(src);

	wlr_output_state_set_buffer(&state, dst);
	wlr_buffer_unlock(dst);
	pixman_region32_t damage;
	pixman_region32_init_rect(&damage, 0, 0, ow, oh);
	wlr_output_state_set_damage(&state, &damage);
	pixman_region32_fini(&damage);
	bool ok = wlr_output_commit_state(output->wlr_output, &state);
	wlr_output_state_finish(&state);
	return ok;
}

// Wrap a cairo ARGB32 image surface as a wlr_buffer so it can live in the scene graph.
struct livi_deco_buffer {
	struct wlr_buffer base;
	cairo_surface_t *surface;
};

static void livi_deco_buffer_destroy(struct wlr_buffer *buffer) {
	struct livi_deco_buffer *b = wl_container_of(buffer, b, base);
	cairo_surface_destroy(b->surface);
	free(b);
}

static bool livi_deco_buffer_begin_data_ptr_access(struct wlr_buffer *buffer, uint32_t flags,
		void **data, uint32_t *format, size_t *stride) {
	(void)flags;
	struct livi_deco_buffer *b = wl_container_of(buffer, b, base);
	*data = cairo_image_surface_get_data(b->surface);
	*format = DRM_FORMAT_ARGB8888;
	*stride = cairo_image_surface_get_stride(b->surface);
	return true;
}

static void livi_deco_buffer_end_data_ptr_access(struct wlr_buffer *buffer) {
	(void)buffer;
}

static const struct wlr_buffer_impl livi_deco_buffer_impl = {
	.destroy = livi_deco_buffer_destroy,
	.begin_data_ptr_access = livi_deco_buffer_begin_data_ptr_access,
	.end_data_ptr_access = livi_deco_buffer_end_data_ptr_access,
};

static void livi_scene_set_cairo(struct wlr_scene_buffer *sb, cairo_surface_t *surface) {
	struct livi_deco_buffer *b = calloc(1, sizeof(*b));
	if (b == NULL) {
		cairo_surface_destroy(surface);
		return;
	}
	b->surface = surface;
	wlr_buffer_init(&b->base, &livi_deco_buffer_impl,
		cairo_image_surface_get_width(surface), cairo_image_surface_get_height(surface));
	wlr_scene_buffer_set_buffer(sb, &b->base);
	wlr_buffer_drop(&b->base);
}

enum livi_btn_sym { LIVI_SYM_MIN, LIVI_SYM_FS, LIVI_SYM_CLOSE };

static cairo_surface_t *livi_draw_button(enum livi_btn_sym sym, int w, int h) {
	cairo_surface_t *s = cairo_image_surface_create(CAIRO_FORMAT_ARGB32, w, h);
	cairo_t *cr = cairo_create(s);
	double cx = w / 2.0, cy = h / 2.0;
	double rad = h * 0.34;
	cairo_arc(cr, cx, cy, rad, 0, 2 * M_PI);
	cairo_set_source_rgba(cr, 1, 1, 1, 0.10);
	cairo_fill(cr);

	cairo_set_source_rgba(cr, 1, 1, 1, 0.80);
	cairo_set_line_width(cr, 1.5);
	cairo_set_line_cap(cr, CAIRO_LINE_CAP_ROUND);
	cairo_set_line_join(cr, CAIRO_LINE_JOIN_ROUND);
	double g = rad * 0.33;
	switch (sym) {
	case LIVI_SYM_CLOSE:
		cairo_move_to(cr, cx - g, cy - g); cairo_line_to(cr, cx + g, cy + g);
		cairo_move_to(cr, cx + g, cy - g); cairo_line_to(cr, cx - g, cy + g);
		cairo_stroke(cr);
		break;
	case LIVI_SYM_MIN:
		cairo_move_to(cr, cx - g, cy); cairo_line_to(cr, cx + g, cy);
		cairo_stroke(cr);
		break;
	case LIVI_SYM_FS: {
		double e = g * 0.8;
		cairo_move_to(cr, cx - g + e, cy - g); cairo_line_to(cr, cx - g, cy - g);
		cairo_line_to(cr, cx - g, cy - g + e);
		cairo_move_to(cr, cx + g - e, cy + g); cairo_line_to(cr, cx + g, cy + g);
		cairo_line_to(cr, cx + g, cy + g - e);
		cairo_stroke(cr);
		break;
	}
	}
	cairo_destroy(cr);
	cairo_surface_flush(s);
	return s;
}

static cairo_surface_t *livi_draw_title(const char *text, int h) {
	cairo_surface_t *probe = cairo_image_surface_create(CAIRO_FORMAT_ARGB32, 1, 1);
	cairo_t *pc = cairo_create(probe);
	cairo_select_font_face(pc, "sans-serif", CAIRO_FONT_SLANT_NORMAL, CAIRO_FONT_WEIGHT_NORMAL);
	double fsize = h * 0.5;
	cairo_set_font_size(pc, fsize);
	cairo_text_extents_t ext;
	cairo_text_extents(pc, text, &ext);
	cairo_destroy(pc);
	cairo_surface_destroy(probe);

	int w = (int)ceil(ext.width) + 4;
	if (w < 1) w = 1;
	cairo_surface_t *s = cairo_image_surface_create(CAIRO_FORMAT_ARGB32, w, h);
	cairo_t *cr = cairo_create(s);
	cairo_select_font_face(cr, "sans-serif", CAIRO_FONT_SLANT_NORMAL, CAIRO_FONT_WEIGHT_NORMAL);
	cairo_set_font_size(cr, fsize);
	cairo_set_source_rgba(cr, 1, 1, 1, 0.85);
	cairo_move_to(cr, 2 - ext.x_bearing, (h - ext.height) / 2.0 - ext.y_bearing);
	cairo_show_text(cr, text);
	cairo_destroy(cr);
	cairo_surface_flush(s);
	return s;
}

static cairo_surface_t *livi_draw_titlebar(int w, int h) {
	cairo_surface_t *s = cairo_image_surface_create(CAIRO_FORMAT_ARGB32, w, h);
	cairo_t *cr = cairo_create(s);
	cairo_set_source_rgba(cr, 0.13, 0.13, 0.16, 1.0);
	cairo_paint(cr);
	cairo_destroy(cr);
	cairo_surface_flush(s);
	return s;
}

static void apply_ui_layout(struct livi_screen *s) {
	if (s == NULL) {
		return;
	}
	int ow = s->width, oh = s->height;
	if (ow <= 0 || oh <= 0) {
		return;
	}
	bool show = !s->fullscreen;
	int top = screen_top_inset(s);
	int slot = LIVI_BTN_W + LIVI_BTN_GAP;

	if (s->titlebar != NULL) {
		wlr_scene_node_set_enabled(&s->titlebar->node, show);
		wlr_scene_node_set_position(&s->titlebar->node, s->x, 0);
		if (show && ow != s->titlebar_w) {
			livi_scene_set_cairo(s->titlebar, livi_draw_titlebar(ow, LIVI_TITLEBAR_H));
			s->titlebar_w = ow;
		}
	}
	if (s->title != NULL) {
		wlr_scene_node_set_enabled(&s->title->node, show);
		wlr_scene_node_set_position(&s->title->node, s->x + 12, 0);
	}
	if (s->btn_close != NULL) {
		wlr_scene_node_set_enabled(&s->btn_close->node, show);
		wlr_scene_node_set_position(&s->btn_close->node, s->x + ow - 1 * slot, 0);
	}
	if (s->btn_fs != NULL) {
		wlr_scene_node_set_enabled(&s->btn_fs->node, show);
		wlr_scene_node_set_position(&s->btn_fs->node, s->x + ow - 2 * slot, 0);
	}
	if (s->btn_min != NULL) {
		wlr_scene_node_set_enabled(&s->btn_min->node, show);
		wlr_scene_node_set_position(&s->btn_min->node, s->x + ow - 3 * slot, 0);
	}

	if (s->ui != NULL && s->ui->xdg_toplevel->base->initialized) {
		wlr_scene_node_set_position(&s->ui->scene_tree->node, s->x, top);
		// Tiled on all edges so the client renders exactly our size, not its own floating size.
		wlr_xdg_toplevel_set_tiled(s->ui->xdg_toplevel,
			WLR_EDGE_TOP | WLR_EDGE_BOTTOM | WLR_EDGE_LEFT | WLR_EDGE_RIGHT);
		wlr_xdg_toplevel_set_size(s->ui->xdg_toplevel, ow, oh - top);
	}
}

static void livi_toggle_fullscreen(struct livi_screen *s) {
	if (s == NULL || s->ui == NULL) {
		return;
	}
	bool want = !s->fullscreen;
	s->fullscreen = want;
	if (s->wlr_output != NULL && wlr_output_is_wl(s->wlr_output)) {
		wlr_wl_output_set_fullscreen(s->wlr_output, want);
	}
	if (s->ui->xdg_toplevel->base->initialized) {
		wlr_xdg_toplevel_set_fullscreen(s->ui->xdg_toplevel, want);
	}
	apply_ui_layout(s);
}

static void output_request_state(struct wl_listener *listener, void *data) {
	struct tinywl_output *output = wl_container_of(listener, output, request_state);
	const struct wlr_output_event_request_state *event = data;
	wlr_output_commit_state(output->wlr_output, event->state);

	struct livi_screen *s = output->screen;
	if (s == NULL) {
		return;
	}
	s->width = output->wlr_output->width;
	s->height = output->wlr_output->height;
	struct tinywl_toplevel *v;
	wl_list_for_each(v, &output->server->videos, video_link) {
		if (v->screen == s) {
			apply_video_layout(v);
		}
	}
	apply_ui_layout(s);
	if (s->backdrop) {
		wlr_scene_rect_set_size(s->backdrop, s->width, s->height);
	}
}

static void output_destroy(struct wl_listener *listener, void *data) {
	struct tinywl_output *output = wl_container_of(listener, output, destroy);
	struct livi_screen *s = output->screen;

	if (output->cal_inter) wlr_swapchain_destroy(output->cal_inter);
	if (output->cal_final) wlr_swapchain_destroy(output->cal_final);

	if (s != NULL) {
		s->wlr_output = NULL;
		if (s->backdrop != NULL) {
			wlr_scene_node_destroy(&s->backdrop->node);
			s->backdrop = NULL;
		}
		if (s->titlebar != NULL) {
			wlr_scene_node_destroy(&s->titlebar->node);
			s->titlebar = NULL;
		}
		if (s->btn_fs != NULL) {
			wlr_scene_node_destroy(&s->btn_fs->node);
			s->btn_fs = NULL;
		}
		if (s->btn_close != NULL) {
			wlr_scene_node_destroy(&s->btn_close->node);
			s->btn_close = NULL;
		}
		if (s == &output->server->screens[0]) {
			/* LIVI: the main window is gone -> the app is closing, take everything down */
			wlr_log(WLR_INFO, "livi: main output gone -> shutting down");
			wl_display_terminate(output->server->wl_display);
		} else if (s->ui != NULL && s->ui->xdg_toplevel->base->initialized) {
			/* a secondary host window was closed directly -> ask its UI to close too */
			wlr_xdg_toplevel_send_close(s->ui->xdg_toplevel);
		}
	}

	wl_list_remove(&output->frame.link);
	wl_list_remove(&output->request_state.link);
	wl_list_remove(&output->destroy.link);
	wl_list_remove(&output->link);
	free(output);
}

static const char *role_title(const char *role) {
	if (strcmp(role, "main") == 0) return "LIVI";
	if (strcmp(role, "dash") == 0) return "Dash";
	if (strcmp(role, "aux") == 0) return "Auxiliary";
	return role;
}

static void server_new_output(struct wl_listener *listener, void *data) {
	struct tinywl_server *server =
		wl_container_of(listener, server, new_output);
	struct wlr_output *wlr_output = data;

	wlr_output_init_render(wlr_output, server->allocator, server->renderer);

	struct wlr_output_state state;
	wlr_output_state_init(&state);
	wlr_output_state_set_enabled(&state, true);

	// nested window size: per-screen request wins, else LIVI_OUTPUT_SIZE ("WxH", default 1280x720)
	int ow = 1280, oh = 720;
	const char *size = getenv("LIVI_OUTPUT_SIZE");
	if (size != NULL) {
		int w, h;
		if (sscanf(size, "%dx%d", &w, &h) == 2 && w > 0 && h > 0) {
			ow = w;
			oh = h;
		}
	}
	if (server->pending_screen != NULL &&
			server->pending_screen->req_width > 0 &&
			server->pending_screen->req_height > 0) {
		ow = server->pending_screen->req_width;
		oh = server->pending_screen->req_height;
	}
	wlr_output_state_set_custom_mode(&state, ow, oh, 0);

	/* LIVI: bind to the host-requested screen (NULL -> main), each role keeps its own
	 * x-slot so its nested window renders just that screen's content */
	struct livi_screen *s = server->pending_screen ? server->pending_screen
		: &server->screens[0];
	server->pending_screen = NULL;

	// Set title/app_id before the first commit: wlroots applies them when the output
	// maps, and the host panel resolves the window icon from app_id at map time.
	if (wlr_output_is_wl(wlr_output)) {
		wlr_wl_output_set_title(wlr_output, role_title(s->role));
		const char *app_id = getenv("LIVI_OUTPUT_APP_ID");
		wlr_wl_output_set_app_id(wlr_output, app_id ? app_id : "livi");
	}

	wlr_output_commit_state(wlr_output, &state);
	wlr_output_state_finish(&state);

	s->wlr_output = wlr_output;
	s->width = wlr_output->width;
	s->height = wlr_output->height;
	s->x = (int32_t)(s - server->screens) * LIVI_SCREEN_X_SLOT;
	// Report the panels millimetres per pixel
	if (wlr_output->phys_width > 0 && wlr_output->phys_height > 0 &&
			wlr_output->width > 0 && wlr_output->height > 0) {
		char panel[128];
		snprintf(panel, sizeof(panel), "panel %s %d %d %d %d\n", s->role,
			wlr_output->phys_width, wlr_output->phys_height,
			wlr_output->width, wlr_output->height);
		ctrl_send(server, panel);
	}
	wlr_log(WLR_INFO, "livi: new output -> screen '%s' at x=%d (%dx%d)",
		s->role, s->x, s->width, s->height);

	struct tinywl_output *output = calloc(1, sizeof(*output));
	output->wlr_output = wlr_output;
	output->server = server;
	output->screen = s;

	output->frame.notify = output_frame;
	wl_signal_add(&wlr_output->events.frame, &output->frame);

	output->request_state.notify = output_request_state;
	wl_signal_add(&wlr_output->events.request_state, &output->request_state);

	output->destroy.notify = output_destroy;
	wl_signal_add(&wlr_output->events.destroy, &output->destroy);

	wl_list_insert(&server->outputs, &output->link);

	struct wlr_output_layout_output *l_output = wlr_output_layout_add(server->output_layout,
		wlr_output, s->x, 0);
	struct wlr_scene_output *scene_output = wlr_scene_output_create(server->scene, wlr_output);
	wlr_scene_output_layout_add_output(server->scene_layout, l_output, scene_output);

	float black[4] = {0.0f, 0.0f, 0.0f, 1.0f};
	float magenta[4] = {0.55f, 0.0f, 0.55f, 1.0f};
	const float *color = getenv("LIVI_DEBUG_BG") ? magenta
		: s->has_backdrop_color ? s->backdrop_color : black;
	s->backdrop = wlr_scene_rect_create(server->layer_bg, s->width, s->height, color);
	wlr_scene_node_set_position(&s->backdrop->node, s->x, 0);

	/* compositor-drawn titlebar + title + round controls, cairo-rendered. Created bottom-up so
	 * the title and buttons sit above the bar; apply_ui_layout places them and draws the bar. */
	s->titlebar = wlr_scene_buffer_create(server->layer_deco, NULL);
	s->title = wlr_scene_buffer_create(server->layer_deco, NULL);
	s->btn_min = wlr_scene_buffer_create(server->layer_deco, NULL);
	s->btn_fs = wlr_scene_buffer_create(server->layer_deco, NULL);
	s->btn_close = wlr_scene_buffer_create(server->layer_deco, NULL);
	if (s->title != NULL)
		livi_scene_set_cairo(s->title, livi_draw_title(role_title(s->role), LIVI_TITLEBAR_H));
	if (s->btn_min != NULL)
		livi_scene_set_cairo(s->btn_min, livi_draw_button(LIVI_SYM_MIN, LIVI_BTN_W, LIVI_TITLEBAR_H));
	if (s->btn_fs != NULL)
		livi_scene_set_cairo(s->btn_fs, livi_draw_button(LIVI_SYM_FS, LIVI_BTN_W, LIVI_TITLEBAR_H));
	if (s->btn_close != NULL)
		livi_scene_set_cairo(s->btn_close, livi_draw_button(LIVI_SYM_CLOSE, LIVI_BTN_W, LIVI_TITLEBAR_H));

	/* a UI/video toplevel may have mapped before this output existed, reflow onto it now */
	apply_ui_layout(s);
	struct tinywl_toplevel *v;
	wl_list_for_each(v, &server->videos, video_link) {
		if (v->screen == s) {
			apply_video_layout(v);
		}
	}
}

static void xdg_toplevel_map(struct wl_listener *listener, void *data) {
	struct tinywl_toplevel *toplevel = wl_container_of(listener, toplevel, map);

	struct livi_screen *s = toplevel->screen;

	wl_list_insert(&toplevel->server->toplevels, &toplevel->link);

	if (toplevel->is_video) {
		// within the video layer: the main stream sits above secondary streams (cluster)
		if (strcmp(toplevel->tag, "main") == 0) {
			wlr_scene_node_raise_to_top(&toplevel->scene_tree->node);
		} else {
			wlr_scene_node_lower_to_bottom(&toplevel->scene_tree->node);
		}
		apply_video_layout(toplevel);
		return;
	}

	apply_ui_layout(s);
	focus_toplevel(toplevel);
}

static void xdg_toplevel_unmap(struct wl_listener *listener, void *data) {
	struct tinywl_toplevel *toplevel = wl_container_of(listener, toplevel, unmap);

	if (toplevel == toplevel->server->grabbed_toplevel) {
		reset_cursor_mode(toplevel->server);
	}

	wl_list_remove(&toplevel->link);
}

static void xdg_toplevel_commit(struct wl_listener *listener, void *data) {
	struct tinywl_toplevel *toplevel = wl_container_of(listener, toplevel, commit);
	struct tinywl_server *server = toplevel->server;

	if (toplevel->xdg_toplevel->base->initial_commit && toplevel->screen == NULL) {
		// surface initialized: now safe to force the server-side decoration mode
		if (toplevel->decoration != NULL) {
			wlr_xdg_toplevel_decoration_v1_set_mode(toplevel->decoration,
				WLR_XDG_TOPLEVEL_DECORATION_V1_MODE_SERVER_SIDE);
		}
		// classify + route: video planes are waylandsink with app_id "livi-video" (set by
		// gst-host) and carry the claim tag. Everything else is Electron UI (routed by its
		// "livi:<role>" title, untitled -> main).
		const char *app_id = toplevel->xdg_toplevel->app_id;
		const char *title = toplevel->xdg_toplevel->title;
		bool is_ui = !(app_id && strcmp(app_id, "livi-video") == 0);
		struct livi_screen *s = NULL;

		if (is_ui) {
			if (title && strncmp(title, "livi:", 5) == 0) {
				s = screen_by_role(server, title + 5);
			}
			if (s == NULL) {
				s = &server->screens[0];
			}
			toplevel->is_video = false;
			const char *ui_app = getenv("LIVI_OUTPUT_APP_ID");
			if (ui_app == NULL) {
				ui_app = "dev.f-io.livi";
			}
			if (!(app_id && strcmp(app_id, ui_app) == 0)) {
				toplevel->is_dialog = true;
				wlr_xdg_toplevel_set_size(toplevel->xdg_toplevel, 0, 0);
			} else {
				s->ui = toplevel;
			}
		} else {
			toplevel->is_video = true;
			if (server->n_pending_video_tags > 0) {
				// take the oldest claim; the host serialises claims (bound-ack below) so at
				// most one is ever pending, making this assignment race-free
				snprintf(toplevel->tag, sizeof(toplevel->tag), "%s",
					server->pending_video_tags[0]);
				for (int i = 1; i < server->n_pending_video_tags; i++) {
					memcpy(server->pending_video_tags[i - 1],
						server->pending_video_tags[i],
						sizeof(server->pending_video_tags[0]));
				}
				server->n_pending_video_tags--;
				char ack[80];
				snprintf(ack, sizeof(ack), "bound %s\n", toplevel->tag);
				ctrl_send(server, ack);
			}
			if (toplevel->tag[0]) {
				struct tinywl_toplevel *stale;
				wl_list_for_each(stale, &server->videos, video_link) {
					if (stale->tag[0] && strcmp(stale->tag, toplevel->tag) == 0) {
						stale->tag[0] = '\0';
						wlr_scene_node_set_enabled(&stale->scene_tree->node, false);
					}
				}
			}
			s = &server->screens[0];   // default; videocfg moves it to its target screen
			wl_list_insert(&server->videos, &toplevel->video_link);
		}
		toplevel->screen = s;
		struct wlr_scene_tree *layer = toplevel->is_dialog ? server->layer_overlay
			: toplevel->is_video ? server->layer_video : server->layer_ui;
		wlr_scene_node_reparent(&toplevel->scene_tree->node, layer);
		wlr_log(WLR_INFO, "livi: app_id='%s' title='%s' tag='%s' -> %s on screen '%s'",
			app_id ? app_id : "(null)", title ? title : "(null)", toplevel->tag,
			toplevel->is_dialog ? "dialog" : toplevel->is_video ? "video" : "ui", s->role);

		if (toplevel->is_video) {
			apply_video_layout(toplevel);
		} else {
			apply_ui_layout(s);
		}

		/* a videocfg/videoshow may have arrived before this surface existed, apply it */
		if (toplevel->is_video && toplevel->tag[0]) {
			struct livi_video_cfg *cfg = cfg_for_tag(server, toplevel->tag, false);
			if (cfg != NULL) {
				apply_cfg_to_video(server, cfg, toplevel);
			}
		}
	}

	if (toplevel->is_dialog && toplevel->screen != NULL
			&& !toplevel->xdg_toplevel->base->initial_commit) {
		int w = toplevel->xdg_toplevel->base->geometry.width;
		int h = toplevel->xdg_toplevel->base->geometry.height;
		if (w <= 0 || h <= 0) {
			w = toplevel->xdg_toplevel->base->surface->current.width;
			h = toplevel->xdg_toplevel->base->surface->current.height;
		}
		if (w > 0 && h > 0) {
			struct livi_screen *s = toplevel->screen;
			int x = s->x + (s->width - w) / 2;
			int y = (s->height - h) / 2;
			wlr_scene_node_set_position(&toplevel->scene_tree->node,
				x < s->x ? s->x : x, y < 0 ? 0 : y);
		}
	}
}

static void xdg_toplevel_destroy(struct wl_listener *listener, void *data) {
	struct tinywl_toplevel *toplevel = wl_container_of(listener, toplevel, destroy);

	if (toplevel->is_video) {
		wl_list_remove(&toplevel->video_link);
	}
	struct livi_screen *s = toplevel->screen;
	if (s != NULL && s->ui == toplevel) {
		s->ui = NULL;
		/* LIVI: the main UI quit -> the app is closing, terminate the loop. On a normal
		 * close this exits, on a "restart" (full_restart set) main() re-execs us. */
		if (toplevel->server->n_screens > 0 && s == &toplevel->server->screens[0]) {
			wlr_log(WLR_INFO, "livi: main UI toplevel gone -> shutting down");
			wl_display_terminate(toplevel->server->wl_display);
		}
	}

	wl_list_remove(&toplevel->map.link);
	wl_list_remove(&toplevel->unmap.link);
	wl_list_remove(&toplevel->commit.link);
	wl_list_remove(&toplevel->destroy.link);
	wl_list_remove(&toplevel->request_move.link);
	wl_list_remove(&toplevel->request_resize.link);
	wl_list_remove(&toplevel->request_maximize.link);
	wl_list_remove(&toplevel->request_fullscreen.link);

	free(toplevel);
}

static void begin_interactive(struct tinywl_toplevel *toplevel,
		enum tinywl_cursor_mode mode, uint32_t edges) {
	// clients never move/resize themselves, the host window resizes, we reflow
	(void)toplevel;
	(void)mode;
	(void)edges;
}

static void xdg_toplevel_request_move(
		struct wl_listener *listener, void *data) {
	struct tinywl_toplevel *toplevel = wl_container_of(listener, toplevel, request_move);
	begin_interactive(toplevel, TINYWL_CURSOR_MOVE, 0);
}

static void xdg_toplevel_request_resize(
		struct wl_listener *listener, void *data) {
	struct wlr_xdg_toplevel_resize_event *event = data;
	struct tinywl_toplevel *toplevel = wl_container_of(listener, toplevel, request_resize);
	begin_interactive(toplevel, TINYWL_CURSOR_RESIZE, event->edges);
}

static void xdg_toplevel_request_maximize(
		struct wl_listener *listener, void *data) {
	struct tinywl_toplevel *toplevel =
		wl_container_of(listener, toplevel, request_maximize);
	if (toplevel->xdg_toplevel->base->initialized) {
		wlr_xdg_surface_schedule_configure(toplevel->xdg_toplevel->base);
	}
}

static void xdg_toplevel_request_fullscreen(
		struct wl_listener *listener, void *data) {
	struct tinywl_toplevel *toplevel =
		wl_container_of(listener, toplevel, request_fullscreen);
	struct tinywl_server *server = toplevel->server;

	// LIVI: forward to the HOST output window so app-driven kiosk/fullscreen fullscreens
	bool want = toplevel->xdg_toplevel->requested.fullscreen;
	for (int i = 0; i < server->n_screens; i++) {
		struct livi_screen *s = &server->screens[i];
		if (s->ui == toplevel && s->wlr_output != NULL &&
				wlr_output_is_wl(s->wlr_output)) {
			wlr_wl_output_set_fullscreen(s->wlr_output, want);
			s->fullscreen = want;
			apply_ui_layout(s);
			wlr_log(WLR_INFO, "livi: request_fullscreen=%d screen '%s' output %dx%d",
				want, s->role, s->width, s->height);
			break;
		}
	}

	if (toplevel->xdg_toplevel->base->initialized) {
		/* Reflect fullscreen onto the inner toplevel too, so Electron confirms it and the
		 * app keeps its kiosk/UI state in sync (its enter/leave-full-screen fires). */
		wlr_xdg_toplevel_set_fullscreen(toplevel->xdg_toplevel, want);
	}
}

// Force server-side decorations so Electron does not use its crash-prone GTK client-side
// decoration path. The compositor draws the titlebar itself (apply_ui_layout).
static void server_new_toplevel_decoration(struct wl_listener *listener, void *data) {
	(void)listener;
	struct wlr_xdg_toplevel_decoration_v1 *deco = data;
	// set_mode asserts before the surface is initialized, so defer to the initial commit unless
	// the surface is already up.
	struct wlr_scene_tree *tree = deco->toplevel->base->data;
	struct tinywl_toplevel *toplevel = tree ? tree->node.data : NULL;
	if (toplevel != NULL) {
		toplevel->decoration = deco;
	}
	if (deco->toplevel->base->initialized) {
		wlr_xdg_toplevel_decoration_v1_set_mode(deco,
			WLR_XDG_TOPLEVEL_DECORATION_V1_MODE_SERVER_SIDE);
	}
}

static void server_new_xdg_toplevel(struct wl_listener *listener, void *data) {
	struct tinywl_server *server = wl_container_of(listener, server, new_xdg_toplevel);
	struct wlr_xdg_toplevel *xdg_toplevel = data;

	struct tinywl_toplevel *toplevel = calloc(1, sizeof(*toplevel));
	toplevel->server = server;
	toplevel->xdg_toplevel = xdg_toplevel;
	wl_list_init(&toplevel->video_link);   // so destroy's wl_list_remove is always safe
	toplevel->scene_tree =
		wlr_scene_xdg_surface_create(&toplevel->server->scene->tree, xdg_toplevel->base);
	toplevel->scene_tree->node.data = toplevel;
	xdg_toplevel->base->data = toplevel->scene_tree;

	toplevel->map.notify = xdg_toplevel_map;
	wl_signal_add(&xdg_toplevel->base->surface->events.map, &toplevel->map);
	toplevel->unmap.notify = xdg_toplevel_unmap;
	wl_signal_add(&xdg_toplevel->base->surface->events.unmap, &toplevel->unmap);
	toplevel->commit.notify = xdg_toplevel_commit;
	wl_signal_add(&xdg_toplevel->base->surface->events.commit, &toplevel->commit);

	toplevel->destroy.notify = xdg_toplevel_destroy;
	wl_signal_add(&xdg_toplevel->events.destroy, &toplevel->destroy);

	toplevel->request_move.notify = xdg_toplevel_request_move;
	wl_signal_add(&xdg_toplevel->events.request_move, &toplevel->request_move);
	toplevel->request_resize.notify = xdg_toplevel_request_resize;
	wl_signal_add(&xdg_toplevel->events.request_resize, &toplevel->request_resize);
	toplevel->request_maximize.notify = xdg_toplevel_request_maximize;
	wl_signal_add(&xdg_toplevel->events.request_maximize, &toplevel->request_maximize);
	toplevel->request_fullscreen.notify = xdg_toplevel_request_fullscreen;
	wl_signal_add(&xdg_toplevel->events.request_fullscreen, &toplevel->request_fullscreen);
}

static void xdg_popup_commit(struct wl_listener *listener, void *data) {
	struct tinywl_popup *popup = wl_container_of(listener, popup, commit);

	if (popup->xdg_popup->base->initial_commit) {
		wlr_xdg_surface_schedule_configure(popup->xdg_popup->base);
	}
}

static void xdg_popup_destroy(struct wl_listener *listener, void *data) {
	struct tinywl_popup *popup = wl_container_of(listener, popup, destroy);

	wl_list_remove(&popup->commit.link);
	wl_list_remove(&popup->destroy.link);

	free(popup);
}

static void server_new_xdg_popup(struct wl_listener *listener, void *data) {
	struct wlr_xdg_popup *xdg_popup = data;

	struct tinywl_popup *popup = calloc(1, sizeof(*popup));
	popup->xdg_popup = xdg_popup;

	struct wlr_xdg_surface *parent = wlr_xdg_surface_try_from_wlr_surface(xdg_popup->parent);
	assert(parent != NULL);
	struct wlr_scene_tree *parent_tree = parent->data;
	xdg_popup->base->data = wlr_scene_xdg_surface_create(parent_tree, xdg_popup->base);

	popup->commit.notify = xdg_popup_commit;
	wl_signal_add(&xdg_popup->base->surface->events.commit, &popup->commit);

	popup->destroy.notify = xdg_popup_destroy;
	wl_signal_add(&xdg_popup->events.destroy, &popup->destroy);
}

static void spawn_startup(struct tinywl_server *server) {
	if (server->startup_cmd == NULL) {
		return;
	}
	pid_t pid = fork();
	if (pid == 0) {
		if (server->ui_socket != NULL) {
			setenv("WAYLAND_DISPLAY", server->ui_socket, 1);
		}
		execl("/bin/sh", "/bin/sh", "-c", server->startup_cmd, (void *)NULL);
		_exit(127);
	}
	server->startup_pid = pid;
}

// Control socket (LIVI_COMPOSITOR_CTRL): line protocol from the host. Commands:
//   screen <role> <0|1> | claim <tag> | videocfg <tag> <screen> <crop...>
//   videoshow <tag> <0|1> | backdrop <r> <g> <b> | restart
struct livi_ctrl_client {
	struct tinywl_server *server;
	struct wl_event_source *source;
	int fd;
	char buf[512];
	size_t len;
};

// Send a line to the current control client (best-effort, non-blocking).
static void ctrl_send(struct tinywl_server *server, const char *line) {
	if (server->ctrl_client_fd < 0) {
		return;
	}
	size_t n = strlen(line);
	ssize_t w = write(server->ctrl_client_fd, line, n);
	(void)w;
}

// Fallback: if the inner UI never quits, SIGKILL it
static int restart_timeout(void *data) {
	struct tinywl_server *server = data;
	wlr_log(WLR_INFO, "livi: inner UI did not quit -> SIGKILL, then re-exec");
	if (server->startup_pid > 0) {
		kill(server->startup_pid, SIGKILL);
	} else {
		wl_display_terminate(server->wl_display);
	}
	return 0;
}

static void ctrl_handle_line(struct tinywl_server *server, const char *line) {
	char tag[64], srole[32];
	double cl, ct, vw, vh, tw, th;
	int onoff, swidth, sheight;

	if (strcmp(line, "restart") == 0) {
		wlr_log(WLR_INFO, "livi: restart requested -> waiting for inner UI to quit, then re-exec");
		server->full_restart = true;
		if (server->startup_pid > 0) {
			if (server->restart_timer == NULL) {
				server->restart_timer = wl_event_loop_add_timer(
					wl_display_get_event_loop(server->wl_display), restart_timeout, server);
			}
			if (server->restart_timer != NULL) {
				wl_event_source_timer_update(server->restart_timer, 8000);
			}
		} else {
			wl_display_terminate(server->wl_display);
		}
		return;
	}

	// open/close a role's nested output window (its own movable host window)
	// optional trailing "<w> <h>" sizes the output to that screen's own resolution
	int sn = sscanf(line, "screen %31s %d %d %d", srole, &onoff, &swidth, &sheight);
	if (sn >= 2) {
		struct livi_screen *s = screen_by_role(server, srole);
		if (s != NULL && server->wl_backend != NULL) {
			if (sn >= 4 && swidth > 0 && sheight > 0) {
				s->req_width = swidth;
				s->req_height = sheight;
			}
			if (onoff && s->wlr_output == NULL) {
				server->pending_screen = s;
				wlr_wl_output_create(server->wl_backend);   // fires server_new_output now
			} else if (!onoff && s->wlr_output != NULL) {
				wlr_output_destroy(s->wlr_output);          // fires output_destroy
			}
		}
		return;
	}
	if (sscanf(line, "claim %63s", tag) == 1) {
		if (server->n_pending_video_tags < LIVI_MAX_VIDEO_CFGS) {
			snprintf(server->pending_video_tags[server->n_pending_video_tags],
				sizeof(server->pending_video_tags[0]), "%s", tag);
			server->n_pending_video_tags++;
		}
		return;
	}
	if (sscanf(line, "unclaim %63s", tag) == 1) {
		for (int i = 0; i < server->n_pending_video_tags; i++) {
			if (strcmp(server->pending_video_tags[i], tag) == 0) {
				for (int j = i + 1; j < server->n_pending_video_tags; j++) {
					memcpy(server->pending_video_tags[j - 1],
						server->pending_video_tags[j],
						sizeof(server->pending_video_tags[0]));
				}
				server->n_pending_video_tags--;
				break;
			}
		}
		return;
	}
	// cached per tag, applied now or when the tagged toplevel first appears
	if (sscanf(line, "videocfg %63s %31s %lf %lf %lf %lf %lf %lf",
			tag, srole, &cl, &ct, &vw, &vh, &tw, &th) == 8) {
		struct livi_video_cfg *cfg = cfg_for_tag(server, tag, true);
		if (cfg != NULL) {
			snprintf(cfg->screen, sizeof(cfg->screen), "%s", srole);
			cfg->has_crop = vw > 0 && vh > 0;
			cfg->crop_l = cl;
			cfg->crop_t = ct;
			cfg->vis_w = vw;
			cfg->vis_h = vh;
			cfg->tier_w = tw;
			cfg->tier_h = th;
			struct tinywl_toplevel *v = find_video_by_tag(server, tag);
			if (v != NULL) {
				apply_cfg_to_video(server, cfg, v);
			}
		}
		return;
	}
	if (sscanf(line, "videoshow %63s %d", tag, &onoff) == 2) {
		struct livi_video_cfg *cfg = cfg_for_tag(server, tag, true);
		if (cfg != NULL) {
			cfg->has_visible = true;
			cfg->visible = onoff != 0;
		}
		struct tinywl_toplevel *v = find_video_by_tag(server, tag);
		if (v != NULL) {
			wlr_scene_node_set_enabled(&v->scene_tree->node, onoff != 0);
		}
		return;
	}
	int r, g, b;
	if (sscanf(line, "backdrop %d %d %d", &r, &g, &b) == 3) {
		bool dbg = getenv("LIVI_DEBUG_BG") != NULL;
		for (int i = 0; i < server->n_screens; i++) {
			struct livi_screen *s = &server->screens[i];
			s->backdrop_color[0] = (float)r / 255.0f;
			s->backdrop_color[1] = (float)g / 255.0f;
			s->backdrop_color[2] = (float)b / 255.0f;
			s->backdrop_color[3] = 1.0f;
			s->has_backdrop_color = true;
			if (s->backdrop && !dbg) {
				wlr_scene_rect_set_color(s->backdrop, s->backdrop_color);
			}
		}
		return;
	}
	double ga, co, cr, cg, cb;
	if (sscanf(line, "gamma %lf %lf %lf %lf %lf", &ga, &co, &cr, &cg, &cb) == 5) {
		server->cal_gamma = (float)ga;
		server->cal_contrast = (float)co;
		server->cal_gain[0] = (float)cr;
		server->cal_gain[1] = (float)cg;
		server->cal_gain[2] = (float)cb;
		server->cal_active =
			ga != 1.0 || co != 1.0 || cr != 1.0 || cg != 1.0 || cb != 1.0;
		return;
	}
}

static int ctrl_client_readable(int fd, uint32_t mask, void *data) {
	struct livi_ctrl_client *c = data;
	if (mask & (WL_EVENT_HANGUP | WL_EVENT_ERROR)) {
		goto close_client;
	}
	ssize_t n = read(fd, c->buf + c->len, sizeof(c->buf) - c->len - 1);
	if (n <= 0) {
		goto close_client;
	}
	c->len += (size_t)n;
	c->buf[c->len] = '\0';
	char *start = c->buf, *nl;
	while ((nl = strchr(start, '\n')) != NULL) {
		*nl = '\0';
		ctrl_handle_line(c->server, start);
		start = nl + 1;
	}
	size_t rem = c->len - (size_t)(start - c->buf);
	memmove(c->buf, start, rem);
	c->len = rem;
	return 0;

close_client:
	if (c->server->ctrl_client_fd == c->fd) {
		c->server->ctrl_client_fd = -1;
	}
	wl_event_source_remove(c->source);
	close(fd);
	free(c);
	return 0;
}

static int ctrl_accept(int fd, uint32_t mask, void *data) {
	(void)mask;
	struct tinywl_server *server = data;
	int client = accept(fd, NULL, NULL);
	if (client < 0) {
		return 0;
	}
	struct livi_ctrl_client *c = calloc(1, sizeof(*c));
	c->server = server;
	c->fd = client;
	server->ctrl_client_fd = client;
	struct wl_event_loop *loop = wl_display_get_event_loop(server->wl_display);
	c->source = wl_event_loop_add_fd(loop, client, WL_EVENT_READABLE,
		ctrl_client_readable, c);
	return 0;
}

static void ctrl_init(struct tinywl_server *server) {
	server->ctrl_fd = -1;
	server->ctrl_client_fd = -1;
	const char *path = getenv("LIVI_COMPOSITOR_CTRL");
	if (!path || !*path) {
		return;
	}
	int fd = socket(AF_UNIX, SOCK_STREAM, 0);
	if (fd < 0) {
		wlr_log(WLR_ERROR, "livi: control socket() failed: %s", strerror(errno));
		return;
	}
	/* the listen fd is created before forking the UI, keep it out of the child */
	fcntl(fd, F_SETFD, FD_CLOEXEC);
	struct sockaddr_un addr = {0};
	addr.sun_family = AF_UNIX;
	if (strlen(path) >= sizeof(addr.sun_path)) {
		wlr_log(WLR_ERROR, "livi: control socket path too long");
		close(fd);
		return;
	}
	strcpy(addr.sun_path, path);
	unlink(path);
	if (bind(fd, (struct sockaddr *)&addr, sizeof(addr)) < 0 || listen(fd, 4) < 0) {
		wlr_log(WLR_ERROR, "livi: control socket bind/listen failed: %s", strerror(errno));
		close(fd);
		return;
	}
	struct wl_event_loop *loop = wl_display_get_event_loop(server->wl_display);
	wl_event_loop_add_fd(loop, fd, WL_EVENT_READABLE, ctrl_accept, server);
	server->ctrl_fd = fd;
	wlr_log(WLR_INFO, "livi: control socket at %s", path);
}

// autocreate returns a multi-backend, grab the nested wayland sub-backend so we can
// open more outputs at runtime
static void find_wl_backend(struct wlr_backend *backend, void *data) {
	struct tinywl_server *server = data;
	if (wlr_backend_is_wl(backend)) {
		server->wl_backend = backend;
	}
}

int main(int argc, char *argv[]) {
	wlr_log_init(getenv("LIVI_WLR_DEBUG") ? WLR_DEBUG : WLR_INFO, NULL);

	char *startup_cmd = NULL;

	int c;
	while ((c = getopt(argc, argv, "s:h")) != -1) {
		switch (c) {
		case 's':
			startup_cmd = optarg;
			break;
		default:
			printf("Usage: %s [-s startup command]\n", argv[0]);
			return 0;
		}
	}
	if (optind < argc) {
		printf("Usage: %s [-s startup command]\n", argv[0]);
		return 0;
	}

	struct tinywl_server server = {0};

	char screens_buf[256];
	const char *screens_env = getenv("LIVI_SCREENS");
	snprintf(screens_buf, sizeof(screens_buf), "%s",
		screens_env && *screens_env ? screens_env : "main,dash,aux");
	server.screens = calloc(8, sizeof(struct livi_screen));
	for (char *tok = strtok(screens_buf, ","); tok != NULL && server.n_screens < 8;
			tok = strtok(NULL, ",")) {
		snprintf(server.screens[server.n_screens].role,
			sizeof(server.screens[0].role), "%s", tok);
		server.n_screens++;
	}
	if (server.n_screens == 0) {
		snprintf(server.screens[0].role, sizeof(server.screens[0].role), "main");
		server.n_screens = 1;
	}

	server.wl_display = wl_display_create();
	server.backend = wlr_backend_autocreate(wl_display_get_event_loop(server.wl_display), NULL);
	if (server.backend == NULL) {
		wlr_log(WLR_ERROR, "failed to create wlr_backend");
		return 1;
	}
	wlr_multi_for_each_backend(server.backend, find_wl_backend, &server);

	server.renderer = wlr_renderer_autocreate(server.backend);
	if (server.renderer == NULL) {
		wlr_log(WLR_ERROR, "failed to create wlr_renderer");
		return 1;
	}

	// This already creates wl_shm + linux-dmabuf
	wlr_renderer_init_wl_display(server.renderer, server.wl_display);

	server.allocator = wlr_allocator_autocreate(server.backend,
		server.renderer);
	if (server.allocator == NULL) {
		wlr_log(WLR_ERROR, "failed to create wlr_allocator");
		return 1;
	}

	wlr_compositor_create(server.wl_display, 5, server.renderer);
	wlr_subcompositor_create(server.wl_display);
	wlr_data_device_manager_create(server.wl_display);
	/* LIVI: waylandsink scales the decoded video to its surface via wp_viewporter */
	wlr_viewporter_create(server.wl_display);

	server.output_layout = wlr_output_layout_create(server.wl_display);

	wl_list_init(&server.outputs);
	server.new_output.notify = server_new_output;
	wl_signal_add(&server.backend->events.new_output, &server.new_output);

	// Force the scene to composite every layer into our intermediate buffer so the full-output
	// calibration pass can sample the whole frame.
	setenv("WLR_SCENE_DISABLE_DIRECT_SCANOUT", "1", 1);
	server.scene = wlr_scene_create();
	server.scene_layout = wlr_scene_attach_output_layout(server.scene, server.output_layout);
	// created in z-order: backdrop (bottom), video planes, UI, decoration, overlay (top)
	server.layer_bg = wlr_scene_tree_create(&server.scene->tree);
	server.layer_video = wlr_scene_tree_create(&server.scene->tree);
	server.layer_ui = wlr_scene_tree_create(&server.scene->tree);
	server.layer_deco = wlr_scene_tree_create(&server.scene->tree);
	server.layer_overlay = wlr_scene_tree_create(&server.scene->tree);

	wl_list_init(&server.toplevels);
	wl_list_init(&server.videos);
	server.xdg_shell = wlr_xdg_shell_create(server.wl_display, 3);
	server.new_xdg_toplevel.notify = server_new_xdg_toplevel;
	wl_signal_add(&server.xdg_shell->events.new_toplevel, &server.new_xdg_toplevel);
	server.new_xdg_popup.notify = server_new_xdg_popup;
	wl_signal_add(&server.xdg_shell->events.new_popup, &server.new_xdg_popup);

	/* advertise xdg-decoration + force server-side so Electron skips its client-side path */
	struct wlr_xdg_decoration_manager_v1 *xdg_decoration =
		wlr_xdg_decoration_manager_v1_create(server.wl_display);
	server.new_toplevel_decoration.notify = server_new_toplevel_decoration;
	wl_signal_add(&xdg_decoration->events.new_toplevel_decoration,
		&server.new_toplevel_decoration);

	server.cursor = wlr_cursor_create();
	wlr_cursor_attach_output_layout(server.cursor, server.output_layout);

	server.cursor_mgr = wlr_xcursor_manager_create(NULL, 24);

	server.cursor_mode = TINYWL_CURSOR_PASSTHROUGH;
	server.cursor_motion.notify = server_cursor_motion;
	wl_signal_add(&server.cursor->events.motion, &server.cursor_motion);
	server.cursor_motion_absolute.notify = server_cursor_motion_absolute;
	wl_signal_add(&server.cursor->events.motion_absolute,
			&server.cursor_motion_absolute);
	server.cursor_button.notify = server_cursor_button;
	wl_signal_add(&server.cursor->events.button, &server.cursor_button);
	server.cursor_axis.notify = server_cursor_axis;
	wl_signal_add(&server.cursor->events.axis, &server.cursor_axis);
	server.cursor_frame.notify = server_cursor_frame;
	wl_signal_add(&server.cursor->events.frame, &server.cursor_frame);
	server.touch_down.notify = server_touch_down;
	wl_signal_add(&server.cursor->events.touch_down, &server.touch_down);
	server.touch_up.notify = server_touch_up;
	wl_signal_add(&server.cursor->events.touch_up, &server.touch_up);
	server.touch_motion.notify = server_touch_motion;
	wl_signal_add(&server.cursor->events.touch_motion, &server.touch_motion);
	server.touch_frame.notify = server_touch_frame;
	wl_signal_add(&server.cursor->events.touch_frame, &server.touch_frame);

	wl_list_init(&server.keyboards);
	server.new_input.notify = server_new_input;
	wl_signal_add(&server.backend->events.new_input, &server.new_input);
	server.seat = wlr_seat_create(server.wl_display, "seat0");
	server.request_cursor.notify = seat_request_cursor;
	wl_signal_add(&server.seat->events.request_set_cursor,
			&server.request_cursor);
	server.pointer_focus_change.notify = seat_pointer_focus_change;
	wl_signal_add(&server.seat->pointer_state.events.focus_change,
			&server.pointer_focus_change);
	server.request_set_selection.notify = seat_request_set_selection;
	wl_signal_add(&server.seat->events.request_set_selection,
			&server.request_set_selection);

	const char *socket = wl_display_add_socket_auto(server.wl_display);
	if (!socket) {
		wlr_backend_destroy(server.backend);
		return 1;
	}
	server.ui_socket = socket;

	if (!wlr_backend_start(server.backend)) {
		wlr_backend_destroy(server.backend);
		wl_display_destroy(server.wl_display);
		return 1;
	}

	// nested backend auto-creates one output -> the main screen. Secondary screens
	// (dash/aux) are opened on demand by the host via the "screen <role> 1" command.

	ctrl_init(&server);   // before forking the UI, so the host can connect immediately
	// Auto-reap the UI child so we never leave zombies.
	signal(SIGCHLD, SIG_IGN);
	server.startup_cmd = startup_cmd;
	server.argv = argv;   // saved for a full-restart re-exec
	spawn_startup(&server);
	wlr_log(WLR_INFO, "Running livi-compositor on WAYLAND_DISPLAY=%s", socket);
	wl_display_run(server.wl_display);

	// LIVI: full restart -> re-exec before the wlroots teardown.
	if (server.full_restart) {
		if (server.startup_pid > 0)
			kill(server.startup_pid, SIGTERM);
		wlr_log(WLR_INFO, "livi: re-exec for full restart");
		execv("/proc/self/exe", server.argv);
		execvp(server.argv[0], server.argv);
		wlr_log(WLR_ERROR, "livi: re-exec failed: %s", strerror(errno));
	}

	if (server.startup_pid > 0) {
		kill(server.startup_pid, SIGTERM);
	}

	if (server.ctrl_fd >= 0) {
		close(server.ctrl_fd);
		const char *ctrl_path = getenv("LIVI_COMPOSITOR_CTRL");
		if (ctrl_path) {
			unlink(ctrl_path);
		}
	}

	wl_display_destroy_clients(server.wl_display);

	wl_list_remove(&server.new_xdg_toplevel.link);
	wl_list_remove(&server.new_xdg_popup.link);
	wl_list_remove(&server.new_toplevel_decoration.link);

	wl_list_remove(&server.cursor_motion.link);
	wl_list_remove(&server.cursor_motion_absolute.link);
	wl_list_remove(&server.cursor_button.link);
	wl_list_remove(&server.cursor_axis.link);
	wl_list_remove(&server.cursor_frame.link);
	wl_list_remove(&server.touch_down.link);
	wl_list_remove(&server.touch_up.link);
	wl_list_remove(&server.touch_motion.link);
	wl_list_remove(&server.touch_frame.link);

	wl_list_remove(&server.new_input.link);
	wl_list_remove(&server.request_cursor.link);
	wl_list_remove(&server.pointer_focus_change.link);
	wl_list_remove(&server.request_set_selection.link);

	wl_list_remove(&server.new_output.link);

	// the backdrop + titlebar rects are freed by the scene-tree destroy below, null them so
	// output_destroy (via wlr_backend_destroy) does not double-free
	for (int i = 0; i < server.n_screens; i++) {
		server.screens[i].backdrop = NULL;
		server.screens[i].titlebar = NULL;
		server.screens[i].btn_fs = NULL;
		server.screens[i].btn_close = NULL;
	}
	wlr_scene_node_destroy(&server.scene->tree.node);
	wlr_xcursor_manager_destroy(server.cursor_mgr);
	wlr_cursor_destroy(server.cursor);
	wlr_allocator_destroy(server.allocator);
	wlr_renderer_destroy(server.renderer);
	wlr_backend_destroy(server.backend);
	wl_display_destroy(server.wl_display);
	free(server.screens);

	return 0;
}
