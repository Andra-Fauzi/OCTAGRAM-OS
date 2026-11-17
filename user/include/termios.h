#ifndef OCTAGRAM_TERMIOS_H
#define OCTAGRAM_TERMIOS_H

typedef unsigned int tcflag_t;
typedef unsigned char cc_t;
typedef unsigned int speed_t;

#define NCCS 32

struct termios {
    tcflag_t c_iflag;
    tcflag_t c_oflag;
    tcflag_t c_cflag;
    tcflag_t c_lflag;
    cc_t c_line;
    cc_t c_cc[NCCS];
    speed_t c_ispeed;
    speed_t c_ospeed;
};

/* c_cc indices */
#define VINTR 0
#define VQUIT 1
#define VERASE 2
#define VKILL 3
#define VEOF 4
#define VTIME 5
#define VMIN 6
#define VSTART 8
#define VSTOP 9
#define VSUSP 10
#define VEOL 11
#define VREPRINT 12
#define VDISCARD 13
#define VWERASE 14
#define VLNEXT 15

/* c_iflag */
#define IGNBRK 0x0001
#define BRKINT 0x0002
#define PARMRK 0x0008
#define ISTRIP 0x0020
#define INLCR 0x0040
#define IGNCR 0x0080
#define ICRNL 0x0100
#define IXON 0x0400

/* c_oflag */
#define OPOST 0x0001
#define ONLCR 0x0004

/* c_cflag */
#define CSIZE 0x0030
#define CS8 0x0030
#define CREAD 0x0080
#define PARENB 0x0100
#define B38400 0x000F

/* c_lflag */
#define ISIG 0x0001
#define ICANON 0x0002
#define ECHO 0x0008
#define ECHOE 0x0010
#define ECHOK 0x0020
#define ECHONL 0x0040
#define IEXTEN 0x8000

/* tcsetattr actions */
#define TCSANOW 0
#define TCSADRAIN 1
#define TCSAFLUSH 2

/* ioctl requests */
#define TCGETS 0x5401
#define TCSETS 0x5402
#define TCSETSW 0x5403
#define TCSETSF 0x5404

int tcgetattr(int fd, struct termios *termios_state);
int tcsetattr(int fd, int action, const struct termios *termios_state);
int isatty(int fd);
void cfmakeraw(struct termios *termios_state);
speed_t cfgetispeed(const struct termios *termios_state);
speed_t cfgetospeed(const struct termios *termios_state);
int cfsetispeed(struct termios *termios_state, speed_t speed);
int cfsetospeed(struct termios *termios_state, speed_t speed);

#endif
