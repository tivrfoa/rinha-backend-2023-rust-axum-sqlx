
cargo build --release

docker build . --no-cache -f DockerfileUbuntu -t ubuntu-rust-rinha:axumsqlx

docker-compose up
