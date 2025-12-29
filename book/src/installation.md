## Installation

Please install `cargo` via the package manager or [rustup.rs](http://rustup.rs).

### Supported Platforms

`oatbar` supports both **X11** and **Wayland** compositors, including:

* **Wayland:** sway, hyprland
* **X11:** i3, bspwm, and other X11 window managers

> [!NOTE]
> Wayland support uses a pure-Rust implementation and does not require any additional system libraries.
> The `libxcb`/`x11-xcb` dependency is only required for X11 support.

### Dependencies

#### ArchLinux

```sh
# For both X11 and Wayland (default)
pacman -Sy pango cairo libxcb pkgconf

# For Wayland only (build with --no-default-features -F wayland)
pacman -Sy pango cairo pkgconf
```

#### Ubuntu/Debian

```sh
# For both X11 and Wayland (default)
apt-get install -y build-essential pkg-config \ 
  libcairo2-dev libpango1.0-dev libx11-xcb-dev

# For Wayland only (build with --no-default-features -F wayland)
apt-get install -y build-essential pkg-config \ 
  libcairo2-dev libpango1.0-dev
```

#### Other

Install the development packages for the following libraries:

* Cairo
* Pango
* x11-xcb (only required for X11 support)

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
