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

#### Keys

##### List mode

<kbd>g</kbd> - refresh

<kbd>RET</kbd> - select and close

<kbd>d</kbd> - delete entry

<kbd>q</kbd> - close

<kbd>+</kbd> - add tag

<kbd>-</kbd> - delete tag

<kbd>f</kbd> - set filter by tag

<kbd>c</kbd> - clear filter

<kbd>j</kbd> - jump to tag

<kbd>S</kbd> - save state

<kbd>L</kbd> - load state

<kbd>E</kbd> - edit entry

##### Edit mode

<kbd>C-c C-c</kbd> - save entry (will be added as new one ;) )

<kbd>C-c C-k</kbd> - quit edit mode

## Tasks

### General [0/5]

* [ ] Pinned entries

* [ ] Masked entries

* [ ] Deadline timeout (for sensitive data)

* [ ] LaunchD plist

* [ ] GUI/global menu

### Refactoring [1/5]

* [X] Switch to LINKED-LIST + SET (or w/o). Looks like it will be a lot easier to reorder entries.

* [ ] Split/simplify command handling

* [ ] Maybe actor-like stuff isn't necessary there?

* [ ] More rusty (try to specify lifetimes + remove all .clone)

* [ ] Reduce dependencies

### Emacs [0/3]

* [ ] Use removal by range / remove selection in clipr-mode

* [ ] Emacs UI display current filter

* [ ] Multi tag selection

### Bugs [1/1]

* [X] Leaking (if there is no changes in PB we still allocating NSString)
