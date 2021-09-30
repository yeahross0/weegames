# Weegames Demo
Weegames is a collection of minigames written in Rust with macroquad. You can play the game on [itch.io](https://yeahross.itch.io/weegames).

# Screenshots
![baby](https://img.itch.zone/aW1hZ2UvNjYyNjQ3LzM1Njg1NjkuanBn/original/EjnlNA.jpg)
![piano](https://img.itch.zone/aW1hZ2UvNjYyNjQ3LzM1Njg1NjguanBn/original/0LpPjA.jpg)

# Video

https://www.youtube.com/watch?v=sstqGppo7L4

# Running

```cd main-game```

To run the executable version run ``cargo run``

To run the web version run

```
cargo build --target wasm32-unknown-unknown
cp ../target/wasm32-unknown-unknown/debug/main-game.wasm weegames.wasm
cargo install basic-http-server # If not already installed
basic-http-server .
```

Then open localhost:4000