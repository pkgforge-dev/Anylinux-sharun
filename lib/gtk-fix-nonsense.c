/*
 * GTK Window Class Override + glycin sandbox disable
 * ===================================================
 *
 * PURPOSE:
 *  GNOME made the window class of applications different between x11 and
 *  wayland, breaking desktop integration of appimages as result.
 *
 *  Glycin forces sandboxing which fails 100% of the time here because the
 *  library is horribly written and does not resolve the full path of the
 *  binaries it passes to bwrap, it does not even check if bwrap is present!
 *
 *  Portable home/config mode needs gsettings to use the keyfile backend
 *  or the application settings end up on the host dconf instead.
 *
 * USAGE:
 *   GTK_WINDOW_CLASS=fuck.gnome LD_PRELOAD=./gtk-fix-nonsense.so /path/to/app
 *
 * WARNING:
 *  This was 100% vibed with AI by someone that has no idea about C
 *  It works, but no idea if this can cause weird issues down the line
*/

#define _GNU_SOURCE
#include <dlfcn.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/stat.h>

/* ------------------------------------------------------------------ */
/*  GTK / GLib window-class overrides                                 */
/* ------------------------------------------------------------------ */

typedef struct _GApplication GApplication;
typedef unsigned int GApplicationFlags;

static const char *override_id = NULL;

static GApplication *(*real_g_application_new)(const char *, GApplicationFlags);
static GApplication *(*real_gtk_application_new)(const char *, GApplicationFlags);
static void (*real_g_application_set_application_id)(GApplication *, const char *);
static const char *(*real_g_application_get_application_id)(GApplication *);
static void (*real_g_set_prgname)(const char *);
static const char *(*real_g_get_prgname)(void);
static void (*real_gdk_surface_set_app_id)(void *surface, const char *app_id);
static void (*real_gdk_wayland_window_set_app_id)(void *window, const char *app_id);
static void (*real_gdk_window_set_app_id)(void *window, const char *app_id);

static int gtk_init_done = 0;

static void gtk_init(void) {
	if (__atomic_load_n(&gtk_init_done, __ATOMIC_ACQUIRE)) return;

	override_id = getenv("GTK_WINDOW_CLASS");
	if (override_id && *override_id) {
		fprintf(stderr, " [gtk-fix-nonsense.so] Setting window class to '%s'\n", override_id);
	} else {
		override_id = NULL;
	}

	real_g_application_new = dlsym(RTLD_NEXT, "g_application_new");
	real_gtk_application_new = dlsym(RTLD_NEXT, "gtk_application_new");
	real_g_application_set_application_id = dlsym(RTLD_NEXT, "g_application_set_application_id");
	real_g_application_get_application_id = dlsym(RTLD_NEXT, "g_application_get_application_id");
	real_g_set_prgname = dlsym(RTLD_NEXT, "g_set_prgname");
	real_g_get_prgname = dlsym(RTLD_NEXT, "g_get_prgname");
	real_gdk_surface_set_app_id = dlsym(RTLD_NEXT, "gdk_surface_set_app_id");
	real_gdk_wayland_window_set_app_id = dlsym(RTLD_NEXT, "gdk_wayland_window_set_app_id");
	real_gdk_window_set_app_id = dlsym(RTLD_NEXT, "gdk_window_set_app_id");
	__atomic_store_n(&gtk_init_done, 1, __ATOMIC_RELEASE);
}

static const char *effective_id(const char *requested) {
	return override_id ? override_id : requested;
}

GApplication *g_application_new(const char *application_id, GApplicationFlags flags) {
	gtk_init();
	return real_g_application_new ? real_g_application_new(effective_id(application_id), flags) : NULL;
}

GApplication *gtk_application_new(const char *application_id, GApplicationFlags flags) {
	gtk_init();
	return real_gtk_application_new ? real_gtk_application_new(effective_id(application_id), flags) : NULL;
}

void g_application_set_application_id(GApplication *app, const char *application_id) {
	gtk_init();
	if (real_g_application_set_application_id) {
		real_g_application_set_application_id(app, effective_id(application_id));
	}
}

void g_set_prgname(const char *prgname) {
	gtk_init();
	if (real_g_set_prgname) {
		real_g_set_prgname(effective_id(prgname));
	}
}

const char *g_application_get_application_id(GApplication *app) {
	gtk_init();
	if (override_id) return override_id;
	return real_g_application_get_application_id ? real_g_application_get_application_id(app) : NULL;
}

const char *g_get_prgname(void) {
	gtk_init();
	if (override_id) return override_id;
	return real_g_get_prgname ? real_g_get_prgname() : NULL;
}

void gdk_surface_set_app_id(void *surface, const char *app_id) {
	gtk_init();
	if (real_gdk_surface_set_app_id) {
		real_gdk_surface_set_app_id(surface, effective_id(app_id));
	}
}

void gdk_wayland_window_set_app_id(void *window, const char *app_id) {
	gtk_init();
	if (real_gdk_wayland_window_set_app_id) {
		real_gdk_wayland_window_set_app_id(window, effective_id(app_id));
	}
}

void gdk_window_set_app_id(void *window, const char *app_id) {
	gtk_init();
	if (real_gdk_window_set_app_id) {
		real_gdk_window_set_app_id(window, effective_id(app_id));
	}
}

/* ------------------------------------------------------------------ */
/*  Glycin sandbox disable                                            */
/* ------------------------------------------------------------------ */

#ifndef GLY_SANDBOX_SELECTOR_NOT_SANDBOXED
#define GLY_SANDBOX_SELECTOR_NOT_SANDBOXED 3
#endif

/*
 * glycin is not always part of the global link map, for example dotnet
 * apps like Pinta load it and everything depending on it with dlopen,
 * so RTLD_NEXT and RTLD_DEFAULT from this preloaded library can never
 * see it. Reach the already loaded library via dlopen(RTLD_NOLOAD) on
 * its known sonames instead.
 *
 * Without this the RTLD_DEFAULT fallback used to find this very wrapper
 * and recurse into itself until the stack blew up, killing the whole
 * app (see Pinta-AppImage#17)
 */
static void *gly_handle(void) {
	static void *handle;
	static const char *sonames[] = {
		"libglycin-2.so.0",
		"libglycin-1.so.0",
		"libglycin.so.0",
		"libglycin.so",
		NULL
	};

	if (handle)
		return handle;
	for (const char **s = sonames; *s && !handle; s++)
		handle = dlopen(*s, RTLD_LAZY | RTLD_NOLOAD);
	return handle;
}

static void force_not_sandboxed(void *loader) {
	if (!loader) return;
	void (*set_sandbox)(void *, int) = dlsym(RTLD_DEFAULT, "gly_loader_set_sandbox_selector");
	if (!set_sandbox) {
		void *handle = gly_handle();
		if (handle)
			set_sandbox = dlsym(handle, "gly_loader_set_sandbox_selector");
	}
	if (set_sandbox)
		set_sandbox(loader, GLY_SANDBOX_SELECTOR_NOT_SANDBOXED);
}

#define GLY_LOADER_WRAPPER(name) \
	void* gly_##name(void* arg) { \
		static void* (*real)(void*) = NULL; \
		if (!real) { \
			real = dlsym(RTLD_NEXT, "gly_" #name); \
			if (!real) { \
				void *handle = gly_handle(); \
				if (handle) \
					real = dlsym(handle, "gly_" #name); \
			} \
			/* never let real point at ourselves, that used to recurse to death */ \
			if (real == (void *)&gly_##name) \
				real = NULL; \
		} \
		void *loader = real ? real(arg) : NULL; \
		force_not_sandboxed(loader); \
		return loader; \
	}

GLY_LOADER_WRAPPER(loader_new)
GLY_LOADER_WRAPPER(loader_new_for_stream)
GLY_LOADER_WRAPPER(loader_new_for_bytes)

/* ------------------------------------------------------------------ */
/*  GSettings backend fix                                             */
/* ------------------------------------------------------------------ */

/*
 * Portable home/config mode keeps the application settings next to the
 * AppImage, gsettings needs to use the keyfile backend for that or
 * settings end up on the host dconf instead
 * (equivalent of quick-sharun's gsettings-backend.hook)
 */
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
	if (override_id && real_g_set_prgname) {
		real_g_set_prgname(override_id);
	}
}
