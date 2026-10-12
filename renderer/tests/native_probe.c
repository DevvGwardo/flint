/* Test-only startup/file/socket isolation probe. Never linked into an app. */
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <sys/socket.h>
#include <unistd.h>

int main(int argc, char **argv) {
    struct rlimit core = {0, 0};
    setrlimit(RLIMIT_CORE, &core);
    const char prefix[] = "/Volumes/T7 Shield/mac-offload/flint-t3-implementation-glct94hg/renderer-evidence/";
    if (argc != 3 || strncmp(argv[1], prefix, sizeof(prefix) - 1) != 0)
        return 64;
    int fd = open(argv[1], O_RDONLY);
    int file_errno = fd < 0 ? errno : 0;
    if (fd >= 0) close(fd);
    int sock = socket(AF_INET, SOCK_STREAM, 0);
    int result = -1, network_errno = sock < 0 ? errno : 0;
    if (sock >= 0) {
        struct sockaddr_in address = {0};
        address.sin_len = sizeof(address);
        address.sin_family = AF_INET;
        address.sin_port = htons((uint16_t)atoi(argv[2]));
        address.sin_addr.s_addr = inet_addr("127.0.0.1");
        result = connect(sock, (struct sockaddr *)&address, sizeof(address));
        network_errno = result < 0 ? errno : 0;
        close(sock);
    }
    printf("{\"sentinelOpened\":%s,\"fileErrno\":%d,\"socketConnected\":%s,\"networkErrno\":%d}\n",
           fd >= 0 ? "true" : "false", file_errno, result == 0 ? "true" : "false", network_errno);
    return 0;
}
