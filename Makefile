.PHONY: build server client clean

build:
	cargo build

server:
	cargo run --bin server

client:
	cargo run --bin client

test:
	cargo run --bin test_client

get:
	cargo run --bin client -- get $(k)

set:
	cargo run --bin client -- set $(k) $(v)

del:
	cargo run --bin client -- del $(k)

keys:
	cargo run --bin client -- keys

test_avl:
	cargo run --bin test_avl

clean:
	cargo clean