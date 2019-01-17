# DCS Radio Station

A command line utility to play OGG/OPUS audio files through a specified SRS frequency (expects a SRS server to run locally on the default SRS ports).

## Usage

```
USAGE:
    dcs-radio-station.exe [FLAGS] [OPTIONS] <PATH>

FLAGS:
    -h, --help       Prints help information
    -l, --loop       Enables endlessly looping the audio file(s)
    -V, --version    Prints version information

OPTIONS:
    -f, --freq <frequency>    Sets the SRS frequency (in Hz, e.g. 255000000 for 255MHz) [default: 255000000]

ARGS:
    <PATH>    Sets the path audio file(s) should be read from
```

## Build

Instead of building you can also use the prebuild mod from one of the [releases](https://github.com/rkusa/dcs-radio-station/releases).

Build with [Rust nightly](https://rustup.rs/):

```
cd .\drs-cmd
cargo build --release
```