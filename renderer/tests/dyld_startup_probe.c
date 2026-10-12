/* R0D diagnostic only: no app, framework, private dyld API or native bridge. */
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

static int record(const char *operation, const char *stage, int allowed, int error) {
    char output[256];
    int length = snprintf(output, sizeof(output),
        "{\"event\":\"probe\",\"operation\":\"%s\",\"stage\":\"%s\",\"allowed\":%s,\"errno\":%d}\n",
        operation, stage, allowed ? "true" : "false", error);
    if (length <= 0 || (size_t)length >= sizeof(output)) return -1;
    return write(STDOUT_FILENO, output, (size_t)length) == length ? 0 : -1;
}

static int fixture_path(const char *path) {
    const char prefix[] =
        "/Volumes/T7 Shield/mac-offload/flint-t3-implementation-glct94hg/renderer-evidence/r0d-";
    size_t length = strnlen(path, 1025);
    return length <= 1024 && length > sizeof(prefix) &&
        strncmp(path, prefix, sizeof(prefix) - 1) == 0 &&
        strstr(path, "/../") == NULL && strstr(path, "/./") == NULL;
}

int main(int argc, char **argv) {
    /* First operation in main. No buffering, formatting, argv/env or allocation. */
    static const char marker[] = "{\"protocol\":1,\"event\":\"main\"}\n";
    if (write(STDOUT_FILENO, marker, sizeof(marker) - 1) != (ssize_t)(sizeof(marker) - 1)) return 65;
    if (argc != 5 || !fixture_path(argv[1]) || !fixture_path(argv[2])) return 64;
    char *end = NULL;
    errno = 0;
    unsigned long port = strtoul(argv[3], &end, 10);
    if (errno || !end || *end || port == 0 || port > UINT16_MAX) return 64;
    errno = 0;
    unsigned long run = strtoul(argv[4], &end, 10);
    if (errno || !end || *end || run == 0 || run > 8) return 64;

    int read_fd = open(argv[1], O_RDONLY);
    int error = read_fd < 0 ? errno : 0;
    if (read_fd >= 0) close(read_fd);
    if (record("read", "open", read_fd >= 0, error)) return 65;

    int write_fd = open(argv[2], O_WRONLY | O_APPEND);
    error = write_fd < 0 ? errno : 0;
    /* A permitted write-capable open is a violation; never modify the sentinel. */
    if (write_fd >= 0) close(write_fd);
    if (record("write", "open", write_fd >= 0, error)) return 65;

    int sock = socket(AF_INET, SOCK_STREAM, 0);
    const char *stage = "socket";
    int allowed = 0;
    error = sock < 0 ? errno : 0;
    if (sock >= 0) {
        stage = "nonblocking";
        if (fcntl(sock, F_SETFL, O_NONBLOCK) < 0) {
            error = errno;
        } else {
            struct sockaddr_in address = {0};
            address.sin_len = sizeof(address);
            address.sin_family = AF_INET;
            address.sin_port = htons((uint16_t)port);
            address.sin_addr.s_addr = inet_addr("127.0.0.1");
            stage = "connect";
            int result = connect(sock, (struct sockaddr *)&address, sizeof(address));
            allowed = result == 0;
            error = result < 0 ? errno : 0;
        }
        close(sock);
    }
    if (record("network", stage, allowed, error)) return 65;
    const char done[] = "{\"event\":\"done\"}\n";
    if (write(STDOUT_FILENO, done, sizeof(done) - 1) != (ssize_t)(sizeof(done) - 1)) return 65;
    return 0;
}
