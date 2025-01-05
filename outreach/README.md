# Outreach

To pilot Lunabot remotely using Lunabase (our mission control software), whether it be for outreach or testing, follow the steps listed below. Keep in mind, the `outreach` account on the computer (lunabot) needs to be unlocked by an administrator account before you can do this.

0. Download Lunabase onto your laptop (link pending). Also verify that the necessary USB devices are plugged into the computer.
1. Turn on the router. It's network name (SSID) is USR-Wifi-5G. At least one of the icons on the top of the router should be green.
2. Turn on the computer by pressing the power button once. Ensure that the router is on or is currently turning on. If the computer is on for more than 5 minutes while the router is off, you should restart the computer.

> To power off the computer, press the power button once. If the lights do not turn off after 1 minutes, press it again. If after another minute it is still on, hold the power button down until it is off.

3. Connect your laptop to USR-Wifi-5G. The password is written on the bottom of the router. The scratched out number is 6. You can do this step in parallel with the previous step. If the router is not plugged into port 2308A of MEB 2340, it will not have internet access. As such, keep this page open on a separate device other than your laptop.
4. Find the private IP address of your laptop. The easiest way to do that is to open [this link](http://192.168.0.102/ip) on your laptop that is connected to the router. If that doesn't work, try restarting the computer. This step must work before you can continue. Do not reuse the private IP address you found the last time you did this step as it can change if your laptop was disconnected for more than an hour.
5. Open Command Prompt if you are on Windows or Terminal on Mac. Run the following command:  
`ssh outreach@192.168.0.102`  
It will then ask for a password, which is just `outreach`. If authentication fails, the account is still locked so you should call me to unlock it. If this step succeeds, you should see the following line:  
`outreach@Lunaserver:~$ `  
This is the outreach terminal.
6. Run Lunabase. It should show you the Godot splash screen and then an interface.
7. Type `run <your private ip>` in the outreach terminal then press enter. You should see many lines printed on screen, and hopefully most of them are not red. This is what my line would look like before I press enter:  
`outreach@Lunaserver:~$ run 192.168.0.100`
8. Check the top left corner of Lunabase. The value of "Last Received" should be green. The default state Lunabot starts in is Software Stop, which prevents it from moving. Click the "Continue Mission" button on Lunabase, then either move the on-screen joystick or connect either a PlayStation Controller or Xbox controller and move the left joystick. Lunabot should start driving. If it does not, verify that the motor controller boards are powered and connected to the computer either directly or through a USB hub.
