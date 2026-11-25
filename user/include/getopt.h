#ifndef OCTAGRAM_GETOPT_H
#define OCTAGRAM_GETOPT_H

#ifdef __cplusplus
extern "C" {
#endif

struct option {
    const char *name;
    int has_arg; /* no_argument, required_argument, optional_argument */
    int *flag;   /* NULL: getopt_long returns val; otherwise *flag = val */
    int val;
};

#define no_argument 0
#define required_argument 1
#define optional_argument 2

/* Set by the parser, read by the caller. */
extern char *optarg;
extern int optind;
extern int optopt;
extern int opterr;

extern int optreset;

int getopt(int argc, char *const argv[], const char *optstring);
int getopt_long(int argc, char *const argv[], const char *optstring,
                const struct option *long_options, int *long_index);
int getopt_long_only(int argc, char *const argv[], const char *optstring,
                     const struct option *long_options, int *long_index);

#ifdef __cplusplus
}
#endif

#endif
