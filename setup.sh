# Run this script when setting up your account on Lunaserver
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
. "$HOME/.cargo/env"
cargo install cargo-make
echo "setup.sh completed successfully. Please close this terminal and open a new one."