// Measurement helper only: public Darwin ABI, same ri_phys_footprint as Rust.
#include <libproc.h>
#include <stdio.h>
#include <stdlib.h>
#include <errno.h>
#include <limits.h>
int main(int argc, char **argv) {
    if (argc < 2) return 2;
    unsigned long long physical = 0, rss = 0;
    for (int i = 1; i < argc; ++i) {
        char *end; errno = 0; long pid = strtol(argv[i], &end, 10);
        if (errno || *end || pid <= 0 || pid > INT_MAX) return 2;
        struct rusage_info_v2 info = {0};
        if (proc_pid_rusage((int)pid, RUSAGE_INFO_V2, (rusage_info_t *)&info) != 0 || !info.ri_phys_footprint || !info.ri_resident_size) return 3;
        physical += info.ri_phys_footprint; rss += info.ri_resident_size;
    }
    printf("{\"physicalBytes\":%llu,\"rssBytes\":%llu,\"processCount\":%d}\n", physical, rss, argc - 1);
    return 0;
}
