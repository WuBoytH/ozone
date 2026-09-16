# Ozone

Ozone is a plugin loader for Super Smash Bros. Ultimate that aims to be a drop-in replacement for skyline.

All skyline plugins will work natively with ozone.

## How to build

1. Install [Devkitpro](https://devkitpro.org/wiki/Getting_Started), make sure to pick Switch support during the installation.
2. Add the ``DEVKITPRO`` environment variables and have it point to the root of your Devkitpro install directory (Defaults to ``C:\devkitpro`` on Windows).
3. Add the ``bin`` directory of ``devkitA64`` to your ``PATH`` environment variable (Should be ``C:\devkitPro\devkitA64\bin`` on Windows).
4. Clone this repository with ``git clone --recurse-submodules https://github.com/WuBoytH/ozone``.
5. Navigate to the ``ozone`` subdirectory.
6. Run the ``cargo skyline build --release --nso`` command.
7. Navigate to ``target/aarch64-skyline-switch/release/`` where you'll find ``libozone.nso``.
8. Rename to ``subsdk9`` and put in the ``exefs`` LayeredFS directory for Super Smash Bros. Ultimate.
