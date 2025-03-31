# Contributing

We welcome any kind of contributions with open arms. Whether it's a help with translation, reporting bugs or code contribution.

Reporting bugs should be done via [GitHub issue tracker](https://github.com/ArmchairDevelopers/Maxima/issues)

Translations are not hosted anywhere yet, please let us know if you wish to contribute by translating Maxima to a new language!

When contributing code you should fork the repository and make any Pull Requests from there. More about [working with forks](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/working-with-forks/)

## Project structure

Currently Maxima is split into crates that serve different purposes. Let's break them down

- [maxima-lib](./maxima-lib) - main crate of maxima, all frontend crates link against it
- [maxima-ui](./maxima-ui) - contains `egui` based frontend code (GUI application code)
- [maxima-cli](./maxima-cli) - CLI frontend code
- [maxima-tui](./maxima-tui) - TUI client built using `ratatui`
- [maxima-bootstrap](./maxima-bootstrap) - separate utility used for handling `link2ea`, `origin`, `origin2` and `qrc` (for auth) protocols
- [maxima-native](./maxima-native) - C bindings for maxima lib
- [maxima-service](./maxima-service) - (Windows only) a background service for tasks requiring privilege elevation
- [maxima-resources](./maxima-resources) - (Windows only) small crate exposing common assets and metadata

Available binary targets are:

- `maxima` (ui frontend)
- `maxima-cli`
- `maxima-tui`
- `maxima-bootstrap`
- `maxima-service` (Windows)

## Development Setup

As of now, Maxima requires a nightly build of rust toolchain, as it makes use of some unstable features of the language.

We recommend setting up [rustup](https://rustup.rs/), it lets you install and manage multiple rust toolchains at the same time.

Then install nightly build
```sh
rustup toolchain install nightly
```

Afterwards build the crate(s) with cargo
```sh
cargo +nightly build
```

And/or run particular binary target
> [!NOTE]
> Some binary targets depend on others, always make sure to build whole project when running from source before reporting any bugs.
> 
```sh
cargo +nightly run --bin maxima-cli
``` 

## Code Style

Please keep consistent code style throughought the project, as it makes it simpler to contribute and collaborate.

Use `rustfmt` to automatically format the code to meet all the expectations
```
cargo +nightly fmt
```