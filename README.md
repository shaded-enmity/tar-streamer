Tar Streamer
============

A tool that takes input archive in various formats (GZIP, ZIP, BZIP2, XZ) and produces a Tar archive.

## Usage

```
tar-streamer - 0.1.0
Usage: tar-streamer [options] SRC DST

Options:
    -h, --help          prints this menu
    -v, --verbose       verbose mode
    -f, --force         overwrite existing files
    -t, --type [GZIP, ZIP, BZIP2, XZ, TAR]
                        input archive type(s)
    -b, --block-size    size of processing block in bytes
        --version       display version information

Multiple parameters for the -t / --type argument can be specified
by separating elements with commas:

    tar-streamer --type=gzip,tar some.tar.gz other.tar
```

## License

GPL-3.0
