Clipr - command line clipboard (pasteboard) manager
===================================================

Simple MacOS clipboard (pasteboard) manager with cli and emacs interfaces. Under development.

## Running

### Server

```bash
cargo run --bin clipr-daemon -- -c config.toml
```

### CLI

```bash
cargo run --bin clipr-cli -- -c config.toml <command>
```

### Emacs module

```bash
cargo build --lib clipr-emacs && target/debug/libclipr.dylib <emacs-load-path>/clipr.so
cp clipr-emacs/src/clipr-mode.el <emacs-load-path>/
```

## Tasks [0/4]

* [ ] Pinned entries

* [ ] Masked entries

* [ ] Deadline timeout (for sensitive data)

* [ ] LaunchD plist

## Bugs [0/1]

* [ ] Leaking
