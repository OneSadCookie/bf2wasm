# bf2wasm

Compile [Brainfuck](https://en.wikipedia.org/wiki/Brainfuck) to
[WebAssembly](https://webassembly.org) using
[Rust](https://rust-lang.org/) and
[Walrus](https://rustwasm.github.io/walrus/walrus/).

# Usage

Install [Node](https://nodejs.org/) to run the compiled WebAssembly. You can install it with [Homebrew](https://brew.sh) if you're on macOS.

```sh
cargo run -- -i hello.bf -o target/bf.wasm
node index.js
```

# Note

Currently pointing to my own fork of Walrus due to a bug. Any release newer than 0.8.0 should include the fix.
