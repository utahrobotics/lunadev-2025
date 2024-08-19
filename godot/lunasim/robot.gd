class_name Robot
extends Node3D


var _timer := LunasimNode.DELTA

@onready var raycast: RayCast3D = $RayCast3D


func _physics_process(delta: float) -> void:
	if raycast.is_colliding():
		position = raycast.get_collision_point()
		var normal := raycast.get_collision_normal()
		var angle := normal.angle_to(global_basis.y)
		if angle > 0.001:
			var cross := global_basis.y.cross(normal).normalized()
			global_rotate(cross, angle)
	
	
	_timer -= delta
	if _timer <= 0.0:
		_timer = LunasimNode.DELTA
		LunasimNode.send_accelerometer(0, global_basis.inverse() * Vector3.DOWN * 9.81)
