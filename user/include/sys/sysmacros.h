#ifndef OCTAGRAM_SYS_SYSMACROS_H
#define OCTAGRAM_SYS_SYSMACROS_H

#define major(device) ((unsigned int)(((device) >> 8) & 0xff))
#define minor(device) ((unsigned int)((device) & 0xff))
#define makedev(major_value, minor_value) \
    (((unsigned int)(major_value) << 8) | ((unsigned int)(minor_value) & 0xff))

#endif
