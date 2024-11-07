extends LunabotConn


func _ready() -> void:
	await get_tree().create_timer(4.0).timeout
	#set_steering(1.0, 0.2)
