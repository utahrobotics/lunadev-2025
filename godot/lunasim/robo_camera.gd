extends Camera3D

const DELTA := 1.0 / 30

var _timer := DELTA

func _process(delta: float) -> void:
	_timer -= delta
	if _timer <= 0.0:
		_timer = DELTA
		
		for tag in get_tree().get_nodes_in_group("Apriltags"):
			if tag.explicit:
				
