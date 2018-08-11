assets:
	cargo web build --release --target=wasm32-unknown-emscripten
	cp ./target/wasm32-unknown-emscripten/release/deps/ld42.data static

release:
	cargo web deploy --release --target=wasm32-unknown-emscripten

watch:
	cargo web start --release --auto-reload --target=wasm32-unknown-emscripten

check:
	cargo check --release --target=wasm32-unknown-emscripten
