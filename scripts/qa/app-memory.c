// QA only. Not linked into or shipped inside the product.
// Coalition ABI checked against Apple's XNU proc_info_private.h and mach/coalition.h:
// https://github.com/apple-oss-distributions/xnu/tree/main/bsd/sys
// A private diagnostic ABI may fail on another OS; failure is NOT a zero-memory result.
#include <libproc.h>
#include <sys/proc_info.h>
#include <sys/resource.h>
#include <unistd.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <limits.h>

typedef struct { uint64_t groups[2]; uint64_t spare[3]; } Membership;
static int group(pid_t pid, Membership *out) {
    memset(out, 0, sizeof(*out));
    return proc_pidinfo(pid, 20, 0, out, sizeof(*out)) == sizeof(*out);
}
int main(int argc, char **argv) {
    int single = argc == 4 && !strcmp(argv[3], "--single");
    if (argc != 3 && !single) return 2;
    char *end = NULL;
    long number = strtol(argv[1], &end, 10);
    if (!end || *end || number <= 1 || number > INT_MAX) return 2;
    pid_t main_pid = (pid_t)number;
    char actual[PROC_PIDPATHINFO_MAXSIZE], expected[PATH_MAX];
    if (!realpath(argv[2], expected) || proc_pidpath(main_pid, actual, sizeof(actual)) <= 0 || strcmp(actual, expected)) return 3;
    Membership root;
    if (!single && (!group(main_pid, &root) || !root.groups[0])) return 4;
    int capacity = proc_listallpids(NULL, 0) + 1024;
    if (capacity < 1 || capacity > 131072) return 5;
    pid_t *pids = calloc((size_t)capacity, sizeof(*pids));
    if (!pids) return 5;
    int count = proc_listallpids(pids, capacity * (int)sizeof(*pids));
    if (count <= 0 || count >= capacity) { free(pids); return 5; }
    uint64_t total = 0, footprint = 0;
    unsigned found = 0, missing = 0, main_seen = 0;
    printf("{\"method\":\"%s; proc_pid_rusage physical footprint and RSS (RSS includes reclaimable/shared pages)\",\"processes\":[", single ? "single validated process" : "same resource coalition");
    for (int i = 0; i < count; i++) {
        Membership membership;
        if (single ? pids[i] != main_pid : (!group(pids[i], &membership) || membership.groups[0] != root.groups[0])) continue;
        struct proc_taskallinfo info;
        if (proc_pidinfo(pids[i], PROC_PIDTASKALLINFO, 0, &info, sizeof(info)) != sizeof(info)) { missing++; continue; }
        if (info.pbsd.pbi_uid != getuid()) { missing++; continue; }
        struct rusage_info_v0 usage;
        if (proc_pid_rusage(pids[i], RUSAGE_INFO_V0, (rusage_info_t *)&usage) != 0) { missing++; continue; }
        total += info.ptinfo.pti_resident_size;
        footprint += usage.ri_phys_footprint;
        if (pids[i] == main_pid) main_seen = 1;
        printf("%s{\"pid\":%d,\"main\":%s,\"rssBytes\":%llu,\"footprintBytes\":%llu}", found ? "," : "", pids[i], pids[i] == main_pid ? "true" : "false", (unsigned long long)info.ptinfo.pti_resident_size, (unsigned long long)usage.ri_phys_footprint);
        found++;
    }
    free(pids);
    printf("],\"rssBytes\":%llu,\"footprintBytes\":%llu,\"members\":%u,\"unreadableMembers\":%u,\"mainSeen\":%s}\n", (unsigned long long)total, (unsigned long long)footprint, found, missing, main_seen ? "true" : "false");
    return main_seen && !missing && found <= 32 ? 0 : 6;
}
