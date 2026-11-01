# livi-crypto

ChaCha20-Poly1305 for the CarPlay media receivers in `livi-audio` (stream,
uplink) and `livi-screen-stream`, used there as a cargo dependency.

The crate takes the AEAD from aws-lc-rs (assembly ChaCha20/Poly1305, NEON on
aarch64) and needs cmake for the AWS-LC build.

## Third-party licences

Binaries that use the crate statically embed [AWS-LC](https://github.com/aws/aws-lc)
via aws-lc-rs (ISC / Apache-2.0, with OpenSSL/SSLeay-licensed portions).
