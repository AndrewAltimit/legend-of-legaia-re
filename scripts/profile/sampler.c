/* A CPU sampling profiler for a machine where `perf` is not allowed: load it
 * into a release-test binary with LD_PRELOAD and it records a stack every
 * 10 ms of CPU time. It needs no rebuild, no debug info and no privilege; the
 * unwind uses the binary's own .eh_frame.
 *
 *   gcc -O2 -shared -fPIC -o /tmp/sampler.so scripts/profile/sampler.c
 *   LD_PRELOAD=/tmp/sampler.so PROF_OUT=/tmp/run <binary> <args>
 *   scripts/profile/report.py <binary> /tmp/run.<pid>
 *
 * Writes <PROF_OUT>.<pid> (samples) and <PROF_OUT>.<pid>.maps (the load map).
 * Each sample is one write(2), so threads do not interleave.
 */
#define _GNU_SOURCE
#include <errno.h>
#include <execinfo.h>
#include <fcntl.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/time.h>
#include <unistd.h>

#define MAX_FRAMES 48

static int out_fd = -1;

static void on_prof(int sig, siginfo_t *info, void *ctx) {
    (void)sig, (void)info, (void)ctx;
    int saved = errno;
    void *record[MAX_FRAMES + 1];
    int n = backtrace(record + 1, MAX_FRAMES);
    record[0] = (void *)(long)n;
    if (write(out_fd, record, (size_t)(n + 1) * sizeof(void *)) < 0) {
        /* A lost sample is not worth disturbing the program for. */
    }
    errno = saved;
}

static void copy_maps(const char *path) {
    int dst = open(path, O_CREAT | O_WRONLY | O_TRUNC, 0644);
    int src = open("/proc/self/maps", O_RDONLY);
    char buf[8192];
    ssize_t k;
    while (src >= 0 && dst >= 0 && (k = read(src, buf, sizeof buf)) > 0) {
        if (write(dst, buf, (size_t)k) < 0) {
            break;
        }
    }
    close(src);
    close(dst);
}

__attribute__((constructor)) static void start(void) {
    const char *prefix = getenv("PROF_OUT");
    if (!prefix) {
        return;
    }
    char path[512];
    snprintf(path, sizeof path, "%s.%d", prefix, getpid());
    out_fd = open(path, O_CREAT | O_WRONLY | O_APPEND, 0644);
    if (out_fd < 0) {
        return;
    }
    snprintf(path, sizeof path, "%s.%d.maps", prefix, getpid());
    copy_maps(path);
    /* The first backtrace() loads libgcc, which a signal handler must not do. */
    void *warm[4];
    backtrace(warm, 4);
    struct sigaction action;
    memset(&action, 0, sizeof action);
    action.sa_sigaction = on_prof;
    action.sa_flags = SA_SIGINFO | SA_RESTART;
    sigaction(SIGPROF, &action, NULL);
    struct itimerval every = {{0, 10000}, {0, 10000}};
    setitimer(ITIMER_PROF, &every, NULL);
}
