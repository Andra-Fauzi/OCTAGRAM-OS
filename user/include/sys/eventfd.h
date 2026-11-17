#ifndef OCTAGRAM_SYS_EVENTFD_H
#define OCTAGRAM_SYS_EVENTFD_H

#define EFD_SEMAPHORE 0x0001
#define EFD_CLOEXEC 02000000
#define EFD_NONBLOCK 00004000

int eventfd(unsigned int initial_value, int flags);

#endif
