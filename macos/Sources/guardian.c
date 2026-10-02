// A separate supervisor survives a crashed UI. Every agent inherits the backend's
// process group; EOF from the UI stops the backend and then any remaining children.
#include <unistd.h>
#include <signal.h>
#include <sys/wait.h>
#include <poll.h>
#include <errno.h>
#include <stdlib.h>
int main(int argc, char **argv) {
    if (argc != 2) return 64;
    int input[2]; if (pipe(input)) return 71;
    pid_t child = fork(); if (child < 0) return 71;
    if (!child) {
        setpgid(0, 0); close(input[1]); dup2(input[0], STDIN_FILENO); close(input[0]);
        execl(argv[1], argv[1], (char *)0); _exit(127);
    }
    setpgid(child, child); close(input[0]); signal(SIGPIPE, SIG_IGN);
    char buffer[8192]; int status = 0, exited = 0;
    for (;;) {
        if (waitpid(child, &status, WNOHANG) == child) { exited = 1; break; }
        struct pollfd f = { STDIN_FILENO, POLLIN | POLLHUP, 0 };
        int ready = poll(&f, 1, 100);
        if (ready < 0 && errno == EINTR) continue;
        if (ready < 0) break;
        if (!ready) continue;
        ssize_t n = read(STDIN_FILENO, buffer, sizeof(buffer)); if (n <= 0) break;
        for (ssize_t at = 0; at < n;) {
            ssize_t sent = write(input[1], buffer + at, n - at);
            if (sent < 0 && errno == EINTR) continue;
            if (sent <= 0) goto done;
            at += sent;
        }
    }
done:
    close(input[1]);
    // Let the backend flush encrypted history before terminating its process group.
    for (int i = 0; !exited && i < 50; i++) {
        if (waitpid(child, &status, WNOHANG) == child) exited = 1;
        else usleep(100000);
    }
    kill(-child, SIGTERM); usleep(200000); kill(-child, SIGKILL);
    if (!exited) waitpid(child, &status, 0);
    return WIFEXITED(status) ? WEXITSTATUS(status) : 1;
}
