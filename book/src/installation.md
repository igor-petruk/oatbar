## Installation

Please install `cargo` via the package manager or [rustup.rs](http://rustup.rs).

### Dependencies

#### ArchLinux

```sh
pacman -Sy pango cairo libxcb pkgconf
```

#### Ubuntu/Debian

```sh
apt-get install -y build-essential pkg-config \ 
  libcairo2-dev libpango1.0-dev libx11-xcb-dev
```

#### Other

Install the development packages for the following libraries

* Cairo
* Pango
* x11-xcb

### Install

```sh
cargo install oatbar
```

During the first launch the bar will create a default config at
`~/.config/oatbar.toml` that should work on most machines. Run: 

```sh
oatbar
```

And you should see:

![New setup](new-setup.png)

### NetBSD

On NetBSD, a package is available from the official repositories.
To install it, simply run:

```
pkgin install oatbar
```

### Next

[Configuration](./configuration)
