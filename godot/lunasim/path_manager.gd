extends Path3D

@export var test_path:Array[Vector2]
@export var height = 0.25
@onready var robot = $"../Robot"


# Called when the node enters the scene tree for the first time.
func _ready() -> void:
	create_path(test_path)

func create_path(path:Array[Vector2]):
	self.curve.add_point(Vector3(robot.position.x, height, robot.position.z))
	for i in path.size():
		self.curve.add_point(Vector3(path[i].x, height, path[i].y))
