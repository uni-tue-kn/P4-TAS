.PHONY: all compile start

compile:
	RUST_LOG=info cargo build

start:
	sudo setcap cap_net_raw+ep target/debug/controller
	RUST_LOG=info target/debug/controller

all: compile start
