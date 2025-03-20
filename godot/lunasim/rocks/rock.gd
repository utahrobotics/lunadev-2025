class_name Rock
extends Node3D





# index = line number - 10
const CHOICES: Array[PackedScene] = [
	preload("res://rocks/Low_Poly_Cuboid_Rock_001.glb"),
	preload("res://rocks/Low_Poly_Cuboid_Rock_003.glb"),
	preload("res://rocks/Low_Poly_Cuboid_Rock_004.glb"),
	preload("res://rocks/Low_Poly_Cuboid_Rock_007.glb"),
	preload("res://rocks/Low_Poly_Cuboid_Rock_009.glb"),
	preload("res://rocks/Low_Poly_Rock_001.glb"),
	preload("res://rocks/Low_Poly_Rock_002.glb"),
	preload("res://rocks/Low_Poly_Rock_003.glb"),
	preload("res://rocks/Low_Poly_Rock_004.glb"),
	preload("res://rocks/Low_Poly_Rock_005.glb"),
	preload("res://rocks/Low_Poly_Rock_Small_001.glb"),
	preload("res://rocks/Low_Poly_Rock_Small_004.glb"),
	#preload("res://rocks/Low_Poly_Rock_Small_002.glb") # This rock is quite large
]

@export var index := -1


func _ready() -> void:
	for _i in range(12):
		await get_tree().physics_frame
		
	var rock: Node
	if index == -1:
		rock = CHOICES.pick_random().instantiate()
	else:
		rock = CHOICES[index].instantiate()
	rock.get_child(0).get_child(0).collision_layer = 16
	rotation.y = randf() * TAU
	rock.scale *= randf_range(1.0, 1.5)
	add_child(rock)
	
	var raycast := RayCast3D.new()
	raycast.collision_mask = 16
	raycast.target_position = Vector3.UP * 10
	raycast.position.y = -10
	add_child(raycast)
	while true:
		await get_tree().physics_frame
		await get_tree().physics_frame
		if is_ancestor_of(raycast.get_collider()):
			break
		else:
			raycast.add_exception(raycast.get_collider())
	rock.get_child(0).get_child(0).collision_layer = 3
	raycast.global_position = raycast.get_collision_point()
	raycast.target_position = Vector3.DOWN * 10
	raycast.collision_mask = 1
	while true:
		await get_tree().physics_frame
		await get_tree().physics_frame
		if raycast.get_collider().name == "Floor":
			break
		else:
			raycast.add_exception(raycast.get_collider())
	global_position += raycast.get_collision_point() - raycast.global_position
	raycast.queue_free()
