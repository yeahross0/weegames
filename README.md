# Weegames
Weegames is a collection of minigames written in Rust with SDL2 (and SFML for audio). Download the windows version from [itch.io](https://yeahross.itch.io/weegames). There is a separate repository for the [demo](https://github.com/yeahross0/Weegames-Demo).

# Screenshots
![baby](https://img.itch.zone/aW1hZ2UvNjYyNjQ3LzM1Njg1NjkuanBn/original/EjnlNA.jpg)
![piano](https://img.itch.zone/aW1hZ2UvNjYyNjQ3LzM1Njg1NjguanBn/original/0LpPjA.jpg)

# Video

https://www.youtube.com/watch?v=sstqGppo7L4

# Installation
## MSVC
Go into the main-game/msvc/lib/ directory. Then go into either the 32-bit or 64-bit directory depending on what you are using. From there, copy csfml-audio.lib and csfml-system.lib to C:\Users\[USERNAME]\.rustup\toolchains\[RUST_TOOLCHAIN]\lib\rustlib\[MSVC_TOOLCHAIN]\lib.
## Other - Mingw or Linux
Install SDL https://github.com/Rust-SDL2/rust-sdl2.

Install SFML using their guide https://github.com/jeremyletang/rust-sfml/wiki

# Running
Change directory into main-game/ and run ``cargo run``
