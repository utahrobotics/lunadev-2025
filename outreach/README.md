# Outreach

To pilot Lunabot remotely using Lunabase (our mission control software), whether it be for outreach or testing, follow the steps listed below:

0. Download Lunabase onto your laptop (link pending).
1. Turn on the router. It's network name (SSID) is USR-Wifi-5G. At least one of the icons on the top of the router should be green.
2. Turn on the computer by pressing the power button once. Ensure that the router is on or is currently turning on. If the computer is on for more than 5 minutes while the router is off, you should restart the computer.

> To power off the computer, press the power button once. If the lights do not turn off after 1 minutes, press it again. If after another minute it is still on, hold the power button down until it is off.

3. Connect your laptop to USR-Wifi-5G. The password is written on the bottom of the router. The scratched out number is 6. You can do this step in parallel with the first 2 steps. If the router is not plugged into port 2308A of MEB 2340, it will not have internet access. As such, keep this page open on a separate device other than your laptop.
4. Find the private IP address of your laptop. Information on how to do that can be found [here](https://myprivateip.com/). Do not use the automatic private IP detection on that website if you are not opening that website on your laptop. Do not reuse the private IP address you found the last time you did this step as it can change if your laptop was disconnected for more than an hour.
5. Open Command Prompt if you are on Windows or Terminal on Mac. Run the following command:  
`ssh outreach@192.168.0.102`  
It will then ask for a password, which is just `outreach`. If authentication fails, the account is still locked so you should call me to unlock it. If this step succeeds, you should see the following line:  
`outreach@Lunaserver:~$ `
6. On your laptop, run Lunabase. It should show you the Godot splash screen and then an interface.
7. Type `run <your private ip>` then press enter. You should see many lines printed on screen, and hopefully most of them are not red. This is what my line would look like before I press enter:  
`outreach@Lunaserver:~$ run 192.168.0.100`
8. Check the top left corner of Lunabase. The value of "Last Received" should be green. The default state Lunabot starts in is Software Stop, which prevents it from moving. Click the "Continue Mission" button on Lunabase, then either move the on-screen joystick or connect either a PlayStation Controller or Xbox controller and move the left joystick. Lunabot should start driving. If it does not, verify that the motor controller boards are powered and connected to the computer either directly or through a USB hub.