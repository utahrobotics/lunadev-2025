class_name Robot
extends CharacterBody3D

const TERRAIN_LERP_SPEED := 150.0
const SPEED := 0.3
const WHEEL_SEPARATION := 0.6
const DELTA := 1.0 / 60

var _timer := DELTA
var _left := 0.0
var _right := 0.0

@onready var raycast: RayCast3D = $RayCast3D


func _ready() -> void:
	LunasimNode.drive.connect(
		func(left: float, right: float):
			_left = left
			_right = right
	)


func _physics_process(delta: float) -> void:
	if raycast.is_colliding():
		position = raycast.get_collision_point()
		var normal := raycast.get_collision_normal()
		var angle := normal.angle_to(global_basis.y)
		if angle > 0.001:
			var cross := global_basis.y.cross(normal).normalized()
			var blend := pow(0.5, delta * TERRAIN_LERP_SPEED)
			global_rotate(cross, angle * blend)
	
	var drive_diff := _right - _left
	var drive_mean := (_right + _left) / 2
	rotation.y += drive_diff * SPEED * delta / WHEEL_SEPARATION
	velocity = -global_basis.z * drive_mean
	move_and_slide()
	
	_timer -= delta
	if _timer <= 0.0:
		_timer = DELTA
		LunasimNode.send_accelerometer(0, global_basis.inverse() * Vector3.DOWN * 9.81)
