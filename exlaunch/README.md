# exlaunch
A framework for injecting C/C++ code into Nintendo Switch applications/applet/sysmodules.

# Note
This project is a work in progress. If you have issues, reach out to Shadów#1337 on Discord.
This fork's standalone Makefile build (`config.json`, `config.mk`) targets Super Smash Bros. Ultimate
(`01006a800016e000`). `config.json` reproduces `ozone/packaged/exefs/main.npdm` byte-for-byte with `npdmtool`.
Ozone itself is built with `cargo skyline` through `ex-build`, which does not use these files.

# Credit
- Atmosphère: A great reference and guide.
- oss-rtld: Included for (pending) interop with rtld in applications (License [here](https://github.com/shadowninja108/exlaunch/blob/main/source/lib/reloc/rtld/LICENSE.txt)).