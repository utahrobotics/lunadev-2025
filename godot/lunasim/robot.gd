class_name Robot
extends CharacterBody3D

const TERRAIN_TRANSLATION_LERP_SPEED := 150.0
const TERRAIN_ROTATION_LERP_SPEED := 150.0
const SPEED := 0.8
const WHEEL_SEPARATION := 0.6
const DELTA := 1.0 / 60

@export var estimate_material: StandardMaterial3D

var _timer := DELTA
var _left := 0.0
var _right := 0.0
var _drive_noise := FastNoiseLite.new()

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
	
	_drive_noise.seed = randi()
	_drive_noise.frequency = 0.03
	push_warning("Using drive noise: %s" % _drive_noise.seed)
	set_physics_process(false)
	
	for _i in range(10):
		await get_tree().physics_frame
	
	set_physics_process(true)


func _physics_process(delta: float) -> void:
	if raycast.is_colliding():
		var blend := pow(0.5, delta * TERRAIN_TRANSLATION_LERP_SPEED)
		global_position = global_position.lerp(raycast.get_collision_point(), blend)
		var normal := raycast.get_collision_normal()
		var angle := normal.angle_to(global_basis.y)
		if angle > 0.001:
			var cross := global_basis.y.cross(normal).normalized()
			blend = pow(0.5, delta * TERRAIN_ROTATION_LERP_SPEED)
			global_rotate(cross, angle * blend)
	
	var noise_origin := global_transform * Vector3.RIGHT * WHEEL_SEPARATION / 2
	var right := _right * remap(_drive_noise.get_noise_2d(noise_origin.x, noise_origin.y), -1,  1, 0.3, 1)
	
	noise_origin = global_transform * Vector3.LEFT * WHEEL_SEPARATION / 2
	var left := _left * remap(_drive_noise.get_noise_2d(noise_origin.x, noise_origin.y), -1,  1, 0.3, 1)
	
	var drive_diff := right - left
	var drive_mean := (right + left) / 2
	
	rotation.y += drive_diff * SPEED * delta / WHEEL_SEPARATION
	velocity = -global_basis.z * drive_mean
	move_and_collide(velocity * delta)
	
	_timer -= delta
	if _timer <= 0.0:
		LunasimNode.send_accelerometer(0, global_basis.inverse() * Vector3.DOWN * 9.81)
		LunasimNode.send_gyroscope(0, quaternion * _last_quat.inverse(), DELTA - _timer)
		_timer = DELTA
		_last_quat = quaternion
