/*
 * Glycin forces sandboxing which fails 100% of the time here because the
 * library is horribly written and does not resolve the full path of the
 * binaries it passes to bwrap, it does not even check if bwrap is present!
 */

#define _GNU_SOURCE
#include <dlfcn.h>
#include <stddef.h>

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
