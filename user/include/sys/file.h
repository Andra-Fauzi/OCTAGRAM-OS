#ifndef OCTAGRAM_SYS_FILE_H
#define OCTAGRAM_SYS_FILE_H

int flock(int fd, int operation);
int lockf(int fd, int operation, off_t length);

#endif
