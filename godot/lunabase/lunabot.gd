extends LunabotConn

signal _disconnected

enum State { AUTO, TELEOP, STOPPED }

var current_state: State = State.STOPPED

var init_ws: WebSocketPeer

@onready var previous_stage = $"VBoxContainer/Buttons Label".text

func _ready() -> void:
	while true:
		init_ws = WebSocketPeer.new()
		if init_ws.connect_to_url("ws://192.168.0.102/init-lunabot") == OK:
			await _disconnected
		await get_tree().create_timer(5).timeout


func _physics_process(_delta: float) -> void:
	if previous_stage != $"VBoxContainer/Buttons Label".text:
		print("State Changed")
		match $"VBoxContainer/Buttons Label".text:
			"Current Stage: Auto":
				Lunabot.current_state = Lunabot.State.AUTO
			"Current Stage: TeleOp":
				Lunabot.current_state = Lunabot.State.TELEOP
			"Current Stage: Stopped":
				Lunabot.current_state = Lunabot.State.STOPPED
	previous_stage = $"VBoxContainer/Buttons Label".text
	if init_ws != null:
		init_ws.poll()
		if init_ws.get_ready_state() == WebSocketPeer.STATE_CLOSED:
			init_ws = null
			_disconnected.emit()
