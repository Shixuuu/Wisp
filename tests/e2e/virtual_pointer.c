/* A virtual pointer for the end-to-end tests.
 *
 * AT-SPI mouse events do not arrive as Wayland pointer events, so a tab drag
 * never starts. This speaks zwlr_virtual_pointer_v1, which the compositor
 * does deliver. Coordinates are the same logical pixels hyprctl cursorpos
 * reports. Extent defaults to 1920x1200 and can be overridden with
 * POINTER_EXTENT=W,H.
 *
 *   virtual_pointer move X Y
 *   virtual_pointer click X Y
 *   virtual_pointer drag X1 Y1 X2 Y2
 */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
#include <wayland-client.h>

static const struct wl_interface pointer_interface;
static const struct wl_interface manager_interface;

static const struct wl_message pointer_requests[] = {
    {"motion", "uff", NULL},
    {"motion_absolute", "uuuuu", NULL},
    {"button", "uuu", NULL},
    {"axis", "uuf", NULL},
    {"frame", "", NULL},
    {"axis_source", "u", NULL},
    {"axis_stop", "uu", NULL},
    {"axis_discrete", "uufi", NULL},
    {"destroy", "", NULL},
};

static const struct wl_interface pointer_interface = {
    "zwlr_virtual_pointer_v1", 1, 9, pointer_requests, 0, NULL,
};

static const struct wl_interface *manager_types[] = {
    NULL,
    &pointer_interface,
};

static const struct wl_message manager_requests[] = {
    {"create_virtual_pointer", "?on", manager_types},
    {"destroy", "", NULL},
};

static const struct wl_interface manager_interface = {
    "zwlr_virtual_pointer_manager_v1", 1, 2, manager_requests, 0, NULL,
};

static struct wl_proxy *manager;

static void on_global(void *data, struct wl_registry *registry, uint32_t name, const char *iface, uint32_t version) {
    (void)data;
    (void)version;
    if (strcmp(iface, "zwlr_virtual_pointer_manager_v1") == 0)
        manager = wl_registry_bind(registry, name, &manager_interface, 1);
}

static void on_remove(void *data, struct wl_registry *registry, uint32_t name) {
    (void)data;
    (void)registry;
    (void)name;
}

static const struct wl_registry_listener registry_listener = {on_global, on_remove};

static uint32_t now_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint32_t)(ts.tv_sec * 1000u + ts.tv_nsec / 1000000u);
}

static void extent(uint32_t *w, uint32_t *h) {
    *w = 1920;
    *h = 1200;
    const char *env = getenv("POINTER_EXTENT");
    if (env)
        sscanf(env, "%u,%u", w, h);
    if (*w == 0 || *h == 0) {
        *w = 1920;
        *h = 1200;
    }
}

static void absolute(struct wl_proxy *pointer, uint32_t x, uint32_t y) {
    uint32_t w, h;
    extent(&w, &h);
    wl_proxy_marshal(pointer, 1, now_ms(), x, y, w, h);
    wl_proxy_marshal(pointer, 4);
}

static void button(struct wl_proxy *pointer, uint32_t state) {
    wl_proxy_marshal(pointer, 2, now_ms(), 0x110u, state);
    wl_proxy_marshal(pointer, 4);
}

int main(int argc, char **argv) {
    if (argc < 2) {
        fprintf(stderr, "usage: virtual_pointer move|click|drag ...\n");
        return 2;
    }
    struct wl_display *display = wl_display_connect(NULL);
    if (!display) {
        fprintf(stderr, "no wayland display\n");
        return 1;
    }
    struct wl_registry *registry = wl_display_get_registry(display);
    wl_registry_add_listener(registry, &registry_listener, NULL);
    wl_display_roundtrip(display);
    if (!manager) {
        fprintf(stderr, "compositor has no virtual pointer\n");
        return 1;
    }
    struct wl_proxy *pointer = wl_proxy_marshal_constructor(manager, 0, &pointer_interface, NULL);
    if (!pointer) {
        fprintf(stderr, "could not create a virtual pointer\n");
        return 1;
    }
    wl_display_roundtrip(display);

    if (strcmp(argv[1], "move") == 0 && argc == 4) {
        absolute(pointer, (uint32_t)atoi(argv[2]), (uint32_t)atoi(argv[3]));
    } else if (strcmp(argv[1], "click") == 0 && argc == 4) {
        absolute(pointer, (uint32_t)atoi(argv[2]), (uint32_t)atoi(argv[3]));
        wl_display_flush(display);
        usleep(30000);
        button(pointer, 1);
        wl_display_flush(display);
        usleep(40000);
        button(pointer, 0);
    } else if (strcmp(argv[1], "drag") == 0 && argc == 6) {
        int x1 = atoi(argv[2]);
        int y1 = atoi(argv[3]);
        int x2 = atoi(argv[4]);
        int y2 = atoi(argv[5]);
        absolute(pointer, (uint32_t)x1, (uint32_t)y1);
        wl_display_flush(display);
        usleep(40000);
        button(pointer, 1);
        wl_display_flush(display);
        usleep(50000);
        for (int i = 1; i <= 20; i++) {
            int x = x1 + (x2 - x1) * i / 20;
            int y = y1 + (y2 - y1) * i / 20;
            absolute(pointer, (uint32_t)x, (uint32_t)y);
            wl_display_flush(display);
            usleep(16000);
        }
        button(pointer, 0);
    } else {
        fprintf(stderr, "usage: virtual_pointer move|click|drag ...\n");
        return 2;
    }
    wl_display_flush(display);
    usleep(20000);
    wl_proxy_marshal(pointer, 8);
    wl_proxy_marshal(manager, 1);
    wl_display_flush(display);
    wl_display_disconnect(display);
    return 0;
}
