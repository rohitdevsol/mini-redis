.PHONY: build server client clean

build:
	cargo build

server:
	cargo run --bin server

client:
	cargo run --bin client

test:
	cargo run --bin test_client

clean:
	cargo clean