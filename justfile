set dotenv-load := true

default:
    @just --list

daemon:
    cargo run -p daemon --bin daemon

login:
    cargo run -p daemon --bin login

web:
    cargo run -p web
