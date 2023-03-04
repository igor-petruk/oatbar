# Oatbar

[![Latest Version](https://img.shields.io/crates/v/oatbar.svg)](https://crates.io/crates/oatbar)
![Crates.io License](https://img.shields.io/crates/l/oatbar)
![Libraries.io dependency status for latest release](https://img.shields.io/librariesio/release/cargo/oatbar)
![GitHub top language](https://img.shields.io/github/languages/top/igor-petruk/oatbar)
![Crates.io](https://img.shields.io/crates/d/oatbar?label=Cargo.io%20downloads)

It is a standalone desktop bar that can be used with various WMs and DEs. This bar aims to become one of the most full-featured bars available.

* Flexibility and pluggability of information sources from existing ecosystems
  * Arbitrary scripts
  * `i3blocks` format 
  * `i3status` format
  * PNG image embedding that could be rendered by a source script at runtime.
* External plugins are preferred, but the most basic modules are embedded: EWHM, layouts, clock, etc.
* Conversion of string sources to other types (numbers, bytes, percentages) that could be rendered by specialize widgets
* Native Pango markup support
* Source data cleaning via regexes so inflexible source module data can be cleaned inside of the `oatbar`.

![Panel Left](panel-sample-left.png)
![Panel Right](panel-sample-right.png)

## Installation

### Pre-requisites

Please install `cargo` via the package manager or http://rustup.rs.

#### ArchLinux

```shell
# pacman -Sy pango cairo libxcb pkgconf
```

#### Ubuntu/Debian

```shell
# apt-get install -y build-essential pkg-config libcairo2-dev libpango1.0-dev libx11-xcb-dev
```

### Install oatbar

```shell
$ cargo install oatbar
```

## Disclaimer

This is not an officially supported Google product.
