extends LunabotConn

signal _disconnected

enum State { AUTO, TELEOP, STOPPED }

var current_state: State = State.STOPPED

var init_ws: WebSocketPeer

@onready var stage_text = get_tree().current_scene.get_node("VBoxContainer/Buttons Label")
@onready var stage_tabs = get_tree().current_scene.get_node("StateInfoTabContainer")

func _ready() -> void:
	Lunabot.entered_manual.connect(_on_teleop_triggered)
	Lunabot.entered_traverse_obstacles.connect(_on_auto_triggered)
	Lunabot.entered_soft_stop.connect(_on_stopped_triggered)
	while true:
		init_ws = WebSocketPeer.new()
		if init_ws.connect_to_url("ws://192.168.0.102/init-lunabot") == OK:
			await _disconnected
		await get_tree().create_timer(5).timeout


func _physics_process(_delta: float) -> void:
	if init_ws != null:
		init_ws.poll()
		if init_ws.get_ready_state() == WebSocketPeer.STATE_CLOSED:
			init_ws = null
			_disconnected.emit()

func _on_auto_triggered():
	Lunabot.traverse_obstacles()
	Lunabot.current_state = Lunabot.State.AUTO
	stage_text.text = "Current Stage: Auto"
	stage_tabs.current_tab = 0

func _on_teleop_triggered():
	Lunabot.continue_mission()
	Lunabot.current_state = Lunabot.State.TELEOP
	stage_text.text = "Current Stage: TeleOp"
	stage_tabs.current_tab = 1
	
func _on_stopped_triggered():
	Lunabot.soft_stop()
	Lunabot.current_state = Lunabot.State.STOPPED
	stage_text.text = "Current Stage: Stopped"
	stage_tabs.current_tab = 2
