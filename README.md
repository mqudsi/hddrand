# About `hddrand`

`hddrand` is a dual-purpose cryptographically secure drive wipe and drive verification tool. It generates a non-compressible stream of random data sourced from a cryptographically secure PRNG derived from a unique seed, and writes that to the disk. The usage of random data makes `hddrand` ideal for benchmarking the performance of SSDs and other storage devices with "smart" controllers that might otherwise benefit from the usage of an all-zero, all-one, or otherwise repeating bit pattern that allows them to compress the bytestream before committing it to disk, appearing to complete writes faster than the underlying storage is actually capable of.

Because `hddrand` generates the random data written to the disk from a seed, `hddrand` can also be used to verify the integrity of a disk by performing a complete drive wipe with `hddrand` followed by a verification pass which reads the data off the disk and compares it to the expected contents by reconstructing the original CSPRNG stream from the seed in memory, without needing to store a copy of that same bytestream elsewhere to compare against.

`hddrand` is intended to be used when a drive is first acquired to a) benchmark its performance when writing a non-compressible, non-repeating byte stream, b) verify the entirety of the disk by performing a validation pass to compare what is read back from the disk against what we can mathematically prove we originally wrote to it. This may optionally be followed by a second `hddrand` write pass to verify that the performance of a second write once the drive has been completely filled with non-compressible data matches that of the initial write pass, to observe degradations in performance caused by a lack of spare NAND cells in the case of SSD drives (expected to a certain degree) or to observe the severe degradation in write performance associated with SMR disks when there are insufficient blank/empty user-addressable sectors on the disk to service the write without the SMR controller resorting to the pathologically slow process of individually clearing pages as they are written to the magnetic medium.

At the time of decommissioning a disk, `hddrand` may be used in lieu of an all-zero or all-one wipe. While it has been claimed that a single pass of any bit pattern is sufficient to clear a device, SSDs and other disks with smart controllers may altogether elide such highly compressible writes; `hddrand`'s write pass allows one to clear the device at the same speed as that of a true write of an all-zero/all-one pattern, but is guaranteed to result in the existing contents of the disk being actually overwritten due to the seemingly truly random nature of the written bytestream.

`hddrand` may be used to prep a device to contain an encrypted volume (GELI, dm-crypt, TrueCrypt, etc) that may or may not span the entirety of the device. In this case, the presence of all-random data spanning the entirety of the disk may mask the presence of a smaller portion of the disk containing the random data.

# Usage

Usage of `hddrand` is straightforward:

```
hddrand [--verify] /dev/disk
```

An initial (write) pass of `hddrand` is used by omitting the `--verify` when invoking the application. `hddrand` will generate a cryptographically secure seed from which it will derive the 8-round ChaCha CSPRNG stream. This seed is written to the start of the disk and is read by the verification pass to initialize the CSPRNG from the same point. `hddrand` will report its progress as well as the current write/wipe speed as it makes its way through the disk (or file) at the path specified.

A verification/validation pass of `hddrand` is performed by invoking `hddrand --verify` against the same path as the write pass, in which case `hddrand` will read the seed saved to the start of the device and initialize the CSPRNG from the same, allowing it to reconstruct in memory the expected contents of the disk as it reads the actual contents that have been written. `hddrand` will report its progress as it validates the entirety of the disk/file along with the speed at which it is reading from the target device. If at any point the actual bytes read back diverge from the expected contents, `hddrand` will display an error with the details.

# Installation

`hddrand` supports all platforms, including Windows, Linux, FreeBSD, and macOS. `hddrand` is not yet included in any of the mainstream package managers, users will need to download a precompiled binary or compile and install `hddrand` from sources (`cargo install hddrand` should do the trick if you have rust installed).

# License and Credits

`hddrand` is written and maintained by Mahmoud Al-Qudsi, development was sponsored by NeoSmart Technologies. `hddrand` is released to the general public as open source under the terms of the MIT public license. Community contributions in the form of pull requests, improvements to the documentation, or help getting `hddrand` into the various package managers is welcome.
