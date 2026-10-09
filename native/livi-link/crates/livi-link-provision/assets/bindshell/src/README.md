# mipsel bind-shell

`../mipsel` is built from `bindshell.c` in this directory — a ~70-line bind-shell: listen on
`argv[1]`, and for each connection, `dup2` the socket onto fds 0/1/2 and `execve("/bin/sh")`. No
pty, no banner, no login: `/bin/sh` reads commands straight off the socket, which is what
`dongle::shell::BindShell` on the host side expects.

The armv7 and riscv32 binaries next to it predate this file and have no source checked in here;
this one does, because it was built fresh for the X1600.

## ABI

The X1600 ("Mini Ultra 3") stock `/bin/busybox` is MIPS32r2, O32, **hard-float** (its
`.MIPS.abiflags` section has `fp_abi = 1`, `ABI_FP_DOUBLE`) — most Ingenic XBurst2 boards do have
an FPU, unlike the soft-float MIPS boards this is sometimes confused with. `mipsel` is built to
match that exactly; `shell.rs`'s tests check the result is a little-endian 32-bit MIPS ELF.

## Toolchain

A native macOS (aarch64-darwin) `mipsel-unknown-linux-gnu` GCC 15.2.0, from
[messense/homebrew-macos-cross-toolchains](https://github.com/messense/homebrew-macos-cross-toolchains)
v15.2.0 (`brew tap messense/macos-cross-toolchains && brew install mipsel-unknown-linux-gnu`).
musl.cc also publishes a `mipsel-linux-musl-cross` toolchain that would make a much smaller static
binary, but its host binaries are Linux ELF and cannot run on macOS directly.

```sh
mipsel-linux-gnu-gcc -march=mips32r2 -mabi=32 -mhard-float -Os -static -s -o mipsel bindshell.c
```

`-static` pulls in glibc whole, so the result is a few hundred KB rather than the tens-of-KB of
the musl-static armv7/riscv32 binaries. That's a one-time cost of this OTA image, not something
that ships continuously, so it was not worth chasing further.
