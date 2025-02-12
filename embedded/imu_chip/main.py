import time
from machine import Pin, I2C, Timer, PWM, ADC, reset
from lsm6dsox import LSM6DSOX
#import network
#import socket

indicator_1 = Pin(8, Pin.OUT)
indicator_1.value(0)

indicator_2 = Pin(7, Pin.OUT)
indicator_2.value(0)

for x in range(2):
    indicator_1.value(1)
    indicator_2.value(1)
    time.sleep(0.2)
    indicator_1.value(0)
    indicator_2.value(0)
    time.sleep(0.2)

i2c = I2C(0, sda=Pin(0), scl=Pin(1))  # Correct I2C pins for RP2040
lsm = LSM6DSOX(i2c)

name = None
m1_pos = 0
m2_pos = 0

with open('info.txt') as f:
    name = f.readline()
    m1_pos = int(f.readline())
    m2_pos = int(f.readline())

def get_info():
    print(name + str((m1_pos + m2_pos) / 2))

def store_pos():
    with open("info.txt", 'w') as f:
        f.write(name + str(m1_pos) + "\n" + str(m2_pos))

active = False
target_pos = int((m1_pos + m2_pos) / 2)

m1_slp = Pin(10, Pin.OUT)
m1_slp.value(0)

m1_dir = Pin(15, Pin.OUT)
m1_dir.value(0)

m1_pwm = PWM(Pin(9, Pin.OUT))
m1_pwm.freq(20000)
m1_pwm.duty_u16(0)

m1_adc = ADC(26)

m2_slp = Pin(17, Pin.OUT)
m2_slp.value(0)

m2_dir = Pin(14, Pin.OUT)
m2_dir.value(0)

m2_pwm = PWM(Pin(16, Pin.OUT))
m2_pwm.freq(20000)
m2_pwm.duty_u16(0)

m2_adc = ADC(27)

m1_slp.value(1)
m2_slp.value(1)

# input should be a value between -65535 and +65535
def set_speed(speed):
    direction = 1
    if(speed < 0):
        direction = 0
    
    m1_dir.value(direction)
    m2_dir.value(direction)
    
    m1_pwm.duty_u16(abs(speed))
    m2_pwm.duty_u16(abs(speed))

enc_1a = Pin(21, Pin.IN)
enc_1b = Pin(22, Pin.IN)
enc_2a = Pin(19, Pin.IN)
enc_2b = Pin(20, Pin.IN)

def enc1_handler(pin):
    global m1_pos
    if enc_1b.value() == 0:
        m1_pos += 1
    else:
        m1_pos -= 1

def enc2_handler(pin):
    global m2_pos
    if enc_2b.value() == 0:
        m2_pos += 1
    else:
        m2_pos -= 1

enc_1a.irq(trigger=Pin.IRQ_RISING, handler=enc1_handler)
enc_2a.irq(trigger=Pin.IRQ_RISING, handler=enc2_handler)

def pos_handler(timer):
    global m1_pwm, m2_pwm, m1_dir, m2_dir, active, m1_pos, m2_pos
    if not active:
        m1_pwm.duty_u16(0)
        m2_pwm.duty_u16(0)
    else:
        m1_speed = 63000
        m2_speed = 63000
        m1_diff = m1_pos - target_pos
        m2_diff = m2_pos - target_pos
        
        if(m1_diff < 0 and m2_diff < 0): #extending
            if m1_pos + 5 < m2_pos:
                m2_speed = 55000
            if m2_pos + 5 < m1_pos:
                m1_speed = 55000
        
        if(m1_diff > 0 and m2_diff > 0): #retracting
            if m1_pos - 10 > m2_pos:
                m2_speed = 55000
            if m2_pos - 10 > m1_pos:
                m1_speed = 55000
        
        m1_stopped = False
        if m1_diff > 10:
            m1_dir.value(0)
            m1_pwm.duty_u16(m1_speed)
        elif m1_diff < -10:
            m1_dir.value(1)
            m1_pwm.duty_u16(m1_speed)
        else:
            m1_pwm.duty_u16(0)
            m1_stopped = True
        
        if m2_diff > 10:
            m2_dir.value(0)
            m2_pwm.duty_u16(m2_speed)
        elif m2_diff < -10:
            m2_dir.value(1)
            m2_pwm.duty_u16(m2_speed)
        else:
            m2_pwm.duty_u16(0)
            if m1_stopped:
                store_pos()
    
pos_check_timer = Timer(mode=Timer.PERIODIC, period=10, callback=pos_handler)

def retract_home():
    global m1_pos, m2_pos
    retract()
    last_m1_pos = m1_pos
    last_m2_pos = m2_pos
    time.sleep(0.2)
    while last_m1_pos != m1_pos or last_m2_pos != m2_pos:
        last_m1_pos = m1_pos
        last_m2_pos = m2_pos
        time.sleep(0.2)
    print("retracted")
    m1_pos = 0
    m2_pos = 0
    stop()

def extend_home():
    global m1_pos, m2_pos
    extend()
    last_m1_pos = m1_pos
    last_m2_pos = m2_pos
    time.sleep(0.2)
    while last_m1_pos != m1_pos or last_m2_pos != m2_pos:
        last_m1_pos = m1_pos
        last_m2_pos = m2_pos
        time.sleep(0.2)
    print("extended")
    m1_pos = 4500
    m2_pos = 4500
    if("lift" in name):
        create_offset(50)
    stop()
'''
def set_pos(pos):
    m1_forward = False
    m2_forward = False
    if(pos > m1_pos):
        m1_dir.value(1)
        m1_forward = True
    else:
        m1_dir.value(0)
    
    if(pos > m2_pos):
        m2_dir.value(1)
        m2_forward = True
    else:
        m2_dir.value(0)
    
    m1_pwm.duty_u16(65535)
    m2_pwm.duty_u16(65535)
    
    m1_reached = False
    m2_reached = False
    while True:
        if m1_forward:
            if m1_pos >= pos:
                m1_pwm.duty_u16(0)
                m1_reached = True
        elif m1_pos <= pos:
            m1_pwm.duty_u16(0)
            m1_reached = True
        
        if m2_forward:
            if m2_pos >= pos:
                m2_pwm.duty_u16(0)
                m2_reached = True
        elif m2_pos <= pos:
            m2_pwm.duty_u16(0)
            m2_reached = True
        
        if m1_reached and m2_reached:
            break
        time.sleep(0.01)
'''
def set_pos(pos):
    global target_pos
    target_pos = pos

def print_pos():
    print("Motor 1: " + str(m1_pos))
    print("Motor 2: " + str(m2_pos))

def activate(state):
    global active
    active = state

def stop():
    global target_pos, m1_pos, m2_pos
    target_pos = int((m1_pos + m2_pos)/2)

def extend():
    global target_pos
    target_pos = 1000000

def retract():
    global target_pos
    target_pos = -1000000

def e():
    extend()

def r():
    retract()

def s():
    stop()

def print_adc():
    print(m1_adc.read_u16())
    print(m2_adc.read_u16())

def print_current():
    print((m1_adc.read_u16() * 3.3/65535 - 0.05) * 50)
    print((m2_adc.read_u16() * 3.3/65535 - 0.05) * 50)

#log_file = open("demofile3.txt", "w")

#def logger_handler():

def print_handler(timer):
    global lsm
    accx, accy, accz = lsm.acceleration
    gyrox, gyroy, gyroz = lsm.gyro
    print(str(m1_pos) + " " + str(m2_pos) + " " + f"{accx:.2f} {accy:.2f} {accz:.2f} {gyrox:.2f} {gyroy:.2f} {gyroz:.2f}")

pos_check_timer = Timer(mode=Timer.PERIODIC, period=250, callback=print_handler)

def start_pos_timer():
    global pos_check_timer
    pos_check_timer = Timer(mode=Timer.PERIODIC, period=250, callback=print_handler)

def stop_pos_timer():
    global pos_check_timer
    pos_check_timer.deinit()

def create_offset(amount_to_reduce_m2_length_relative_to_m1): #for lift actuators should probably reduce right side by 1/8 inch iirc
    global m2_pos
    m2_pos += amount_to_reduce_m2_length_relative_to_m1

activate(True)
