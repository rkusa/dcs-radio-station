# DCS Radio Station

Broadcast audio files (ogg/opus) to DCS World's [Simple Radio Standalone](https://github.com/ciribob/DCS-SimpleRadioStandalone).

[Changelog](./CHANGELOG.md) | [Prebuild Releases](https://github.com/rkusa/dcs-radio-station/releases)

## Sub-Projects

- [**drs-cmd**](./drs-cmd) - a command line tool to start a radio station from outside DCS
    
    Example Usage:
    
    ```bash
    .\dcs-radio-station.exe .\audio-files
    ```

- [**drs-module**](./drs) - a Lua module that can be loaded from DCS to start a station from inside a mission (not ready yet)
- [**drs-player**](./drs-player) - the actual functionality, which is used by the sub-projects above

## Audio Format

**Audio files have to be of the format OGG/OPUS (not OGG/VORBIS)!**

Instructions to convert audio files to OGG/OPUS:
- [using VLC](./docs/convert-with-vlc.md)

## License

[MIT](./LICENSE.md)
