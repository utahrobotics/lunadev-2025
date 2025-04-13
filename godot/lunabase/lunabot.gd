extends LunabotConn

signal _disconnected

enum State { AUTO, TELEOP, STOPPED }

var init_ws: WebSocketPeer


func _ready() -> void:
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
