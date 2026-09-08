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

static void force_not_sandboxed(void *loader) {
	if (!loader) return;
	void (*set_sandbox)(void*, int) = dlsym(RTLD_DEFAULT, "gly_loader_set_sandbox_selector");
	if (set_sandbox)
		set_sandbox(loader, GLY_SANDBOX_SELECTOR_NOT_SANDBOXED);
}

#define GLY_LOADER_WRAPPER(name) \
	void* gly_##name(void* arg) { \
		static void* (*real)(void*) = NULL; \
		if (!real) { \
		real = dlsym(RTLD_NEXT, "gly_" #name); \
		if (!real) real = dlsym(RTLD_DEFAULT, "gly_" #name); \
		} \
		void *loader = real ? real(arg) : NULL; \
		force_not_sandboxed(loader); \
		return loader; \
	}

GLY_LOADER_WRAPPER(loader_new)
GLY_LOADER_WRAPPER(loader_new_for_stream)
GLY_LOADER_WRAPPER(loader_new_for_bytes)
