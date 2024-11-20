# lunadev-2025

This is the official workspace for the software team for Utah Student Robotics for the NASA Lunabotics 2025 competition.

## Quickstart

You will need Visual Studio Code (We'll call it VSCode). Any other IDE that can do remote development will also work, but these instructions are for VSCode specifically.

Lunaserver is the computer that will run on the robot on competition day, but until then is just a computer that is on 24/7.
It will be connected to all the sensors we will use, and maybe a microcontroller for you to test stuff on. You will use a technique called SSH to connect to Lunaserver. There is a dedicated extension in VScode for this that you should use. [Here is a guide](https://code.visualstudio.com/docs/remote/ssh#_connect-to-a-remote-host).

The address is [`lunaserver.coe.utah.edu`](http://lunaserver.coe.utah.edu) and the port is the default port: `22`. The SSH Fingerprint is `SHA256:QfSo3cslqdKEtn9XAo5X/LMQ1AiNdazxJQLCqiynL9g`. When you connect to Lunaserver for the first time, VSCode will show you the fingerprint it received from what it thinks is Lunaserver. You should verify that this fingerprint is the same as that one. Read about the significance of the fingerprint [here](https://superuser.com/a/422008).

Before connecting for your first time, provide me with your preferred username and password for me to set up an account on Lunaserver for you.

*By connecting to Lunaserver, you are agreeing to the terms and conditions. [Refer to the wiki for the terms and conditions](https://github.com/utahrobotics/lunadev-2024/wiki/Terms-and-Conditions).*

After connecting, VSCode may ask you to type in your password very frequently. Since Lunaserver is exposed to the internet, I do want to enforce some cybersecurity. As such, you will have strong passwords that should not be convenient to type frequently. Thus, you should use SSH keys. Here is a [guide for how you can set that up](https://www.digitalocean.com/community/tutorials/how-to-configure-ssh-key-based-authentication-on-a-linux-server). You must already have an account to do this. Refer back to the first guide on SSH in VSCode to see how to add this key to your SSH config file. Do note that using SSH keys does not eliminate the need for a password; Lunaserver may still ask you to provide a password occasionally, but less often.

## Cargo

Every external dependency needed to run the code in lunadev-2025 on Lunaserver is already installed globally, with the exception of Rust itself, as it can only be installed for individual users. The simplest way is to run `setup.sh`, which is located at the top level in this repository. It installs Rust and `cargo-make`.

To install Rust manually, run the following command in Lunaserver:  
`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`  
and select the default options. Install `cargo-make` using `cargo install --force cargo-make`

## Lunabase and Lunaserver

Lunabase uses a different port than `22` and that port is not exposed to the internet. As such, a tunnel has to be made first. On your own computer, run the following command in this repository:  
`cargo run -p lunaserver-web-client`  
and follow the instructions provided. After that, you can just run `lunabot` (with `cargo run -p lunabot -- main` or `cargo make main`) on Lunaserver and it will be able to connect to Lunabase running on your computer. The disadvantage is the latency is higher than if you ran Lunabase over a direct connection to Lunaserver (more on this later). The latency is worse if the quality of your internet connection is poor.

If you happen to be in MEB 2340, you can connect to USR-Wifi-5G for a better connection. The password is on the bottom of the router (the scratched out number is 6). If you are connected to the router, you do not need to run `lunaserver-web-client`. Instead, you can just use your private IP address for `lunabase_address` (eg. my `lunabase_address` would be `192.168.0.100:10600`). You can find your private IP address using `ifconfig` on mac or linux, and `ipconfig` on windows. Do note that the `:10600` is not part of your private IP address; You just concatenate it after. If Lunaserver is still not able to connect, check that your firewall is disabled, or that it allows port `10600` over UDP. If you are connected to this router, you can also SSH to `192.168.0.102` for a lower latency SSH connection, but this is not as beneficial.

## Godot

Both Lunabase and Lunasim depend on Rust code to run correctly. Simply build `lunasim-lib` and `lunabase-lib` and Godot will use them automatically. As a shortcut, you can also run `cargo make godot`. If you modify that Rust code, you have to rebuild it again. If Lunabase or Lunasim are open in the Godot editor *and* you are on Windows, you may need to minimize the window and maximize (or some other way to switch focus) to reload the new library. You must also have opened Lunaabse or Lunasim once in the Godot editor (everytime you clone this repository) for it to run correctly.

## Directory

1. [urobotics](https://github.com/utahrobotics/lunadev-2025/tree/main/urobotics) - The core Rust framework and ecosystem for URobotics
2. [misc](https://github.com/utahrobotics/lunadev-2025/tree/main/misc) - Generic utility libraries, or "forks" of existing libraries
3. [unros](https://github.com/utahrobotics/lunadev-2025/tree/main/unros) - Legacy code that has not been fully ported over.
4. [urobotics-guide](https://github.com/utahrobotics/lunadev-2025/tree/main/urobotics-guide) - The source code for the URobotics Guide Book
5. [lunabotics](https://github.com/utahrobotics/lunadev-2025/tree/main/lunabotics) - Rust code relevant only to the lunabotics competition
6. [godot](https://github.com/utahrobotics/lunadev-2025/tree/main/godot) - Front-end software created with the Godot Game Engine
7. [examples](https://github.com/utahrobotics/lunadev-2025/tree/main/examples) - Sample projects demonstrating how to use the URobotics framework
8. [camera-db](https://github.com/utahrobotics/lunadev-2025/tree/main/camera-db) - Legacy data used for camera calibration
9. [urdf](https://github.com/utahrobotics/lunadev-2025/tree/main/urdf) - Collection of Universal Robot Description Formats
10. [.micropico](https://github.com/utahrobotics/lunadev-2025/tree/main/.micropico) - Related to experimental micropico support

### Local-only

The following files/folders are not provided in this repository and you may need to generate some of them yourself. However, most will be auto-generated.

1. [app-config.toml](https://github.com/utahrobotics/lunadev-2025/tree/main/examples/app-config.toml) - This file needs to be in the top-most directory and is *not* auto-generated. An example file can be found in the `examples` folder
2. `urobotics-venv` - A Python Virtual Environment that is used for Python interop
3. `cabinet` - Logging folder that is auto-generated everytime a [urobotics-app](https://github.com/utahrobotics/lunadev-2025/tree/main/urobotics/urobotics-app) is executed

## Third Party Assets

1. [Low Poly Rock Pack 001](https://emerald-eel-entertainment.itch.io/low-poly-rock-pack-001). Accessed Aug 26 2024. Used in Lunasim.
