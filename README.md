# Maxima
## A free and open-source replacement for the EA Desktop Launcher
![Logo](images/1500x500.jpg)

> [!WARNING]
> Maxima is in beta. Parts of it may not be fully stable, and we're actively fixing bugs and adding features. Please create an [issue](https://github.com/ArmchairDevelopers/Maxima/issues) if you notice strange behavior.

Maxima is an open-source replacement for the EA Desktop/Origin game launcher, running natively on Linux and Windows, with MacOS support in progress.

Maxima itself is a library (`maxima-lib`), with povided CLI (`maxima-cli`), TUI (`maxima-tui`), and GUI (`maxima-ui`) frontends. Other launchers compatible with Maxima's license may implement it as a backend. It's used by our sister project, [KYBER](https://uplink.kyber.gg/news/features-overview).

![UI](images/UI.png)

**Features:**
 - EA Authentication
 - Downloading/Updating games
 - Download & Play any build of a game
 - DRM & Licensing support
 - Multiplayer game support
 - Syncing EA cloud saves
 - Launch EA games owned on Epic/Steam through Maxima directly
 - Playing games installed with EA Desktop on Maxima + vice versa
 - Displaying your in-game status to your friends, and viewing your friends' status'
 - Locating games (aka. game importing)
 - Running games under [proton](https://github.com/GloriousEggroll/proton-ge-custom) on Linux/SteamDeck
   - `proton-ge` is automatically installed together with [umu](https://github.com/Open-Wine-Components/umu-launcher).

**In-Dev:**
 - MacOS support
 - Support for launching Maxima through Epic/Steam

**Planned:**
 - Library documentation/examples
 - Support for installing DLCs
 - Full EA Desktop interopability. Games installed with EA Desktop already appear on Maxima, but to take it a step further we'd like the ability to, for example, start a download on EA Desktop and continue it on Maxima.
 - Cleaner/Stabler downloader implementation
 - Progressive/Selective installs
   - Some games are able to start without being fully installed, and some games contain language-specific files.
 - Support for the store (buying games)
 - Friend Adding/Removing/Inviting
 - Status setting; locked to "online" at the moment
 - Refactoring Maxima to new architecture allowing multiple frontends to co-exist
 
**Unsupported:**
 - Battlefield 3/4 are currently unsupported due to how battlelog does game launching. This is on our radar, but isn't a huge priority at the moment.
   - Please file an issue if you find more games that don't work
 - Old games like Dead Space 2 and BFBC2 are unsupported due to being pre-"Download-In-Place" era games. They have a different manifest format which we need to make a parser for.

# CLI Usage
`maxima-cli` standalone will launch an interactive CLI mode to install and launch games.

`maxima-cli help` will bring up the subcommand list, with things like `locate-game`, `cloud-sync`, `create-auth-code`, `list-friends`, etc.

## Why the name 'Maxima'?
It's the farthest you can get from the Origin.

## Contributing
See [CONTRIBUTING.md](./CONTRIBUTING.md)

## Maintainers:
 - [Sean Kahler](https://github.com/BattleDash) (Lib, Bootstrap, Service)
 - [Nick Whelan](https://github.com/headassbtw) (UI)
