# Oatbar

[![Latest Version](https://img.shields.io/crates/v/oatbar.svg)](https://crates.io/crates/oatbar)
![Crates.io License](https://img.shields.io/crates/l/oatbar)
![Libraries.io dependency status for latest release](https://img.shields.io/librariesio/release/cargo/oatbar)
![GitHub top language](https://img.shields.io/github/languages/top/igor-petruk/oatbar)
![Crates.io](https://img.shields.io/crates/d/oatbar?label=Cargo.io%20downloads)

It is a standalone desktop bar that can be used with various WMs and DEs.

![Panel Left](panel-sample-left.png)
![Panel Right](panel-sample-right.png)

This bar supports conversion of inputs in various data formats to strong types that can be displayed on specialized bar widgets.

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
