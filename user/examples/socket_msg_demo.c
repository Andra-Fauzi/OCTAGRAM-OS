#include <errno.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/uio.h>
#include <unistd.h>

int main(void)
{
    int pair[2];
    if (socketpair(AF_UNIX, SOCK_STREAM, 0, pair) != 0) {
        printf("socketpair failed: %s\n", strerror(errno));
        return 1;
    }

    char first[] = "hello ";
    char second[] = "octagram\n";
    struct iovec outgoing[2];
    outgoing[0].iov_base = first;
    outgoing[0].iov_len = sizeof(first) - 1;
    outgoing[1].iov_base = second;
    outgoing[1].iov_len = sizeof(second) - 1;

    struct msghdr request;
    memset(&request, 0, sizeof(request));
    request.msg_iov = outgoing;
    request.msg_iovlen = 2;
    if (sendmsg(pair[0], &request, 0) != 14) {
        printf("sendmsg gather failed: %s\n", strerror(errno));
        return 2;
    }

    char greeting[6];
    char name[16];
    struct iovec incoming[2];
    incoming[0].iov_base = greeting;
    incoming[0].iov_len = sizeof(greeting);
    incoming[1].iov_base = name;
    incoming[1].iov_len = sizeof(name);

    struct msghdr response;
    memset(&response, 0, sizeof(response));
    response.msg_iov = incoming;
    response.msg_iovlen = 2;
    if (recvmsg(pair[1], &response, 0) != 14) {
        printf("recvmsg scatter failed: %s\n", strerror(errno));
        return 3;
    }
    if (memcmp(greeting, "hello ", 6) != 0) {
        return 4;
    }
    if (memcmp(name, "octagram\n", 10) != 0) {
        return 5;
    }
    if (response.msg_controllen != 0 || response.msg_flags != 0) {
        return 6;
    }

    if (CMSG_SPACE(sizeof(int)) < CMSG_LEN(sizeof(int))) {
        return 7;
    }
    unsigned char control[CMSG_SPACE(sizeof(int))];
    memset(control, 0, sizeof(control));
    struct cmsghdr *header = (struct cmsghdr *)control;
    header->cmsg_len = CMSG_LEN(sizeof(int));
    header->cmsg_level = SOL_SOCKET;
    header->cmsg_type = 0x01; /* SCM_RIGHTS */
    memcpy(CMSG_DATA(header), &pair[0], sizeof(int));

    struct msghdr control_message;
    memset(&control_message, 0, sizeof(control_message));
    control_message.msg_iov = outgoing;
    control_message.msg_iovlen = 1;
    control_message.msg_control = control;
    control_message.msg_controllen = sizeof(control);
    if (CMSG_FIRSTHDR(&control_message) != header) {
        return 8;
    }
    if (CMSG_NXTHDR(&control_message, header) != NULL) {
        return 9;
    }
    errno = 0;
    if (sendmsg(pair[0], &control_message, 0) != -1 || errno != ENOTSUP) {
        return 10;
    }

    struct msghdr empty;
    memset(&empty, 0, sizeof(empty));
    if (sendmsg(pair[0], &empty, 0) != 0) {
        return 11;
    }

    errno = 0;
    if (sendmsg(pair[0], NULL, 0) != -1 || errno != EFAULT) {
        return 12;
    }
    errno = 0;
    if (recvmsg(pair[1], NULL, 0) != -1 || errno != EFAULT) {
        return 13;
    }

    struct msghdr invalid;
    memset(&invalid, 0, sizeof(invalid));
    invalid.msg_iovlen = 1;
    errno = 0;
    if (sendmsg(pair[0], &invalid, 0) != -1 || errno != EFAULT) {
        return 14;
    }
    invalid.msg_iov = outgoing;
    invalid.msg_iovlen = 4096;
    errno = 0;
    if (recvmsg(pair[1], &invalid, 0) != -1 || errno != EMSGSIZE) {
        return 15;
    }

    close(pair[0]);
    close(pair[1]);
    printf("sendmsg/recvmsg passed\n");
    return 0;
}
