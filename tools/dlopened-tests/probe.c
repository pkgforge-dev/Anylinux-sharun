/* A program that does what its arguments say, in order. It is the traced
 * binary of tools/test-dlopened.sh.
 *
 *   open:PATH      dlopen PATH
 *   sleep:MS       sleep
 *   hang           sleep for a long time
 *   ignoreterm     ignore SIGTERM from now on
 *   bg             fork, the parent exits at once and the child goes on
 *   argv:FILE      write the arguments that are left to FILE, then stop
 *   stderr:TEXT    write TEXT and a newline to stderr
 *   stdout:TEXT    write TEXT and a newline to stdout
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

static void sleep_ms(long ms) {
	struct timespec ts = { ms / 1000, (ms % 1000) * 1000000L };
	while (nanosleep(&ts, &ts) == -1) {}
}

int main(int argc, char **argv) {
	for (int i = 1; i < argc; i++) {
		const char *a = argv[i];
		if (!strncmp(a, "open:", 5)) {
			if (!dlopen(a + 5, RTLD_NOW)) { fprintf(stderr, "dlopen failed: %s\n", dlerror()); return 3; }
		} else if (!strncmp(a, "sleep:", 6)) {
			sleep_ms(atol(a + 6));
		} else if (!strcmp(a, "hang")) {
			sleep_ms(60000);
		} else if (!strcmp(a, "ignoreterm")) {
			signal(SIGTERM, SIG_IGN);
		} else if (!strcmp(a, "bg")) {
			if (fork() != 0) _exit(0);
		} else if (!strncmp(a, "argv:", 5)) {
			FILE *f = fopen(a + 5, "w");
			for (int j = i + 1; j < argc; j++) fprintf(f, "%s\n", argv[j]);
			fclose(f);
			return 0;
		} else if (!strncmp(a, "stderr:", 7)) {
			fprintf(stderr, "%s\n", a + 7); fflush(stderr);
		} else if (!strncmp(a, "stdout:", 7)) {
			printf("%s\n", a + 7); fflush(stdout);
		}
	}
	return 0;
}
