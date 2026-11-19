# C++ mission countdown

From this directory, run `make run` for macOS or `make serve` for the browser.
Open the printed `?client=cpp` URL. Edit `client.cpp` to change the countdown.

`make native` builds `./mtl-cpp` in this directory. `make web` builds the browser client. Both reuse
the
compiled engine. After Rust engine edits, run `make native-engine` or `make web-engine`.
