# lunadev-2024

This is the official workspace for the software team for Utah Student Robotics for the NASA Lunabotics 2024 competition.  
If you are a student (ie. not an active contributor) you will find more value on the [wiki](https://github.com/utahrobotics/lunadev-2024/wiki) page.

## Quickstart

You will need Visual Studio Code (We'll call it VSCode). Any other IDE that can do remote development will also work, but these instructions are for VSCode specifically.

Lunaserver is the computer that will run on the robot on competition day, but until then is just a computer that is on 24/7.
It will be connected to all the sensors we will use, and maybe a microcontroller for you to test stuff on. You will use a technique called SSH to connect to Lunaserver. There is a dedicated extension in VScode for this that you should use. [Here is a guide](https://code.visualstudio.com/docs/remote/ssh#_connect-to-a-remote-host).

The address is `5.tcp.ngrok.io` and the port is `22735`. The SSH Fingerprint is `SHA256:/XYiztKXqFHny36RCsustFw5qByHRmgKy0ONRsAbWHY`. When you connect to Lunaserver for the first time, VSCode will show you the fingerprint it received from what it thinks is Lunaserver. You should verify that this fingerprint is the same as that one. If they are different, disconnect and double check that you have written the address and port correctly. If they are correct, then it is likely that someone is trying to [piggy back](https://en.wikipedia.org/wiki/Man-in-the-middle_attack) off of your SSH connection for whatever reason. The chances of that happening is very low, but still worth checking. After the first connection, you will not need to verify the SSH fingerprint anymore as VSCode will check it for you. Very rarely, you may face a warning that the SSH fingerprint check has failed, which usually means that someone is once again trying to use your SSH connection. Simply let me know, and don't worry as your computer is not compromised.

Before connecting for your first time, provide me with your preferred username and password for me to set up an account on Lunaserver for you. There is a guest account, username is `usr`. This account does not have any access outside of its home directory so you shouldn't use it to do your work.

*By connecting to Lunaserver, you are agreeing to the terms and conditions. [Refer to the wiki for the terms and conditions](https://github.com/utahrobotics/lunadev-2024/wiki/Terms-and-Conditions).*

After connecting, VSCode may ask you to type in your password very frequently. Since Lunaserver is exposed to the internet, I do want to enforce some cybersecurity. As such, you will have strong passwords that should not be convenient to type frequently. Thus, you should use SSH keys. Here is a [guide for how you can set that up](https://www.digitalocean.com/community/tutorials/how-to-configure-ssh-key-based-authentication-on-a-linux-server). You must already have an account to do this. Refer back to the first guide on SSH in VSCode to see how to add this key to your SSH config file. Do note that using SSH keys does not eliminate the need for a password; Lunaserver may still ask you to provide a password occasionally, but less often.

## Cargo

Every external dependency needed to run the code in lunadev-2024 on Lunaserver is already installed globally, with the exception of Rust itself, as it can only be installed for individual users. To install Rust, run the following command in Lunaserver:  
`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`  
and select the default options.

## What about a display?

I am actively working on finding a solution that will allow multiple people to have separate displays into Lunaserver without devcontainers.  
If you need a display soon, please let me know as there are temporary solutions.

## Directory

1. [unros-core](https://github.com/utahrobotics/lunadev-2024/tree/main/unros-core) - The core framework for Unros
2. [rig](https://github.com/utahrobotics/lunadev-2024/tree/main/rig) - Tracks the various moving parts of a robot
3. [realsense](https://github.com/utahrobotics/lunadev-2024/tree/main/realsense) - Working with realsense cameras
4. [camera](https://github.com/utahrobotics/lunadev-2024/tree/main/camera) - Working with generic cameras
5. [apriltag](https://github.com/utahrobotics/lunadev-2024/tree/main/apriltag) - Identifying apriltags
6. [drive](https://github.com/utahrobotics/lunadev-2024/tree/main/drive) - Controlling the wheels
7. [global-msgs](https://github.com/utahrobotics/lunadev-2024/tree/main/global-msgs) - Messages used for lunabotics that are not related to Unros
8. [navigator](https://github.com/utahrobotics/lunadev-2024/tree/main/navigator) - Drive controller for driving to waypoints
9. [localization](https://github.com/utahrobotics/lunadev-2024/tree/main/localization) - Localizes the robot in space based on sensor data
10. [serial](https://github.com/utahrobotics/lunadev-2024/tree/main/serial) - Working with serial ports
11. [telemetry](https://github.com/utahrobotics/lunadev-2024/tree/main/telemetry) - Connecting with Lunabase
12. [lunabase](https://github.com/utahrobotics/lunadev-2024/tree/main/lunabase) - Mission control software, with some development utilities
13. [tools](https://github.com/utahrobotics/lunadev-2024/tree/main/tools) - Command Line Application for controlling Unros externally
14. [lunabot](https://github.com/utahrobotics/lunadev-2024/tree/main/lunabot) - The final executable for lunabot
15. [lunadev](https://github.com/utahrobotics/lunadev-2024/tree/main/lunadev) - Some stuff that help with development

## Third Part Assets

1. [Low Poly Rock Pack 001](https://emerald-eel-entertainment.itch.io/low-poly-rock-pack-001). Accessed Aug 26 2024. Used in Lunasim.
