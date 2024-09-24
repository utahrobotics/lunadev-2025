class_name Control3D
extends Control


@export var node3d: Node3D
@export var camera: Camera3D


func _process(_delta: float) -> void:
	if node3d == null or camera == null or !camera.get_parent().get_parent().is_visible_in_tree():
		visible = false
		return
	
	if camera.is_position_behind(node3d.global_position):
		visible = false
		return
	
	visible = true
	position = camera.unproject_position(node3d.global_position)
