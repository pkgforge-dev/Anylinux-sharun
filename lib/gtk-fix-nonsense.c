/*
 * GTK fixes
 * ===================================================
 *
 * - Forces the window class / app id to $GTK_WINDOW_CLASS. GNOME uses
 *   a different class in wayland than in x11, breaking desktop
 *   integration of appimages.
 * - Disables the glycin sandbox. glycin does not resolve the full path
 *   of the binaries it passes to bwrap and never checks if bwrap even
 *   exists, it never works inside an AppImage.
 * - Switches gsettings to the keyfile backend when portable
 *   home/config mode is used, otherwise settings end up on the host
 *   dconf.
 *
 * USAGE:
 *   GTK_WINDOW_CLASS=fuck.gnome LD_PRELOAD=./gtk-fix-nonsense.so /path/to/app
*/

#define _GNU_SOURCE
#include <dlfcn.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/stat.h>

/* ------------------------------------------------------------------ */
/*  Real symbol resolution                                            */
/* ------------------------------------------------------------------ */

/*
 * Reach an already loaded library that RTLD_NEXT cannot see: apps may
 * load their gtk/glib stack with dlopen (dotnet apps like Pinta), and
 * glycin-ng lacks the glycin symbols entirely.
 * RTLD_DEFAULT must never be used here, it would find this very
 * wrapper and recurse into itself until the stack blew up
 * (Pinta-AppImage#17)
 */
static void *loaded_handle(void **cache, const char **sonames) {
	if (*cache)
		return *cache;
	for (const char **s = sonames; *s && !*cache; s++)
		*cache = dlopen(*s, RTLD_LAZY | RTLD_NOLOAD);
	return *cache;
}

static void *gly_handle(void) {
	static void *cache;
	static const char *sonames[] = {
		"libglycin-2.so.0",
		"libglycin-1.so.0",
		"libglycin.so.0",
		"libglycin.so",
		NULL
	};
	return loaded_handle(&cache, sonames);
}

static void *glib_handle(void) {
	static void *cache;
	static const char *sonames[] = {
		"libgtk-4.so.1",
		"libgtk-3.so.0",
		"libgdk-3.so.0",
		"libgtk-x11-2.0.so.0",
		"libgio-2.0.so.0",
		"libglib-2.0.so.0",
		"libgobject-2.0.so.0",
		NULL
	};
	return loaded_handle(&cache, sonames);
}

/*
 * Find the real symbol for a wrapper. Unresolved slots are retried on
 * every call, the gtk/glib stack may not be loaded yet when we get
 * preloaded.
 */
static void *real_sym(void *slot[static 1], const char *name, void *(*get_handle)(void)) {
	if (!*slot) {
		*slot = dlsym(RTLD_NEXT, name);
		if (!*slot) {
			void *handle = get_handle();
			if (handle)
				*slot = dlsym(handle, name);
		}
	}
	return *slot;
}

/* ------------------------------------------------------------------ */
/*  GTK / GLib window-class overrides                                 */
/* ------------------------------------------------------------------ */

typedef struct _GApplication GApplication;
typedef unsigned int GApplicationFlags;

static const char *override_id = NULL;

static void *real_g_application_new;
static void *real_gtk_application_new;
static void *real_g_application_set_application_id;
static void *real_g_application_get_application_id;
static void *real_g_set_prgname;
static void *real_g_get_prgname;
static void *real_gdk_surface_set_app_id;
static void *real_gdk_wayland_window_set_app_id;
static void *real_gdk_window_set_app_id;

static int gtk_init_done = 0;

static void gtk_init(void) {
	if (__atomic_load_n(&gtk_init_done, __ATOMIC_ACQUIRE)) return;

	override_id = getenv("GTK_WINDOW_CLASS");
	if (override_id && *override_id) {
		fprintf(stderr, " [gtk-fix-nonsense.so] Setting window class to '%s'\n", override_id);
	} else {
		override_id = NULL;
	}
	__atomic_store_n(&gtk_init_done, 1, __ATOMIC_RELEASE);
}

static const char *effective_id(const char *requested) {
	return override_id ? override_id : requested;
}

GApplication *g_application_new(const char *application_id, GApplicationFlags flags) {
	gtk_init();
	GApplication *(*real)(const char *, GApplicationFlags) =
		(GApplication *(*)(const char *, GApplicationFlags))
		real_sym(&real_g_application_new, "g_application_new", glib_handle);
	return real ? real(effective_id(application_id), flags) : NULL;
}

GApplication *gtk_application_new(const char *application_id, GApplicationFlags flags) {
	gtk_init();
	GApplication *(*real)(const char *, GApplicationFlags) =
		(GApplication *(*)(const char *, GApplicationFlags))
		real_sym(&real_gtk_application_new, "gtk_application_new", glib_handle);
	return real ? real(effective_id(application_id), flags) : NULL;
}

void g_application_set_application_id(GApplication *app, const char *application_id) {
	gtk_init();
	void (*real)(GApplication *, const char *) =
		(void (*)(GApplication *, const char *))
		real_sym(&real_g_application_set_application_id, "g_application_set_application_id", glib_handle);
	if (real)
		real(app, effective_id(application_id));
}

void g_set_prgname(const char *prgname) {
	gtk_init();
	void (*real)(const char *) =
		(void (*)(const char *))
		real_sym(&real_g_set_prgname, "g_set_prgname", glib_handle);
	if (real)
		real(effective_id(prgname));
}

const char *g_application_get_application_id(GApplication *app) {
	gtk_init();
	if (override_id) return override_id;
	const char *(*real)(GApplication *) =
		(const char *(*)(GApplication *))
		real_sym(&real_g_application_get_application_id, "g_application_get_application_id", glib_handle);
	return real ? real(app) : NULL;
}

const char *g_get_prgname(void) {
	gtk_init();
	if (override_id) return override_id;
	const char *(*real)(void) =
		(const char *(*)(void))
		real_sym(&real_g_get_prgname, "g_get_prgname", glib_handle);
	return real ? real() : NULL;
}

void gdk_surface_set_app_id(void *surface, const char *app_id) {
	gtk_init();
	void (*real)(void *, const char *) =
		(void (*)(void *, const char *))
		real_sym(&real_gdk_surface_set_app_id, "gdk_surface_set_app_id", glib_handle);
	if (real)
		real(surface, effective_id(app_id));
}

void gdk_wayland_window_set_app_id(void *window, const char *app_id) {
	gtk_init();
	void (*real)(void *, const char *) =
		(void (*)(void *, const char *))
		real_sym(&real_gdk_wayland_window_set_app_id, "gdk_wayland_window_set_app_id", glib_handle);
	if (real)
		real(window, effective_id(app_id));
}

void gdk_window_set_app_id(void *window, const char *app_id) {
	gtk_init();
	void (*real)(void *, const char *) =
		(void (*)(void *, const char *))
		real_sym(&real_gdk_window_set_app_id, "gdk_window_set_app_id", glib_handle);
	if (real)
		real(window, effective_id(app_id));
}

/* ------------------------------------------------------------------ */
/*  Glycin sandbox disable                                            */
/* ------------------------------------------------------------------ */

#ifndef GLY_SANDBOX_SELECTOR_NOT_SANDBOXED
#define GLY_SANDBOX_SELECTOR_NOT_SANDBOXED 3
#endif

static void *real_gly_loader_set_sandbox_selector;

static void force_not_sandboxed(void *loader) {
	if (!loader) return;
	void (*set_sandbox)(void *, int) =
		(void (*)(void *, int))
		real_sym(&real_gly_loader_set_sandbox_selector, "gly_loader_set_sandbox_selector", gly_handle);
	if (set_sandbox)
		set_sandbox(loader, GLY_SANDBOX_SELECTOR_NOT_SANDBOXED);
}

#define GLY_LOADER_WRAPPER(name) \
	void* gly_##name(void* arg) { \
		static void *real; \
		void *(*fn)(void*) = (void *(*)(void*)) real_sym(&real, "gly_" #name, gly_handle); \
		void *loader = fn ? fn(arg) : NULL; \
		force_not_sandboxed(loader); \
		return loader; \
	}

GLY_LOADER_WRAPPER(loader_new)
GLY_LOADER_WRAPPER(loader_new_for_stream)
GLY_LOADER_WRAPPER(loader_new_for_bytes)

/* ------------------------------------------------------------------ */
/*  GSettings backend fix                                             */
/* ------------------------------------------------------------------ */

/* portable home/config mode, without keyfile settings end up on the host dconf */
__attribute__((constructor))
static void fix_gsettings_backend(void) {
	const char *appimage = getenv("APPIMAGE");
	if (!appimage || !*appimage)
		return;

	const char *portable_dirs[] = { ".config", ".home" };
	for (size_t i = 0; i < sizeof portable_dirs / sizeof *portable_dirs; i++) {
		char portable_dir[PATH_MAX];
		snprintf(portable_dir, sizeof portable_dir, "%s%s", appimage, portable_dirs[i]);
		struct stat st;
		if (stat(portable_dir, &st) == 0 && S_ISDIR(st.st_mode)) {
			setenv("GSETTINGS_BACKEND", "keyfile", 1);
			return;
		}
	}
}

/* ------------------------------------------------------------------ */
/*  Constructor                                                       */
/* ------------------------------------------------------------------ */

__attribute__((constructor))
static void gtk_class_fix_ctor(void) {
	gtk_init();
	if (override_id) {
		void (*real)(const char *) =
			(void (*)(const char *))
			real_sym(&real_g_set_prgname, "g_set_prgname", glib_handle);
		if (real)
			real(override_id);
	}
}
