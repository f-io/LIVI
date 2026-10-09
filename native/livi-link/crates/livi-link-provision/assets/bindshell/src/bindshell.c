/* A minimal bind-shell: listens on argv[1], and for each connection in turn, wires the socket to
 * fds 0/1/2 and execs /bin/sh on it (no pty, no banner, no login) — the dongle's /bin/sh reads
 * commands straight off the socket. Matches the armv7/riscv32 bind-shells already in this repo's
 * assets/bindshell, which livi-link-provision's shell.rs talks to.
 *
 * This dongle's /bin/sh prints a "# " prompt even on a plain socket (no tty), apparently because
 * /etc/profile (sourced by rcS before any init script runs) exports PS1 for root. shell.rs's
 * protocol has no room for a prompt byte, so it is blanked here rather than worked around there —
 * it is this shell's job to look like a pipe, not the host's job to filter prompts out of data.
 */
#include <netinet/in.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <unistd.h>
#include <stdlib.h>

extern char **environ;

static void blank_prompt_vars(void) {
    setenv("PS1", "", 1);
    setenv("PS2", "", 1);
    setenv("ENV", "", 1);
    setenv("BASH_ENV", "", 1);
}

int main(int argc, char **argv) {
    int port = argc > 1 ? atoi(argv[1]) : 2323;

    int srv = socket(AF_INET, SOCK_STREAM, 0);
    if (srv < 0) return 1;
    int one = 1;
    setsockopt(srv, SOL_SOCKET, SO_REUSEADDR, &one, sizeof(one));

    struct sockaddr_in addr;
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_addr.s_addr = INADDR_ANY;
    addr.sin_port = htons((unsigned short)port);
    if (bind(srv, (struct sockaddr *)&addr, sizeof(addr)) < 0) return 1;
    if (listen(srv, 1) < 0) return 1;

    for (;;) {
        int cli = accept(srv, (struct sockaddr *)0, (socklen_t *)0);
        if (cli < 0) continue;

        pid_t pid = fork();
        if (pid == 0) {
            close(srv);
            dup2(cli, 0);
            dup2(cli, 1);
            dup2(cli, 2);
            if (cli > 2) close(cli);
            blank_prompt_vars();
            char *sh_argv[] = {"/bin/sh", 0};
            execve("/bin/sh", sh_argv, environ);
            _exit(127);
        }
        close(cli);
        /* Reap finished children without blocking the accept loop. */
        while (waitpid(-1, 0, WNOHANG) > 0) {
        }
    }
}
