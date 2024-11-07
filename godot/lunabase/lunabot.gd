extends LunabotConn

enum State{
	AUTO,
	TELEOP,
	STOPPED
}

var current_state:State = State.STOPPED

func _ready() -> void:
	await get_tree().create_timer(4.0).timeout
	#set_steering(1.0, 0.2)
