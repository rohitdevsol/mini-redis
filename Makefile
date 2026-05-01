.PHONY: build server client clean

build:
	cargo build

server:
	cargo run --bin server

client:
	cargo run --bin client

clean:
	cargo clean