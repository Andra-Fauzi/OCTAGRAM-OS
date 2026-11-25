#ifndef OCTAGRAM_SYS_SYSINFO_H
#define OCTAGRAM_SYS_SYSINFO_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

struct sysinfo {
    long uptime;             /* seconds since boot */
    unsigned long loads[3];  /* 1, 5, 15 minute load averages */
    unsigned long totalram;  /* total usable main memory */
    unsigned long freeram;   /* available memory */
    unsigned long sharedram; /* shared memory */
    unsigned long bufferram; /* buffers */
    unsigned long totalswap;
    unsigned long freeswap;
    unsigned short procs;    /* number of processes */
    unsigned long totalhigh;
    unsigned long freehigh;
    uint32_t mem_unit;       /* memory unit size in bytes */
    char padding[0];
};

/*
 * uptime and procs are real; the memory fields are zero because OCTAGRAM has
 * no kernel call that reports them. Zero means "unknown" here, never "none
 * installed".
 */
int sysinfo(struct sysinfo *information);

/* glibc's processor-count helpers: both read _SC_NPROCESSORS_ONLN and
 * _SC_NPROCESSORS_CONF from the kernel, so they report what sysconf reports. */
int get_nprocs(void);
int get_nprocs_conf(void);

#ifdef __cplusplus
}
#endif

#endif
