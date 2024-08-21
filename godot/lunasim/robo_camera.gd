extends Camera3D

const DELTA := 1.0 / 30

var _timer := DELTA

func _process(delta: float) -> void:
	_timer -= delta
	if _timer <= 0.0:
		_timer = DELTA
		
		for tag in get_tree().get_nodes_in_group("Apriltags"):
			if !tag.explicit:
				continue
			if !is_position_in_frustum(tag.global_position):
				continue
			LunasimNode.send_explicit_apriltag(get_tree().get_first_node_in_group("Robot").global_transform)
