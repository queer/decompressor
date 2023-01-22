# de(c)ompressor

the de(c)ompressor operates on streams compression streams automagically. input
streams are detected by magic numbers, output streams are determined by cli
flags (or decompress if none given).

## supported compression formats

- plain text
- brotli
- deflate
- gzip
- xz
- zlib
- zstd

## example usage

```bash
$ echo "this is a test" | gzip | c
# this is a test
```

```bash
$ echo "this is a test" | gzip | c -o brotli | brotli -c -d
# this is a test
```

```bash
# brotli cannot be autodetected, so hint it
$ echo "this is a test" | brotli -c | c brotli
# this is a test
# without hint: `�%R�B�[d��`
```

## help

```bash
git:(mistress) | ▶  c --help
Usage: c [OPTIONS] [HINT]

Arguments:
  [HINT]  Hint for the compression type, e.g. `brotli` [default: unknown]

Options:
  -q, --quiet
  -o, --output-type <OUTPUT_TYPE>  Force the output to be compressed with the given type, e.g. `brotli` [default: none] [possible values: zstd, brotli, gzip, deflate, zlib, xz, none]
  -h, --help                       Print help
git:(mistress) | ▶
```

de(c)ompressor must be run via stdin/out pipes, otherwise it will fail:

```bash
git:(mistress) 7 | ▶  c
Error:
   0: input is a terminal, please pipe data via stdin!

Location:
   src/main.rs:36

Backtrace omitted. Run with RUST_BACKTRACE=1 environment variable to display it.
Run with RUST_BACKTRACE=full to include source snippets.
git:(mistress) | ▶
```