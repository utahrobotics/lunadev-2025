class_name Robot
extends CharacterBody3D

const TERRAIN_LERP_SPEED := 150.0
const SPEED := 0.3
const WHEEL_SEPARATION := 0.6
const DELTA := 1.0 / 60

@export var estimate_material: StandardMaterial3D

var _timer := DELTA
var _left := 0.0
var _right := 0.0

@onready var raycast: RayCast3D = $RaycastOrigin/RayCast3D
@onready var estimate: Node3D = $Estimate
@onready var _last_quat := quaternion


func _ready() -> void:
	LunasimNode.drive.connect(
		func(left: float, right: float):
			_left = left
			_right = right
	)
	@warning_ignore("shadowed_variable_base_class")
	LunasimNode.transform.connect(
		func(transform: Transform3D):
			estimate.global_transform = transform
	)
	for node in get_children():
		if node is not MeshInstance3D:
			continue
		var mesh_inst: MeshInstance3D = node.duplicate()
		mesh_inst.mesh = mesh_inst.mesh.duplicate()
		mesh_inst.mesh.material = estimate_material
		mesh_inst.layers = 4
		estimate.add_child(mesh_inst)


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
		LunasimNode.send_gyroscope(0, quaternion * _last_quat.inverse())
		_last_quat = quaternion
