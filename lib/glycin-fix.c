/*
 * glycin sandbox disable
 * ===================================================
 *
 * Glycin forces sandboxing which fails 100% of the time because the
 * library does not resolve the full path of the binaries it passes to
 * bwrap and never checks if bwrap even exists, it never works inside
 * an AppImage.
 *
 * Only preload this library for applications that ship real glycin
 * (libglycin-2.so.0 and friends). For glycin-ng based applications
 * there is nothing to fix, glycin-ng has a working sandbox and none
 * of these symbols.
 *
 * quick-sharun handles the preload decision automatically.
*/

#define _GNU_SOURCE
#include <dlfcn.h>
#include <stddef.h>

#ifndef GLY_SANDBOX_SELECTOR_NOT_SANDBOXED
#define GLY_SANDBOX_SELECTOR_NOT_SANDBOXED 3
#endif

/*
 * glycin is not always in the global link map, dotnet apps like Pinta
 * load it with dlopen so RTLD_NEXT cannot see it. Reach the already
 * loaded library via dlopen(RTLD_NOLOAD) on its sonames instead.
 * RTLD_DEFAULT must never be used here, it would find the wrapper and
 * recurse into itself until the stack blew up (Pinta-AppImage#17)
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

/*
 * Unresolved slots are retried on every call, glycin may not be loaded
 * yet when we get preloaded.
 */
static void *real_sym(void *slot[static 1], const char *name) {
	if (!*slot) {
		*slot = dlsym(RTLD_NEXT, name);
		if (!*slot) {
			void *handle = gly_handle();
			if (handle)
				*slot = dlsym(handle, name);
		}
	}
	return *slot;
}

static void *real_gly_loader_set_sandbox_selector;

static void force_not_sandboxed(void *loader) {
	if (!loader) return;
	void (*set_sandbox)(void *, int) =
		(void (*)(void *, int))
		real_sym(&real_gly_loader_set_sandbox_selector, "gly_loader_set_sandbox_selector");
	if (set_sandbox)
		set_sandbox(loader, GLY_SANDBOX_SELECTOR_NOT_SANDBOXED);
}

#define GLY_LOADER_WRAPPER(name) \
	void* gly_##name(void* arg) { \
		static void *real; \
		void *(*fn)(void*) = (void *(*)(void*)) real_sym(&real, "gly_" #name); \
		void *loader = fn ? fn(arg) : NULL; \
		force_not_sandboxed(loader); \
		return loader; \
	}

GLY_LOADER_WRAPPER(loader_new)
GLY_LOADER_WRAPPER(loader_new_for_stream)
GLY_LOADER_WRAPPER(loader_new_for_bytes)
